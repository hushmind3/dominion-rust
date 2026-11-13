use super::*;
use crate::{config::Config,db::Store,vault::Vault};
#[test]
fn three_claimed_improvements_are_not_a_method_closure_without_real_receipts(){
    let root=std::env::temp_dir().join(format!("rsi-method-closure-{}",uuid::Uuid::new_v4()));
    let config=Config{port:0,data_dir:root.join("data"),web_dir:root.join("web"),
        workspace:root.clone()};
    let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
        Vault::open(&config.key_path()));
    let fake=(0..3).map(|i|serde_json::json!({
        "accepted":true,"acceptedCommit":format!("{:040x}",i+2),
        "generatorCommit":format!("{:040x}",i+1),
        "operatorExecution":{"source":"verified_compiled_operator_tool_edit_v1"},
        "evaluation":{"metrics":{"eligible":true,"metaHoldoutGains":1}}
    })).collect::<Vec<_>>();
    assert!(!proven(&state,&fake));
    let fake_four=fake.iter().cloned().chain(std::iter::once(
        serde_json::json!({"accepted":true,"generatorCommit":format!("{:040x}",4),
            "acceptedCommit":format!("{:040x}",5),
            "methodAblation":{"qualityProven":true,
                "methodAssociationObserved":true}}))).collect::<Vec<_>>();
    assert!(!proven(&state,&fake_four),
        "four accepted patches and claimed method superiority are no proof");
    assert!(evidence(&state,&fake_four).is_none(),
        "unbacked ON/OFF quality claims must fail the four-stage audit");
    assert!(evidence(&state,&fake).is_none(),
        "a fabricated A-B-A narrative must not manufacture a receipt chain");
    let _=std::fs::remove_dir_all(root);
}
