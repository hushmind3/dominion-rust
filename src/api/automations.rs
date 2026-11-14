//! Product automation API: durable recurring task definition with validated
//! real execution targets. Scheduler work is separately guarded by immutable
//! occurrence IDs; a UI-only recurring task is not a runnable automation.
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

fn normalize(state:&AppState,body:&Value)->Result<Value,String>{
    let kind=body["kind"].as_str().unwrap_or("prompt");
    let input=&body["input"];
    let clean=match kind {
        "prompt"=>{
            let prompt=input["prompt"].as_str()
                .filter(|x|!x.trim().is_empty()&&x.len()<=16_000)
                .ok_or("자동 작업 지시문은 1~16000자여야 합니다.")?;
            let agent_id=input["agentId"].as_str()
                .filter(|x|state.store.get("agent",x).is_some())
                .ok_or("실제 저장된 에이전트를 선택하세요.")?;
            let mut step=json!({"prompt":prompt,"agentId":agent_id,
                "toolsEnabled":input["toolsEnabled"].as_bool().unwrap_or(true)});
            for key in ["providerId","model"]{
                if let Some(v)=input[key].as_str().filter(|x|!x.is_empty()){step[key]=json!(v)}
            }
            step
        },
        "meeting"=>{
            let room=input["roomId"].as_str()
                .filter(|x|state.store.get("room",x).is_some())
                .ok_or("존재하는 그룹대화 공간을 지정하세요.")?;
            let agenda=input["goal"].as_str()
                .filter(|x|!x.trim().is_empty()&&x.len()<=16_000)
                .ok_or("반복 회의의 안건을 입력하세요.")?;
            json!({"roomId":room,"goal":agenda})
        },
        "workflow"=>{
            let id=input["workflowId"].as_str()
                .filter(|x|state.store.get("workflow",x).is_some())
                .ok_or("저장된 워크플로를 선택하세요.")?;
            let payload=input["input"].as_str().unwrap_or("");
            if payload.chars().count()>16_000 {
                return Err("워크플로 반복 입력은 최대 16000자입니다.".into())
            }
            // Validate model availability NOW and again at scheduled wake.
            let w=state.store.get("workflow",id).unwrap();
            crate::api::workflows::runnable(state,&w)?;
            json!({"workflowId":id,"input":payload})
        },
        _=>return Err("반복 작업은 prompt·meeting·workflow만 지원합니다.".into()),
    };
    let interval=body["intervalMinutes"].as_u64()
        .filter(|n|*n>=5&&*n<=525_600)
        .ok_or("반복 주기는 5분~365일 사이여야 합니다.")?;
    let next=body["nextAt"].as_str()
        .ok_or("다음 실행 시각이 필요합니다.")?;
    let timestamp=chrono::DateTime::parse_from_rfc3339(next)
        .map_err(|_|"다음 실행 시각은 시간대가 포함된 ISO 문자열이어야 합니다.")?;
    // Misconfigured historical schedules cannot generate unbounded catch-up
    // jobs, and an explicit schedule beyond 2 years is likely accidental.
    if timestamp<chrono::Utc::now()-chrono::Duration::days(1)||
       timestamp>chrono::Utc::now()+chrono::Duration::days(730){
        return Err("다음 실행은 하루 전부터 2년 이내로 지정하세요.".into())
    }
    let id=body["id"].as_str()
        .filter(|id|uuid::Uuid::parse_str(id).is_ok())
        .map(str::to_string).unwrap_or_else(||uuid::Uuid::new_v4().to_string());
    Ok(json!({"id":id,"kind":kind,"input":clean,
        "intervalMinutes":interval,"nextAt":timestamp.to_rfc3339(),
        "enabled":body["enabled"]==true,
        "updatedAt":chrono::Utc::now().to_rfc3339(),
        "lastAt":body.get("lastAt"),
    }))
}
pub async fn list(State(state):State<AppState>)->Json<Value>{
    Json(json!(state.store.all("automation")))
}
pub async fn save(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let mut record=match normalize(&state,&body) {
        Ok(item)=>item,Err(message)=>return error(StatusCode::BAD_REQUEST,&message)
    };
    // The owner controls recurrence; browser supplied runtime/receipt fields
    // are discarded. Preserve only the server's last-run metadata.
    if let Some(previous)=record["id"].as_str().and_then(|id|state.store.get("automation",id)){
        record["lastAt"]=previous["lastAt"].clone();
    }
    let Some(saved)=state.store.put("automation",&record) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR,"반복 작업을 저장하지 못했습니다.")
    };
    state.bus.record("automation",&saved);
    Json(saved).into_response()
}
pub async fn delete(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if state.store.get("automation",&id).is_none(){
        return error(StatusCode::NOT_FOUND,"반복 작업이 없습니다.")
    }
    state.store.remove("automation",&id);
    state.bus.emit(json!({"type":"record","kind":"automation","removed":id}));
    Json(json!({"ok":true})).into_response()
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[tokio::test]
    async fn stored_workflow_recurs_only_with_real_enabled_provider_and_owner_opt_in(){
        let root=std::env::temp_dir().join(format!("automation-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("provider",&json!({"id":"p","enabled":true,"model":"test"}));
        state.store.put("agent",&json!({"id":"a","providerId":"p","model":"test","name":"agent"}));
        state.store.put("workflow",&json!({"id":"w","name":"report",
            "steps":[{"id":"s","name":"draft","agentId":"a","prompt":"{{input}}"}]}));
        let at=(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339();
        let body=json!({"kind":"workflow","input":{"workflowId":"w","input":"report"},
            "intervalMinutes":30,"nextAt":at,"enabled":true});
        assert_eq!(save(State(state.clone()),Json(body.clone())).await.status(),StatusCode::OK);
        assert_eq!(state.store.count("automation"),1);
        state.store.patch_if("provider","p","id",&json!("p"),&json!({"enabled":false}));
        assert_eq!(save(State(state.clone()),Json(body)).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(state.store.count("automation"),1);
        let _=std::fs::remove_dir_all(root);
    }
}
