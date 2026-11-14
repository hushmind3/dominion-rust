use super::*;
use serde_json::json;

#[test]
fn explicit_full_room_preserves_owner_turns() {
    let full=json!({"economyMode":"full","routingMode":"discussion","rounds":5});
    assert_eq!(effective_rounds(&full,"안녕하세요",false,5,false),5);
    assert!(!active(&full));
}
#[test]
fn adaptive_policy_bounds_simple_and_complex_discussion() {
    let room=json!({"economyMode":"adaptive","routingMode":"discussion",
        "economyKernel":{"parameters":{"casualSpeakers":1,
        "mediumSpeakers":2,"deepSpeakers":5,"deepRounds":2}}});
    assert!(active(&room));
    assert_eq!(target_speakers(&room,false,5),1);
    assert_eq!(target_speakers(&room,true,500),5);
    assert_eq!(effective_rounds(&room,"안녕하세요",false,4,false),1);
    assert_eq!(effective_rounds(&room,"분석",false,4,true),2);
    assert_eq!(effective_rounds(&room,"분석",true,4,true),4);
    assert_eq!(effective_rounds(&room,"모두 분석해주세요",false,4,true),4);
}
#[test]
fn invalid_or_missing_algorithm_parameters_do_not_expand_work() {
    let room=json!({"economyMode":"adaptive","routingMode":"discussion",
        "economyKernel":{"parameters":{"deepSpeakers":999999,"deepRounds":0}}});
    assert_eq!(target_speakers(&room,true,300),32);
    assert_eq!(effective_rounds(&room,"분석",false,2,true),1);
}

#[test]
fn resumed_room_job_reuses_original_metering_baseline() {
    let first=json!({"roomId":"room-x","generation":0});
    assert_eq!(metering_baseline(&first,400),400);
    assert!(!budget_spent(200,400,590));
    assert!(budget_spent(200,400,600));
    let persisted=json!({"roomId":"room-x","usageBaselineTokens":400});
    assert_eq!(metering_baseline(&persisted,600),400);
    assert!(budget_spent(200,metering_baseline(&persisted,600),600));
}
#[test]
fn unlimited_room_and_new_owner_exchange_preserve_correct_boundaries() {
    let unlimited=json!({"usageBaselineTokens":100});
    assert!(!budget_spent(0,metering_baseline(&unlimited,5000),5000));
    // Different owner message -> different durable job -> new baseline.
    let fresh=json!({"roomId":"room-x","sourceMessageId":"owner-new"});
    assert_eq!(metering_baseline(&fresh,610),610);
    assert!(!budget_spent(300,metering_baseline(&fresh,610),610));
    assert!(budget_spent(300,metering_baseline(&fresh,610),910));
    assert!(!budget_spent(300,910,900)); // accounting restored to an older cursor
}

#[test]
fn per_exchange_metrics_only_include_measured_work_since_frozen_baseline() {
    let original=json!({
        "calls":5,"promptTokens":1000,"completionTokens":200,"totalTokens":1200,
        "estimatedCalls":1,"cachedPromptTokens":150,
        "cacheEligiblePromptTokens":400,"cacheReportedCalls":2
    });
    let observed=json!({
        "calls":8,"promptTokens":1700,"completionTokens":370,"totalTokens":2070,
        "estimatedCalls":1,"cachedPromptTokens":350,
        "cacheEligiblePromptTokens":1000,"cacheReportedCalls":4
    });
    let delta=observed_delta(&original,&observed);
    assert_eq!(delta["calls"],3);
    assert_eq!(delta["promptTokens"],700);
    assert_eq!(delta["completionTokens"],170);
    assert_eq!(delta["totalTokens"],870);
    assert_eq!(delta["cachedPromptTokens"],200);
    assert_eq!(delta["cacheReportedCalls"],2);
    assert_eq!(delta["scope"],"one-room-exchange");
}
#[test]
fn migrated_job_only_knows_original_total_tokens_not_other_usage() {
    let original=json!({"totalTokens":300});
    let observed=json!({"totalTokens":450,"calls":8,"cachedPromptTokens":150});
    let delta=observed_delta(&original,&observed);
    assert_eq!(delta["totalTokens"],150);
    assert!(delta["calls"].is_null());
    assert!(delta["cachedPromptTokens"].is_null());
}
