use crate::engine::jobs::JobContext;
use crate::state::AppState;
use crate::engine::{chat::TurnRequest, llm, run_quiet_with_stats, TurnStats};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Autonomous goal harness with an evidence gate:
///   plan -> execute -> review -> repair/replan -> complete
/// A goal can only be marked complete when the reviewer agrees *and* the
/// engine has real tool evidence (and, when the goal names files, they exist).
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let state = &context.state;
    let goal_id = job
        .get("input")
        .and_then(|value| value.get("goalId"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let mut goal = state
        .store
        .get("goal", goal_id)
        .ok_or_else(|| "목표를 찾을 수 없습니다.".to_string())?;

    let text = goal
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("목표 내용이 비어 있습니다.".into());
    }

    let executor = goal.get("executor").cloned().unwrap_or_else(|| json!({}));
    let provider_id = executor
        .get("providerId")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let model = executor
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let agent_id = executor
        .get("agentId")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let max_iterations = goal
        .get("maxIterations")
        .and_then(|value| value.as_i64())
        .unwrap_or(6)
        .clamp(1, 12) as usize;

    goal["status"] = json!("running");
    goal["phase"] = json!("plan");
    goal["startedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let goal = state.store.put("goal", &goal).unwrap_or(goal);
    context.emit(json!({ "type": "goal_started", "goalId": goal_id }));

    let session_id = match goal.get("sessionId").and_then(|value| value.as_str()) {
        Some(id) if state.store.get("session", id).is_some() => id.to_string(),
        _ => {
            let created = state
                .store
                .put(
                    "session",
                    &json!({
                        "title": format!("Goal · {}", text.chars().take(36).collect::<String>()),
                        "agentId": agent_id,
                    }),
                )
                .ok_or("목표 세션을 만들 수 없습니다.")?;
            created
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        }
    };

    // ---- plan ------------------------------------------------------------
    let plan = llm::complete(
        state,
        &provider_id,
        &model,
        "You are the planner of an autonomous agent. Produce a short ordered plan (max 5 steps) in Korean. No tool calls.",
        &format!("목표: {text}\n\n목표를 달성하기 위한 실행 단계를 3~5개로 나열하세요."),
    )
    .await
    .unwrap_or_else(|_| format!("1. 목표를 직접 수행한다.\n2. 결과를 확인한다.\n목표: {text}"));
    let plan: String = plan.chars().take(1500).collect();
    let mut record = state.store.get("goal", goal_id).unwrap_or_else(|| json!({}));
    record["plan"] = json!(plan);
    record["phase"] = json!("execute");
    state.store.put("goal", &record);

    let actionable = looks_actionable(&text);
    let mut evidence: Vec<String> = Vec::new();
    let mut transcript = String::new();
    let mut gaps: Vec<String> = Vec::new();
    let mut iterations = 0usize;
    let mut status = "needs_review";
    let mut cancelled = false;
    let mut review_error: Option<String> = None;

    for index in 0..max_iterations {
        if state.is_cancelled(&session_id) {
            cancelled = true;
            break;
        }
        let prompt = format!(
            "목표: {text}\n\n계획:\n{plan}\n\n이전 검토에서 지적된 점:\n{}\n\n지금까지 진행:\n{}\n\n\
             다음 단계를 실제 도구로 수행하세요. 말로만 완료했다고 하지 말고, 필요한 도구를 지금 호출하세요.",
            if gaps.is_empty() { "(없음)".to_string() } else { gaps.join("\n- ") },
            if transcript.is_empty() {
                "(아직 없음)".to_string()
            } else {
                transcript.chars().rev().take(6000).collect::<String>().chars().rev().collect()
            }
        );

        let request = TurnRequest {
            session_id: Some(session_id.clone()),
            input: prompt,
            agent_id: agent_id.clone(),
            provider_id: Some(provider_id.clone()).filter(|value| !value.is_empty()),
            model: Some(model.clone()).filter(|value| !value.is_empty()),
            tools_enabled: true,
            thinking: None,
            auto_learning: false,
            reasoning_effort: None,
            reasoning_language: None,
            system_override: Some(
                "You are executing a durable autonomous goal. Use real tools, never claim \
                 a result that a tool did not produce."
                    .to_string(),
            ),
        };

        let (message, stats): (Option<Value>, TurnStats) =
            run_quiet_with_stats(state, &request).await?;
        iterations = index + 1;
        if let Some(message) = &message {
            let content = message
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            transcript.push_str(&format!("\n[단계 {}] {content}\n", iterations));
        }
        evidence.extend(stats.tool_success.clone());

        let mut record = state.store.get("goal", goal_id).unwrap_or_else(|| json!({}));
        record["phase"] = json!("review");
        record["iterations"] = json!(iterations);
        record["evidence"] = json!(evidence);
        record["sessionId"] = json!(session_id);
        state.store.put("goal", &record);
        context.patch(json!({ "stepIndex": iterations, "stepTotal": max_iterations }));
        context.emit(json!({
            "type": "goal_progress",
            "goalId": goal_id,
            "iteration": iterations,
            "evidence": stats.tool_success,
        }));

        // ---- review ------------------------------------------------------
        let missing = missing_files(state, &text);
        let review_prompt = format!(
            "목표: {text}\n\n계획:\n{plan}\n\n진행 기록:\n{transcript}\n\n\
             실제 도구 실행 증거: {}\n없는 파일: {}\n\n\
             위 증거만 근거로 목표 달성 여부를 판정하세요. 모델이 말로만 완료했다고 하면 미완료입니다.\n\
             JSON만 출력: {{\"complete\":true|false,\"gaps\":[\"부족한 점\"]}}",
            if evidence.is_empty() { "(없음)".to_string() } else { evidence.join(", ") },
            if missing.is_empty() { "(없음)".to_string() } else { missing.join(", ") }
        );
        let review = llm::complete(
            state,
            &provider_id,
            &model,
            "You are a strict reviewer. Judge only from tool evidence and file state. Output JSON only.",
            &review_prompt,
        )
        .await;
        let (mut complete, mut review_gaps) = match &review {
            Ok(text) => parse_review(text),
            Err(error) => {
                review_error = Some(error.clone());
                (false, vec!["검토 단계 실패".to_string()])
            }
        };
        if let Some(gate) = evidence_gate(actionable, &evidence, &missing) {
            complete = false;
            review_gaps.push(gate);
        }
        gaps = review_gaps;

        let mut record = state.store.get("goal", goal_id).unwrap_or_else(|| json!({}));
        record["phase"] = json!(if complete { "complete" } else { "repair" });
        record["reviewer"] = review.map(|text| json!(text)).unwrap_or(Value::Null);
        record["gaps"] = json!(gaps);
        record["evidence"] = json!(evidence);
        state.store.put("goal", &record);

        if complete {
            status = "completed";
            break;
        }
    }

    if cancelled {
        status = "cancelled";
    } else if status != "completed" && iterations >= max_iterations {
        status = "needs_review";
    }

    if let Some(mut record) = state.store.get("goal", goal_id) {
        record["status"] = json!(status);
        record["phase"] = json!(if status == "completed" { "complete" } else { "review" });
        record["iterations"] = json!(iterations);
        record["sessionId"] = json!(session_id);
        record["evidence"] = json!(evidence);
        record["gaps"] = json!(gaps);
        record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        if let Some(error) = review_error {
            record["error"] = json!(error);
        }
        state.store.put("goal", &record);
    }
    state.store.audit(
        "goal.finished",
        json!({ "goalId": goal_id, "status": status, "iterations": iterations }),
    );
    context.emit(json!({ "type": "goal_finished", "goalId": goal_id, "status": status }));

    Ok(json!({
        "goalId": goal_id,
        "status": status,
        "sessionId": session_id,
        "iterations": iterations,
        "evidence": evidence,
        "gaps": gaps,
    }))
}

fn looks_actionable(goal: &str) -> bool {
    const VERBS: [&str; 24] = [
        "만들", "생성", "저장", "작성", "수정", "편집", "삭제", "제거", "실행", "설치", "복사",
        "이동", "다운로드", "빌드", "커밋", "배포", "create", "write", "save", "edit", "delete",
        "run", "install", "build",
    ];
    let lower = goal.to_lowercase();
    VERBS.iter().any(|verb| lower.contains(verb))
}

fn evidence_gate(actionable: bool, evidence: &[String], missing: &[String]) -> Option<String> {
    if actionable && evidence.is_empty() {
        return Some("도구 실행 증거가 없습니다. 말로만 완료할 수 없습니다.".to_string());
    }
    if !missing.is_empty() {
        return Some(format!("파일이 존재하지 않습니다: {}", missing.join(", ")));
    }
    None
}

fn missing_files(state: &AppState, goal: &str) -> Vec<String> {
    const EXTS: [&str; 10] = [
        "txt", "md", "rs", "json", "py", "js", "ts", "csv", "html", "toml",
    ];
    let mut missing = Vec::new();
    for raw in goal.split_whitespace() {
        let token = raw.trim_matches(|ch: char| {
            !(ch.is_alphanumeric() || matches!(ch, '.' | '/' | '_' | '-'))
        });
        let Some(ext) = token.rsplit('.').next() else {
            continue;
        };
        if !EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
            continue;
        }
        let path = if token.starts_with('/') {
            PathBuf::from(token)
        } else {
            state.config.workspace.join(token)
        };
        if !path.exists() {
            missing.push(token.to_string());
        }
    }
    missing
}

