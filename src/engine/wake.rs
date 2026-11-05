//! H4 automatic wake policy for interrupted runs.
//!
//! A crashed run is never blindly replayed. The wake record explains the
//! decision: resume only when no external side effect can have happened and no
//! owner decision is outstanding; otherwise request review.
use crate::state::AppState;
use serde_json::{json, Value};

/// Side-effecting tools whose completion proves the crash happened after a
/// real external change. Replaying those automatically could duplicate work.
fn has_side_effect(operations: &[Value]) -> bool {
    operations.iter().any(|operation| {
        operation["status"].as_str() == Some("completed")
            && matches!(
                operation["tool"].as_str(),
                Some(
                    "workspace_write"
                        | "file_write"
                        | "shell_exec"
                        | "terminal_exec"
                        | "github_api"
                        | "delegate_task"
                )
            )
    })
}

/// Creates one durable wake decision per interrupted execution. Returns the
/// decisions created by this call.
pub fn recover(state: &AppState) -> Vec<Value> {
    let operations = state.store.all("execution_operation");
    let approvals = state.store.all("approval");
    let mut created = Vec::new();
    for execution in state.store.all("execution") {
        if execution["status"].as_str() != Some("interrupted") {
            continue;
        }
        let Some(execution_id) = execution["id"].as_str() else {
            continue;
        };
        let wake_id = format!("wake:{execution_id}");
        if state.store.get("run_wake", &wake_id).is_some() {
            continue;
        }
        let run_operations = operations
            .iter()
            .filter(|operation| operation["runId"].as_str() == Some(execution_id))
            .cloned()
            .collect::<Vec<_>>();
        let pending_approval = approvals.iter().any(|approval| {
            approval["runId"].as_str() == Some(execution_id)
                && matches!(approval["status"].as_str(), Some("pending" | "interrupted"))
        });
        let unknown = run_operations.iter().any(|operation| {
            operation["outcomeUnknown"] == true
                || matches!(operation["status"].as_str(), Some("interrupted" | "running"))
        });
        let request = execution["request"].as_str().unwrap_or("");
        let session_id = execution["sessionId"].as_str().unwrap_or("");
        let task_outcome = execution["taskOutcome"].as_str().unwrap_or("");
        let (decision, reason, job_id) = if task_outcome == "succeeded" {
            ("skipped", "이미 성공한 실행입니다.", Value::Null)
        } else if unknown {
            ("needs_review", "중단된 도구의 외부 결과를 확인해야 합니다.", Value::Null)
        } else if pending_approval {
            ("needs_review", "해결되지 않은 승인 요청이 있습니다.", Value::Null)
        } else if has_side_effect(&run_operations) {
            (
                "needs_review",
                "완료된 외부 변경이 있어 자동 재실행하지 않습니다.",
                Value::Null,
            )
        } else if request.trim().is_empty() || session_id.is_empty() {
            ("needs_review", "원래 요청 또는 세션을 복구할 수 없습니다.", Value::Null)
        } else if state.store.get("session", session_id).is_none() {
            ("needs_review", "원래 세션을 찾을 수 없습니다.", Value::Null)
        } else {
            let input = json!({
                "prompt": request,
                "sessionId": session_id,
                "agentId": execution["agentId"],
                "providerId": execution["providerId"],
                "model": execution["model"],
                "taskId": execution["taskId"],
                "taskRevision": execution["taskRevision"],
                "toolsEnabled": true,
                "toolsExecutionEnabled": true,
                "resumedExecutionId": execution_id,
            });
            match crate::engine::jobs::enqueue(state, "resume", input) {
                Some(job) => (
                    "queued",
                    "부작용 없는 중단 실행을 자동 재개합니다.",
                    job["id"].clone(),
                ),
                None => ("needs_review", "재개 작업을 만들 수 없습니다.", Value::Null),
            }
        };
        let decision = json!({
            "id": wake_id,
            "executionId": execution_id,
            "sessionId": session_id,
            "taskId": execution["taskId"],
            "decision": decision,
            "reason": reason,
            "jobId": job_id,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(saved) = state.store.put("run_wake", &decision) {
            state.bus.record("run_wake", &saved);
            state.store.audit(
                "run.wake",
                json!({
                    "executionId": execution_id,
                    "decision": saved["decision"],
                    "jobId": saved["jobId"],
                }),
            );
            created.push(saved);
        }
    }
    created
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-wake-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put(
            "session",
            &json!({"id":"session-1","agentId":null,"providerId":"p","model":"m"}),
        );
        (state, dir)
    }

    fn interrupted(state: &AppState, id: &str) {
        state.store.put(
            "execution",
            &json!({
                "id":id,"status":"interrupted","phase":"interrupted",
                "taskOutcome":"cancelled","sessionId":"session-1",
                "request":"파일을 확인해","providerId":"p","model":"m",
                "taskId":id,"taskRevision":"r1","agentId":null
            }),
        );
    }

    #[test]
    fn a_clean_interrupted_run_is_queued_once() {
        let (state, dir) = state();
        interrupted(&state, "run-a");
        let created = recover(&state);
        assert_eq!(created.len(), 1);
        assert_eq!(created[0]["decision"], "queued");
        let job_id = created[0]["jobId"].as_str().unwrap().to_string();
        assert_eq!(
            state.store.get("job", &job_id).unwrap()["kind"],
            "resume"
        );
        // A second startup must not enqueue another resume.
        assert!(recover(&state).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn external_effects_and_approvals_require_review() {
        let (state, dir) = state();
        interrupted(&state, "run-effect");
        state.store.put(
            "execution_operation",
            &json!({"id":"op1","runId":"run-effect","tool":"shell_exec","status":"completed"}),
        );
        interrupted(&state, "run-unknown");
        state.store.put(
            "execution_operation",
            &json!({"id":"op2","runId":"run-unknown","tool":"workspace_write","status":"interrupted","outcomeUnknown":true}),
        );
        interrupted(&state, "run-approval");
        state.store.put(
            "approval",
            &json!({"id":"a1","runId":"run-approval","status":"interrupted","tool":"workspace_write"}),
        );
        let created = recover(&state);
        assert_eq!(created.len(), 3);
        assert!(created.iter().all(|item| item["decision"] == "needs_review"));
        assert!(created.iter().all(|item| item["jobId"].is_null()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn successful_runs_are_not_woken() {
        let (state, dir) = state();
        state.store.put(
            "execution",
            &json!({"id":"run-ok","status":"interrupted","taskOutcome":"succeeded","sessionId":"session-1","request":"x"}),
        );
        let created = recover(&state);
        assert_eq!(created[0]["decision"], "skipped");
        let _ = std::fs::remove_dir_all(dir);
    }
}
