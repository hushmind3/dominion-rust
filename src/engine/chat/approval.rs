//! Durable, operation-bound tool approval lifecycle.
//!
//! An approval is a persisted record first and an in-memory waiter second. The
//! record carries the execution/operation that asked for it plus a digest of
//! the exact tool arguments, so a decision can be audited and an approval that
//! outlived its process can be reused exactly once instead of silently dying
//! with the in-memory waiter.
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

fn approval_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("DOMINION_APPROVAL_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60)
            .max(5),
    )
}
/// How long an orphaned approval may be reused after its engine process died.
const REUSE_WINDOW_SECS: i64 = 600;

/// Canonical JSON so the same arguments always produce the same digest.
fn canonical_json(value: &Value) -> String {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                out.push('{');
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort();
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).unwrap_or_default());
                    out.push(':');
                    write(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

pub(super) fn args_digest(tool: &str, args: &Value) -> String {
    crate::engine::memory::stable_id(&[tool, &canonical_json(args)])
}

/// Durable lineage that makes an orphaned approval resumable. A retry may only
/// consume the decision when it is provably the same Task revision and the same
/// logical operation from the run that died. Same session/tool/args is not
/// enough: another task in the same session could intentionally repeat them.
pub(super) struct ApprovalLineage<'a> {
    pub session_id: &'a str,
    pub task_id: &'a str,
    pub task_revision: &'a str,
    pub operation_key: &'a str,
    pub tool: &'a str,
    pub args_digest: &'a str,
}

/// Finds an approved decision that lost its live waiter (engine restart) and
/// was never consumed. Denials are deliberately not reused: a retry must ask
/// the owner again rather than inherit a stale "no".
pub(super) fn reusable_decision<'a>(
    approvals: &'a [Value],
    executions: &[Value],
    lineage: &ApprovalLineage<'_>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<&'a Value> {
    if lineage.task_id.is_empty()
        || lineage.task_revision.is_empty()
        || lineage.operation_key.is_empty()
    {
        return None;
    }
    approvals.iter().find(|record| {
        let original_run = record["runId"].as_str().unwrap_or("");
        let original_is_dead = executions.iter().any(|execution| {
            execution["id"].as_str() == Some(original_run)
                && execution["taskId"].as_str() == Some(lineage.task_id)
                && execution["status"].as_str() == Some("interrupted")
        });
        record["sessionId"].as_str() == Some(lineage.session_id)
            && record["taskId"].as_str() == Some(lineage.task_id)
            && record["taskRevision"].as_str() == Some(lineage.task_revision)
            && record["operationKey"].as_str() == Some(lineage.operation_key)
            && record["tool"].as_str() == Some(lineage.tool)
            && record["argsDigest"].as_str() == Some(lineage.args_digest)
            && record["status"].as_str() == Some("approved")
            && record["orphaned"] == true
            && record["consumed"] != true
            && original_is_dead
            && record["decidedAt"]
                .as_str()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|at| {
                    (now - at.with_timezone(&chrono::Utc)).num_seconds() <= REUSE_WINDOW_SECS
                })
    })
}

fn consume_decision(state: &AppState, id: &str, execution_id: &str, operation_id: &str) -> bool {
    state
        .store
        .patch_if(
            "approval",
            id,
            "consumed",
            &Value::Null,
            &json!({
                "consumed": true,
                "consumedAt": chrono::Utc::now().to_rfc3339(),
                "consumedByExecutionId": execution_id,
                "consumedByOperationId": operation_id,
            }),
        )
        .is_some()
}

pub(super) fn close_pending_approvals(state: &AppState, session_id: &str) {
    for record in state.store.all("approval") {
        if record["sessionId"].as_str() != Some(session_id) || record["status"] != "pending" {
            continue;
        }
        let Some(id) = record["id"].as_str() else {
            continue;
        };
        state.approvals.lock().unwrap().remove(id);
        if let Some(saved) = state.store.patch_if(
            "approval",
            id,
            "status",
            &json!("pending"),
            &json!({
                "status": "cancelled",
                "decision": "deny",
                "resolvedAt": chrono::Utc::now().to_rfc3339(),
            }),
        ) {
            state.bus.approval(&saved);
        }
    }
}

