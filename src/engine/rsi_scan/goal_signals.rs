//! Verified owner-goal delivery signals for RSI research prioritization.
//! These are operational observations, never independent RSI holdout gains.
use super::signal;
use crate::state::AppState;
use serde_json::{json,Value};
use std::collections::BTreeMap;

fn failure_category(goal:&Value)->Option<&'static str>{
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed")) {
        return None;
    }
    // User rollback is a deliberate preference, not a software failure.
    if matches!(goal["phase"].as_str(),Some("owner_undo"|"worktree_discarded")){
        return None;
    }
    if goal["metrics"]["verifiedCompleted"]==true {
        return None;
    }
    let evidence=goal["evidence"].as_array().map(Vec::len).unwrap_or(0);
    let gaps=goal["gaps"].as_array().map(Vec::len).unwrap_or(0);
    if evidence==0 {
        Some("no_confirmed_tool_progress")
    } else if gaps>0 {
        Some("unmet_acceptance")
    } else {
        Some("unverified_completion")
    }
}
fn targets(category:&str)->&'static [&'static str]{
    match category {
        "no_confirmed_tool_progress"=>&[
            "src/engine/chat/runtime/turn.rs","src/engine/tools/dispatch.rs"],
        "unmet_acceptance"=>&[
            "src/engine/goals.rs","src/engine/verification.rs","src/engine/goal_verification.rs"],
        _=>&["src/engine/goals.rs","src/engine/goal_review.rs"],
    }
}
fn hint(category:&str)->&'static str{
    match category {
        "no_confirmed_tool_progress"=>
            "완료되지 않은 목표에서 모델의 도구 선택과 실제 실행 영수증을 비교하고, 기존 검증 조건을 보존하는 작은 하네스 개선을 제안하세요.",
        "unmet_acceptance"=>
            "실제 도구 실행은 있었으나 완료 조건을 만족하지 못한 목표의 검증·재계획 경로를 조사하고, 독립 동결 평가에서 시험할 수정안을 만드세요.",
        _=>
            "실제 사용자 목표의 검토·체크포인트 경로를 분석하고 잘못된 완료 판정과 재실행을 방지하는 소스 개선안을 검토하세요.",
    }
}
pub(super) fn collect(state:&AppState)->(Vec<Value>,Value){
    let mut rows=state.store.all("goal");
    rows.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    rows.truncate(64);
    let total=rows.len();
    let verified=rows.iter().filter(|row|row["status"]=="completed"
        && row["metrics"]["verifiedCompleted"]==true).count();
    let mut grouped=BTreeMap::<&'static str,Vec<&Value>>::new();
    for goal in &rows {
        if let Some(category)=failure_category(goal) {
            grouped.entry(category).or_default().push(goal);
        }
    }
    let unresolved=grouped.values().map(Vec::len).sum::<usize>();
    let mut signals=Vec::new();
    for (category,goals) in grouped {
        if goals.len()<2 {continue;}
        // No free-text prompts, code, or private messages enter this feedback:
        // only bounded objective receipts and count-based diagnostics.
        let evidence=goals.iter().take(4).map(|goal|json!({
            "goalId":goal["id"],"status":goal["status"],
            "iterations":goal["iterations"],"replans":goal["replans"],
            "toolEvidenceCount":goal["evidence"].as_array().map(Vec::len).unwrap_or(0),
            "unmetCheckCount":goal["gaps"].as_array().map(Vec::len).unwrap_or(0),
            "recordedTokens":goal["metrics"]["usage"]["totalTokens"],
            "observedVerifiedCompletion":false,
        })).collect::<Vec<_>>();
        signals.push(signal("owner_goal_repeated_failure",category,
            "실제 사용자 목표에서 반복된 미완료 유형",
            format!("최근 목표 기록 {}건에서 {category} 유형 미완료 {}건을 관측했습니다. 독립 RSI 성공 증거는 아닙니다.",total,goals.len()),
            evidence,hint(category).into(),targets(category)));
    }
    let summary=json!({"source":"owner_goal_operational_receipts",
        "inspected":total,"verifiedCompletions":verified,
        "unresolved":unresolved,"independentPairedEvaluation":false,
        "countAsRsiAdoption":false,
        "purpose":"prioritize source research; frozen evaluator retains sole adoption authority"});
    (signals,summary)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_repeated_observed_failures_guide_research_not_adoption(){
        let home=std::env::temp_dir().join(format!("goal-rsi-signal-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let cfg=crate::config::Config{port:0,data_dir:home.clone(),
            web_dir:home.join("web"),workspace:home.clone()};
        let store=crate::db::Store::open(&cfg.db_path()).unwrap();
        let vault=crate::vault::Vault::open(&cfg.key_path());
        let state=AppState::new(cfg,store,vault);
        for i in 0..3 {
            state.store.put("goal",&json!({"status":if i==2 {"completed"}else{"needs_review"},
                "phase":"review","gaps":["redacted user data"],"evidence":[],
                "createdAt":format!("2026-11-14T11:00:0{i}+09:00"),
                "metrics":{"verifiedCompleted":i==2,
                    "usage":{"totalTokens":100+i}}})).unwrap();
        }
        let (signals,summary)=collect(&state);
        assert_eq!(signals.len(),1);
        assert_eq!(summary["verifiedCompletions"],1);
        assert_eq!(summary["countAsRsiAdoption"],false);
        let serialized=signals[0].to_string();
        assert!(!serialized.contains("redacted user data"));
        assert!(serialized.contains("owner_goal_repeated_failure"));
        drop(state);
        std::fs::remove_dir_all(home).unwrap();
    }
}
