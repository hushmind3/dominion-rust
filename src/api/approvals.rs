use crate::state::AppState;
use axum::{extract::{State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list_approvals(State(state): State<AppState>) -> Json<Value> {
    let pending = state
        .store
        .all("approval")
        .into_iter()
        .filter(|record| {
            matches!(
                record.get("status").and_then(|value| value.as_str()),
                Some("pending") | Some("interrupted")
            )
        })
        .collect::<Vec<_>>();
    Json(json!(pending))
}

pub async fn resolve_approval(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let allow = body
        .get("allow")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if id.is_empty() {
        return error(StatusCode::BAD_REQUEST, "승인 id 가 필요합니다.");
    }
    let Some(record) = state.store.get("approval", &id) else {
        return error(StatusCode::NOT_FOUND, "승인 요청을 찾을 수 없습니다.");
    };
    let current_status = record["status"].as_str().unwrap_or("");
    if !matches!(current_status, "pending" | "interrupted") {
        return Json(json!({
            "ok": record["status"] == "approved",
            "resolved": false,
            "status": record["status"],
            "error": "이미 처리된 승인입니다."
        }))
        .into_response();
    }

    // The live waiter may already be gone (engine restart or timeout). The
    // decision is still stored, so a matching retry can consume it once.
    let has_waiter = state.approvals.lock().unwrap().contains_key(&id);
    let decided_at = chrono::Utc::now().to_rfc3339();
    let Some(saved) = state.store.patch_if(
        "approval",
        &id,
        "status",
        &json!(current_status),
        &json!({
            "status": if allow { "approved" } else { "denied" },
            "decision": if allow { "allow" } else { "deny" },
            "decidedAt": decided_at,
            "orphaned": !has_waiter,
        }),
    ) else {
        return error(
            StatusCode::CONFLICT,
            "다른 요청이 먼저 이 승인을 처리했습니다.",
        );
    };
    state.bus.approval(&saved);

    if let Some(sender) = state.approvals.lock().unwrap().remove(&id) {
        if sender.send(allow).is_ok() {
            state.store.patch_if(
                "approval",
                &id,
                "status",
                &json!(if allow { "approved" } else { "denied" }),
                &json!({"orphaned": false}),
            );
        } else {
            state.store.patch_if(
                "approval",
                &id,
                "status",
                &json!(if allow { "approved" } else { "denied" }),
                &json!({"orphaned": true}),
            );
        }
    }

    Json(json!({
        "ok": true,
        "resolved": true,
        "status": saved["status"],
        "resumed": has_waiter,
    }))
    .into_response()
}

// ------------------------------------------------------------------- policy

pub async fn get_policy(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.get("policy", "default").unwrap_or_else(|| {
        json!({
            "autoApproveAll": false,
            "autoApproveMcp": false,
            "autoApprove": []
        })
    }))
}

pub async fn save_policy(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut record = body.clone();
    record["id"] = json!("default");
    let saved = state.store.put("policy", &record).unwrap_or(record);
    state.bus.record("policy", &saved);
    Json(saved)
}
