use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet_outcome, verification, goal_artifacts, jobs};
use crate::state::AppState;
use sha2::{Digest,Sha256};
use serde_json::{json, Value};

/// A run is immutable after enqueue: edits to the workflow cannot change
/// the model/tool program under a queued job. An explicit requestId makes
/// network retries idempotent without forbidding intentional repeat runs.
pub fn enqueue_run(state:&AppState, workflow_id:&str,body:&Value)->Result<Value,String>{
    let payload=body["input"].as_str().unwrap_or("");
    if payload.len()>16_000{return Err("워크플로 입력이 16,000바이트를 초과합니다.".into());}
    let request_id=match body["requestId"].as_str(){
        Some(id)=>uuid::Uuid::parse_str(id)
            .map_err(|_|"requestId는 UUID여야 합니다.")?.to_string(),
        None=>uuid::Uuid::new_v4().to_string(),
    };
    let key=format!("dominion:workflow:{workflow_id}:{request_id}");
    let digest=Sha256::digest(key.as_bytes());
    let mut bytes=[0u8;16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6]=(bytes[6]&0x0f)|0x40;
    bytes[8]=(bytes[8]&0x3f)|0x80;
    let job_id=uuid::Uuid::from_bytes(bytes).to_string();
    if let Some(saved)=state.store.get("job",&job_id){
        if saved["kind"]=="workflow"&&saved["input"]["workflowId"]==workflow_id
            && saved["input"]["input"]==payload
            && saved["input"]["requestId"]==request_id {
            return Ok(saved);
        }
        return Err("워크플로 실행 키가 이미 다른 입력에 사용됐습니다.".into());
    }
    let definition=state.store.get("workflow",workflow_id)
        .ok_or("저장된 워크플로가 없습니다.")?;
    let steps=definition["steps"].as_array()
        .filter(|items|!items.is_empty()&&items.len()<=30)
        .ok_or("워크플로 실행 단계가 없습니다.")?;
    if steps.iter().any(|step|
        !step["prompt"].as_str().is_some_and(|text|!text.trim().is_empty())
        || !step["agentId"].as_str().is_some_and(|id|
            state.store.get("agent",id).is_some())){
        return Err("실행할 워크플로의 에이전트 또는 단계가 잘못됐습니다.".into());
    }
    let input=json!({"workflowId":workflow_id,"input":payload,
        "requestId":request_id,"definition":definition});
    jobs::enqueue_once(state,&job_id,"workflow",input)
        .ok_or("동일한 실행 키가 다른 내용으로 등록됐습니다.".into())
}

fn usage_delta(before:&Value,after:&Value)->Value{
    let mut result=json!({});
    for name in ["calls","promptTokens","completionTokens","totalTokens","estimatedCalls"] {
        let old=before[name].as_u64().unwrap_or(0);
        let next=after[name].as_u64().unwrap_or(0);
        result[name]=json!(next.saturating_sub(old));
    }
    result
}

