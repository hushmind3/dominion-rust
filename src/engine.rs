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
    pub(crate) algorithm:Arc<dyn AgentAlgorithm>,
    pub(crate) loop_driver:Arc<dyn dominion_agent_core::AgentLoop>,
    /// All GUI, memory, account, room and model-catalog services live outside
    /// the Run kernel. The Deref compatibility surface does not own a runner.
    product: crate::product::ProductHost,

    pub(crate) hooks: RwLock<HookChain>,
    pub(crate) model: ModelClient,
    pub(crate) active: Mutex<HashSet<Uuid>>,
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
        Self::from_ports_with_loop(settings,store,product,algorithm,
            Arc::new(dominion_agent_algo::ReActLoop))
    }
    pub(crate) fn from_ports_with_loop(settings:Settings,store:Arc<crate::ports::RunStorePort>,
        product:crate::product::ProductHost,algorithm:Arc<dyn AgentAlgorithm>,
        loop_driver:Arc<dyn dominion_agent_core::AgentLoop>)->Result<Arc<Self>,String>{
        let model=ModelClient::new(&settings)?;
        let (wake_tx,mut wake_rx)=mpsc::unbounded_channel::<Uuid>();
        let hooks=RwLock::new(HookChain::from_settings(&settings));
        let engine=Arc::new(Self { settings, store, product, algorithm, loop_driver, hooks,
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
    /// A pure algorithm can only omit existing, operator-authorized schemas.
    /// The runtime—not the strategy—owns the admission boundary.
    pub(crate) fn admit_algorithm_catalog(authorized:&[Value],proposed:&[Value])->Result<(),String>{
        let mut seen=HashSet::<String>::new();
        if proposed.iter().any(|tool|{
            let Some(name)=tool.pointer("/function/name").and_then(Value::as_str)
                else{return true;};
            !authorized.contains(tool)||!seen.insert(name.into())
        }){
            return Err("algorithm attempted to advertise an unauthorized or duplicate tool schema".into());
        }
        Ok(())
    }
    /// Expose the pinned algorithm identity for diagnostics without exposing
    /// strategy internals, model credentials or mutable runtime state.
    pub fn algorithm_id(&self)->&str{self.algorithm.id()}
    /// Stable identifier of the effect-free workflow loop controller.
    pub fn loop_id(&self)->&str{self.loop_driver.name()}
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
            if !run.model_only{
                if let Some(binding)=run.events.iter().find(|event|event.kind=="loop_bound"){
                    if binding.message!=self.loop_driver.name(){
                        return Err(format!(
                            "agent loop changed from {} to {}; resume requires its original controller",
                            binding.message,self.loop_driver.name()));
                    }
                }else{
                    run.event("loop_bound",self.loop_driver.name(),None);
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
    /// Consume remote progress without detaching the HTTP call. Progress is
    /// advisory; failure to persist a progress event never drops an executing
    /// remote mutation and thereby creates an unobserved side effect.
    pub(crate) async fn perform_tool(self: &Arc<Self>, id: Uuid, call: ToolCall,mut cancellation:watch::Receiver<bool>) -> Result<(), String> {
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

