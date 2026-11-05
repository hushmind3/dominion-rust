use crate::state::AppState;
use axum::{extract::State, Json};
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("experience")))
}
