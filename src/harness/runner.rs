//! Mandatory native Run loop: *not* a replaceable strategy.
//! Strategies choose plans, prompts and tools; the harness alone advances
//! durable Runs and submits approved effects through the kernel.
use crate::{engine::Engine,config::Settings,domain::{Run,RunStatus,ToolCall},
    program_session,policy,journal,tool_schema,tool_batch,tools};
use dominion_agent_core::{LoopStep,TurnSnapshot};
use crate::provider::Reply;
use serde_json::{json,Value};
use std::{sync::Arc,collections::HashSet};
use tokio::sync::watch;
use uuid::Uuid;

impl Engine {
    /// Tool-free chat shares the native worker registry, cancellation channel,
    /// provider routing, budget ledger and crash recovery with agent jobs.
    /// Exactly one billable model turn runs; no tool schemas are offered.
    pub(crate) async fn drive_model_only(&self,id:Uuid,mut cancellation:watch::Receiver<bool>)
        ->Result<(),String>{
        let run=self.store.get(id).await.ok_or("execution not found")?;
        if run.status!=RunStatus::Running||!run.model_only{return Ok(());}
        let messages=self.algorithm.model_only_messages(&run);
        self.store.mutate(id,|current|{
            current.event("model_request_footprint","Model-only chat: no tool schemas",
                Some(json!({"context_chars":messages.iter().map(|m|m.to_string().chars().count()).sum::<usize>(),
                    "history_messages":messages.len(),"tool_count":0,"tool_schema_chars":0,
                    "mode":"model_only"})));
            Ok(())
        }).await?;
        self.services.throttle(&run.provider_id).await;
        let reply=tokio::select!{
            biased;
            _=cancellation.changed()=>return Ok(()),
            answer=self.services.complete(&self.settings,&self.model,
                &run,&messages,&[],false)=>answer
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
    pub(crate) async fn drive(self: &Arc<Self>, id: Uuid, mut cancellation:watch::Receiver<bool>) -> Result<(), String> {
        loop {
            let run = self.store.get(id).await.ok_or("execution not found")?;
            let state=TurnSnapshot{
                running:run.status==RunStatus::Running,
                terminal:run.terminal(),
                approval_pending:run.pending_approval.is_some(),
                program_ready:run.program_session.is_some() && run.queued_calls.is_empty()
                    && run.in_flight.is_none() && run.read_batch_in_flight.is_empty(),
                has_queued_tools:!run.queued_calls.is_empty(),
                plan_exists:run.plan.is_some(),
                separate_planning:self.algorithm.separate_planning(&self.settings,&run),
                steps:run.steps,max_steps:run.max_steps,
            };
            let phase=self.loop_driver.next(state);
            state.admit(phase).map_err(|reason|
                format!("algorithm loop '{}' rejected: {reason}",
                    self.loop_driver.name()))?;
            if phase==LoopStep::Stop{return Ok(())}
            if phase==LoopStep::ResolveApproval {
                let pending=run.pending_approval.clone().ok_or("loop selected missing approval")?;
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
            if phase==LoopStep::AdvanceProgram {
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
                   self.services.requires_approval(&call.name) {
                    self.store.mutate(id, |run| {
                        run.queued_calls.remove(0);
                        run.pending_approval = Some(call.clone());
                        run.approval_decision = None;
                        run.status = RunStatus::AwaitingApproval;
                        run.event("approval_required",
                            if call.name == "shell" { format!("Shell command needs approval: {}",call.arguments.get("command").and_then(Value::as_str).unwrap_or("?")) }
                            else if call.name == "merge_subagent" {format!("Integrating a child Git patch needs approval: {}",call.arguments.get("id").and_then(Value::as_str).unwrap_or("?"))}
                             else if self.services.connected_approval(&call.name) {
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
            if phase==LoopStep::FusePlan {
                // Preserve any already-durable user corrections and avoid a
                // paid preplanning request. The first normal tool-capable model
                // call decides both its plan and first actions.
                self.store.mutate(id, |current| {
                    if current.status!=RunStatus::Running || current.plan.is_some(){return Ok(());}
                    let prepared=self.algorithm.fused_messages(current);
                    current.messages=prepared;
                    current.plan=Some(String::new());
                    current.event("planning_fused",
                        "Initial planning folded into the tool-capable model turn (zero additional model calls)",None);
                    Ok(())
                }).await?;
                continue;
            }
            if phase==LoopStep::RequestPlan {
                // The root instruction is already durable in run.instruction.
                // Never stringify historic tool outputs into an initial plan.
                let messages=self.algorithm.planning_request(&run,self.settings.context_max_chars)?;
                let planning_history_len=run.messages.len();
                self.services.throttle(&run.provider_id).await;
                let reply=tokio::select!{
                    biased;
                    _=cancellation.changed()=>return Ok(()),
                    response=self.services.complete(&self.settings,&self.model,
                        &run,&messages,&[],true)=>response?
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
            if phase==LoopStep::MaxStepsExceeded {
                self.store.mutate(id, |run| {
                    run.status = RunStatus::Failed;
                    run.event("limit",format!("Model round limit ({}) reached",run.max_steps),None);
                    Ok(())
                }).await?;
                return Ok(());
            }
            if phase!=LoopStep::ModelTurn {
                return Err(format!("loop controller selected inconsistent phase: {phase:?}"));
            }
            if run.cost_krw > policy::effective_budget_krw(&run) { return Err("hierarchical budget exceeded".into()); }
            // Authoritative schemas are filtered in the kernel BEFORE a
            // strategy can choose which ones to expose. Returning invented
            // or rewritten schemas cannot grant a capability.
            let native=tools::definitions(self.settings.shell_enabled);
            let external=self.services.tools(&self.settings).await;
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
            Self::admit_algorithm_catalog(&authorized,&presentation.tools)?;
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
            let notes=self.services.focused_notes(&run).await;
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
            self.services.throttle(&run.provider_id).await;
            let reply=tokio::select!{
                biased;
                _=cancellation.changed()=>return Ok(()),
                response=self.services.complete(&self.settings,&self.model,
                    &run,&outbound,&catalog,false)=>response?
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
            self.services.throttle(&latest.provider_id).await;
            let verdict=tokio::select!{
                biased;
                _=cancellation.changed()=>return Ok(()),
                response=self.services.complete(&self.settings,&self.model,
                    &latest,&verify_messages,&[],true)=>response?
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

}
