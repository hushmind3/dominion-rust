use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

/// Start a real durable goal with this registered agent, preserving the
/// existing goal→job SQLite transaction and acceptance/stop/resume engine.
/// The user may specify the work, never spoof executor/provider identity.
pub async fn run_agent_goal(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response {
    let Some(agent)=state.store.get("agent",&id) else {
        return error(StatusCode::NOT_FOUND,"에이전트를 찾을 수 없습니다.");
    };
    let Some(task)=body["goal"].as_str().map(str::trim)
        .filter(|t|!t.is_empty()&&t.chars().count()<=12000) else {
        return error(StatusCode::BAD_REQUEST,"작업 목표는 1~12000자여야 합니다.");
    };
    if agent["providerId"].as_str().is_some_and(|id|!id.is_empty())
        && !state.store.get("provider",agent["providerId"].as_str().unwrap_or(""))
            .is_some_and(|p|p["enabled"]!=false) {
        return error(StatusCode::BAD_REQUEST,"에이전트의 모델 제공자가 없거나 비활성화됐습니다.");
    }
    let isolate=body["isolateWorktree"].as_bool().unwrap_or(false);
    let budget=body["tokenBudget"].as_u64().unwrap_or(60_000);
    if budget>2_000_000 || body["tokenBudget"].is_number()
        && body["tokenBudget"].as_u64().is_none() {
        return error(StatusCode::BAD_REQUEST,"토큰 예산은 0~200만이어야 합니다.");
    }
    let criteria=body.get("successCriteria").cloned().unwrap_or(json!([]));
    if let Err(reason)=crate::engine::goals::validate_criteria(&criteria){
        return error(StatusCode::BAD_REQUEST,&reason);
    }
    // Reuse the one authoritative goal admission path. Agent selection is
    // server-owned; no arbitrary runtime state, job ID or role override.
    crate::api::goals::create_goal(
        State(state),Json(json!({
            "goal":task,"successCriteria":criteria,
            "executor":{"agentId":id},
            "tokenBudget":budget,"isolateWorktree":isolate,
            "maxIterations":12,"maxReplans":4,"replanAfter":2
        }))
    ).await
}

/// One bounded collection of agent outcomes, without N requests or
/// inventing a token allocation for shared multi-agent workflow sessions.
pub async fn portfolio(State(state):State<AppState>)->Json<Value>{
    let agents=state.store.all("agent");
    let goals=state.store.all("goal");
    let jobs=state.store.all("job");
    let workflows=state.store.all("workflow");
    let mut results=Vec::new();
    for agent in agents{
        let id=agent["id"].as_str().unwrap_or("");
        if id.is_empty(){continue;}
        let owned=goals.iter().filter(|goal|
            goal["executor"]["agentId"].as_str()==Some(id)).collect::<Vec<_>>();
        let mut success=0_u64;
        let mut applied=0_u64;
        let mut running=0_u64;
        let mut review=0_u64;
        let mut recorded_tokens=0_u64;
        let mut recorded_calls=0_u64;
        let mut estimated_calls=0_u64;
        let mut token_missing=0_u64;
        let mut recent_goals=Vec::new();
        for goal in &owned{
            let job=jobs.iter().find(|job|job["id"]==goal["jobId"]
                && job["kind"]=="goal"&&job["input"]["goalId"]==goal["id"]);
            let accepted=goal["status"]=="completed"&&goal["finalReport"]["accepted"]==true
                && job.is_some_and(|job|job["status"]=="completed");
            if accepted {
                success+=1;
                if goal["isolateWorktree"]!=true||goal["gitWorktree"]["applied"]==true {
                    applied+=1;
                }
            }
            if matches!(goal["status"].as_str(),Some("queued"|"running")){running+=1;}
            if goal["status"]=="needs_review"{review+=1;}
            if let Some(session)=goal["sessionId"].as_str().filter(|s|!s.is_empty()){
                let usage=state.store.session_usage(session);
                recorded_tokens=recorded_tokens.saturating_add(usage["totalTokens"].as_u64().unwrap_or(0));
                recorded_calls=recorded_calls.saturating_add(usage["calls"].as_u64().unwrap_or(0));
                estimated_calls=estimated_calls.saturating_add(usage["estimatedCalls"].as_u64().unwrap_or(0));
                if usage["calls"].as_u64()==Some(0) {token_missing+=1;}
            } else if goal["status"]!="queued"{token_missing+=1;}
            recent_goals.push(json!({
                "id":goal["id"],"goal":goal["goal"],"status":goal["status"],
                "accepted":accepted,"createdAt":goal["createdAt"],
                "finishedAt":goal["finishedAt"],"jobId":goal["jobId"]
            }));
        }
        recent_goals.sort_by(|a,b|b["createdAt"].as_str().unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or("")));
        // Workflow attribution is step-based: a shared workflow session's
        // provider usage is never counted as a single agent's cost.
        let mut assigned_steps=0_u64;
        let mut completed_steps=0_u64;
        for job in jobs.iter().filter(|job|job["kind"]=="workflow"){
            let def=job.pointer("/workflowState/definition").filter(|d|d.is_object())
                .or_else(||workflows.iter().find(|wf|wf["id"]==job["input"]["workflowId"]).copied());
            let Some(steps)=def.and_then(|d|d["steps"].as_array()) else {continue;};
            for (index,step) in steps.iter().enumerate(){
                if step["agentId"].as_str()!=Some(id){continue;}
                assigned_steps+=1;
                let output=job.pointer("/workflowState/outputs")
                    .and_then(Value::as_array)
                    .and_then(|outputs|outputs.get(index));
                if output.is_some_and(|o|o["stepId"]==step["id"]
                    && o["messageId"].is_string()){completed_steps+=1;}
            }
        }
        results.push(json!({
            "agentId":id,"name":agent["name"],"goalCount":owned.len(),
            "internallyAccepted":success,"deliveredOrNonIsolated":applied,
            "active":running,"needsReview":review,
            "workflowAssignedSteps":assigned_steps,
            "workflowCompletedSteps":completed_steps,
            "recordedGoalTokens":recorded_tokens,
            "recordedGoalModelCalls":recorded_calls,
            "estimatedGoalModelCalls":estimated_calls,
            "tokenTelemetryMissingGoals":token_missing,
            "workflowSharedTokenCost":Value::Null,
            "recentGoals":recent_goals.into_iter().take(4).collect::<Vec<_>>()
        }));
    }
    Json(json!({"source":"durable_goal_jobs_and_workflow_step_checkpoints",
        "proofLevel":"internal_runtime_observation","realBillingKrw":Value::Null,
        "agents":results}))
}

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
