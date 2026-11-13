use super::*;
use serde_json::json;

#[test]
fn room_session_reports_measured_prompt_cache_without_counting_unknown_as_misses() {
    let dir=std::env::temp_dir().join(format!("dominion-cache-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store=Store::open(&dir.join("cache.sqlite")).unwrap();
    store.audit("model.usage",json!({"sessionId":"room:a","promptTokens":1000,"completionTokens":10,
        "totalTokens":1010,"estimated":false,"cachedPromptTokens":800}));
    store.audit("model.usage",json!({"sessionId":"room:a","promptTokens":500,"completionTokens":20,
        "totalTokens":520,"estimated":true,"cachedPromptTokens":400}));
    store.audit("model.usage",json!({"sessionId":"room:a","promptTokens":100,"completionTokens":10,
        "totalTokens":110}));
    store.audit("model.usage",json!({"sessionId":"room:b","promptTokens":900,
        "completionTokens":10,"totalTokens":910,"cachedPromptTokens":900}));
    let usage=store.session_usage("room:a");
    assert_eq!(usage["calls"],3);
    assert_eq!(usage["promptTokens"],1600);
    assert_eq!(usage["cachedPromptTokens"],800);
    assert_eq!(usage["cacheEligiblePromptTokens"],1000);
    assert_eq!(usage["cacheReportedCalls"],1);
    drop(store);
    let _=std::fs::remove_dir_all(&dir);
}