/// Handoff carries actual file receipts, not imaginary model deliverables.
/// Bounded previous text avoids an accidentally enormous next paid request.
fn handoff(previous:Option<&Value>)->String{
    let Some(previous)=previous else{return String::new()};
    let text=previous["content"].as_str().unwrap_or("");
    let len=text.chars().count();
    let mut result=text.chars().take(12_000).collect::<String>();
    if len>12_000 {result.push_str("\n[앞 단계 본문 12,000자까지만 전달됨]");}
    if let Some(artifacts)=previous["artifacts"].as_array().filter(|xs|!xs.is_empty()) {
        result.push_str("\n\n[실제 파일 도구에서 확인한 산출물 — 품질 검증은 별개]");
        for file in artifacts.iter().take(12){
            if let (Some(path),Some(sha))=(file["path"].as_str(),file["sha256"].as_str()){
                result.push_str(&format!("\n- {} (sha256: {})",path,sha));
            }
        }
    }
    result
}

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

    let workflow = input.get("definition").filter(|value|value.is_object())
        .cloned().or_else(||context.state.store.get("workflow",workflow_id))
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

        let previous = handoff(outputs.last());
        let template = step
            .get("prompt")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let prompt = render(template, &payload, &previous);
        if prompt.trim().is_empty() {
            return Err(format!("{step_name} 단계의 지시문이 비어 있습니다."));
        }

        context.checkpoint(json!({
            "workflowState": { "sessionId": session_id, "outputs": outputs, "definition": definition, "inFlight": true, "activeStepIndex": index },
        }))?;
        let request = TurnRequest {
            session_id: Some(session_id.clone()),
            input: prompt,
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

        let usage_before=state.store.session_usage(&session_id);
        let (outcome,stats)=run_quiet_outcome(state,&request).await;
        // Persist observed effects on all paths, including model error.
        // Never silently reissue an interrupted turn whose tools may have run.
        let evidence=stats.evidence.iter().filter(|item|verification::successful_evidence(item))
            .cloned().collect::<Vec<_>>();
        let artifacts=goal_artifacts::collect(state,&evidence);
        let tool_receipts=evidence.iter().take(48).map(|item|json!({
            "tool":verification::tool_name(item),"callId":item["callId"]
        })).collect::<Vec<_>>();
        let usage=usage_delta(&usage_before,&state.store.session_usage(&session_id));
        let unresolved=state.store.session_has_uncertain_effects(&session_id)
            .map_err(|err|format!("needs_review: 작업 도구 영수증을 읽을 수 없습니다: {err}"))?;
        if unresolved{
            context.checkpoint(json!({"workflowState":{"sessionId":session_id,
                "outputs":outputs,"definition":definition,"inFlight":true,
                "activeStepIndex":index,"lastToolReceipts":tool_receipts,
                "lastObservedArtifacts":artifacts,"lastStepUsage":usage}}))?;
            return Err("needs_review: 워크플로 단계의 외부 도구 결과가 미확정입니다. 상태 확인 후 재개하세요.".into());
        }
        let message=match outcome{
            Ok(Some(message))=>message,
            Ok(None)=>return Err(format!("needs_review: {step_name} 단계가 빈 응답을 반환했습니다.")),
            Err(_) if state.is_cancelled(&session_id)=>return Ok(json!({
                "workflowId":workflow_id,"sessionId":session_id,"outputs":outputs,"status":"cancelled"
            })),
            Err(error)=>{
                context.checkpoint(json!({"workflowState":{"sessionId":session_id,
                    "outputs":outputs,"definition":definition,"inFlight":true,
                    "activeStepIndex":index,"lastToolReceipts":tool_receipts,
                    "lastObservedArtifacts":artifacts,"lastStepUsage":usage}}))?;
                return Err(format!("needs_review: 실행 단계 오류: {error}"));
            }
        };
        outputs.push(json!({
            "stepId":step.get("id"),"name":step_name,
            "agentId":step.get("agentId"),"providerId":step.get("providerId"),
            "model":step.get("model"),"content":message.get("content")
                .cloned().unwrap_or(json!("")),"messageId":message.get("id"),
            "toolReceipts":tool_receipts,"toolErrors":stats.tool_errors,
            "artifacts":artifacts,"usage":usage,
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

    let verified_artifacts=outputs.iter().flat_map(|item|
        item["artifacts"].as_array().cloned().unwrap_or_default())
        .take(96).collect::<Vec<_>>();
    let usage=state.store.session_usage(&session_id);
    Ok(json!({
        "workflowId":workflow_id,"sessionId":session_id,
        "outputs":outputs,"artifacts":verified_artifacts,
        "usage":usage,"status":status,
        "qualityVerified":false,
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
    #[test]
    fn handoff_includes_verified_artifact_paths_and_usage_delta(){
        let output=json!({"content":"draft {{input}}","artifacts":[{"path":"build/report.md","sha256":"123abc"}]});
        let handoff=handoff(Some(&output));
        assert!(handoff.contains("build/report.md"));
        assert!(handoff.contains("123abc"));
        assert!(handoff.contains("draft {{input}}"));
        assert_eq!(usage_delta(&json!({"totalTokens":12,"calls":1}),
            &json!({"totalTokens":35,"calls":2}))["totalTokens"],23);
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
