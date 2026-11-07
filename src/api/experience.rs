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
    let principle_by_id = principles
        .iter()
        .filter_map(|item| {
            item["id"]
                .as_str()
                .map(|id| (id.to_string(), item.clone()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut usages = state.store.all("principle_usage");
    // Newest first: the UI answers "what reused which principle recently".
    usages.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
    });
    for usage in usages.iter_mut() {
        let principle = usage["principleId"]
            .as_str()
            .and_then(|id| principle_by_id.get(id));
        usage["contributingModels"] = principle
            .map(|item| item["supportingModels"].clone())
            .unwrap_or_else(|| json!([]));
        usage["contributingAgents"] = principle
            .map(|item| item["supportingAgents"].clone())
            .unwrap_or_else(|| json!([]));
        usage["principle"] = principle
            .map(|item| item["principle"].clone())
            .unwrap_or(Value::Null);
        let execution = usage["executionId"]
            .as_str()
            .and_then(|id| state.store.get("execution", id));
        usage["execution"] = execution
            .as_ref()
            .map(|item| {
                json!({
                    "id": item["id"],
                    "status": item["status"],
                    "taskOutcome": item["taskOutcome"],
                    "agentId": item["agentId"],
                    "startedAt": item["startedAt"],
                })
            })
            .unwrap_or(Value::Null);
        usage["outcome"] = execution
            .as_ref()
            .and_then(|item| {
                item["taskOutcome"]
                    .as_str()
                    .or_else(|| item["status"].as_str())
                    .map(str::to_string)
            })
            .or_else(|| usage["status"].as_str().map(str::to_string))
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
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
