//! Route selection is a real Rust database read; no model or mock agent loop.
use super::*;
use crate::{config::Config,db::Store,vault::Vault};
use axum::{extract::State,Json};
use serde_json::json;

fn fixture()->(AppState,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!("dominion-routing-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    (state,root)
}
#[test]
fn no_model_does_not_invent_executor_or_create_a_goal(){
    let (state,root)=fixture();
    let advice=admission::prepare(&state,&json!({"goal":"고장난 기능을 고쳐 주세요"})).unwrap();
    assert_eq!(advice["ready"],false);
    assert_eq!(advice["nextPage"],"models");
    assert!(state.store.all("goal").is_empty());
    assert!(state.store.all("job").is_empty());
    drop(state);std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn explicit_agent_specialty_beats_unrelated_default_and_disabled_routes(){
    let (state,root)=fixture();
    state.store.put("provider",&json!({"id":"p1","name":"기본 모델","enabled":true,"model":"model-1"}));
    state.store.put("provider",&json!({"id":"p2","name":"다른 모델","enabled":true,"model":"model-2"}));
    state.store.put("provider",&json!({"id":"off","name":"disabled","enabled":false,"model":"free"}));
    state.store.put("agent",&json!({"id":"a0","name":"일반 업무","providerId":"p1","model":"model-1"}));
    state.store.put("agent",&json!({"id":"a1","name":"Rust 개발자","role":"coder",
        "providerId":"p2","model":"model-2"}));
    state.store.put("agent",&json!({"id":"a2","name":"코딩 전문가","providerId":"off","model":"free"}));
    let result=admission::prepare(&state,&json!({"goal":"Rust 코드 버그 수정"})).unwrap();
    assert_eq!(result["ready"],true);
    assert_eq!(result["taskKind"],"coding");
    assert_eq!(result["executor"]["agentId"],"a1");
    assert_eq!(result["executor"]["providerId"],"p2");
    assert_eq!(result["recommendedWorktree"],true);
    assert!(state.store.all("job").is_empty());
    drop(state);std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn shared_chat_router_quality_and_declared_prices_are_preserved(){
    let (state,root)=fixture();
    state.store.put("provider",&json!({"id":"weak","kind":"ollama","enabled":true,
        "model":"3b","capabilities":{"quality":0.1}}));
    state.store.put("provider",&json!({"id":"strong","kind":"openai","enabled":true,
        "model":"frontier","capabilities":{"quality":0.95},
        "costPerMillionInputKrw":2000.0,"costPerMillionOutputKrw":6000.0}));
    // This is a coding task despite being a short message. The engine uses
    // its existing quality-first Deep router, not local-only as a default.
    let selected=admission::prepare(&state,&json!({"goal":"Rust 코드 버그 수정"})).unwrap();
    assert_eq!(selected["executor"]["providerId"],"strong");
    assert_eq!(selected["declaredRateKrwPerMillionTokens"]["input"],2000.0);
    assert_eq!(selected["declaredRateKrwPerMillionTokens"]["output"],6000.0);
    assert!(selected["estimatedCostKrw"].is_null());
    drop(state);std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn local_configured_fallback_when_no_agent_never_claims_zero_price(){
    let (state,root)=fixture();
    state.store.put("provider",&json!({"id":"cloud","kind":"openai","enabled":true,"model":"cloud-model"}));
    state.store.put("provider",&json!({"id":"local","kind":"ollama","enabled":true,"model":"local-model",
        "capabilities":{"quality":0.7}}));
    let selected=admission::prepare(&state,&json!({"goal":"문서를 요약하세요"})).unwrap();
    assert_eq!(selected["executor"]["providerId"],"local");
    assert!(selected["estimatedCostKrw"].is_null());
    assert_eq!(selected["createsJob"],false);
    assert!(admission::prepare(&state,&json!({"goal":" " })).is_err());
    drop(state);std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn auto_route_enqueue_is_durable_and_missing_route_fails_without_ghost_job(){
    let (state,root)=fixture();
    let fail=crate::api::goals::create_goal(State(state.clone()),
        Json(json!({"goal":"요약을 해주세요","autoRoute":true}))).await;
    assert_eq!(fail.status(),axum::http::StatusCode::BAD_REQUEST);
    assert!(state.store.all("job").is_empty());
    state.store.put("provider",&json!({"id":"p1","name":"Configured","model":"m1","enabled":true}));
    let preview=admission::prepare(&state,&json!({"goal":"요약을 해주세요"})).unwrap();
    assert_eq!(preview["ready"],true);
    let result=crate::api::goals::create_goal(State(state.clone()),
        Json(json!({"goal":"요약을 해주세요","autoRoute":true,"tokenBudget":1234}))).await;
    assert_eq!(result.status(),axum::http::StatusCode::OK);
    let goals=state.store.all("goal");
    assert_eq!(goals.len(),1);
    assert_eq!(goals[0]["executor"]["providerId"],"p1");
    assert_eq!(goals[0]["routeSelection"]["source"],"configured_provider");
    assert_eq!(goals[0]["tokenBudget"],1234);
    assert_eq!(state.store.all("job").len(),1);
    drop(state);std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn explicit_user_agent_never_gets_overwritten_by_automatic_router(){
    let (state,root)=fixture();
    let result=crate::api::goals::create_goal(State(state.clone()),Json(json!({
        "goal":"특정 모델로 실행","autoRoute":true,"executor":{"agentId":"explicit-user"}
    }))).await;
    assert_eq!(result.status(),axum::http::StatusCode::OK);
    let goal=state.store.all("goal").pop().unwrap();
    assert_eq!(goal["executor"]["agentId"],"explicit-user");
    assert!(goal["routeSelection"].is_null());
    let invalid=crate::api::goals::create_goal(State(state.clone()),Json(json!({
        "goal":"잘못된 옵션","autoRoute":"yes"
    }))).await;
    assert_eq!(invalid.status(),axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(state.store.all("goal").len(),1);
    drop(state);std::fs::remove_dir_all(root).unwrap();
}
