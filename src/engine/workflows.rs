use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet};
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
            tools_enabled: true,
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
        };

        let message = match run_quiet(state, &request).await {
            Ok(Some(message)) => message,
            Ok(None) => return Err(format!("{step_name} 단계가 빈 응답을 반환했습니다.")),
            Err(_) if state.is_cancelled(&session_id) => {
                return Ok(
                    json!({"workflowId":workflow_id,"sessionId":session_id,"outputs":outputs,"status":"cancelled"}),
                )
            }
            Err(error) => return Err(error),
        };
        outputs.push(json!({
            "stepId": step.get("id"),
            "name": step_name,
            "content": message.get("content").cloned().unwrap_or(json!("")),
            "messageId": message.get("id"),
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

    Ok(json!({
        "workflowId": workflow_id,
        "sessionId": session_id,
        "outputs": outputs,
        "status": if state.is_cancelled(&session_id) {"cancelled"} else {"completed"},
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
    use super::render;

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