pub fn parse_review(text: &str) -> (bool, Vec<String>) {
    let Some(start) = text.find('{') else {
        return (false, vec!["검토 응답이 JSON이 아닙니다.".into()]);
    };
    let Some(end) = text.rfind('}') else {
        return (false, vec!["검토 응답이 JSON이 아닙니다.".into()]);
    };
    let Ok(value) = serde_json::from_str::<Value>(&text[start..=end]) else {
        return (false, vec!["검토 JSON을 해석할 수 없습니다.".into()]);
    };
    let complete = value
        .get("complete")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    let gaps = value
        .get("gaps")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (complete, gaps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_detection_is_conservative() {
        assert!(looks_actionable("워크스페이스에 a.txt 파일을 만들어줘"));
        assert!(looks_actionable("run the tests"));
        assert!(!looks_actionable("러스트가 왜 좋은지 설명해줘"));
    }

    #[test]
    fn evidence_gate_blocks_words_without_tools() {
        assert!(evidence_gate(true, &[], &[]).is_some());
        assert!(evidence_gate(true, &["workspace_write".into()], &[]).is_none());
        assert!(evidence_gate(false, &[], &[]).is_none());
        assert!(evidence_gate(true, &["workspace_write".into()], &["a.txt".into()]).is_some());
    }

    #[test]
    fn review_json_parses() {
        let (complete, gaps) =
            parse_review("```json\n{\"complete\":true,\"gaps\":[]}\n```");
        assert!(complete);
        assert!(gaps.is_empty());
        let (complete, gaps) = parse_review("{\"complete\":false,\"gaps\":[\"파일 없음\"]}");
        assert!(!complete);
        assert_eq!(gaps, vec!["파일 없음"]);
        assert!(!parse_review("not json").0);
    }
}
