use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet_outcome};
use serde_json::{json, Value};

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
    // A queued run owns an immutable, admission-validated agent+model
    // snapshot. Editing a reusable workflow or agent must not silently alter
    // the provider of a previously approved run after a crash.
    let definition = existing.get("definition").filter(|v|v.is_object())
        .or_else(||input.get("definition").filter(|v|v.is_object()))
        .cloned().unwrap_or_else(||workflow.clone());
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
        if prompt.trim().is_empty() {
            return Err(format!("{step_name} 단계의 지시문이 비어 있습니다."));
        }

        let checks=crate::engine::verification::baseline_checks(state,
            step["successCriteria"].as_array().cloned().unwrap_or_default());
        context.checkpoint(json!({
            "workflowState": { "sessionId": session_id, "outputs": outputs, "definition": definition, "inFlight": true, "activeStepIndex": index,
                "acceptanceChecks":checks },
        }))?;
        let request = TurnRequest {
            session_id: Some(session_id.clone()),
            input: prompt.clone(),
            agent_id: optional(step, "agentId"),
            provider_id: optional(step, "providerId"),
            model: optional(step, "model"),
            attachment_ids: Vec::new(),
            tools_enabled: true,tools_execution_enabled: true,
            thinking: None,
            auto_learning: false,
            reasoning_effort: None,
            reasoning_language: None,
            system_override: Some(format!(
                "You are executing step {} of the workflow \"{}\". \
                 Produce the step result directly and concisely.",
                index + 1,
                workflow
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Workflow")
            )),
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };

        let (turn, stats)=run_quiet_outcome(state, &request).await;
        let receipt_ids=stats.evidence.iter().filter_map(|receipt|
            receipt["id"].as_str()).map(str::to_string).collect::<Vec<_>>();
        let message=match turn {
            Ok(Some(message))=>message,
            Ok(None)=>return Err(format!("needs_review: {step_name} 단계가 빈 응답을 반환했습니다. 도구 영수증을 확인하세요.")),
            Err(_) if state.is_cancelled(&session_id) => {
                return Ok(json!({"workflowId":workflow_id,
                    "sessionId":session_id,"outputs":outputs,"status":"cancelled"}))
            },
            Err(error)=>return Err(format!(
                "needs_review: {step_name} 단계 응답이 중단됐습니다: {error}. 도구 영수증 {receipt_ids:?}을 확인하세요."
            ))
        };
        let gaps=crate::engine::verification::check_acceptance(
            state,&prompt,&checks,&stats.evidence);
        if !gaps.is_empty(){
            // Block downstream agent turns if the previous step did not
            // actually produce its required file change. Preserve evidence
            // and expected checks in a durable review checkpoint.
            context.checkpoint(json!({"workflowState":{
                "sessionId":session_id,"outputs":outputs,"definition":definition,
                "inFlight":true,"activeStepIndex":index,
                "acceptanceChecks":checks,"acceptanceGaps":gaps,
                "lastAttempt":{"messageId":message.get("id"),
                    "receiptIds":receipt_ids,"toolErrorCount":stats.tool_errors.len()}
            }}))?;
            return Err(format!("needs_review: {} 단계 검증이 미달했습니다. 후속 단계 실행을 중단하고 실행 영수증을 확인하세요: {}",
                step_name,gaps.join("; ")));
        }
        // A model saying 'done' is a draft deliverable, not independent
        // quality proof; only the configured file-change checks are machine
        // verified. Preserve the real tool IDs and role origin.
        outputs.push(json!({
            "stepId":step.get("id"),"name":step_name,
            "agentId":step.get("agentId"),"providerId":step.get("providerId"),
            "model":step.get("model"),
            "content":message.get("content").cloned().unwrap_or(json!("")),
            "messageId":message.get("id"),"committedToolReceiptIds":receipt_ids,
            "toolErrorCount":stats.tool_errors.len(),
            "acceptanceChecks":checks,"acceptanceVerified":checks.as_array().is_some_and(|x|!x.is_empty()),
            "usageAfterStep":state.store.session_usage(&session_id),
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

    let mut result=json!({
        "workflowId":workflow_id,"sessionId":session_id,
        "outputs":outputs,"status":status,
    });
    if status=="completed" {
        // A real reusable deliverable, linked to immutable workflow version,
        // engine message IDs, committed operation receipts and model usage.
        // Upserting by JOB ID is idempotent after crash-between-save-and-done.
        let last=outputs.last().cloned().unwrap_or(Value::Null);
        let artifact=json!({
            "id":context.job_id,"workflowId":workflow_id,"sessionId":session_id,
            "workflowName":definition["name"],"runInput":payload,
            "definition":definition,"steps":outputs,
            "finalContent":last["content"],"finalMessageId":last["messageId"],
            "modelUsage":state.store.session_usage(&session_id),
            "verification":"model_output_with_persisted_tool_receipts",
            "independentlyVerified":false,
            "completedAt":chrono::Utc::now().to_rfc3339(),
        });
        let saved=state.store.put("workflow_deliverable",&artifact)
            .ok_or("워크플로 산출물 저장에 실패했습니다.")?;
        state.bus.record("workflow_deliverable",&saved);
        result["deliverableId"]=saved["id"].clone();
    }
    Ok(result)
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
    use super::render;
    use serde_json::{json,Value};

    #[tokio::test]
    async fn actual_multi_agent_turns_persist_a_reusable_deliverable() {
        use crate::{config::Config,db::Store,vault::Vault};
        use axum::{extract::State,response::IntoResponse,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        let observed=Arc::new(Mutex::new(Vec::<Value>::new()));
        async fn respond(State(observed):State<Arc<Mutex<Vec<Value>>>>,
            Json(body):Json<Value>)->impl IntoResponse{
            observed.lock().unwrap().push(body);
            let payload=format!("data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"completed step output"}}]}));
            ([("content-type","text/event-stream")],payload)
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let router=Router::new().route("/v1/chat/completions",post(respond))
            .with_state(observed.clone());
        let server=tokio::spawn(async move {axum::serve(listener,router).await.unwrap()});
        let root=std::env::temp_dir().join(format!("workflow-delivery-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        state.store.put("provider",&json!({"id":"p","name":"mock","kind":"openai",
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"sample","enabled":true,"toolMode":"native"}));
        for agent in ["writer","reviewer"]{
            state.store.put("agent",&json!({"id":agent,"name":agent,
                "providerId":"p","model":"sample"}));
        }
        let definition=json!({"id":"workflow-one","name":"Build and review",
            "steps":[
              {"id":"s1","name":"Draft","agentId":"writer",
               "providerId":"p","model":"sample","prompt":"{{input}}"},
              {"id":"s2","name":"Review","agentId":"reviewer",
               "providerId":"p","model":"sample","prompt":"Check {{previous}}"}]});
        state.store.put("workflow",&definition);
        let job=state.store.put("job",&json!({"id":"job-one","kind":"workflow",
            "status":"running","input":{"workflowId":"workflow-one",
                "input":"Create a report","definition":definition}})).unwrap();
        let context=crate::engine::jobs::JobContext::new(state.clone(),"job-one".into());
        let result=super::run(&context,&job).await.unwrap();
        assert_eq!(result["status"],"completed","{result}");
        assert_eq!(result["outputs"].as_array().unwrap().len(),2);
        let artifact=state.store.get("workflow_deliverable","job-one").unwrap();
        assert_eq!(artifact["finalContent"],"completed step output");
        assert_eq!(artifact["steps"][0]["agentId"],"writer");
        assert_eq!(artifact["steps"][1]["agentId"],"reviewer");
        assert_eq!(artifact["steps"][1]["providerId"],"p");
        assert_eq!(artifact["independentlyVerified"],false);
        assert_eq!(artifact["steps"][0]["committedToolReceiptIds"].as_array().unwrap().len(),0);
        assert!(observed.lock().unwrap().len()>=2);
        server.abort();
        let _=std::fs::remove_dir_all(root);
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
