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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
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
