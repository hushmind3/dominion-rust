//! Source-of-truth telemetry for an autonomous goal's final outcome.
/// Audit-backed token usage is reported with its measurement limitations.
/// This module never invents provider pricing or model-quality scores.
use crate::state::AppState;
use serde_json::{json,Value};

pub(crate) fn final_report(state:&AppState, goal:&Value, status:&str, gaps:&[String]) -> Value {
    let session = goal["sessionId"].as_str().unwrap_or("");
    let usage = if session.is_empty() { Value::Null } else {state.store.session_usage(session)};
    let now = chrono::Utc::now();
    let duration_ms = goal["startedAt"].as_str()
        .and_then(|date|chrono::DateTime::parse_from_rfc3339(date).ok())
        .map(|start|now.signed_duration_since(start.with_timezone(&chrono::Utc))
            .num_milliseconds().max(0));
    let evidence_count = goal["evidence"].as_array().map(Vec::len).unwrap_or(0);
    let review_count = goal["reviewHistory"].as_array().map(Vec::len).unwrap_or(0);
    let checks = goal["acceptanceChecks"].as_array().map(Vec::len).unwrap_or(0);
    json!({
        "source":"goal_final_audit_v1",
        "verifiedCompleted":status=="completed"&&gaps.is_empty(),
        "status":status,"durationMs":duration_ms,
        "iterations":goal["iterations"].as_u64().unwrap_or(0),
        "replans":goal["replans"].as_u64().unwrap_or(0),
        "reviewCount":review_count,
        "acceptanceCheckCount":checks,
        "unmetGapCount":gaps.len(),
        "successfulToolEvidenceCount":evidence_count,
        "usage":if usage.is_null() {Value::Null}else{json!({
            "calls":usage["calls"],"promptTokens":usage["promptTokens"],
            "completionTokens":usage["completionTokens"],"totalTokens":usage["totalTokens"],
            "estimatedCalls":usage["estimatedCalls"],
            "cacheReportedCalls":usage["cacheReportedCalls"],
            "cachedPromptTokens":usage["cachedPromptTokens"],
        })},
        "billingAmount":Value::Null,
    })
}

