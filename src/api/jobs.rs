use crate::{engine,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;
pub async fn list(State(state):State<AppState>)->Json<Value>{Json(json!(state.store.all("job")))}
pub async fn one(State(state):State<AppState>,Path(id):Path<String>)->Response{match state.store.get("job",&id){Some(j)=>Json(j).into_response(),None=>error(StatusCode::NOT_FOUND,"작업을 찾을 수 없습니다.")}}
/// Client requests describe work, never mutable runtime status or evidence.
pub async fn create(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let kind=body["kind"].as_str().unwrap_or("prompt");let input=&body["input"];
    let clean=match kind{
        "prompt"=>{
            let Some(prompt)=input["prompt"].as_str().filter(|p|!p.trim().is_empty()&&p.len()<=64000)else{return error(StatusCode::BAD_REQUEST,"실행할 작업 지시가 필요합니다.")};
            let mut value=json!({"prompt":prompt,"toolsEnabled":input["toolsEnabled"].as_bool().unwrap_or(true)});
            for key in ["agentId","providerId","model","reasoningEffort"]{if let Some(v)=input[key].as_str().filter(|s|!s.is_empty()){value[key]=json!(v);}}
            if let Some(v)=input["thinking"].as_bool(){value["thinking"]=json!(v);}
            if let Some(id)=value["agentId"].as_str(){if state.store.get("agent",id).is_none(){return error(StatusCode::BAD_REQUEST,"실행 에이전트를 찾을 수 없습니다.");}}
            value
        },
        "workflow"=>{
            let Some(id)=input["workflowId"].as_str().filter(|id|state.store.get("workflow",id).is_some())else{return error(StatusCode::BAD_REQUEST,"워크플로를 찾을 수 없습니다.")};json!({"workflowId":id,"input":input["input"]})
        },
        _=>return error(StatusCode::BAD_REQUEST,"해당 작업은 대화·회의·영상의 실행 API에서 시작하세요."),
    };
    match engine::jobs::enqueue(&state,kind,clean){Some(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),None=>error(StatusCode::INTERNAL_SERVER_ERROR,"작업을 대기열에 저장할 수 없습니다.")}
}
/// Jobs created for a persistent session can still be running before their
/// first checkpoint writes a top-level sessionId. Resolve the original session
/// from all durable job ownership paths so Stop reaches the active Rust turn.
fn cancellable_session(state:&AppState,job:&Value)->Option<String>{
    for path in [
        "/sessionId","/input/sessionId","/goalState/sessionId",
        "/promptState/sessionId","/roomState/sessionId","/workflowState/sessionId"
    ] {
        if let Some(id)=job.pointer(path).and_then(Value::as_str).filter(|s|!s.is_empty()){
            return Some(id.to_string());
        }
    }
    if job["kind"]=="goal" {
        let goal_id=job.pointer("/input/goalId").and_then(Value::as_str)?;
        return state.store.get("goal",goal_id)
            .and_then(|goal|goal["sessionId"].as_str().map(str::to_string));
    }
    None
}

pub async fn cancel(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id)else{return error(StatusCode::NOT_FOUND,"작업이 없습니다.")};
    if job["kind"]=="rsi_evolve" {
        // Also clean up descendants when a client repeats Stop on a terminal parent.
        engine::jobs::cancel_rsi_children(&state,&id);
        if job["status"]=="cancelled" {return Json(job).into_response();}
    }
    // Stop is idempotent across duplicate UI clicks and timeout retries.
    if job["status"]=="cancelled"{return Json(job).into_response();}
    if !matches!(job["status"].as_str(),Some("queued"|"running")){return error(StatusCode::CONFLICT,"진행 중인 작업만 중단할 수 있습니다.");}
    if let Some(session)=cancellable_session(&state,&job){state.cancel_session(&session);}
    match state.store.patch_if("job",&id,"status",&job["status"],&json!({"status":"cancelled","cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339()})){
        Some(saved)=>{
            state.bus.record("job",&saved);
            if saved["kind"]=="goal" {
                if let Some(goal_id)=saved["input"]["goalId"].as_str() {
                    if let Some(goal)=state.store.get("goal",goal_id) {
                        if matches!(goal["status"].as_str(),Some("queued"|"running")) {
                            if let Some(stopped)=state.store.patch_if("goal",goal_id,
                                "status",&goal["status"],&json!({"status":"cancelled",
                                    "phase":"cancelled","finishedAt":chrono::Utc::now().to_rfc3339()})){
                                state.bus.record("goal",&stopped);
                            }
                        }
                    }
                }
            }
            Json(saved).into_response()
        },
        None=>state.store.get("job",&id).map(|j|Json(j).into_response())
            .unwrap_or_else(||error(StatusCode::CONFLICT,"작업 상태가 변경되었습니다."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    fn state()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-cancel-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }

    #[tokio::test]
    async fn stop_reaches_input_session_before_first_job_checkpoint(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"child"})).unwrap();
        state.store.put("job",&json!({"id":"job","kind":"prompt",
            "status":"running","input":{"sessionId":"child"}})).unwrap();
        let response=cancel(State(state.clone()),Path("job".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        assert!(state.is_cancelled("child"));
        let job=state.store.get("job","job").unwrap();
        assert_eq!(job["status"],"cancelled");
        assert_eq!(job["cancelRequested"],true);
        // Repeating Stop must report the existing durable result, not 409.
        let repeat=cancel(State(state.clone()),Path("job".into())).await;
        assert_eq!(repeat.status(),StatusCode::OK);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn stop_uses_goal_session_even_without_job_checkpoint(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"goal-session"})).unwrap();
        state.store.put("goal",&json!({"id":"goal-1","sessionId":"goal-session",
            "status":"running"})).unwrap();
        state.store.put("job",&json!({"id":"goal-job","kind":"goal","status":"running",
            "input":{"goalId":"goal-1"}})).unwrap();
        let response=cancel(State(state.clone()),Path("goal-job".into())).await;
        assert_eq!(response.status(),StatusCode::OK);
        assert!(state.is_cancelled("goal-session"));
        assert_eq!(state.store.get("goal","goal-1").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("job","goal-job").unwrap()["status"],"cancelled");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
