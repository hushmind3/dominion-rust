use crate::state::AppState;
use axum::{extract::State, Json};
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("experience")))
}

/// Model-agnostic transfer surface. Principles and learned rules are global
/// assets; usages and cases show which agents/models contributed and reused
/// them. This is evidence, not a per-model filter.
pub async fn transfer(State(state): State<AppState>) -> Json<Value> {
    let mut principles = state.store.all("experience_principle");
    principles.sort_by(|a, b| {
        b["updatedAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["updatedAt"].as_str().unwrap_or(""))
    });
    let mut rules = state.store.all("experience_rule");
    rules.sort_by(|a, b| {
        b["updatedAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["updatedAt"].as_str().unwrap_or(""))
    });
    let mut usages = state.store.all("principle_usage");
    usages.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
    });
    let mut cases = state.store.all("learning_case");
    cases.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
    });
    Json(json!({
        "principles": principles,
        "rules": rules,
        "usages": usages,
        "learningCases": cases,
    }))
}
