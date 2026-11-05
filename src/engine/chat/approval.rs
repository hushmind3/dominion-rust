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

const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
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

/// Finds an approved decision that lost its live waiter (engine restart) and
/// was never consumed. Denials are deliberately not reused: a retry must ask
/// the owner again rather than inherit a stale "no".
pub(super) fn reusable_decision<'a>(
    records: &'a [Value],
    session_id: &str,
    tool: &str,
    digest: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<&'a Value> {
    records.iter().find(|record| {
        record["sessionId"].as_str() == Some(session_id)
            && record["tool"].as_str() == Some(tool)
            && record["argsDigest"].as_str() == Some(digest)
            && record["status"].as_str() == Some("approved")
            && record["orphaned"] == true
            && record["consumed"] != true
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

pub(super) async fn request_approval(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    tool: &str,
    args: &Value,
    execution_id: &str,
    operation_id: &str,
) -> bool {
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
    let existing = state.store.all("approval");
    if let Some(decision) = reusable_decision(
        &existing,
        session_id,
        tool,
        &digest,
        chrono::Utc::now(),
    ) {
        if let Some(id) = decision["id"].as_str() {
            if consume_decision(state, id, execution_id, operation_id) {
                let _ = tx.send(json!({
                    "type": "approval_resumed",
                    "id": id,
                    "tool": tool,
                    "decision": "allow",
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

    let outcome = tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await;
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

    fn record(id: &str, status: &str, orphaned: bool, digest: &str, decided: &str) -> Value {
        json!({
            "id": id,
            "sessionId": "s1",
            "tool": "workspace_write",
            "argsDigest": digest,
            "status": status,
            "orphaned": orphaned,
            "decidedAt": decided,
        })
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
            args_digest("workspace_write", &json!({"b": 3, "a": [1, {"d": 4, "c": 3}]}))
        );
    }

    #[test]
    fn only_live_orphaned_approvals_are_reusable() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-05T10:10:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let stale = "2026-11-05T09:00:00Z";
        let fresh = "2026-11-05T10:05:00Z";
        let records = vec![
            record("denied", "denied", true, "d1", fresh),
            record("live", "approved", false, "d2", fresh),
            record("stale", "approved", true, "d3", stale),
            record("ok", "approved", true, "d4", fresh),
        ];
        assert!(reusable_decision(&records, "s1", "workspace_write", "d1", now).is_none());
        assert!(reusable_decision(&records, "s1", "workspace_write", "d2", now).is_none());
        assert!(reusable_decision(&records, "s1", "workspace_write", "d3", now).is_none());
        assert_eq!(
            reusable_decision(&records, "s1", "workspace_write", "d4", now)
                .map(|record| record["id"].as_str().unwrap_or("")),
            Some("ok")
        );
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
