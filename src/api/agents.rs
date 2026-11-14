use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list_agents(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("agent")))
}

pub async fn save_agent(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Err(message) = crate::engine::reasoning::validate(&body) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    if body
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return error(StatusCode::BAD_REQUEST, "에이전트 이름이 필요합니다.");
    }
    // Explicit agent skill assignments are the same across chat, rooms and
    // workflow steps. Never bind a disabled/draft or deleted skill.
    // Omitted skillIds preserves compatibility with the legacy shared catalog.
    if let Some(selected)=body.get("skillIds"){
        let Some(ids)=selected.as_array().filter(|ids|ids.len()<=5) else{
            return error(StatusCode::BAD_REQUEST,
                "에이전트 스킬은 최대 5개의 ID 배열로 지정하세요.");
        };
        let mut seen=std::collections::HashSet::new();
        for id in ids{
            let Some(id)=id.as_str().filter(|s|!s.trim().is_empty()) else{
                return error(StatusCode::BAD_REQUEST,"스킬 ID가 올바르지 않습니다.");
            };
            if !seen.insert(id){
                return error(StatusCode::BAD_REQUEST,"중복된 에이전트 스킬 ID입니다.");
            }
            let active=state.store.get("skill",id).is_some_and(|skill|
                skill["enabled"]!=false && skill["status"]!="draft");
            if !active{
                return error(StatusCode::BAD_REQUEST,
                    "비활성화되었거나 삭제된 스킬은 에이전트에게 배정할 수 없습니다.");
            }
        }
    }
    let saved = match state.store.put("agent", &body) {
        Some(record) => record,
        None => return error(StatusCode::BAD_REQUEST, "에이전트를 저장할 수 없습니다."),
    };
    state.bus.record("agent", &saved);
    Json(saved).into_response()
}

pub async fn bulk_agent_model(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Err(message) = crate::engine::reasoning::validate(&body) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let Some(items) = body["agentIds"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= 100)
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "변경할 에이전트를 1~100명 선택하세요.",
        );
    };
    let Some(provider_id) = body["providerId"].as_str().filter(|id| !id.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "모델 제공자가 필요합니다.");
    };
    if !state
        .store
        .get("provider", provider_id)
        .is_some_and(|provider| provider["enabled"].as_bool() != Some(false))
    {
        return error(
            StatusCode::BAD_REQUEST,
            "활성화된 모델 제공자를 선택하세요.",
        );
    }
    let Some(model) = body["model"]
        .as_str()
        .map(str::trim)
        .filter(|model| !model.is_empty() && model.len() <= 256)
    else {
        return error(StatusCode::BAD_REQUEST, "유효한 모델 ID가 필요합니다.");
    };
    let mut ids = Vec::new();
    for item in items {
        let Some(id) = item.as_str() else {
            return error(StatusCode::BAD_REQUEST, "에이전트 ID가 올바르지 않습니다.");
        };
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
    }
    let mut patch = json!({"providerId":provider_id,"model":model});
    if let Some(effort) = body.get("reasoningEffort") {
        patch["reasoningEffort"] = effort.clone();
        patch["thinking"] = Value::Null;
    }
    let Some(records) = state.store.patch_records(
        "agent",
        &ids,
        &patch,
    ) else {
        return error(
            StatusCode::BAD_REQUEST,
            "선택한 에이전트를 모두 찾고 저장할 수 없습니다.",
        );
    };
    for record in &records {
        state.bus.record("agent", record);
    }
    Json(json!({"count":records.len(),"agents":records})).into_response()
}

pub async fn delete_agent(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("agent", &id);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "agent", "removed": id }));
    Json(json!({ "ok": true }))
}


pub async fn growth(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(crate::engine::agent_growth::growth(&state, &id))
}

pub async fn synergy(State(state): State<AppState>) -> Json<Value> {
    let agents = state.store.all("agent");
    let shared_methods = state
        .store
        .all("experience_rule")
        .into_iter()
        .filter(|rule| {
            rule["scope"].as_str() == Some("shared")
                && rule["supportingAgents"]
                    .as_array()
                    .map(|items| items.len() >= 2)
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let mut algorithm_stats = state.store.all("algorithm_stat");
    algorithm_stats.sort_by(|a, b| {
        b["uses"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["uses"].as_u64().unwrap_or(0))
    });
    Json(json!({
        "agents": agents.iter().map(|agent| json!({
            "id": agent["id"],
            "name": agent["name"],
            "growth": crate::engine::agent_growth::growth(
                &state,
                agent["id"].as_str().unwrap_or(""),
            ),
        })).collect::<Vec<_>>(),
        "sharedMethods": shared_methods,
        "algorithmStats": algorithm_stats.into_iter().take(40).collect::<Vec<_>>(),
    }))
}
