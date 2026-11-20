use crate::{algorithms::AgentAlgorithm,program_session,hooks::{HookChain,ToolHook},config::Settings, domain::{CreateRun, ReconcileTool, Run, RunStatus, ToolCall, ToolEffectOutcome}, provider::{ModelClient, Reply}, ports::RunStorePort, tools, policy, journal, tool_schema, tool_batch};
use serde_json::{json, Value};
use std::{collections::{HashMap,HashSet}, sync::Arc};
use tokio::sync::{mpsc,Mutex,Notify,watch,RwLock};
use uuid::Uuid;

mod delegation;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod benchmark_tests;
#[cfg(test)]
mod model_only_tests;

pub struct Engine {
    pub settings: Settings,
    pub store: Arc<RunStorePort>,
    algorithm:Arc<dyn AgentAlgorithm>,
    /// All GUI, memory, account, room and model-catalog services live outside
    /// the Run kernel. The Deref compatibility surface does not own a runner.
    product: crate::product::ProductHost,

    hooks: RwLock<HookChain>,
    pub(crate) model: ModelClient,
    active: Mutex<HashSet<Uuid>>,
    cancellation: Mutex<HashMap<Uuid,watch::Sender<bool>>>,
    active_notify: Notify,
    delegate_lock: Mutex<()>,
    wake_tx: mpsc::UnboundedSender<Uuid>,
}
// Existing SDK/HTTP adapters retain product field access without moving
// persistence or UI catalogs into the Run kernel. They are owned by ProductHost.
impl std::ops::Deref for Engine {
    type Target=crate::product::ProductHost;
    fn deref(&self)->&Self::Target{&self.product}
}
impl Engine {
    /// Compose a pure Run executor from an injected storage port and product
    /// adapter. No filesystem, JSON database, migration or DB open occurs here.
    pub(crate) fn from_ports(settings:Settings,store:Arc<crate::ports::RunStorePort>,
        product:crate::product::ProductHost,algorithm:Arc<dyn AgentAlgorithm>)
        ->Result<Arc<Self>,String>{
        let model=ModelClient::new(&settings)?;
        let (wake_tx,mut wake_rx)=mpsc::unbounded_channel::<Uuid>();
        let hooks=RwLock::new(HookChain::from_settings(&settings));
        let engine=Arc::new(Self { settings, store, product, algorithm, hooks,
            model, active: Mutex::new(HashSet::new()), cancellation: Mutex::new(HashMap::new()), active_notify: Notify::new(), delegate_lock: Mutex::new(()), wake_tx });
        // Wakeups never recursively call launch() inside the run task. The
        // queued scheduler waits for active markers to detach and keeps only
        // a Weak reference, so it does not keep an abandoned engine alive.
        let weak=Arc::downgrade(&engine);
        tokio::spawn(async move {
            while let Some(id)=wake_rx.recv().await {
                let weak=weak.clone();
                // One stuck or slow worker must never stall other queued runs.
                tokio::spawn(async move {
                    let Some(engine)=weak.upgrade() else {return};
                    loop {
                        // Register the wake before checking the active marker,
                        // so detachment cannot be missed between the two steps.
                        let notified=engine.active_notify.notified();
                        tokio::pin!(notified);
                        notified.as_mut().enable();
                        let Some(run)=engine.store.get(id).await else {return};
                        let ready=(run.status==RunStatus::Queued
                                && run.in_flight.is_none() && !run.waiting_for_children)
                            || (run.status==RunStatus::Paused && !run.waiting_for_children
                                && run.in_flight.is_none()
                                && (run.pending_approval.is_none() || run.approval_decision.is_some()))
                            || (run.status==RunStatus::AwaitingApproval
                                && run.pending_approval.is_some() && run.approval_decision.is_some());
                        if !ready {return;}
                        if !engine.active.lock().await.contains(&id){
                            if let Err(error)=engine.launch(id).await{
                                tracing::warn!(%id,"queued worker wake rejected: {error}");
                            }
                            return;
                        }
                        notified.await;
                    }
                });
            }
        });
        Ok(engine)
    }
    /// Attach a synchronous, fail-closed policy/receipt hook to this runtime.
    /// The native SDK and HTTP share the same engine and hook chain.
    pub async fn register_tool_hook(&self, hook:Arc<dyn ToolHook>){self.hooks.write().await.add(hook);}

