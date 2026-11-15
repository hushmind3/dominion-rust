use crate::{config::Settings, domain::{CreateRun, ReconcileTool, Run, RunStatus, ToolCall}, provider::{ModelClient, Reply}, store::Store, tools};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

const SYSTEM: &str = "You are Dominion's autonomous execution engine. The user's instruction is authoritative. Work inside your assigned workspace. Use available tools to produce real, reviewable artifacts. Do not claim a file, command or test was executed unless its tool response confirms it. Keep progress concise. Request shell operations through the shell tool; they require approval. If a tool fails, inspect the error and correct the approach. For code changes prefer exact edits and inspect Git diff, then run approved tests before final verification. Finish only when the request is materially done, explaining remaining limitations.";
pub struct Engine {
    pub settings: Settings,
    pub store: Arc<Store>,
    pub groups: Arc<crate::group::Groups>,
    pub videos: Arc<crate::video::Videos>,
    pub improvements: Arc<crate::improvement::Improvements>,
    pub mcp: Arc<crate::mcp::Mcp>,
    pub(crate) model: ModelClient,
    active: Mutex<HashSet<Uuid>>,
    delegate_lock: Mutex<()>,
}
impl Engine {
    pub fn new(settings: Settings, store: Arc<Store>, groups: Arc<crate::group::Groups>, videos: Arc<crate::video::Videos>, improvements: Arc<crate::improvement::Improvements>) -> Result<Arc<Self>, String> {
        let model = ModelClient::new(&settings)?;
        let mcp = Arc::new(crate::mcp::Mcp::from_env()?);
        Ok(Arc::new(Self { settings, store, groups, videos, improvements, mcp, model, active: Mutex::new(HashSet::new()), delegate_lock: Mutex::new(()) }))
    }
    pub async fn launch(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        {
            let mut guard = self.active.lock().await;
            if !guard.insert(id) { return Err("execution is already active".into()); }
        }
        let started = self.store.mutate(id, |run| {
            if run.terminal() { return Err("cannot restart finished execution".into()); }
            if run.in_flight.is_some() { return Err("interrupted tool may have side effects; reconcile before resuming".into()); }
            if run.pending_approval.is_some() && run.approval_decision.is_none() {
                return Err("approval required before resuming".into());
            }
            run.status = RunStatus::Running;
            run.event("resumed", "Execution worker started", None);
            Ok(())
        }).await;
        if let Err(e) = started {
            self.active.lock().await.remove(&id);
            return Err(e);
        }
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = engine.drive(id).await {
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
        });
        Ok(())
    }
    fn account(run: &mut Run, reply: &Reply) {
        run.input_tokens = run.input_tokens.saturating_add(reply.input_tokens);
        run.output_tokens = run.output_tokens.saturating_add(reply.output_tokens);
        run.cost_krw += reply.cost_krw;
        run.event("model", format!("{} · in {} / out {} tokens · {:.4} KRW", reply.model, reply.input_tokens, reply.output_tokens, reply.cost_krw), None);
        if run.cost_krw > run.budget_krw {
            run.status = RunStatus::Failed;
            run.event("budget_exceeded", "Estimated token charges exceeded configured budget", None);
        }
    }
    async fn drive(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        loop {
            let run = self.store.get(id).await.ok_or("execution not found")?;
            if run.terminal() || run.status != RunStatus::Running { return Ok(()); }
            if let Some(pending) = run.pending_approval.clone() {
                let Some(allowed) = run.approval_decision else { return Ok(()); };
                self.perform_tool(id, pending, allowed).await?;
                continue;
            }
            if let Some(call) = run.queued_calls.first().cloned() {
                if call.name == "shell" || call.name.starts_with("mcp__") {
                    self.store.mutate(id, |run| {
                        run.queued_calls.remove(0);
                        run.pending_approval = Some(call.clone());
                        run.approval_decision = None;
                        run.status = RunStatus::AwaitingApproval;
                        run.event("approval_required",
                            if call.name == "shell" { format!("Shell command needs approval: {}",call.arguments.get("command").and_then(Value::as_str).unwrap_or("?")) }
                            else { format!("External MCP tool needs approval: {}",call.name) },
                            Some(call.arguments.clone()));
                        Ok(())
                    }).await?;
                    return Ok(());
                }
                self.perform_tool(id,call,true).await?;
                continue;
            }
            if run.plan.is_none() {
                let messages = vec![
                    json!({"role":"system","content":"Produce a concise actionable numbered plan for the request. Avoid asserting results. This plan is guidance and may change during execution."}),
                    json!({"role":"user","content":format!("{}\n\nAdditional user instructions already received:\n{}",run.instruction,serde_json::to_string(&run.messages).unwrap_or_default())}),
                ];
                let planning_history_len=run.messages.len();
                let reply = self.model.complete(&messages,&[],true).await?;
                self.store.mutate(id, |run| {
                    Self::account(run,&reply);
                    if run.terminal() || run.status!=RunStatus::Running {return Ok(());}
                    if run.messages.len()!=planning_history_len {
                        run.event("stale_plan","New user directive arrived during planning; recomputing",None);
                        return Ok(());
                    }
                    run.plan = Some(reply.text.clone());
                    run.messages = vec![
                        json!({"role":"system","content":SYSTEM}),
                        json!({"role":"user","content":format!("Request:\n{}\n\nInitial plan:\n{}\n\nProceed with tools and adapt the plan as evidence arrives.",run.instruction,reply.text)}),
                    ];
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
            if run.cost_krw > run.budget_krw { return Err("budget exceeded".into()); }
            let mut catalog = tools::definitions(self.settings.shell_enabled);
            catalog.extend(self.mcp.definitions().await);
            let projection=crate::context::compact(&run.messages,self.settings.context_max_chars);
            let expected_history=run.messages.len();
            let reply = self.model.complete(&projection.messages, &catalog, false).await?;
            let terminal_candidate = reply.calls.is_empty();
            let mut accepted=false;
            self.store.mutate(id, |run| {
                Self::account(run,&reply);
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
                accepted=true;
                run.steps += 1;
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
            let latest = self.store.get(id).await.ok_or("execution not found")?;
            if latest.terminal() { return Ok(()); }
            let workspace = self.settings.workspace_dir.join(id.to_string());
            let paths = tools::execute(&self.settings,id,&ToolCall{
                id:"verify-list".into(),name:"list_files".into(),arguments:json!({"path":""})
            }).await.unwrap_or_else(|e|format!("workspace unavailable: {e}"));
            let verify_messages = vec![
                json!({"role":"system","content":"You are the execution verifier. Return only PASS or REVISE: followed by the concrete missing work. Use the actual tool history and workspace file list; do not assume unexecuted actions happened. Evaluate completion against the user request."}),
                json!({"role":"user","content":format!("Original request:\n{}\nWorkspace: {}\nFiles: {}\nExecution messages:\n{}\nCandidate answer:\n{}",latest.instruction,workspace.display(),paths,serde_json::to_string(&latest.messages).unwrap_or_default().chars().rev().take(24000).collect::<String>().chars().rev().collect::<String>(),reply.text)}),
            ];
            let expected_verify_history=latest.messages.len();
            let verdict = self.model.complete(&verify_messages,&[],true).await?;
            let passed = verdict.text.trim().starts_with("PASS");
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
    async fn perform_tool(self: &Arc<Self>, id: Uuid, call: ToolCall, approved: bool) -> Result<(), String> {
        // Persist intent before an external side effect; receipt is a separate durable write.
        self.store.mutate(id, |run| {
            if run.status != RunStatus::Running { return Err("execution no longer running".into()); }
            if run.in_flight.is_some() { return Err("another tool is in flight".into()); }
            if !run.queued_calls.iter().any(|queued| queued.id == call.id)
                && !run.pending_approval.as_ref().is_some_and(|pending| pending.id == call.id) {
                return Err("tool is no longer queued".into());
            }
            run.in_flight = Some(call.clone());
            run.event("tool_started", format!("Executing {} ({})",call.name,call.id),
                Some(json!({"name":call.name,"id":call.id})));
            Ok(())
        }).await?;
        let output = if approved {
            if call.name.starts_with("mcp__") {
                self.mcp.call(&call).await
            } else if call.name == "spawn_subagent" {
                let instruction=call.arguments.get("instruction").and_then(Value::as_str)
                    .ok_or("spawn_subagent needs instruction")?;
                let budget=call.arguments.get("budget_krw").and_then(Value::as_f64);
                let steps=call.arguments.get("max_steps").and_then(Value::as_u64).map(|n|n as usize);
                self.spawn_subagent(id,CreateRun{instruction:instruction.into(),budget_krw:budget,max_steps:steps})
                    .await.map(|run|json!({"id":run.id,"status":run.status,"budget_krw":run.budget_krw}).to_string())
            } else if call.name == "check_subagent" {
                let child=call.arguments.get("id").and_then(Value::as_str).ok_or("subagent id required")?;
                let child=Uuid::parse_str(child).map_err(|e|e.to_string())?;
                self.check_subagent(id,child).await
            } else {
                tools::execute(&self.settings,id,&call).await
            }
        } else {
            Err("User denied the requested operation".into())
        };
        self.store.mutate(id,|run| {
            if !run.in_flight.as_ref().is_some_and(|pending|pending.id == call.id) {
                return Err("in-flight tool receipt does not match".into());
            }
            run.in_flight = None;
            if run.pending_approval.as_ref().is_some_and(|p| p.id == call.id) {
                run.pending_approval = None;
                run.approval_decision = None;
            } else if let Some(pos) = run.queued_calls.iter().position(|p|p.id == call.id) {
                run.queued_calls.remove(pos);
            }
            let body=match &output {Ok(v)=>v.clone(),Err(e)=>format!("TOOL ERROR: {e}")};
            if !run.terminal() {run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":body}));}
            run.event(if output.is_ok() {"tool_result"}else{"tool_error"},
                format!("{}: {}",call.name,if output.is_ok() {"ok"}else{"failed"}),
                Some(json!({"call_id":call.id,"result":body.chars().take(1000).collect::<String>()})));
            Ok(())
        }).await?;
        Ok(())
    }
    /// Records observed outcomes; never re-executes an interrupted tool.
    pub async fn reconcile(&self,id:Uuid,input:ReconcileTool)->Result<Run,String> {
        if self.active.lock().await.contains(&id) {return Err("execution worker is still active".into());}
        if input.result.len()>16_384 {return Err("reconciliation result exceeds 16 KB".into());}
        self.store.mutate(id,|run|{
            if run.status!=RunStatus::Paused {return Err("execution must be paused".into());}
            let call=run.in_flight.take().ok_or("no in-flight tool to reconcile")?;
            if run.pending_approval.as_ref().is_some_and(|p|p.id==call.id) {
                run.pending_approval=None;run.approval_decision=None;
            } else if let Some(index)=run.queued_calls.iter().position(|p|p.id==call.id) {
                run.queued_calls.remove(index);
            }
            let body=if input.succeeded {input.result.clone()}else{format!("TOOL ERROR: {}",input.result)};
            run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":body}));
            run.event("tool_reconciled",
                format!("Observed result for {}: {}",call.name,if input.succeeded{"success"}else{"failure"}),
                Some(json!({"call_id":call.id,"succeeded":input.succeeded})));
            Ok(())
        }).await
    }

    /// Native delegated work: separate durable transcript and workspace.
    /// Budget reservations across siblings prevent oversubscription.
    pub async fn spawn_subagent(self:&Arc<Self>,parent:Uuid,input:CreateRun)->Result<Run,String>{
        let _guard=self.delegate_lock.lock().await;
        let root=self.store.get(parent).await.ok_or("parent execution not found")?;
        if root.terminal(){return Err("parent already terminated".into());}
        let all=self.store.list().await;
        let children=all.iter().filter(|r|r.parent_run_id==Some(parent)).collect::<Vec<_>>();
        if children.len()>=4{return Err("maximum of four direct subagents per run".into());}
        let reserved=children.iter().map(|r|r.budget_krw).sum::<f64>();
        let remaining=(root.budget_krw-root.cost_krw-reserved).max(0.0);
        let budget=input.budget_krw.unwrap_or(remaining.min(2000.0));
        if !budget.is_finite()||budget<0.0||budget>remaining{
            return Err(format!("subagent budget {budget} KRW exceeds unreserved {remaining} KRW"));
        }
        let mut child=Run::new(CreateRun{
            instruction:input.instruction,
            budget_krw:Some(budget),
            max_steps:input.max_steps
        },self.settings.max_steps)?;
        child.parent_run_id=Some(parent);
        child.event("delegated",format!("Delegated by parent {parent} with {budget} KRW reserved"),None);
        self.store.put(child.clone()).await?;
        self.launch(child.id).await?;
        self.store.get(child.id).await.ok_or("subagent missing after spawn".into())
    }

    /// A parent can observe only its own children; never return another run.
    pub async fn check_subagent(&self,parent:Uuid,child:Uuid)->Result<String,String>{
        let run=self.store.get(child).await.ok_or("subagent not found")?;
        if run.parent_run_id!=Some(parent){return Err("subagent does not belong to this parent".into());}
        Ok(json!({
            "id":run.id,
            "status":run.status,
            "answer":run.answer,
            "steps":run.steps,
            "cost_krw":run.cost_krw,
            "error":run.events.iter().rev().find(|e|e.kind=="error"||e.kind=="paused_error").map(|e|&e.message)
        }).to_string())
    }

    pub async fn cancel(&self,id:Uuid) -> Result<Run,String> {
        self.store.mutate(id,|run| {
            if run.terminal() { return Err("execution already finished".into()); }
            run.status = RunStatus::Cancelled;
            run.event("cancelled","Execution cancelled by user",None);
            Ok(())
        }).await
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
        // The worker may still be releasing its active marker after publishing
        // the approval request. Wait for that exact worker to detach first.
        for _ in 0..200 {
            if !self.active.lock().await.contains(&id) { return self.launch(id).await; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Err("approval recorded but worker is still active; use resume".into())
    }
    pub async fn directive(&self,id:Uuid,instruction:String) -> Result<Run,String> {
        let content=instruction.trim();
        if content.is_empty() || content.len()>10_000 {return Err("invalid directive".into());}
        self.store.mutate(id,|run| {
            if run.terminal() {return Err("execution finished".into());}
            run.messages.push(json!({"role":"user","content":format!("New user instruction: {content}")}));
            run.event("directive","User updated instructions",Some(json!({"text":content})));
            Ok(())
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize,Ordering};
    async fn fake_model(State(turn):State<Arc<AtomicUsize>>, Json(_request):Json<Value>)->Json<Value>{
        let current=turn.fetch_add(1,Ordering::SeqCst);
        let message=match current {
            0=>json!({"role":"assistant","content":"1. Write output file. 2. Verify."}),
            1=>json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"call-1","type":"function","function":{"name":"write_file","arguments":json!({"path":"result.txt","content":"created by the harness"}).to_string()}
            }]}),
            2=>json!({"role":"assistant","content":"Created result.txt containing the requested artifact."}),
            _=>json!({"role":"assistant","content":"PASS"}),
        };
        Json(json!({"choices":[{"message":message}],"usage":{"prompt_tokens":20,"completion_tokens":10}}))
    }
    #[tokio::test]
    async fn completes_real_plan_tool_verify_cycle() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let count=Arc::new(AtomicUsize::new(0));
        let count_for_server=Arc::clone(&count);
        let server=tokio::spawn(async move { axum::serve(listener,Router::new().route("/chat/completions",post(fake_model)).with_state(count_for_server)).await.unwrap(); });
        let dir=std::env::temp_dir().join(format!("dominion-cycle-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=dir.join("data");
        settings.workspace_dir=dir.join("workspace");
        settings.main.base_url=format!("http://{addr}");
        settings.main.name="test-model".into();
        settings.main.api_key.clear();
        settings.economy=settings.main.clone();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
        let videos=Arc::new(crate::video::Videos::open(settings.data_dir.join("videos")).await.unwrap());
        let improvements=Arc::new(crate::improvement::Improvements::open(settings.data_dir.join("improvements"),settings.workspace_dir.clone()).await.unwrap());
        let engine=Engine::new(settings.clone(),store.clone(),groups,videos,improvements).unwrap();
        let run=Run::new(crate::domain::CreateRun{instruction:"Write result.txt".into(),budget_krw:Some(1000.0),max_steps:Some(8)},8).unwrap();
        let id=run.id;store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        let mut result=None;
        for _ in 0..80 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let snapshot=store.get(id).await.unwrap();
            if snapshot.terminal() {result=Some(snapshot);break;}
        }
        let result=result.expect("execution timed out");
        assert_eq!(result.status,RunStatus::Completed,"events: {:?}",result.events);
        assert_eq!(tokio::fs::read_to_string(settings.workspace_dir.join(id.to_string()).join("result.txt")).await.unwrap(),"created by the harness");
        assert!(result.events.iter().any(|e|e.kind=="verification"));
        assert!(count.load(Ordering::SeqCst)>=4);
        server.abort();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::{domain::{CreateRun, ReconcileTool}, group::Groups, improvement::Improvements, video::Videos};
    #[tokio::test]
    async fn uncertain_tool_is_not_replayed_without_reconciliation() {
        let root=std::env::temp_dir().join(format!("dominion-reconcile-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        tokio::fs::create_dir_all(&settings.workspace_dir).await.unwrap();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());
        let videos=Arc::new(Videos::open(settings.data_dir.join("videos")).await.unwrap());
        let improvements=Arc::new(Improvements::open(settings.data_dir.join("improvements"),settings.workspace_dir.clone()).await.unwrap());
        let engine=Engine::new(settings,Arc::clone(&store),groups,videos,improvements).unwrap();
        let call=ToolCall{id:"call-1".into(),name:"write_file".into(),arguments:json!({"path":"a.txt","content":"done"})};
        let mut run=Run::new(CreateRun{instruction:"Write a file".into(),budget_krw:Some(10.0),max_steps:None},5).unwrap();
        let id=run.id;
        run.status=RunStatus::Paused;
        run.in_flight=Some(call.clone());
        run.queued_calls.push(call);
        store.put(run).await.unwrap();
        assert!(engine.launch(id).await.unwrap_err().contains("reconcile"));
        let reconciled=engine.reconcile(id,ReconcileTool{succeeded:true,result:"confirmed".into()}).await.unwrap();
        assert_eq!(reconciled.status,RunStatus::Paused);
        assert!(reconciled.in_flight.is_none());
        assert!(reconciled.queued_calls.is_empty());
        assert_eq!(reconciled.messages.last().unwrap()["tool_call_id"],"call-1");
        assert!(reconciled.events.iter().any(|event|event.kind=="tool_reconciled"));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
