//! Explicit tool approval lifecycle and cancelled waiter cleanup.
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

pub(super) fn close_pending_approvals(state: &AppState, session_id: &str) {
    for mut record in state.store.all("approval") {
        if record["sessionId"].as_str() == Some(session_id) && record["status"] == "pending" {
            if let Some(id) = record["id"].as_str() {
                state.approvals.lock().unwrap().remove(id);
            }
            record["status"] = json!("cancelled");
            record["resolvedAt"] = json!(chrono::Utc::now().to_rfc3339());
            if let Some(saved) = state.store.put("approval", &record) {
                state.bus.approval(&saved);
            }
        }
    }
}

pub(super) async fn request_approval(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    tool: &str,
    args: &Value,
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

    let id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    state.approvals.lock().unwrap().insert(id.clone(), sender);

    let record = state.store.put(
        "approval",
        &json!({
            "id": id,
            "sessionId": session_id,
            "tool": tool,
            "args": args,
            "status": "pending",
            "requestedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    state.bus.approval(record.as_ref().unwrap_or(&json!({})));
    let _ = tx.send(json!({
        "type": "approval",
        "id": id,
        "tool": tool,
        "args": args,
    }));

    let outcome = tokio::time::timeout(Duration::from_secs(120), receiver).await;
    state.approvals.lock().unwrap().remove(&id);

    let (allowed, status) = match outcome {
        Ok(Ok(true)) => (true, "approved"),
        Ok(Ok(false)) => (false, "denied"),
        Ok(Err(_)) => (false, "cancelled"),
        Err(_) => (false, "expired"),
    };

    if let Some(mut record) = state.store.get("approval", &id) {
        record["status"] = json!(status);
        record["decision"] = json!(if allowed { "allow" } else { "deny" });
        record["resolvedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("approval", &record);
    }
    allowed
}