    /// Model and provider catalogs belong to the product host, not the
    /// native instruction/execute/verify kernel.
    pub fn model_for_selection(&self,provider:&str,model:&str)->Result<ModelClient,String>{
        self.product.model_for_selection(&self.settings,&self.model,provider,model)
    }
    pub fn model_for_run(&self,run:&Run)->Result<ModelClient,String>{
        // Every native Run receives a durable UUID-bound routing identity.
        // Retries and resumed requests keep the same provider affinity;
        // unrelated jobs never compete for one shared OpenCode Go session.
        let affinity=if run.provider_session_affinity.is_empty(){
            format!("dominion-run-{}",run.id)
        }else{run.provider_session_affinity.clone()};
        self.model_for_selection(&run.provider_id,&run.model_name)
            .map(|model|model.with_session_affinity(&affinity))
    }
    pub async fn launch(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        {
            let mut guard = self.active.lock().await;
            if !guard.insert(id) { return Err("execution is already active".into()); }
        }
        let started = self.store.mutate(id, |run| {
            if run.terminal() { return Err("cannot restart finished execution".into()); }
            // Archived reservations no longer have a live owning adapter.
            // An old approval receipt cannot authorize new external effects.
            if run.events.iter().any(|event|event.kind=="reserved_unlaunched"){
                return Err("retired reserved run cannot be resumed; submit a new task".into());
            }
            // Archived experiments cannot relaunch orphaned workers.
            if run.events.iter().any(|event|matches!(event.kind.as_str(),
                "strategy_candidate"|"strategy_trials_launched"|"strategy_trials_partial")){
                return Err("retired experiment run cannot be resumed; submit a new task".into());
            }
            if run.in_flight.is_some() || !run.read_batch_in_flight.is_empty() { return Err("interrupted tool may have side effects; reconcile before resuming".into()); }
            if run.waiting_for_children {return Err("delegated workers are still active; parent resumes automatically after settlement".into());}
            if run.pending_approval.is_some() && run.approval_decision.is_none() {
                return Err("approval required before resuming".into());
            }
            // Bind each Run to the exact operator-selected algorithm. A
            // restart with a different strategy must not silently reinterpret
            // previously authorized queued work.
            if !run.model_only{
                if let Some(binding)=run.events.iter().find(|event|event.kind=="algorithm_bound"){
                    if binding.message!=self.algorithm.id(){
                        return Err(format!("algorithm changed from {} to {}; submit a new Run or restore its algorithm",
                            binding.message,self.algorithm.id()));
                    }
                }else{
                    run.event("algorithm_bound",self.algorithm.id(),None);
                }
            }
            run.status = RunStatus::Running;
            run.event(if run.model_only {"model_only"}else{"resumed"},
                if run.model_only {"Native tool-free chat worker started"}else{"Execution worker started"},None);
            Ok(())
        }).await;
        if let Err(e) = started {
            self.active.lock().await.remove(&id);
            self.active_notify.notify_waiters();
            return Err(e);
        }
        let (cancel_tx,cancel_rx)=watch::channel(false);
        self.cancellation.lock().await.insert(id,cancel_tx);
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let model_only=engine.store.get(id).await.is_some_and(|run|run.model_only);
            let result=if model_only{
                engine.drive_model_only(id,cancel_rx).await
            }else{
                engine.drive(id,cancel_rx).await
            };
            if let Err(error) = result {
                tracing::warn!(%id, "execution error: {error}");
                let _ = engine.store.mutate(id, |run| {
                    if !run.terminal() {
                        run.status = RunStatus::Paused;
                        run.event("paused_error", format!("Recoverable error: {error}"), None);
                    }
                    Ok(())
                }).await;
            }
            engine.active.lock().await.remove(&id);
            engine.cancellation.lock().await.remove(&id);
            engine.product.release_run_context(id).await;
            engine.active_notify.notify_waiters();
            // A completed worker releases unused budget to its parent. The
            // notification propagates up the delegation tree, and may wake a
            // parent that explicitly suspended itself waiting for children.
            if let Some(child)=engine.store.get(id).await {
                if let Some(parent)=child.parent_run_id {
                    if let Err(error)=engine.settle_and_wake_ancestors(parent).await{
                        tracing::warn!(%id,"child settlement needs retry: {error}");
                    }
                }
            }
        });
        Ok(())
    }
    /// Tool-free chat shares the native worker registry, cancellation channel,
    /// provider routing, budget ledger and crash recovery with agent jobs.
    /// Exactly one billable model turn runs; no tool schemas are offered.
    async fn drive_model_only(&self,id:Uuid,mut cancellation:watch::Receiver<bool>)
        ->Result<(),String>{
        let run=self.store.get(id).await.ok_or("execution not found")?;
        if run.status!=RunStatus::Running||!run.model_only{return Ok(());}
        let selected=self.model_for_run(&run)?;
        let messages=self.algorithm.model_only_messages(&run);
        self.store.mutate(id,|current|{
            current.event("model_request_footprint","Model-only chat: no tool schemas",
                Some(json!({"context_chars":messages.iter().map(|m|m.to_string().chars().count()).sum::<usize>(),
                    "history_messages":messages.len(),"tool_count":0,"tool_schema_chars":0,
                    "mode":"model_only"})));
            Ok(())
        }).await?;
        self.providers.throttle(&run.provider_id).await;
        let reply=tokio::select!{
            biased;
            _=cancellation.changed()=>return Ok(()),
            answer=selected.complete(&messages,&[],false)=>answer
        };
        self.store.mutate(id,|run|{
            // A concurrent cancellation or restart always wins: no answer
            // publication or resurrection after an authoritative transition.
            if run.terminal()||run.status!=RunStatus::Running{return Ok(());}
            match reply{
                Ok(ref result)=>{
                    Self::account(run,result);
                    if !result.calls.is_empty(){
                        run.status=RunStatus::Failed;
                        run.event("model_only_tool_denied",
                            "Provider returned tool calls despite a tool-free request",None);
                    }else if run.cost_krw>run.budget_krw{
                        run.status=RunStatus::Failed;
                        run.event("budget_exceeded","Tool-free provider response exceeded the configured budget",None);
                    }else if result.text.trim().is_empty(){
                        run.status=RunStatus::Failed;
                        run.event("model_empty","Provider returned no chat response",None);
                    }else{
                        run.answer=Some(result.text.clone());
                        run.status=RunStatus::Completed;
                        run.event("completed","Tool-free chat completed in native execution kernel",None);
                    }
                },
                Err(ref error)=>{
                    run.status=RunStatus::Failed;
                    run.event("provider_error",error.clone(),None);
                }
            }
            Ok(())
        }).await?;
        Ok(())
    }
    fn account(run: &mut Run, reply: &Reply) {
        run.input_tokens = run.input_tokens.saturating_add(reply.input_tokens);
        run.cached_input_tokens = run.cached_input_tokens.saturating_add(reply.cached_input_tokens);
        run.cache_write_tokens=run.cache_write_tokens.saturating_add(reply.cache_write_tokens);
        if !reply.cache_usage_reported{
            run.cache_unreported_requests=run.cache_unreported_requests.saturating_add(1);
        }
        run.output_tokens = run.output_tokens.saturating_add(reply.output_tokens);
        run.cost_krw += reply.cost_krw;
        run.event("model", format!("{} · in {} / out {} tokens · {:.4} KRW", reply.model, reply.input_tokens, reply.output_tokens, reply.cost_krw), Some(json!({"input_tokens":reply.input_tokens,
            "uncached_input_tokens":reply.input_tokens.saturating_sub(reply.cached_input_tokens),
            "cached_input_tokens":reply.cached_input_tokens,
            "cache_write_tokens":reply.cache_write_tokens,
            "cache_usage_reported":reply.cache_usage_reported,
            "output_tokens":reply.output_tokens,
            "cost_krw":reply.cost_krw,"model":reply.model,
            "usage_reported":reply.usage_reported})));
        if run.cost_krw > policy::effective_budget_krw(run) {
            run.status = RunStatus::Failed;
            run.event("budget_exceeded", "Own model charges exceed budget remaining after delegated worker reservations", Some(json!({"cost_krw":run.cost_krw,"reserved_krw":run.reserved_budget_krw,"budget_krw":run.budget_krw})));
        }
    }
    async fn drive(self: &Arc<Self>, id: Uuid, mut cancellation:watch::Receiver<bool>) -> Result<(), String> {
        loop {
            let run = self.store.get(id).await.ok_or("execution not found")?;
            if run.terminal() || run.status != RunStatus::Running { return Ok(()); }
            if let Some(pending) = run.pending_approval.clone() {
                // Approval only authorizes the exact frozen tool + arguments.
                // Re-check against the advertised schema before starting.
                if !run.advertised_tools.is_empty(){
                    if let Err(error)=tool_schema::admit(&pending,&run.advertised_tools){
                        return Err(format!("approved tool no longer matches its model-turn schema: {}",error.reason));
                    }
                }
                let Some(allowed) = run.approval_decision else { return Ok(()); };
                if allowed {
                    self.perform_tool(id,pending,cancellation.clone()).await?;
                } else {
                    self.store.mutate(id,|run|journal::deny(run,&pending)).await?;
                }
                continue;
            }
            // Continue a previously installed model-authored program using
            // its original Run, durable queue and tool-approval gateway.
            // Never ask the model between dependent native tool receipts.
            if run.program_session.is_some() && run.queued_calls.is_empty()
                && run.in_flight.is_none() && run.read_batch_in_flight.is_empty(){
                self.store.mutate(id,|state|program_session::advance(state,&self.settings)).await?;
                continue;
            }
            // Codex/DeepSeek-style concurrency: pure inspection calls are
            // grouped in a bounded pool. Exclusive mutations are barriers.
            // A single Store transaction logs batch intent before dispatch,
            // then another atomically saves model-ordered tool receipts.
            let chain=self.hooks.read().await.clone();
            let mut parallel=tool_batch::parallel_prefix(&run,self.settings.max_parallel_reads);
            // A denied or invalid hook at index N is a barrier, not a
            // speculative dispatch of later reads.
            if let Some(index)=parallel.iter().position(|call|
                chain.before(&run,call).is_err()||
                !policy::admitted_tool(&run,&call.name,&self.settings.denied_tools)||
                self.algorithm.repeated_tool_action(&run,call).is_some()){
                parallel.truncate(index);
            }
            if parallel.len()>1{
                self.store.mutate(id,|run|journal::begin_read_batch(run,&parallel)).await?;
                let mut outcomes=tool_batch::execute_reads_cancellable(
                    &self.settings,id,parallel.clone(),cancellation.clone()).await;
                for (call,result) in parallel.iter().zip(outcomes.iter_mut()){
                    chain.after(&run,call,result);
                }
                self.store.mutate(id,|run|journal::finish_read_batch(run,&parallel,&outcomes)).await?;
                continue;
            }
            if let Some(call) = run.queued_calls.first().cloned() {
                if let Some(action)=self.algorithm.repeated_tool_action(&run,&call){
                    // No external action starts: journal a correlated tool
                    // error and give the model a chance to change strategy.
                    // Four identical prior failures pause to cap wasted spend.
                    self.store.mutate(id,|run|{
                        journal::reject_admission(run,&call,"REPEATED_TOOL_FAILURE",
                            "The exact same tool and arguments have failed repeatedly. Inspect the prior error and change path, arguments or approach; do not call the unchanged tool again.")?;
                        match action{
                            crate::loop_guard::Action::Reject=>{
                                run.event("loop_guard","Blocked repeated identical failed tool call",Some(json!({
                                    "name":call.name,"call_id":call.id,"dispatched":false
                                })));
                            },
                            crate::loop_guard::Action::Pause=>{
                                run.status=RunStatus::Paused;
                                run.event("loop_guard_paused",
                                    "Repeated identical failed tool calls paused; amend the task or choose a different approach",Some(json!({
                                        "name":call.name,"call_id":call.id,"dispatched":false
                                    })));
                            }
                        }
                        Ok(())
                    }).await?;
                    if action==crate::loop_guard::Action::Pause{return Ok(());}
                    continue;
                }
                // The catalog is frozen when the model turn is accepted and
                // survives crash recovery. Legacy sessions without a snapshot
                // retain the previous policy-based execution semantics.
                if !run.advertised_tools.is_empty(){
                    if let Err(error)=tool_schema::admit(&call,&run.advertised_tools){
                        self.store.mutate(id,|run|journal::reject_admission(
                            run,&call,error.code.as_str(),&error.reason)).await?;
                        continue;
                    }
                }
                if !policy::allowed_tool(&run,&call.name){
                    self.store.mutate(id,|run|journal::reject_policy(run,&call)).await?;
                    continue;
                }
                if !policy::admitted_tool(&run,&call.name,&self.settings.denied_tools){
                    self.store.mutate(id,|run|journal::reject_admission(
                        run,&call,"OPERATOR_TOOL_DENIED",
                        "Tool is disabled by the current operator configuration")).await?;
                    continue;
                }
                if let Err(deny)=chain.before(&run,&call){
                    self.store.mutate(id,|run|journal::reject_admission(
                        run,&call,&deny.code,&deny.reason)).await?;
                    continue;
                }
                if matches!(call.name.as_str(),"shell"|"merge_subagent") ||
                   self.addons.needs_external_approval(&call.name) {
                    self.store.mutate(id, |run| {
                        run.queued_calls.remove(0);
                        run.pending_approval = Some(call.clone());
                        run.approval_decision = None;
                        run.status = RunStatus::AwaitingApproval;
                        run.event("approval_required",
                            if call.name == "shell" { format!("Shell command needs approval: {}",call.arguments.get("command").and_then(Value::as_str).unwrap_or("?")) }
                            else if call.name == "merge_subagent" {format!("Integrating a child Git patch needs approval: {}",call.arguments.get("id").and_then(Value::as_str).unwrap_or("?"))}
                             else if self.addons.needs_connected_approval(&call.name) {
                                format!("Connected account action needs approval: {} args={}",
                                    call.name,call.arguments.to_string().chars().take(1200).collect::<String>())
                            }
                            else { format!("External MCP tool needs approval: {}",call.name) },
                            Some(call.arguments.clone()));
                        Ok(())
                    }).await?;
                    return Ok(());
                }
                self.perform_tool(id,call,cancellation.clone()).await?;
                continue;
            }
            if run.plan.is_none() && !self.algorithm.separate_planning(&self.settings,&run) {
                // Preserve any already-durable user corrections and avoid a
                // paid preplanning request. The first normal tool-capable model
                // call decides both its plan and first actions.
                self.store.mutate(id, |current| {
                    if current.status!=RunStatus::Running || current.plan.is_some(){return Ok(());}
                    self.algorithm.initialize_fused(current);
                    current.event("planning_fused",
                        "Initial planning folded into the tool-capable model turn (zero additional model calls)",None);
                    Ok(())
                }).await?;
                continue;
            }
            if run.plan.is_none() {
                // The root instruction is already durable in run.instruction.
                // Never stringify historic tool outputs into an initial plan.
                let messages=self.algorithm.planning_request(&run,self.settings.context_max_chars)?;
                let planning_history_len=run.messages.len();
                let selected_model=self.model_for_run(&run)?;
                self.providers.throttle(&run.provider_id).await;
                let reply=tokio::select!{
                    biased;
                    _=cancellation.changed()=>return Ok(()),
                    response=selected_model.complete(&messages,&[],true)=>response?
                };
                self.store.mutate(id, |run| {
                    Self::account(run,&reply);
                    if run.terminal() || run.status!=RunStatus::Running {return Ok(());}
                    if run.messages.len()!=planning_history_len {
                        run.event("stale_plan","New user directive arrived during planning; recomputing",None);
                        return Ok(());
                    }
                    run.plan=Some(reply.text.clone());
                    let prepared=self.algorithm.messages_after_plan(run,&reply.text);
                    run.messages=prepared;
                    run.event("plan","Initial plan created",Some(json!({"text":reply.text})));
                    Ok(())
                }).await?;
                continue;
            }
            if run.steps >= run.max_steps {
                self.store.mutate(id, |run| {
                    run.status = RunStatus::Failed;
                    run.event("limit",format!("Model round limit ({}) reached",run.max_steps),None);
                    Ok(())
                }).await?;
                return Ok(());
            }
            if run.cost_krw > policy::effective_budget_krw(&run) { return Err("hierarchical budget exceeded".into()); }
            // Authoritative schemas are filtered in the kernel BEFORE a
            // strategy can choose which ones to expose. Returning invented
            // or rewritten schemas cannot grant a capability.
            let native=tools::definitions(self.settings.shell_enabled);
            let external=self.addons.tool_definitions(&self.settings).await;
            let mut combined=native.clone();
            combined.extend(external.iter().cloned());
            let has_children=self.store.has_children(id).await;
            let (authorized,policy_saved)=policy::select_catalog(
                &run,combined,has_children,&self.settings.denied_tools);
            let allowed_native=authorized.iter().filter(|tool|native.contains(tool))
                .cloned().collect();
            let allowed_external=authorized.iter().filter(|tool|external.contains(tool))
                .cloned().collect();
            let presentation=self.algorithm.present_tools(
                &run,allowed_native,allowed_external,&self.settings.denied_tools);
            let mut seen=HashSet::<String>::new();
            if presentation.tools.iter().any(|tool|{
                let Some(name)=tool.pointer("/function/name").and_then(Value::as_str)
                    else{return true;};
                !authorized.contains(tool)||!seen.insert(name.into())
            }){
                return Err("algorithm attempted to advertise an unauthorized or duplicate tool schema".into());
            }
            let catalog=presentation.tools;
            let schema_chars_saved=policy_saved.saturating_add(presentation.saved_chars);
            let projection=self.algorithm.project_context(&run,self.settings.context_max_chars)?;
            let mut outbound=projection.messages.clone();
            // The source is a durable tool/verifier receipt, not model or
            // external content. The guidance is a bounded, optional reference
            // and never changes approvals, execution rights or retries.
            let repair=self.algorithm.recovery_advice(&run);
            let mut historical=None;
            if run.strategy_learning && repair.is_some(){
                if let Some(repository)=run.repository.as_deref(){
                    let evidence=self.store.strategy_evidence(repository).await;
                    let matches=crate::strategy::rank_for_run(evidence,&run,2);
                    if !matches.is_empty(){
                        historical=Some(crate::strategy::transfer_prompt(&matches));
                    }
                }
            }
            let notes=self.focused_notes(&run).await;
            let mut skill_names=None;
            if run.steps==0{
                let skills=crate::skills::list(&self.settings,id).await.unwrap_or_default();
                if !skills.is_empty(){
                    skill_names=Some(skills.iter().map(|s|s.name.as_str())
                        .collect::<Vec<_>>().join(", "));
                }
            }
            let repair_applied=self.algorithm.augment_context(
                &run,&mut outbound,repair,historical.as_deref(),&notes,
                skill_names.as_deref(),self.settings.context_max_chars);
            let expected_history=run.messages.len();
            // These figures are actual serialized API components, NOT a
            // speculative tokenizer estimate and never an invented cache hit.
            // Recording the footprint BEFORE dispatch also explains 502 and
            // in-flight failures with no provider usage receipt.
            let context_chars=outbound.iter().map(|v|v.to_string().chars().count())
                .sum::<usize>();
            let schema_chars=catalog.iter().map(|v|v.to_string().chars().count())
                .sum::<usize>();
            let marked=outbound.iter().filter(|v|
                v["dominion_cache_anchor"]==true).count();
            self.store.mutate(id,|current|{
                current.event("model_request_footprint",
                    "Actual provider request context/tool-schema character footprint",
                    Some(json!({"context_chars":context_chars,
                        "tool_schema_chars":schema_chars,
                        "tool_count":catalog.len(),
                        "history_messages":outbound.len(),
                        "stable_prefix_markers":marked,
                        "omitted_context_messages":projection.omitted_messages,
                        "mode":if current.chat_context.is_empty(){"agent"}else{"chat_agent"}})));
                Ok(())
            }).await?;
            let selected_model=self.model_for_run(&run)?;
            self.providers.throttle(&run.provider_id).await;
            let reply=tokio::select!{
                biased;
                _=cancellation.changed()=>return Ok(()),
                response=selected_model.complete(&outbound,&catalog,false)=>response?
            };
            let terminal_candidate = reply.calls.is_empty();
            let mut accepted=false;
            self.store.mutate(id, |run| {
                Self::account(run,&reply);
                if schema_chars_saved>0 {
                    run.tool_schema_chars_avoided=run.tool_schema_chars_avoided.saturating_add(schema_chars_saved as u64);
                    run.event("tool_schema_scoped",format!("Omitted {schema_chars_saved} tool-schema bytes from this request"),None);
                }
                if projection.projected_chars < projection.original_chars {
                    let saved=projection.original_chars-projection.projected_chars;
                    run.context_chars_avoided=run.context_chars_avoided.saturating_add(saved as u64);
                    run.event("context_projected",format!("Model input reduced by {saved} serialized characters"),
                        Some(json!({"source_chars":projection.original_chars,"outbound_chars":projection.projected_chars,"omitted_messages":projection.omitted_messages})));
                }
                if run.terminal() || run.status!=RunStatus::Running {return Ok(());}
                if run.messages.len()!=expected_history {
                    run.event("stale_response","A newer user directive superseded the model response",None);
                    return Ok(());
                }
                if let Err(error)=journal::validate_new_calls(run,&reply.calls){
                    run.steps+=1;
                    run.event("tool_protocol_rejected",format!("Invalid model tool calls: {error}"),None);
                    run.messages.push(json!({"role":"user","content":format!(
                        "Your tool call protocol is invalid: {error}. Retry with fresh unique call IDs, valid JSON object arguments, and at most 16 tool calls."
                    )}));
                    return Ok(());
                }
                accepted=true;
                run.steps += 1;
                if repair_applied{
                    if let Some(advice)=repair{
                        run.event("repair_strategy_applied",
                            format!("Scoped next-step guidance: {}",advice.kind),
                            Some(json!({"strategy":advice.kind,"failure_seq":advice.source_seq,
                                "model_step":run.steps,
                                "input_tokens":reply.input_tokens,
                                "output_tokens":reply.output_tokens})));
                    }
                }
                run.advertised_tools=catalog.clone();
                run.messages.push(reply.message.clone());
                if !reply.calls.is_empty() {
                    run.queued_calls.extend(reply.calls.clone());
                    run.event("tool_requests", format!("{} tool call(s) requested",reply.calls.len()),Some(json!(reply.calls)));
                } else {
                    run.event("candidate","Candidate answer submitted for verification",Some(json!({"text":reply.text})));
                }
                Ok(())
            }).await?;
            if !accepted || !terminal_candidate { continue; }
            let active_ids=self.active.lock().await.clone();
            let active_children=self.store.children_of(id).await.into_iter()
                .filter(|r|!r.terminal()||r.in_flight.is_some()||active_ids.contains(&r.id))
                .map(|r|r.id).collect::<Vec<_>>();
            if !active_children.is_empty() {
                self.store.mutate(id,|run|{
                    if run.terminal()||run.status!=RunStatus::Running{return Ok(());}
                    run.waiting_for_children=true;
                    run.status=RunStatus::Paused;
                    run.messages.push(json!({"role":"user","content":"Delegated workers are not finished. After they complete, check each result, inspect artifacts, reconcile any failures, and explicitly integrate necessary patches before concluding."}));
                    run.event("subagents_wait",format!("Waiting on {} delegated worker(s)",active_children.len()),Some(json!({"children":active_children})));
                    Ok(())
                }).await?;
                return Ok(());
            }
            // The model's final text is not acceptance evidence. Inspect the
            // actual run-id Git worktree before either default completion or
            // the optional paid model auditor. Reuse the existing repair loop:
            // no second planner, tool executor, or automatic shell invocation.
            let artifact=crate::acceptance::inspect(&self.settings,&run).await?;
            if artifact.inspected {
                let observed=self.store.mutate(id,|current|{
                    if current.terminal()||current.status!=RunStatus::Running
                        ||current.messages.len()!=expected_history+1{return Ok(());}
                    current.event(if artifact.satisfied{"artifact_evidence_checked"}
                        else{"artifact_evidence_missing"},artifact.summary.clone(),
                        Some(artifact.json()));
                    if !artifact.satisfied{
                        current.messages.push(json!({"role":"user","content":format!(
                            "Verifier requested corrections. {} Actual changed files: {}.                             You must perform the missing workspace change before reporting completion.",
                            artifact.summary,artifact.changed.join(", "))}));
                        current.event("replan","Deterministic artifact pre-completion check rejected the final answer",None);
                    }
                    Ok(())
                }).await?;
                if observed.terminal(){return Ok(());}
                if observed.messages.len()!=expected_history+1{continue;}
            }
            if !self.settings.separate_model_audit_enabled {
                // External judge is distinct from the harness. Do not pay for
                // another model to rubber-stamp the model's final answer.
                // Tool outcomes are already durable; do not call this verified.
                let finished=self.store.mutate(id,|current|{
                    if current.terminal() || current.status!=RunStatus::Running {
                        return Ok(());
                    }
                    if current.messages.len()!=expected_history+1 {
                        return Ok(());
                    }
                    if reply.text.trim().is_empty(){
                        current.messages.push(json!({"role":"user","content":
                            "The final answer was empty. Continue with tools or provide a concrete result."}));
                        current.event("empty_final","Empty final answer rejected without a second model audit",None);
                        return Ok(());
                    }
                    let receipt_count=current.events.iter()
                        .filter(|e|matches!(e.kind.as_str(),"tool_result"|"tool_error"|"tool_reconciled"))
                        .count();
                    current.answer=Some(reply.text.clone());
                    current.status=RunStatus::Completed;
                    current.event("completed",
                        "Model final submitted; independent model audit disabled (external judge separate)",
                        Some(json!({"audit":"external_or_disabled",
                            "tool_receipts":receipt_count})));
                    Ok(())
                }).await?;
                if finished.status==RunStatus::Completed{return Ok(());}
                continue;
            }
            let latest = self.store.get(id).await.ok_or("execution not found")?;
            if latest.terminal() { return Ok(()); }
            let workspace = self.settings.workspace_dir.join(id.to_string());
            let paths = tools::execute(&self.settings,id,&ToolCall{
                id:"verify-list".into(),name:"list_files".into(),arguments:json!({"path":""})
            }).await.unwrap_or_else(|e|format!("workspace unavailable: {e}"));
            let git_evidence=if latest.repository.is_some(){
                let status=tools::execute(&self.settings,id,&ToolCall{
                    id:"verify-git-status".into(),name:"git_status".into(),arguments:json!({})
                }).await.unwrap_or_else(|e|format!("Git status unavailable: {e}"));
                let diff=tools::execute(&self.settings,id,&ToolCall{
                    id:"verify-git-diff".into(),name:"git_diff".into(),arguments:json!({})
                }).await.unwrap_or_else(|e|format!("Git diff unavailable: {e}"));
                format!("Git status:\n{status}\nGit diff:\n{diff}")
            }else{
                "No source repository attached. Use durable file/tool receipts only.".to_string()
            };
            // The independent auditor sees a bounded, structured receipt
            // view, not a raw, unlimited copy of model-authored history.
            // Direct user changes remain exact; insufficient context pauses
            // before spending tokens on a misleading verdict.
            let verify_messages=self.algorithm.verifier_messages(
                &latest,&workspace.display().to_string(),&paths,
                &git_evidence,&reply.text,self.settings.context_max_chars)?;
            let expected_verify_history=latest.messages.len();
            let selected_model=self.model_for_run(&latest)?;
            self.providers.throttle(&latest.provider_id).await;
            let verdict=tokio::select!{
                biased;
                _=cancellation.changed()=>return Ok(()),
                response=selected_model.complete(&verify_messages,&[],true)=>response?
            };
            let passed = crate::verification::strict_pass(&verdict.text);
            let mut verified=false;
            self.store.mutate(id, |run| {
                Self::account(run,&verdict);
                if run.terminal() || run.status!=RunStatus::Running {return Ok(());}
                if run.messages.len()!=expected_verify_history {
                    run.event("stale_verification","New instructions arrived during verification; verifying again after continuation",None);
                    return Ok(());
                }
                verified=true;
                run.event("verification",verdict.text.clone(),None);
                if passed {
                    run.answer = Some(reply.text.clone());
                    run.status = RunStatus::Completed;
                    run.event("completed","Execution verified and completed",None);
                } else {
                    run.messages.push(json!({"role":"user","content":format!("Verifier requested corrections. Continue the task, use tools as needed, and only finish after the issues are addressed. {}",verdict.text)}));
                    run.event("replan","Verification failed; resume execution with corrective instructions",None);
                }
                Ok(())
            }).await?;
            if verified && passed { return Ok(()); }
        }
    }
    /// Consume remote progress without detaching the HTTP call. Progress is
    /// advisory; failure to persist a progress event never drops an executing
    /// remote mutation and thereby creates an unobserved side effect.
    async fn perform_tool(self: &Arc<Self>, id: Uuid, call: ToolCall,mut cancellation:watch::Receiver<bool>) -> Result<(), String> {
        // Re-apply the current hook immediately before dispatch. This also
        // protects calls approved under an older policy snapshot.
        let snapshot=self.store.get(id).await.ok_or("execution not found")?;
        // Re-evaluate the *current* policy immediately before a persisted
        // queued/approved call can create an external effect. The model-facing
        // schema from an earlier configuration is not sufficient authority.
        if !policy::admitted_tool(&snapshot,&call.name,&self.settings.denied_tools){
            self.store.mutate(id,|run|{
                if run.pending_approval.is_some(){
                    journal::reject_approved_hook(run,&call,"OPERATOR_TOOL_DENIED",
                        "Tool disallowed by current operator policy")
                }else{
                    journal::reject_admission(run,&call,"OPERATOR_TOOL_DENIED",
                        "Tool disallowed by current operator policy")
                }
            }).await?;
            return Ok(());
        }
        let chain=self.hooks.read().await.clone();
        if let Err(deny)=chain.before(&snapshot,&call){
            self.store.mutate(id,|run|{
                if run.pending_approval.is_some(){
                    journal::reject_approved_hook(run,&call,&deny.code,&deny.reason)
                }else{
                    journal::reject_admission(run,&call,&deny.code,&deny.reason)
                }
            }).await?;
            return Ok(());
        }
        // In-flight intent MUST be synced to disk before any outside effect.
        let mut started=false;
        self.store.mutate(id,|run|{
            // A concurrent owner directive can supersede an unstarted tool
            // after this worker selected it. Treat that race as a no-op,
            // never as a failed Run or permission to execute stale work.
            let queued=run.queued_calls.iter().any(|next|
                next.id==call.id && next.name==call.name && next.arguments==call.arguments);
            let pending=run.pending_approval.as_ref().is_some_and(|next|
                next.id==call.id && next.name==call.name && next.arguments==call.arguments);
            if !queued && !pending {return Ok(());}
            journal::begin(run,&call)?;
            started=true;
            Ok(())
        }).await?;
        if !started {return Ok(());}
        let mut remote_uncertain=None;
        // Keep the transport signal independent from the reader cancellation
        // receiver borrowed by tokio::select! below.
        let remote_cancellation=cancellation.clone();
        let execute = async {
            if self.addons.handles_tool(&call.name) {
                // The kernel journals and authorizes all effects; the add-on
                // adapter owns its own protocol, progress and remote IO.
                match self.addons.invoke_external(
                    &self.settings,&self.store,id,&call,remote_cancellation.clone()).await {
                    Ok(ToolEffectOutcome::Settled(result))=>result,
                    Ok(ToolEffectOutcome::Uncertain(reason))=>{
                        remote_uncertain=Some(reason.clone());
                        Err(reason)
                    }
                    Err(error)=>Err(error),
                }
            } else if call.name == "run_program" {
                let mut installed=String::new();
                self.store.mutate(id,|state|{
                    installed=program_session::install(state,&call,&self.settings)?;
                    Ok(())
                }).await?;
                Ok(installed)
            } else if call.name == "read_program" {
                let current=self.store.get(id).await.ok_or("read_program run not found")?;
                crate::read_program::execute_with_hooks(&self.settings,&current,&call.arguments,chain.clone()).await
            } else if call.name == "spawn_subagent" {
                let instruction=call.arguments.get("instruction").and_then(Value::as_str)
                    .ok_or("spawn_subagent needs instruction")?;
                let budget=call.arguments.get("budget_krw").and_then(Value::as_f64);
                let steps=call.arguments.get("max_steps").and_then(Value::as_u64).map(|n|n as usize);
                let read_only=call.arguments.get("read_only").and_then(Value::as_bool).unwrap_or(false);
                let profile=call.arguments.get("profile").and_then(Value::as_str).unwrap_or("general");
                let request=CreateRun{instruction:instruction.into(),budget_krw:budget,max_steps:steps};
                let child=if profile=="general"{
                    self.spawn_subagent_with_policy(id,request,read_only).await
                }else{
                    if read_only && profile!="inspection"{
                        return Err("explicit read_only child requires inspection profile".into());
                    }
                    self.spawn_subagent_with_profile(id,request,profile).await
                };
                child.map(|run|json!({"id":run.id,"status":run.status,
                    "budget_krw":run.budget_krw,"profile":run.delegation_profile}).to_string())
            } else if call.name == "check_subagent" {
                let child=call.arguments.get("id").and_then(Value::as_str).ok_or("subagent id required")?;
                let child=Uuid::parse_str(child).map_err(|e|e.to_string())?;
                self.check_subagent(id,child).await
            } else if call.name == "tool_search" {
                let query=call.arguments.get("query").and_then(Value::as_str)
                    .ok_or("tool_search query required")?;
                let limit=call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
                // Native and external tools share search, current Run
                // permissions, and the exact same subsequent schema admission.
                let mut available=tools::definitions(self.settings.shell_enabled);
                available.extend(self.addons.tool_definitions(&self.settings).await);
                let has_children=self.store.has_children(id).await;
                let (authorized,_)=policy::select_catalog(&snapshot,available,
                    has_children,&self.settings.denied_tools);
                self.algorithm.search_tools(&snapshot,authorized,
                    &self.settings.denied_tools,query,limit)
            } else if call.name == "list_skills" {
                let found=crate::skills::list(&self.settings,id).await?;
                Ok(json!(found).to_string())
            } else if call.name == "load_skill" {
                let name=call.arguments.get("name").and_then(Value::as_str).ok_or("skill name required")?;
                crate::skills::load(&self.settings,id,name).await
            } else if call.name == "read_tool_receipt" {
                let receipt_id=call.arguments.get("call_id").and_then(Value::as_str)
                    .ok_or("receipt call_id required")?;
                let offset=call.arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                let limit=call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(3000) as usize;
                let current=self.store.get(id).await.ok_or("run not found")?;
                crate::context::receipt_slice(&current.messages,receipt_id,offset,limit)
                    .map(|receipt|receipt.to_string())
            } else if call.name == "search_tool_receipts" {
                let query=call.arguments.get("query").and_then(Value::as_str)
                    .ok_or("receipt search query required")?;
                let limit=call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
                let current=self.store.get(id).await.ok_or("run not found")?;
                crate::context::receipt_search(&current.messages,query,limit)
                    .map(|matches|matches.to_string())
            } else if call.name == "memory_search" {
                let query=call.arguments.get("query").and_then(Value::as_str).ok_or("memory search query required")?;
                if query.len()>512{return Err("memory search query too long".into());}
                let run=self.store.get(id).await.ok_or("run not found")?;
                let found=self.knowledge.search(run.repository.as_deref().unwrap_or("global"),query,8).await;
                Ok(json!(found).to_string())
            } else if call.name == "memory_store" {
                let title=call.arguments.get("title").and_then(Value::as_str).ok_or("memory title required")?;
                let content=call.arguments.get("content").and_then(Value::as_str).ok_or("memory content required")?;
                let run=self.store.get(id).await.ok_or("run not found")?;
                let saved=self.knowledge.remember(run.repository.as_deref().unwrap_or("global"),title,content).await?;
                Ok(json!(saved).to_string())
            } else if call.name == "merge_subagent" {
                let child=call.arguments.get("id").and_then(Value::as_str).ok_or("subagent id required")?;
                let child=Uuid::parse_str(child).map_err(|e|e.to_string())?;
                self.merge_subagent(id,child).await
            } else if call.name == "read_subagent_file" {
                let child=call.arguments.get("id").and_then(Value::as_str).ok_or("subagent id required")?;
                let child=Uuid::parse_str(child).map_err(|e|e.to_string())?;
                let path=call.arguments.get("path").and_then(Value::as_str).ok_or("subagent file path required")?;
                self.read_subagent_file(id,child,path).await
            } else {
                tools::execute(&self.settings,id,&call).await
            }
        };
        // Only explicitly pure tools can be aborted cooperatively. Unknown
        // external effects must run until their receipt is durable or require
        // manual reconciliation if the runtime crashes.
        let mut output=if journal::replay_safe(&call.name){
            tokio::select!{
                biased;
                _=cancellation.changed()=>Err("ABORTED: read-only tool cancelled".into()),
                result=execute=>result
            }
        }else{
            execute.await
        };
        if let Some(reason)=remote_uncertain {
            // A dropped Streamable HTTP response is a cancellation signal,
            // NOT proof that the remote side effect was rolled back.
            self.store.mutate(id,|run|journal::mark_remote_unknown(run,&call,&reason)).await?;
            return Ok(());
        }
        chain.after(&snapshot,&call,&mut output);
        // Receipt + dequeuing + cancellation are one durable Store mutation.
        let newly_discovered=if call.name=="tool_search" {
            output.as_ref().ok().map(|body|self.algorithm.names_from_receipt(body))
        }else{None};
        self.store.mutate(id,|run|{
            journal::finish(run,&call,&output,false)?;
            if let Some(names)=&newly_discovered{
                self.algorithm.remember_discovered(run,&snapshot,names);
            }
            Ok(())
        }).await?;
        Ok(())
    }
    /// Records observed outcomes; never re-executes an interrupted tool.
    pub async fn reconcile(&self,id:Uuid,input:ReconcileTool)->Result<Run,String> {
        if self.active.lock().await.contains(&id) {return Err("execution worker is still active".into());}
        if input.result.len()>16_384 {return Err("reconciliation result exceeds 16 KB".into());}
        self.store.mutate(id,|run|{
            if run.status!=RunStatus::Paused{return Err("execution must be paused".into());}
            let call=run.in_flight.clone().ok_or("no in-flight tool to reconcile")?;
            let observed=if input.succeeded{Ok(input.result.clone())}else{Err(input.result.clone())};
            journal::finish(run,&call,&observed,true)
        }).await
    }

    #[cfg(test)]
    pub(crate) async fn worker_active_for_test(&self,id:Uuid)->bool{
        self.active.lock().await.contains(&id)
    }
    pub async fn cancel(&self,id:Uuid) -> Result<Run,String> {
        // Serialize with child creation. If a comparison is cancelled during
        // the first candidate's startup, the next candidate cannot start and
        // no just-created worker escapes the cancellation scan.
        let _delegation=self.delegate_lock.lock().await;
        // Publish durable cancellation BEFORE signalling running futures.
        // This prevents an abort signal from racing ahead of the persisted
        // state transition. Write/MCP tools never get dropped speculatively.
        let snapshot=self.store.mutate(id,|run| {
            if run.terminal() { return Err("execution already finished".into()); }
            if run.in_flight.is_some() || !run.read_batch_in_flight.is_empty(){
                run.status=RunStatus::Paused;
                run.cancel_after_tool=true;
                run.event("cancel_pending_reconciliation",
                    "Cancellation requested during tool execution; final cancellation follows its receipt or manual reconciliation",None);
            }else{
                run.status = RunStatus::Cancelled;
                run.event("cancelled","Execution cancelled by user",None);
            }
            Ok(())
        }).await?;
        if let Some(sender)=self.cancellation.lock().await.get(&id){
            sender.send_replace(true);
        }
        Ok(snapshot)
    }
    pub async fn approve(self: &Arc<Self>, id: Uuid, allow: bool) -> Result<(),String> {
        self.store.mutate(id, |run| {
            if run.status != RunStatus::AwaitingApproval || run.pending_approval.is_none() || run.approval_decision.is_some() {
                return Err("no pending approval".into());
            }
            run.approval_decision = Some(allow);
            run.event("decision",if allow {"User approved tool"} else {"User rejected tool"},None);
            Ok(())
        }).await?;
        // Durable decision is committed before the scheduler starts the call.
        // The channel handles an active worker without timing-dependent failures.
        self.wake_tx.send(id).map_err(|_|"approval wake scheduler unavailable".to_string())?;
        Ok(())
    }
    pub async fn directive(&self,id:Uuid,instruction:String) -> Result<Run,String> {
        let content=instruction.trim();
        if content.is_empty() || content.len()>10_000 {return Err("invalid directive".into());}
        let snapshot=self.store.mutate(id,|run| {
            if run.terminal() {return Err("execution finished".into());}
            // Settle the old model turn before appending a new owner message;
            // providers expect each assistant tool call to have a correlated
            // result, even when the requested operation was never dispatched.
            // Started tool receipts remain under their original Run intent.
            program_session::supersede(run)?;
            journal::supersede_on_directive(run);
            run.messages.push(json!({"role":"user","content":format!("New user instruction: {content}")}));
            run.discovered_tools.clear();
            run.event("directive","User updated instructions",Some(json!({"text":content})));
            Ok(())
        }).await?;
        if snapshot.status==RunStatus::Running &&
            !self.active.lock().await.contains(&id) {
            // Wake the original scheduler rather than creating a second
            // worker. A queued program step may have been superseded while
            // the agent was waiting for explicit approval.
            let _=self.wake_tx.send(id);
        }
        Ok(snapshot)
    }
}

