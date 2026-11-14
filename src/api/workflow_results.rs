//! First-class user deliverables for multi-agent workflows.
//! These APIs expose local results with source and token provenance, and
//! assemble reusable investigator/reviewer workflows from existing agents.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};

/// Optional third-party agent harness availability, without leaking user
/// tokens or treating SDK license as free inference.
pub async fn harnesses()->Json<Value>{
    Json(json!({
        "native":{"compiled":true,"runtime":"DOMINION","requiresAuth":false},
        "copilot":{
            "compiled":crate::engine::copilot_harness::enabled(),
            "runtime":"GitHub Copilot SDK (official Rust)",
            "requiresAuth":true,
            "defaultAutoApproval":false,
            "measuredBilling":false
        }
    }))
}

pub async fn list(State(state):State<AppState>)->Json<Value>{
    let mut rows=state.store.all("workflow_artifact");
    rows.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    rows.truncate(100);
    // A listing must stay small even when 100 deliverables each carry 256KiB.
    // The full, SHA-bound body is fetched only for preview or download.
    for record in &mut rows {
        let content=record["content"].as_str().unwrap_or("");
        let preview=content.chars().take(180).collect::<String>();
        let bytes=content.len();
        if let Some(fields)=record.as_object_mut() {
            fields.remove("content");
            fields.insert("preview".into(),json!(preview));
            fields.insert("bytes".into(),json!(bytes));
        }
    }
    Json(json!(rows))
}

pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("workflow_artifact",&id){
        Some(record)=>Json(record).into_response(),
        None=>(StatusCode::NOT_FOUND,Json(json!({
            "error":"완료한 워크플로 결과물을 찾지 못했습니다."
        }))).into_response()
    }
}

/// One owner action turns an existing agent into an actual two-step
/// investigator -> independent reviewer workflow. It is a reusable draft;
/// no models or paid tools run until the owner explicitly starts it.
pub async fn from_agent(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    let Some(agent)=state.store.get("agent",&id) else {
        return (StatusCode::NOT_FOUND,Json(json!({
            "error":"연결할 에이전트를 찾지 못했습니다."
        }))).into_response();
    };
    let reviewer=body["reviewerAgentId"].as_str().filter(|s|!s.is_empty())
        .unwrap_or(&id);
    if state.store.get("agent",reviewer).is_none(){
        return (StatusCode::BAD_REQUEST,Json(json!({
            "error":"검토 에이전트를 찾지 못했습니다."
        }))).into_response();
    }
    let title=agent["name"].as_str().unwrap_or("Agent");
    let name=format!("{} · 조사→검토→납품",title.chars().take(48).collect::<String>());
    let flow=json!({
        "name":name,
        "description":"에이전트 조사 결과를 다른 검토 단계에서 교정하고 최종 결과물로 보존합니다.",
        "createdFromAgentId":id,
        "steps":[
            {"id":uuid::Uuid::new_v4().to_string(),
                "name":"조사 및 실행","agentId":id,
                "providerId":"","model":"",
                "prompt":"사용자 요청: {{input}}\n필요한 도구와 연결 능력을 사용해 실제로 작업하세요. 근거와 미완료 부분을 명확히 구분하고 조사·작업 결과를 작성하세요."},
            {"id":uuid::Uuid::new_v4().to_string(),
                "name":"검토·최종 산출물","agentId":reviewer,
                "providerId":"","model":"",
                "prompt":"최초 사용자 요청을 기준으로 조사 및 실행 결과를 독립적으로 검토하세요.\n요청: {{input}}\n실행 결과: {{previous}}\n사실과 추론, 완료/미완료, 근거를 구분하고 바로 활용할 최종 결과물로 완성하세요."}
        ]
    });
    match state.store.put("workflow",&flow){
        Some(saved)=>{
            state.bus.record("workflow",&saved);
            state.store.audit("workflow.created_from_agent",json!({
                "workflowId":saved["id"],"agentId":id,"reviewerAgentId":reviewer
            }));
            Json(saved).into_response()
        }
        None=>(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({
            "error":"협업 워크플로를 저장하지 못했습니다."
        }))).into_response()
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->AppState{
        let root=std::env::temp_dir().join(format!("workflow-result-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),workspace:root.clone(),web_dir:root.join("web")};
        AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()))
    }
    #[tokio::test]
    async fn real_agents_create_reusable_two_step_flow_without_spending(){
        let state=state();
        state.store.put("agent",&json!({"id":"researcher","name":"Researcher"})).unwrap();
        state.store.put("agent",&json!({"id":"reviewer","name":"Reviewer"})).unwrap();
        let resp=from_agent(State(state.clone()),Path("researcher".into()),
            Json(json!({"reviewerAgentId":"reviewer"}))).await;
        assert_eq!(resp.status(),StatusCode::OK);
        let flows=state.store.all("workflow");
        assert_eq!(flows.len(),1);
        assert_eq!(flows[0]["steps"][0]["agentId"],"researcher");
        assert_eq!(flows[0]["steps"][1]["agentId"],"reviewer");
        assert!(flows[0]["steps"][1]["prompt"].as_str().unwrap().contains("{{previous}}"));
        assert!(state.store.all("job").is_empty());
    }
}