/// Expire orphaned approval records after the same 120-second window as the
/// live waiter. This runs on startup and periodically; compare-and-swap keeps
/// a concurrent owner decision from being overwritten.
pub(super) fn expire_stale_approvals(state: &AppState) -> usize {
    let now = chrono::Utc::now();
    let mut expired = 0;
    for record in state.store.all("approval") {
        if record["status"] != "pending" {
            continue;
        }
        let stale = match record["requestedAt"].as_str()
            .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        {
            Some(at) => (now - at.with_timezone(&chrono::Utc))
                .to_std()
                .is_ok_and(|elapsed| elapsed >= approval_timeout()),
            // Legacy records without a timestamp cannot retain an orphaned
            // pending request when its original execution no longer runs.
            None => record["runId"].as_str().is_none_or(|run_id| {
                state.store.get("execution", run_id)
                    .is_none_or(|execution| execution["status"] != "running")
            }),
        };
        if !stale {
            continue;
        }
        let Some(id) = record["id"].as_str() else { continue };
        if let Some(saved) = state.store.patch_if(
            "approval", id, "status", &json!("pending"), &json!({
                "status":"expired",
                "decision":"deny",
                "resolvedAt":now.to_rfc3339(),
                "reason":"approval timeout",
            }),
        ) {
            state.approvals.lock().unwrap().remove(id);
            state.bus.approval(&saved);
            state.store.audit("approval.expired", json!({"approvalId":id,"runId":record["runId"]}));
            expired += 1;
        }
    }
    expired
}