/// Aggregates actual durable outcomes, not mocked review claims. Every
/// terminal goal is in the denominator; a "completed" label without a
/// matching server final-audit receipt is NOT a verified success.
/// Missing token/cache/billing evidence is null or counted as missing, never 0.
pub(crate) fn aggregate(state:&AppState)->Value {
    use std::collections::BTreeMap;
    let mut terminal=0u64;
    let mut active=0u64;
    let mut verified=0u64;
    let mut unknown_audits=0u64;
    let mut zero_usage=0u64;
    let mut observed_calls=0u64;
    let mut estimated_calls=0u64;
    let mut total_tokens=0u64;
    let mut measured_tokens=0u64;
    let mut measured_goals=0u64;
    let mut observed_cache_calls=0u64;
    let mut cached_tokens=0u64;
    let mut cache_measured_tokens=0u64;
    let mut duration_ms=Vec::new();
    let mut by_status:BTreeMap<String,u64>=BTreeMap::new();
    let mut failures=BTreeMap::<String,u64>::new();
    for goal in state.store.all("goal") {
        let status=goal["status"].as_str().unwrap_or("unknown");
        *by_status.entry(status.to_string()).or_default()+=1;
        if !matches!(status,"completed"|"failed"|"cancelled"|"needs_review"){
            active+=1;
            continue;
        }
        terminal+=1;
        let report=&goal["metrics"];
        let receipt=report["source"]=="goal_final_audit_v1"
            && report["status"]==status;
        if !receipt {unknown_audits+=1;}
        if receipt&&report["verifiedCompleted"]==true&&status=="completed"
            && report["unmetGapCount"].as_u64()==Some(0)
        {
            verified+=1;
        } else {
            let reason=if !receipt {"missing_final_audit"}
                else if status=="completed" {"unverified_completion"}
                else {status};
            *failures.entry(reason.to_string()).or_default()+=1;
        }
        if let Some(ms)=report["durationMs"].as_u64(){duration_ms.push(ms);}
        let session=goal["sessionId"].as_str().unwrap_or("");
        if session.is_empty(){zero_usage+=1;continue;}
        let usage=state.store.session_usage(session);
        let calls=usage["calls"].as_u64().unwrap_or(0);
        if calls==0 {zero_usage+=1;continue;}
        observed_calls+=calls;
        let estimates=usage["estimatedCalls"].as_u64().unwrap_or(0);
        estimated_calls+=estimates;
        total_tokens=total_tokens.saturating_add(usage["totalTokens"].as_u64().unwrap_or(0));
        if estimates==0 {
            measured_goals+=1;
            measured_tokens=measured_tokens.saturating_add(usage["totalTokens"].as_u64().unwrap_or(0));
        }
        observed_cache_calls+=usage["cacheReportedCalls"].as_u64().unwrap_or(0);
        cached_tokens+=usage["cachedPromptTokens"].as_u64().unwrap_or(0);
        cache_measured_tokens+=usage["cacheMeasuredPromptTokens"].as_u64().unwrap_or(0);
    }
    duration_ms.sort_unstable();
    let median=duration_ms.get(duration_ms.len()/2).copied();
    json!({
        "source":"durable_goal_audit_aggregate_v1",
        "observedGoalCount":terminal+active,
        "terminalGoals":terminal,
        "activeGoals":active,
        "verifiedCompletions":verified,
        "verifiedSuccessRate":if terminal==0 {Value::Null}
            else {json!(verified as f64/terminal as f64)},
        "unknownOrLegacyAuditCount":unknown_audits,
        "byStatus":by_status,
        "failureCategories":failures,
        "medianObservedDurationMs":median,
        "modelUsage":{
            "goalsWithoutUsage":zero_usage,
            "observedCalls":observed_calls,
            "estimatedCalls":estimated_calls,
            "reportedTotalTokens":if observed_calls==0 {Value::Null}
                else {json!(total_tokens)},
            "strictlyMeasuredGoalCount":measured_goals,
            "strictlyMeasuredTokens":if measured_goals==0 {Value::Null}
                else {json!(measured_tokens)},
            "cacheReportedCalls":observed_cache_calls,
            "cachedPromptTokens":if observed_cache_calls==0 {Value::Null}
                else {json!(cached_tokens)},
            "cacheHitRateFromReportedCalls":if cache_measured_tokens==0 {Value::Null}
                else {json!(cached_tokens as f64/cache_measured_tokens as f64)}
        },
        "actualProviderBillingKrw":Value::Null,
        "measurement":"Counts persisted production goals; not an isolated benchmark. Provider bills require external invoice reconciliation.",
        "recordedAt":chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn aggregate_never_counts_unverified_or_legacy_completion_as_success() {
        let dir=std::env::temp_dir().join(format!("goal-aggregate-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let empty=aggregate(&state);
        assert_eq!(empty["terminalGoals"],0);
        assert!(empty["verifiedSuccessRate"].is_null());
        assert!(empty["modelUsage"]["reportedTotalTokens"].is_null());
        state.store.put("goal",&json!({"id":"verified","status":"completed",
            "sessionId":"s1","metrics":{"source":"goal_final_audit_v1",
                "status":"completed","verifiedCompleted":true,
                "unmetGapCount":0,"durationMs":1050}}));
        state.store.put("goal",&json!({"id":"claim-only","status":"completed",
            "metrics":{"verifiedCompleted":true}}));
        state.store.put("goal",&json!({"id":"failed","status":"failed",
            "metrics":{"source":"goal_final_audit_v1","status":"failed",
                "verifiedCompleted":false,"unmetGapCount":1}}));
        state.store.put("goal",&json!({"id":"queued","status":"queued"}));
        // Persisted provider receipt, NOT a fabricated 'tokens saved' claim.
        state.store.audit("model.usage",json!({"sessionId":"s1",
            "promptTokens":82,"completionTokens":18,"totalTokens":100,
            "estimated":false}));
        let report=aggregate(&state);
        assert_eq!(report["terminalGoals"],3);
        assert_eq!(report["activeGoals"],1);
        assert_eq!(report["verifiedCompletions"],1);
        assert_eq!(report["unknownOrLegacyAuditCount"],1);
        assert!((report["verifiedSuccessRate"].as_f64().unwrap()-1.0/3.0).abs()<1e-9);
        assert_eq!(report["modelUsage"]["reportedTotalTokens"],100);
        assert_eq!(report["modelUsage"]["strictlyMeasuredGoalCount"],1);
        assert!(report["actualProviderBillingKrw"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn final_report_never_invents_money_or_model_quality() {
        let dir=std::env::temp_dir().join(format!("goal-metrics-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        let goal=json!({"iterations":2,"replans":1,"reviewHistory":[{},{}],
            "evidence":[{"tool":"workspace_write"}],"acceptanceChecks":[{}]});
        let report=final_report(&state,&goal,"completed",&[]);
        assert_eq!(report["verifiedCompleted"],true);
        assert!(report["durationMs"].is_null());
        assert!(report["billingAmount"].is_null());
        assert_eq!(report["successfulToolEvidenceCount"],1);
        let failed=final_report(&state,&goal,"needs_review",&["missing".into()]);
        assert_eq!(failed["verifiedCompleted"],false);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
