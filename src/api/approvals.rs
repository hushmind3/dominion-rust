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
    let waiter = state.approvals.lock().unwrap().remove(&id);
    match waiter {
        Some(sender) => {
            let _ = sender.send(allow);
            Json(json!({ "ok": true, "resolved": true })).into_response()
        }
        None => {
            if let Some(mut record) = state.store.get("approval", &id) {
                if record.get("status").and_then(|value| value.as_str()) == Some("pending") {
                    record["status"] = json!("interrupted");
                    state.store.put("approval", &record);
                }
            }
            Json(json!({
                "ok": false,
                "resolved": false,
                "error": "진행 중인 실행이 없습니다. 작업을 다시 요청하세요."
            }))
            .into_response()
        }
    }
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

