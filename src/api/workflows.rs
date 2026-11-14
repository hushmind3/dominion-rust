//! Durable multi-agent workflow API. The model never selects a fake agent
//! and a queued run snapshots its complete step/provider/model contract.
//! A request UUID deduplicates client retries without hiding a changed request.
use crate::{engine::jobs,state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list(State(state):State<AppState>)->Json<Value>{
    Json(json!(state.store.all("workflow")))
}
fn required<'a>(v:&'a Value,key:&str,max:usize)->Result<&'a str,String>{
    v[key].as_str().map(str::trim).filter(|s|!s.is_empty()&&s.len()<=max)
        .ok_or_else(||format!("필수 항목 '{key}'는 1~{max}자입니다."))
}
fn normalize(state:&AppState,body:&Value)->Result<Value,String>{
    let name=required(body,"name",120)?;
    let steps=body["steps"].as_array()
        .filter(|a|!a.is_empty()&&a.len()<=24)
        .ok_or("워크플로 단계는 1~24개여야 합니다.")?;
    let mut known=std::collections::HashSet::new();
    let mut normalized=Vec::new();
    for (i,step) in steps.iter().enumerate(){
        let title=required(step,"name",120)?;
        let prompt=required(step,"prompt",12000)?;
        let agent_id=required(step,"agentId",128)?;
        if state.store.get("agent",agent_id).is_none(){
            return Err(format!("{}단계에서 존재하지 않는 에이전트를 선택했습니다: {agent_id}",i+1))
        }
        let id=step["id"].as_str().filter(|x|uuid::Uuid::parse_str(x).is_ok())
            .map(str::to_string).unwrap_or_else(||uuid::Uuid::new_v4().to_string());
        if !known.insert(id.clone()){return Err("중복된 단계 ID입니다.".into())}
        let provider_id=step["providerId"].as_str().unwrap_or("").trim();
        let model=step["model"].as_str().unwrap_or("").trim();
        if provider_id.len()>128||model.len()>256 {
            return Err("모델·제공자 ID가 너무 깁니다.".into())
        }
        if !provider_id.is_empty() && state.store.get("provider",provider_id)
            .is_none_or(|provider|provider["enabled"]==false){
            return Err(format!("{}단계의 지정 모델 제공자를 연결하세요.",i+1))
        }
        // An optional real file-change acceptance criterion makes workflow
        // success testable without trusting the model's own prose.
        let verify_path=step["verifyPath"].as_str().unwrap_or("").trim();
        if verify_path.len()>512||verify_path.starts_with('/')||
            std::path::Path::new(verify_path).is_absolute() {
            return Err("검증 파일은 워크스페이스 내 상대 경로(512자 이하)여야 합니다.".into())
        }
        let checks=if !verify_path.is_empty() {
            json!([{"type":"file_changed","path":verify_path}])
        }else{step.get("successCriteria").cloned().unwrap_or_else(||json!([]))};
        crate::engine::verification::validate_criteria(&checks)?;
        normalized.push(json!({"id":id,"name":title,"prompt":prompt,
            "agentId":agent_id,"providerId":provider_id,"model":model,
            "verifyPath":verify_path,"successCriteria":checks}));
    }
    if normalized.iter().map(|s|s["prompt"].as_str().unwrap_or("").len()).sum::<usize>()>70_000 {
        return Err("전체 워크플로 지시문은 최대 70KB입니다.".into())
    }
    let id=body["id"].as_str().filter(|s|uuid::Uuid::parse_str(s).is_ok())
        .map(str::to_string).unwrap_or_else(||uuid::Uuid::new_v4().to_string());
    Ok(json!({"id":id,"name":name,
        "description":body["description"].as_str().unwrap_or("").chars().take(2000).collect::<String>(),
        "steps":normalized,"updatedAt":chrono::Utc::now().to_rfc3339()}))
}
/// Freeze the agent identity/model at admission time, not mid-execution.
/// This also fails BEFORE any billed provider call if credentials/models are
/// absent or disabled; it does not blindly claim API credentials work.
pub(crate) fn runnable(state:&AppState,definition:&Value)->Result<Value,String>{
    let mut result=definition.clone();
    let steps=result["steps"].as_array_mut().ok_or("워크플로 단계가 없습니다.")?;
    for step in steps {
        let agent_id=required(step,"agentId",128)?;
        let agent=state.store.get("agent",agent_id)
            .ok_or_else(||format!("에이전트가 삭제됐습니다: {agent_id}"))?;
        let provider_id=step["providerId"].as_str().filter(|s|!s.is_empty())
            .or_else(||agent["providerId"].as_str().filter(|s|!s.is_empty()))
            .ok_or_else(||format!("'{}' 에이전트에 모델 제공자를 먼저 연결하세요.",
                agent["name"].as_str().unwrap_or(agent_id)))?;
        let provider=state.store.get("provider",provider_id)
            .filter(|p|p["enabled"]!=false)
            .ok_or_else(||format!("비활성 또는 삭제된 모델 제공자입니다: {provider_id}"))?;
        let model=step["model"].as_str().filter(|s|!s.is_empty())
            .or_else(||agent["model"].as_str().filter(|s|!s.is_empty()))
            .or_else(||provider["model"].as_str().filter(|s|!s.is_empty()))
            .ok_or_else(||format!("제공자 {provider_id}에서 실행할 모델을 선택하세요."))?;
        // Clone borrowed identifiers before mutating the JSON step; frozen
        // admission values must not borrow the editable definition.
        let provider_id=provider_id.to_string();
        let model=model.to_string();
        step["providerId"]=json!(provider_id);
        step["model"]=json!(model);
    }
    Ok(result)
}
pub async fn save(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let normalized=match normalize(&state,&body){
        Ok(item)=>item,Err(message)=>return error(StatusCode::BAD_REQUEST,&message)
    };
    // A saved workflow is a reusable plan; every individual run receives an
    // immutable copy for incident/restart recovery.
    let Some(saved)=state.store.put("workflow",&normalized) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR,"워크플로를 저장하지 못했습니다.")
    };
    state.bus.record("workflow",&saved);
    Json(saved).into_response()
}
pub async fn delete(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(old)=state.store.get("workflow",&id) else {
        return error(StatusCode::NOT_FOUND,"워크플로를 찾을 수 없습니다.")
    };
    if state.store.all("job").iter().any(|job|
        job["kind"]=="workflow"&&job["input"]["workflowId"]==id&&
        matches!(job["status"].as_str(),Some("queued"|"running"))){
        return error(StatusCode::CONFLICT,"실행 중인 워크플로 작업이 있어 삭제할 수 없습니다.")
    }
    state.store.remove("workflow",&id);
    state.bus.emit(json!({"type":"record","kind":"workflow","removed":id}));
    Json(json!({"ok":true,"deleted":old["id"]})).into_response()
}
pub async fn run(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let Some(workflow)=state.store.get("workflow",&id) else {
        return error(StatusCode::NOT_FOUND,"워크플로를 찾을 수 없습니다.")
    };
    let input=body["input"].as_str().unwrap_or("");
    if input.chars().count()>16000 {
        return error(StatusCode::BAD_REQUEST,"작업 입력은 16000자를 넘을 수 없습니다.")
    }
    let definition=match runnable(&state,&workflow){
        Ok(value)=>value,Err(message)=>return error(StatusCode::CONFLICT,&message)
    };
    let request_id=if let Some(id)=body["requestId"].as_str(){
        match uuid::Uuid::parse_str(id){
            Ok(id)=>id.to_string(),
            Err(_)=>return error(StatusCode::BAD_REQUEST,"requestId는 UUID여야 합니다."),
        }
    }else {uuid::Uuid::new_v4().to_string()};
    let job=jobs::enqueue_once(&state,&request_id,"workflow",json!({
        "workflowId":id,"input":input,"definition":definition,
    }));
    match job {
        Some(value)=>Json(value).into_response(),
        None=>error(StatusCode::CONFLICT,
            "같은 요청 ID에 다른 워크플로 또는 입력이 등록돼 있습니다."),
    }
}
pub async fn history(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if state.store.get("workflow",&id).is_none(){
        return error(StatusCode::NOT_FOUND,"워크플로를 찾을 수 없습니다.")
    }
    let mut jobs=state.store.all("job").into_iter().filter(|j|
        j["kind"]=="workflow"&&j["input"]["workflowId"]==id).collect::<Vec<_>>();
    jobs.sort_by(|a,b|b["queuedAt"].as_str().unwrap_or("")
        .cmp(a["queuedAt"].as_str().unwrap_or("")));
    Json(json!(jobs.into_iter().take(60).collect::<Vec<_>>())).into_response()
}
pub async fn deliverable(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match state.store.get("workflow_deliverable",&id){
        Some(value)=>Json(value).into_response(),
        None=>error(StatusCode::NOT_FOUND,"완료된 워크플로 산출물이 없습니다."),
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,std::path::PathBuf){
        let root=std::env::temp_dir().join(format!("workflow-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        (AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path())),root)
    }
    #[tokio::test]
    async fn real_agent_workflow_is_frozen_and_retry_is_idempotent(){
        let (s,root)=state();
        s.store.put("provider",&json!({"id":"p","enabled":true,"model":"model-v1"}));
        s.store.put("agent",&json!({"id":"agent-one","name":"Writer","providerId":"p","model":"model-v1"}));
        let stored=normalize(&s,&json!({"name":"Write, review, deliver","steps":[
            {"name":"Write","agentId":"agent-one","prompt":"{{input}}"},
            {"name":"Review","agentId":"agent-one","prompt":"Review: {{previous}}"}]})).unwrap();
        let w=s.store.put("workflow",&stored).unwrap();
        let id=w["id"].as_str().unwrap();
        let request_id=uuid::Uuid::new_v4().to_string();
        let body=json!({"input":"Make a report","requestId":request_id});
        assert_eq!(run(State(s.clone()),Path(id.into()),Json(body.clone())).await.status(),StatusCode::OK);
        assert_eq!(run(State(s.clone()),Path(id.into()),Json(body.clone())).await.status(),StatusCode::OK);
        assert_eq!(s.store.all("job").len(),1);
        let original=s.store.all("job").remove(0);
        assert_eq!(original["input"]["definition"]["steps"][0]["model"],"model-v1");
        assert_eq!(original["input"]["definition"]["steps"][1]["agentId"],"agent-one");
        s.store.patch_if("provider","p","id",&json!("p"),&json!({"model":"model-v2"}));
        s.store.patch_if("agent","agent-one","id",&json!("agent-one"),&json!({"model":"model-v2"}));
        let frozen=s.store.get("job",&request_id).unwrap();
        assert_eq!(frozen["input"]["definition"]["steps"][0]["model"],"model-v1");
        let bad=json!({"input":"DIFFERENT","requestId":request_id});
        assert_eq!(run(State(s.clone()),Path(id.into()),Json(bad)).await.status(),StatusCode::CONFLICT);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn disabled_or_missing_model_cannot_queue_billable_work(){
        let (s,root)=state();
        s.store.put("agent",&json!({"id":"a","name":"No model"}));
        let w=normalize(&s,&json!({"name":"x","steps":[
            {"name":"stage","agentId":"a","prompt":"deliver"}]})).unwrap();
        assert!(runnable(&s,&w).is_err());
        s.store.put("provider",&json!({"id":"p","enabled":false,"model":"model"}));
        s.store.patch_if("agent","a","id",&json!("a"),&json!({"providerId":"p"}));
        assert!(runnable(&s,&w).is_err());
        let _=std::fs::remove_dir_all(root);
    }
}