pub(super) async fn request_approval(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    tool: &str,
    args: &Value,
    execution_id: &str,
    operation_id: &str,
    procedure_node: Option<&str>,
) -> bool {
    if crate::engine::rsi_evolve::auto_write_allowed(state,session_id,tool,args) {
        state.store.audit("rsi.isolated_write_authorized",json!({"sessionId":session_id,"runId":execution_id,"operationId":operation_id,"args":args}));
        return true;
    }
    let policy = state
        .store
        .get("policy", "default")
        .unwrap_or_else(|| json!({}));
    if policy
        .get("autoApproveAll")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return true;
    }
    if let Some(list) = policy.get("autoApprove").and_then(|value| value.as_array()) {
        if list.iter().any(|entry| entry.as_str() == Some(tool)) {
            return true;
        }
    }

    let digest = args_digest(tool, args);
    let execution = state.store.get("execution", execution_id).unwrap_or(Value::Null);
    let task_id = execution["taskId"].as_str().unwrap_or("").to_string();
    let task_revision = execution["taskRevision"].as_str().unwrap_or("").to_string();
    let operation_key = match procedure_node {
        Some(node) if !node.is_empty() => format!("procedure:{node}"),
        _ => format!("tool:{digest}"),
    };
    let lineage = ApprovalLineage {
        session_id,
        task_id: &task_id,
        task_revision: &task_revision,
        operation_key: &operation_key,
        tool,
        args_digest: &digest,
    };
    let existing = state.store.all("approval");
    let executions = state.store.all("execution");
    if let Some(decision) = reusable_decision(
        &existing,
        &executions,
        &lineage,
        chrono::Utc::now(),
    ) {
        if let Some(id) = decision["id"].as_str() {
            if consume_decision(state, id, execution_id, operation_id) {
                let _ = tx.send(json!({
                    "type": "approval_resumed",
                    "id": id,
                    "tool": tool,
                    "decision": "allow",
                    "taskId": task_id,
                }));
                return true;
            }
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    let record = state.store.put(
        "approval",
        &json!({
            "id": id,
            "sessionId": session_id,
            "runId": execution_id,
            "operationId": operation_id,
            "operationKey": operation_key,
            "taskId": task_id,
            "taskRevision": task_revision,
            "tool": tool,
            "args": args,
            "argsDigest": digest,
            "status": "pending",
            "requestedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    // Fail closed: if the durable record cannot be written, the owner could
    // never see or resolve the request, so the tool must not run.
    let Some(record) = record else {
        let _ = tx.send(json!({
            "type": "approval",
            "id": Value::Null,
            "tool": tool,
            "args": args,
            "error": "승인 요청을 저장할 수 없어 도구 실행을 거부했습니다.",
        }));
        return false;
    };

    state.approvals.lock().unwrap().insert(id.clone(), sender);
    state.bus.approval(&record);
    let _ = tx.send(json!({
        "type": "approval",
        "id": id,
        "tool": tool,
        "args": args,
        "operationId": operation_id,
    }));

    let outcome = tokio::time::timeout(approval_timeout(), receiver).await;
    state.approvals.lock().unwrap().remove(&id);

    let (allowed, status) = match outcome {
        Ok(Ok(true)) => (true, "approved"),
        Ok(Ok(false)) => (false, "denied"),
        Ok(Err(_)) => (false, "cancelled"),
        Err(_) => (false, "expired"),
    };
    let resolved = state.store.patch_if(
        "approval",
        &id,
        "status",
        &json!("pending"),
        &json!({
            "status": status,
            "decision": if allowed { "allow" } else { "deny" },
            "resolvedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    if let Some(saved) = resolved {
        state.bus.approval(&saved);
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;

    fn record(id: &str, status: &str, orphaned: bool, decided: &str) -> Value {
        json!({
            "id": id,
            "sessionId": "s1",
            "taskId": "task-1",
            "taskRevision": "rev-1",
            "runId": "run-old",
            "operationKey": "tool:digest-1",
            "tool": "workspace_write",
            "argsDigest": "digest-1",
            "status": status,
            "orphaned": orphaned,
            "decidedAt": decided,
        })
    }

    #[test]
    fn stale_approvals_expire_once_while_fresh_and_approved_records_survive() {
        let dir = std::env::temp_dir().join(format!("dominion-approval-expire-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port:0, data_dir:dir.clone(), web_dir:dir.join("web"), workspace:dir.clone(),
        };
        let state = AppState::new(config,
            crate::db::Store::open(&dir.join("store.sqlite")).unwrap(),
            Vault::open(&dir.join("keys.json")));
        let old = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        let fresh = chrono::Utc::now().to_rfc3339();
        for (id, status, requested_at) in [
            ("old", "pending", old.as_str()),
            ("fresh", "pending", fresh.as_str()),
            ("decided", "approved", old.as_str()),
        ] {
            state.store.put("approval", &json!({
                "id":id, "status":status, "runId":"old-run",
                "requestedAt":requested_at,
            }));
        }
        assert_eq!(expire_stale_approvals(&state), 1);
        assert_eq!(state.store.get("approval","old").unwrap()["status"], "expired");
        assert_eq!(state.store.get("approval","old").unwrap()["decision"], "deny");
        assert_eq!(state.store.get("approval","fresh").unwrap()["status"], "pending");
        assert_eq!(state.store.get("approval","decided").unwrap()["status"], "approved");
        assert_eq!(expire_stale_approvals(&state), 0, "no second expiration");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn digest_is_stable_across_key_order() {
        let first = json!({"b": 2, "a": [1, {"d": 4, "c": 3}]});
        let second = json!({"a": [1, {"c": 3, "d": 4}], "b": 2});
        assert_eq!(
            args_digest("workspace_write", &first),
            args_digest("workspace_write", &second)
        );
        assert_ne!(
            args_digest("workspace_write", &first),
            args_digest(
                "workspace_write",
                &json!({"b": 3, "a": [1, {"d": 4, "c": 3}]})
            )
        );
    }

    #[test]
    fn reuse_requires_task_revision_operation_and_dead_run() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-05T10:10:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let stale = "2026-11-05T09:00:00Z";
        let fresh = "2026-11-05T10:05:00Z";
        let executions = vec![
            json!({"id":"run-old","taskId":"task-1","status":"interrupted"}),
            json!({"id":"run-live","taskId":"task-1","status":"running"}),
        ];
        let lineage = ApprovalLineage {
            session_id: "s1",
            task_id: "task-1",
            task_revision: "rev-1",
            operation_key: "tool:digest-1",
            tool: "workspace_write",
            args_digest: "digest-1",
        };
        let approvals = vec![
            record("denied", "denied", true, fresh),
            record("live", "approved", false, fresh),
            record("stale", "approved", true, stale),
            record("ok", "approved", true, fresh),
            json!({
                "id":"other-task","sessionId":"s1","taskId":"task-2","taskRevision":"rev-1",
                "runId":"run-old","operationKey":"tool:digest-1","tool":"workspace_write",
                "argsDigest":"digest-1","status":"approved","orphaned":true,"decidedAt":fresh
            }),
            json!({
                "id":"other-revision","sessionId":"s1","taskId":"task-1","taskRevision":"rev-2",
                "runId":"run-old","operationKey":"tool:digest-1","tool":"workspace_write",
                "argsDigest":"digest-1","status":"approved","orphaned":true,"decidedAt":fresh
            }),
            json!({
                "id":"other-node","sessionId":"s1","taskId":"task-1","taskRevision":"rev-1",
                "runId":"run-old","operationKey":"procedure:audit","tool":"workspace_write",
                "argsDigest":"digest-1","status":"approved","orphaned":true,"decidedAt":fresh
            }),
        ];
        let find = |items: &[Value]| {
            reusable_decision(items, &executions, &lineage, now)
                .and_then(|record| record["id"].as_str())
                .map(str::to_string)
        };
        assert_eq!(find(&approvals), Some("ok".to_string()));
        for bad in [
            "denied",
            "live",
            "stale",
            "other-task",
            "other-revision",
            "other-node",
        ] {
            let single = approvals
                .iter()
                .filter(|record| record["id"] == bad)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(single.len(), 1, "{bad} fixture missing");
            assert!(find(&single).is_none(), "{bad} must not be reused");
        }

        // Same task/operation but the original run is still alive: never reuse.
        let live_executions = vec![json!({"id":"run-old","taskId":"task-1","status":"running"})];
        assert!(reusable_decision(&approvals[3..4], &live_executions, &lineage, now).is_none());

        // Missing lineage (legacy record) must never fall back to session+args.
        let legacy = ApprovalLineage {
            session_id: "s1",
            task_id: "",
            task_revision: "",
            operation_key: "",
            tool: "workspace_write",
            args_digest: "digest-1",
        };
        assert!(reusable_decision(&approvals[3..4], &executions, &legacy, now).is_none());
    }

    #[test]
    fn consumption_and_resolution_are_compare_and_swap() {
        let dir = std::env::temp_dir().join(format!("dominion-approval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);

        state.store.put(
            "approval",
            &json!({"id":"a1","status":"pending","tool":"workspace_write"}),
        );
        assert!(state
            .store
            .patch_if("approval", "a1", "status", &json!("pending"), &json!({"status":"approved"}))
            .is_some());
        // A second resolve must not overwrite the first decision.
        assert!(state
            .store
            .patch_if("approval", "a1", "status", &json!("pending"), &json!({"status":"denied"}))
            .is_none());
        assert_eq!(state.store.get("approval", "a1").unwrap()["status"], "approved");

        assert!(consume_decision(&state, "a1", "exec", "op"));
        assert!(!consume_decision(&state, "a1", "exec", "op"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
