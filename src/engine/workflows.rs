use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet_outcome, verification};
use serde_json::{json, Value};

#[path = "workflows/repair.rs"]
mod repair;

/// Sequential workflow runner. Each step is a real Rust engine turn, the
/// previous step's output is substituted into `{{previous}}`, and progress is
/// persisted on the job so a restart can resume from the last finished step.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let workflow_id = input
        .get("workflowId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let payload = input
        .get("input")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .chars()
        .take(16_000)
        .collect::<String>();

    let workflow = context
        .state
        .store
        .get("workflow", workflow_id)
        .ok_or_else(|| "워크플로를 찾을 수 없습니다.".to_string())?;
    let existing = job
        .get("workflowState")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if existing.get("inFlight").and_then(Value::as_bool) == Some(true) {
        return Ok(
            json!({"workflowId":workflow_id,"sessionId":existing.get("sessionId"),"status":"needs_review","reason":"중단된 단계의 도구 실행 여부를 확인해야 합니다."}),
        );
    }
    let definition = existing
        .get("definition")
        .cloned()
        .unwrap_or_else(|| workflow.clone());
    let steps = definition
        .get("steps")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if steps.is_empty() {
        return Err("워크플로에 단계가 없습니다.".into());
    }

    let state = &context.state;
    let mut outputs = existing
        .get("outputs")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let session_id = match existing.get("sessionId").and_then(|value| value.as_str()) {
        Some(id) if state.store.get("session", id).is_some() => id.to_string(),
        _ => {
            let name = workflow
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("Workflow");
            let created = state
                .store
                .put(
                    "session",
                    &json!({
                        "title": format!("Workflow · {name}"),
                        "agentId": steps.first().and_then(|step| step.get("agentId")),
                    }),
                )
                .ok_or("워크플로 세션을 만들 수 없습니다.")?;
            created
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        }
    };

    let total = steps.len();
    if outputs.len() > total {
        return Err("워크플로 체크포인트 단계 수가 실행 계획과 다릅니다.".into());
    }
    for (index, output) in outputs.iter().enumerate() {
        if output.get("stepId") != steps[index].get("id") {
            return Err("워크플로 체크포인트 단계 ID가 변경되었습니다.".into());
        }
    }
    context.checkpoint(json!({
        "sessionId": session_id,
        "workflowState": { "sessionId": session_id, "outputs": outputs, "definition": definition, "inFlight": false },
    }))?;
    for index in outputs.len()..total {
        if state.is_cancelled(&session_id) {
            return Ok(
                json!({"workflowId":workflow_id,"sessionId":session_id,"outputs":outputs,"status":"cancelled"}),
            );
        }
        let step = &steps[index];
        let step_name = step
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("Step");
        context.emit(json!({
            "type": "workflow_step_start",
            "stepIndex": index,
            "stepName": step_name,
            "stepTotal": total,
        }));

        let previous = outputs
            .last()
            .and_then(|output| output.get("content"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let template = step
            .get("prompt")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let prompt = render(template, &payload, previous);
        let checks=step.get("acceptanceChecks").cloned().unwrap_or_else(||json!([]));
        verification::validate_criteria(&checks)?;
        if checks.as_array().is_some_and(|rows|rows.len()>8){
            return Err("워크플로 단계별 검증 조건은 최대 8개입니다.".into());
        }
        let prompt=if checks.as_array().is_some_and(|rows|!rows.is_empty()){
            format!("{prompt}\n\n[단계 산출물 검증 조건 · 실제 도구 실행과 검증 필요]\n{}",
                serde_json::to_string_pretty(&checks).unwrap_or_default())
        }else{prompt};
        if prompt.trim().is_empty() {
            return Err(format!("{step_name} 단계의 지시문이 비어 있습니다."));
        }

        let verified_step=checks.as_array().is_some_and(|rows|!rows.is_empty());
        let max_attempts=repair::max_attempts(step,verified_step);
        let before_usage=state.store.session_usage(&session_id);
        let mut evidence=Vec::<Value>::new();
        let mut successful_message:Option<Value>=None;
        let mut previous_failures=Vec::<String>::new();
        let mut repair_history=Vec::<Value>::new();
        let mut tool_error_count=0usize;
        let mut attempts=0usize;
        for attempt in 1..=max_attempts {
            if state.is_cancelled(&session_id){
                return Ok(json!({"workflowId":workflow_id,"sessionId":session_id,
                    "outputs":outputs,"status":"cancelled"}));
            }
            let attempt_prompt=repair::repair_prompt(&prompt,attempt,max_attempts,&previous_failures);
            // Persist an in-flight receipt *before* invoking another model or
            // tool. Process crashes cannot blindly replay a side effect.
            context.checkpoint(json!({
                "workflowState":{"sessionId":session_id,"outputs":outputs,
                    "definition":definition,"inFlight":true,
                    "activeStepIndex":index,"activeAttempt":attempt,
                    "repairHistory":repair_history},
            }))?;
            let request=TurnRequest{
                session_id:Some(session_id.clone()),
                input:attempt_prompt,
                agent_id:optional(step,"agentId"),
                provider_id:optional(step,"providerId"),
                model:optional(step,"model"),
                attachment_ids:Vec::new(),
                tools_enabled:true,tools_execution_enabled:true,
                thinking:None,auto_learning:false,
                reasoning_effort:None,reasoning_language:None,
                system_override:Some(format!(
                    "You are executing step {} of the workflow \"{}\". \
                     Complete the original user task, using tools for actual \
                     artifact changes and evidence.",
                    index+1,workflow.get("name").and_then(Value::as_str).unwrap_or("Workflow")
                )),
                task_id:None,parent_task_id:None,task_revision:None,
                // One owner of verification: the workflow's Rust gate.
                // Avoid a second, conflicting model-based chat review.
                verification_owner:verified_step.then(||"parent".to_string()),
            };
            let (turn,stats)=run_quiet_outcome(state,&request).await;
            attempts=attempt;
            tool_error_count+=stats.tool_errors.len();
            for item in &stats.evidence {
                let id=item.get("id").and_then(Value::as_str);
                if id.is_none() || !evidence.iter().any(|old|
                    old.get("id").and_then(Value::as_str)==id){
                    evidence.push(item.clone());
                }
            }
            if state.is_cancelled(&session_id){
                return Ok(json!({"workflowId":workflow_id,"sessionId":session_id,
                    "outputs":outputs,"status":"cancelled"}));
            }
            let unknown=match state.store.session_has_uncertain_effects(&session_id){
                Ok(value)=>value,
                Err(error)=>{
                    previous_failures=vec![format!("도구 상태 조회 실패: {error}")];
                    true
                }
            };
            if unknown {
                if previous_failures.is_empty(){
                    previous_failures.push("외부 도구 결과가 미확정입니다. 기존 결과를 확인해야 합니다.".to_string());
                }
                context.checkpoint(json!({
                    "workflowState":{"sessionId":session_id,"outputs":outputs,
                        "definition":definition,"inFlight":false,
                        "failedStepIndex":index,"activeAttempt":attempt,
                        "verificationGaps":previous_failures,
                        "repairHistory":repair_history},
                    "stepIndex":index,"stepTotal":total
                }))?;
                state.store.audit("workflow.step_needs_review",json!({
                    "workflowId":workflow_id,"jobId":context.job_id,
                    "stepIndex":index,"attempts":attempt,
                    "gaps":previous_failures,"unknownEffects":true
                }));
                return Ok(json!({"workflowId":workflow_id,"sessionId":session_id,
                    "outputs":outputs,"status":"needs_review",
                    "failedStepIndex":index,"attempts":attempt,
                    "verificationGaps":previous_failures}));
            }
            // Re-read real files after each turn. Previous successful tool
            // effects count; a model's prose never clears a predicate.
            let mut gaps=if verified_step {
                verification::acceptance_gaps_for_workspace(
                    state,&state.config.workspace,&prompt,&checks,&evidence)
            }else{Vec::new()};
            let turn_message=match turn{
                Ok(Some(message))=>Some(message),
                Ok(None)=>{
                    gaps.push(format!("{step_name} 단계가 빈 응답을 반환했습니다."));
                    None
                }
                Err(error)=>{
                    gaps.push(format!("모델/도구 실행 오류: {}",error.chars().take(600).collect::<String>()));
                    None
                }
            };
            // A later failed tool is relevant feedback if the requested
            // artifacts are still missing; verified output wins otherwise.
            if verified_step&&!gaps.is_empty(){
                gaps.extend(stats.tool_errors.iter().take(5).map(|err|
                    format!("실패한 도구: {}",err.chars().take(450).collect::<String>())));
            }
            if gaps.is_empty(){
                successful_message=turn_message;
                break;
            }
            if !verified_step {
                return Err(gaps.join("\n"));
            }
            repair_history.push(json!({
                "attempt":attempt,"verificationGaps":gaps,
                "toolErrorCount":stats.tool_errors.len(),
                "recordedModelCalls":state.store.session_usage(&session_id)["calls"],
            }));
            if attempt==max_attempts {
                context.checkpoint(json!({
                    "workflowState":{"sessionId":session_id,"outputs":outputs,
                        "definition":definition,"inFlight":false,
                        "failedStepIndex":index,"activeAttempt":attempt,
                        "verificationGaps":gaps,"repairHistory":repair_history},
                    "stepIndex":index,"stepTotal":total
                }))?;
                state.store.audit("workflow.step_needs_review",json!({
                    "workflowId":workflow_id,"jobId":context.job_id,
                    "stepIndex":index,"attempts":attempt,"gaps":gaps,
                    "unknownEffects":false,
                }));
                return Ok(json!({"workflowId":workflow_id,"sessionId":session_id,
                    "outputs":outputs,"status":"needs_review",
                    "failedStepIndex":index,"attempts":attempt,
                    "verificationGaps":gaps,"repairHistory":repair_history}));
            }
            // Continue the same agent session with bounded, concrete feedback,
            // instead of abandoning the owner's task after the first miss.
            previous_failures=gaps;
            context.emit(json!({"type":"workflow_step_repair",
                "stepIndex":index,"stepName":step_name,
                "attempt":attempt+1,"maxAttempts":max_attempts,
                "remainingGaps":previous_failures.len()}));
        }
        let message=successful_message.ok_or_else(||
            format!("{step_name} 단계가 완료 응답을 반환하지 않았습니다."))?;
        let after_usage=state.store.session_usage(&session_id);
        let usage_delta=|key:&str|after_usage[key].as_u64().unwrap_or(0)
            .saturating_sub(before_usage[key].as_u64().unwrap_or(0));
        let proof=json!({
            "status":if checks.as_array().is_some_and(|rows|!rows.is_empty()){
                "verified"
            }else{"unchecked"},
            "criteriaCount":checks.as_array().map_or(0,Vec::len),
            "attempts":attempts,
            "repairAttempts":attempts.saturating_sub(1),
            "repairHistory":repair_history,
            "operationIds":evidence.iter().filter_map(|v|v["id"].as_str())
                .collect::<Vec<_>>(),
            "toolErrorCount":tool_error_count,
            "modelCalls":usage_delta("calls"),
            "totalTokens":usage_delta("totalTokens"),
            "usageEstimated":after_usage["estimatedCalls"].as_u64().unwrap_or(0)>
                before_usage["estimatedCalls"].as_u64().unwrap_or(0),
        });
        outputs.push(json!({
            "stepId": step.get("id"),
            "name": step_name,
            "agentId":step.get("agentId"),
            "content": message.get("content").cloned().unwrap_or(json!("")),
            "messageId": message.get("id"),
            "verification":proof,
        }));

        context.checkpoint(json!({
            "stepIndex": index + 1,
            "stepTotal": total,
            "workflowState": { "sessionId": session_id, "outputs": outputs, "definition": definition, "inFlight": false },
        }))?;
        context.emit(json!({
            "type": "workflow_step_done",
            "stepIndex": index,
            "stepName": step_name,
            "stepTotal": total,
        }));
    }

    state.store.audit(
        "workflow.completed",
        json!({ "workflowId": workflow_id, "sessionId": session_id, "steps": outputs.len() }),
    );

    // A workflow is a real multi-agent execution path. Its result must feed the
    // same semantic learning pipeline as chat and rooms, not only tool logs.
    let status = if state.is_cancelled(&session_id) {
        "cancelled"
    } else {
        "completed"
    };
    if status == "completed" {
        let through = state
            .store
            .messages(&session_id)
            .last()
            .and_then(|message| message["id"].as_str().map(str::to_string));
        let mut learned = std::collections::HashSet::new();
        for step in steps.iter().take(4) {
            let Some(agent_id) = step["agentId"].as_str().filter(|id| !id.is_empty()) else {
                continue;
            };
            if !learned.insert(agent_id.to_string()) {
                continue;
            }
            let agent = state.store.get("agent", agent_id);
            let provider_id = step["providerId"]
                .as_str()
                .or_else(|| agent.as_ref().and_then(|agent| agent["providerId"].as_str()))
                .unwrap_or("");
            let model = step["model"]
                .as_str()
                .or_else(|| agent.as_ref().and_then(|agent| agent["model"].as_str()))
                .unwrap_or("");
            crate::engine::learning::enqueue_semantic_for_agent_through(
                state,
                &session_id,
                Some(agent_id),
                provider_id,
                model,
                through.as_deref(),
            );
        }
    }

    Ok(json!({
        "workflowId": workflow_id,
        "sessionId": session_id,
        "outputs": outputs,
        "verificationSummary":{
            "verifiedSteps":outputs.iter().filter(|v|v["verification"]["status"]=="verified").count(),
            "uncheckedSteps":outputs.iter().filter(|v|v["verification"]["status"]=="unchecked").count(),
            "totalTokensRecorded":state.store.session_usage(&session_id)["totalTokens"],
            "usage":state.store.session_usage(&session_id),
        },
        "status": status,
    }))
}

fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

/// `{{input}}` is the run payload, `{{previous}}` the last step output.
pub fn render(template: &str, input: &str, previous: &str) -> String {
    // Substitute only placeholders in the template. User input and prior
    // output may themselves contain braces and must stay literal.
    let mut rendered = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        rendered.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(next) = rest.strip_prefix("{{input}}") {
            rendered.push_str(input);
            rest = next;
        } else if let Some(next) = rest.strip_prefix("{{previous}}") {
            rendered.push_str(previous);
            rest = next;
        } else {
            rendered.push_str("{{");
            rest = &rest[2..];
        }
    }
    rendered.push_str(rest);
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,state::AppState,vault::Vault};
    use crate::engine::jobs::JobContext;
    use axum::{extract::State,response::{IntoResponse,Response},routing::post,Json,Router};
    use std::sync::{atomic::{AtomicUsize,Ordering},Arc};

    fn fixture()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-flow-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("policy",&json!({"id":"default","autoApproveAll":true})).unwrap();
        (state,dir)
    }
    #[derive(Clone)]
    struct Mock {
        tool:&'static str,
        calls:Arc<AtomicUsize>,
    }
    async fn mock_api(State(mock):State<Mock>,Json(body):Json<Value>)->Response{
        mock.calls.fetch_add(1,Ordering::SeqCst);
        if body["stream"]!=true{
            return Json(json!({"choices":[{"message":{"content":"1. Execute the step."}}],
                "usage":{"prompt_tokens":12,"completion_tokens":6,"total_tokens":18}})).into_response();
        }
        let has_tool_result=body["messages"].as_array().is_some_and(|messages|
            messages.iter().any(|v|v["role"]=="tool"));
        let delta=if has_tool_result{
            json!({"content":"Completed with a saved artifact."})
        }else{
            let args=if mock.tool=="workspace_write"{
                json!({"path":"verified.txt","content":"EXPECTED"})
            }else{json!({"path":"existing.txt"})};
            json!({"tool_calls":[{"index":0,"id":"flow-call",
                "type":"function","function":{"name":mock.tool,
                "arguments":args.to_string()}}]})
        };
        let stream=format!("data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":delta}]}));
        ([("content-type","text/event-stream")],stream).into_response()
    }
    async fn execute_with_check(tool:&'static str)->(Value,AppState,std::path::PathBuf,usize){
        let (state,dir)=fixture();
        std::fs::write(dir.join("existing.txt"),"readable").unwrap();
        let calls=Arc::new(AtomicUsize::new(0));
        let app=Router::new().route("/v1/chat/completions",post(mock_api))
            .with_state(Mock{tool,calls:calls.clone()});
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let server=tokio::spawn(async move {axum::serve(listener,app).await.unwrap()});
        state.store.put("provider",&json!({"id":"mock","name":"mock",
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"test","enabled":true,"toolMode":"native"})).unwrap();
        state.store.put("agent",&json!({"id":"worker","name":"worker",
            "providerId":"mock","model":"test"})).unwrap();
        state.store.put("workflow",&json!({"id":"flow","name":"verified workflow",
            "steps":[
                {"id":"step-1","name":"Artifact","agentId":"worker",
                "providerId":"mock","model":"test",
                "prompt":"Create verified.txt with exact EXPECTED.",
                "acceptanceChecks":[{"type":"file_equals","path":"verified.txt","content":"EXPECTED"}]},
                {"id":"step-2","name":"Handoff","agentId":"worker",
                "providerId":"mock","model":"test","prompt":"Report {{previous}}"}
            ]})).unwrap();
        let job=state.store.put("job",&json!({"id":"job","kind":"workflow",
            "status":"running","input":{"workflowId":"flow","input":"do it"}})).unwrap();
        let ctx=JobContext::new(state.clone(),"job".into());
        let output=run(&ctx,&job).await.unwrap();
        let total=calls.load(Ordering::SeqCst);
        server.abort();
        (output,state,dir,total)
    }
    #[tokio::test]
    async fn verified_first_step_hands_durable_artifact_to_next_agent(){
        let (result,state,dir,calls)=execute_with_check("workspace_write").await;
        assert_eq!(result["status"],"completed","{result}");
        assert_eq!(result["outputs"].as_array().unwrap().len(),2);
        assert_eq!(result["outputs"][0]["verification"]["status"],"verified");
        assert_eq!(result["outputs"][1]["verification"]["status"],"unchecked");
        assert!(!result["outputs"][0]["verification"]["operationIds"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("verified.txt")).unwrap(),"EXPECTED");
        assert!(calls>=2);
        assert_eq!(result["verificationSummary"]["verifiedSteps"],1);
        drop(state);std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn failed_artifact_gate_blocks_downstream_agent_and_preserves_review(){
        let (result,state,dir,_)=execute_with_check("workspace_read").await;
        assert_eq!(result["status"],"needs_review","{result}");
        assert_eq!(result["failedStepIndex"],0);
        assert_eq!(result["outputs"].as_array().unwrap().len(),0,
            "second agent must never run on unverified first result");
        assert!(result["verificationGaps"].as_array().is_some_and(|v|!v.is_empty()));
        assert_eq!(state.store.get("job","job").unwrap()["workflowState"]["inFlight"],false);
        assert!(!dir.join("verified.txt").exists());
        drop(state);std::fs::remove_dir_all(dir).unwrap();
    }


    #[test]
    fn substitutions_do_not_interpret_placeholder_text_in_payloads() {
        assert_eq!(
            render(
                "{{input}} / {{previous}}",
                "literal {{previous}}",
                "literal {{input}}"
            ),
            "literal {{previous}} / literal {{input}}"
        );
    }

    #[test]
    fn substitutes_run_input_and_previous_step() {
        let prompt = "요청: {{input}}\n이전: {{previous}}";
        let rendered = render(prompt, "Rust로 실행", "1단계 결과");
        assert_eq!(rendered, "요청: Rust로 실행\n이전: 1단계 결과");
    }
}
