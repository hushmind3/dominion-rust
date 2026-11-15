use crate::{config::Settings, domain::{Run, RunStatus, ToolCall}, provider::{ModelClient, Reply}, store::Store, tools};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

const SYSTEM: &str = "You are Dominion's autonomous execution engine. The user's instruction is authoritative. Work inside your assigned workspace. Use available tools to produce real, reviewable artifacts. Do not claim a file, command or test was executed unless its tool response confirms it. Keep progress concise. Request shell operations through the shell tool; they require approval. If a tool fails, inspect the error and correct the approach. Finish only when the request is materially done, explaining remaining limitations.";
pub struct Engine {
    pub settings: Settings,
    pub store: Arc<Store>,
    pub groups: Arc<crate::group::Groups>,
    pub videos: Arc<crate::video::Videos>,
    pub improvements: Arc<crate::improvement::Improvements>,
    pub(crate) model: ModelClient,
    active: Mutex<HashSet<Uuid>>,
}
impl Engine {
    pub fn new(settings: Settings, store: Arc<Store>, groups: Arc<crate::group::Groups>, videos: Arc<crate::video::Videos>, improvements: Arc<crate::improvement::Improvements>) -> Result<Arc<Self>, String> {
        let model = ModelClient::new(&settings)?;
        Ok(Arc::new(Self { settings, store, groups, videos, improvements, model, active: Mutex::new(HashSet::new()) }))
    }
    pub async fn launch(self: &Arc<Self>, id: Uuid) -> Result<(), String> {
        {
            let mut guard = self.active.lock().await;
            if !guard.insert(id) { return Err("execution is already active".into()); }
        }
        let started = self.store.mutate(id, |run| {
            if run.terminal() { return Err("cannot restart finished execution".into()); }
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
                        run.status = RunStatus::Failed;
                        run.event("error", error.clone(), None);
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
    async fn drive(&self, id: Uuid) -> Result<(), String> {
        loop {
            let run = self.store.get(id).await.ok_or("execution not found")?;
            if run.terminal() || run.status != RunStatus::Running { return Ok(()); }
            if let Some(pending) = run.pending_approval.clone() {
                let Some(allowed) = run.approval_decision else { return Ok(()); };
                self.perform_tool(id, pending, allowed).await?;
                continue;
            }
            if let Some(call) = run.queued_calls.first().cloned() {
                if call.name == "shell" {
                    self.store.mutate(id, |run| {
                        run.queued_calls.remove(0);
                        run.pending_approval = Some(call.clone());
                        run.approval_decision = None;
                        run.status = RunStatus::AwaitingApproval;
                        run.event("approval_required", format!("Shell command needs approval: {}",call.arguments.get("command").and_then(Value::as_str).unwrap_or("?")), Some(call.arguments.clone()));
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
                    json!({"role":"user","content":run.instruction}),
                ];
                let reply = self.model.complete(&messages,&[],true).await?;
                self.store.mutate(id, |run| {
                    Self::account(run,&reply);
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
            let catalog = tools::definitions(self.settings.shell_enabled);
            let reply = self.model.complete(&run.messages, &catalog, false).await?;
            let terminal_candidate = reply.calls.is_empty();
            self.store.mutate(id, |run| {
                Self::account(run,&reply);
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
            if !terminal_candidate { continue; }
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
            let verdict = self.model.complete(&verify_messages,&[],true).await?;
            let passed = verdict.text.trim().starts_with("PASS");
            self.store.mutate(id, |run| {
                Self::account(run,&verdict);
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
            if passed { return Ok(()); }
        }
    }
    async fn perform_tool(&self, id: Uuid, call: ToolCall, approved: bool) -> Result<(), String> {
        let output = if approved {
            tools::execute(&self.settings,id,&call).await
        } else {
            Err("User denied the requested operation".into())
        };
        self.store.mutate(id,|run| {
            if run.terminal() { return Ok(()); }
            if run.pending_approval.as_ref().is_some_and(|p| p.id == call.id) {
                run.pending_approval = None;
                run.approval_decision = None;
            } else if let Some(position) = run.queued_calls.iter().position(|p| p.id == call.id) {
                run.queued_calls.remove(position);
            }
            let body = match &output { Ok(value) => value.clone(), Err(error) => format!("TOOL ERROR: {error}") };
            run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":body}));
            run.event(if output.is_ok() {"tool_result"} else {"tool_error"},format!("{}: {}",call.name, if output.is_ok() {"ok"}else{"failed"}), Some(json!({"call_id":call.id,"result":body.chars().take(1000).collect::<String>()})));
            Ok(())
        }).await?;
        Ok(())
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
                "id":"call-1","type":"function","function":{"name":"write_file","arguments":"{\\"path\\":\\"result.txt\\",\\"content\\":\\"created by the harness\\"}"}
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
        let server=tokio::spawn(axum::serve(listener,Router::new().route("/chat/completions",post(fake_model)).with_state(count.clone())).into_future());
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
