use super::*;
use crate::{config::Config,db::Store,vault::Vault};
#[test]
fn model_tool_receipts_are_bounded_to_the_actual_descendant_and_policy(){
    let root=std::env::temp_dir().join(
        format!("dominion-observation-{}",uuid::Uuid::new_v4()));
    let cfg=Config{port:0,data_dir:root.join("data"),
        web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
        Vault::open(&cfg.key_path()));
    for (session,job,revision,origin) in [
        ("actual","worker-1","revision-1","model"),
        ("other","worker-2","revision-2","model"),
        ("actual","worker-2","revision-2","procedure")
    ] {
        state.store.put("execution_operation",&json!({
            "sessionId":session,"runId":job,"tool":"code_search",
            "status":"completed","origin":origin,
            "opSeq":1,"startedAt":"2026-11-13T00:00:00Z",
            "arguments":{"path":"src/engine/memory.rs"},
            "result":{"written":false},"toolReceipt":{"verified":true}
        })).unwrap();
        state.store.put("rsi_program_phase",&json!({
            "generationJobId":job,"policyRevision":revision,
            "recordedAt":"2026-11-13T00:00:00Z","phase":"ground"
        })).unwrap();
        state.store.put("rsi_rule_execution",&json!({
            "generationJobId":job,"policyRevision":revision,
            "recordedAt":"2026-11-13T00:00:00Z","status":"completed"
        })).unwrap();
    }
    let current=json!({
        "sessionId":"actual",
        "input":{"providerId":"unconfigured-provider","model":"unconfigured-model",
            "rsiEditPolicy":{"revision":"revision-1"}},
        "result":{"rsi":{"modelRouteVerified":false}}
    });
    let found=collect_generation_observation(&state,&current,"worker-1");
    assert_eq!(found.program_stages.len(),1);
    assert_eq!(found.rule_executions.len(),1);
    assert_eq!(found.tool_timeline.len(),1,
        "other sessions and procedure origins must not enter the model timeline");
    assert_eq!(found.tool_timeline[0]["runId"],"worker-1");
    assert!(!found.api_route_verified,
        "an unverified provider must never inherit a successful route claim");
    assert_eq!(found.selected_provider,"unconfigured-provider");
    assert_eq!(found.selected_model,"unconfigured-model");
    let _=std::fs::remove_dir_all(root);
}
