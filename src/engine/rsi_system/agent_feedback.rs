//! Real autonomous goal receipts become bounded research feedback for RSI.
//! These records are NEVER frozen benchmark wins, and cannot adopt a patch.
//! Only the independent paired evaluator may establish improvement quality.
use crate::state::AppState;
use serde_json::{json,Value};

fn hint(gaps:&[String])->(&'static str,Vec<&'static str>){
    let joined=gaps.iter().take(8).map(String::as_str).collect::<Vec<_>>().join(" ").to_lowercase();
    if joined.contains("토큰")||joined.contains("budget")||joined.contains("비용"){
        ("usage",vec!["src/engine/gateway.rs","src/engine/goals.rs"])
    }else if joined.contains("도구")||joined.contains("tool")||joined.contains("mcp"){
        ("tool",vec!["src/engine/tools.rs","src/engine/chat/runtime.rs"])
    }else if joined.contains("검증")||joined.contains("acceptance")||joined.contains("criteria"){
        ("verification",vec!["src/engine/verification.rs","src/engine/goals.rs"])
    }else{
        ("goal_harness",vec!["src/engine/goals.rs","src/engine/jobs.rs"])
    }
}
pub(super) fn aggregate(rows:&[Value])->Value{
    let mut total=0usize;
    let mut completed=0usize;
    let mut unresolved=0usize;
    let mut iterations=0u64;
    let mut replans=0u64;
    let mut tokens=0u64;
    let mut signals=Vec::new();
    for row in rows.iter().take(64){
        let Some(status)=row["status"].as_str().filter(|s|
            matches!(*s,"completed"|"failed"|"cancelled"|"needs_review")) else{continue;};
        let receipt=&row["receipt"];
        if receipt["status"]!=status
            || receipt["verificationOwner"]!="goal-parent"
            || receipt["verified"]!=(status=="completed"){continue;}
        total+=1;
        iterations=iterations.saturating_add(receipt["iterations"].as_u64().unwrap_or(0));
        replans=replans.saturating_add(receipt["replans"].as_u64().unwrap_or(0));
        tokens=tokens.saturating_add(receipt["tokenUsage"]["totalTokens"].as_u64().unwrap_or(0));
        if status=="completed"{completed+=1;continue;}
        unresolved+=1;
        let gaps=row["gaps"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(8)
            .map(str::to_string).collect::<Vec<_>>();
        let (category,targets)=hint(&gaps);
        // Curated, real source files are hints for an experiment, not causes
        // established by an evaluator. Preserve goal ID for audit replay.
        if signals.len()<6 {
            if let Some(id)=row["id"].as_str().filter(|id|id.len()<=100){
                signals.push(json!({
                    "id":format!("rsi-signal-agent-goal-{id}"),
                    "kind":"agent_goal_failure","sourceGoalId":id,
                    "categoryHypothesis":category,
                    "targetFiles":targets,
                    "title":"실제 사용자 목표의 미완수 관측",
                    "detail":gaps.iter().take(3)
                        .map(|v|v.chars().take(140).collect::<String>())
                        .collect::<Vec<_>>().join(" | "),
                    "qualityProven":false,
                    "pairedEvaluation":false,
                    "suggestedRequest":"실제 목표 실패 증거를 조사하고 하네스·검증·도구 구조에 대한 독립 후보를 생성하세요. 동결 A/B에서 성공 전에는 개선으로 취급하지 마세요."
                }));
            }
        }
    }
    json!({
        "source":"persisted_owner_goal_receipts",
        "sampleCount":total,"completedCount":completed,"unresolvedCount":unresolved,
        "totalIterations":iterations,"totalReplans":replans,
        "recordedExecutorTokens":tokens,
        "signals":signals,
        "qualityProven":false,"pairedEvaluation":false,
        "purpose":"RSI source target prioritization, never automatic reward or adoption"
    })
}
pub(super) fn observe(state:&AppState)->Value{
    let mut goals=state.store.all("goal");
    goals.retain(|g|g["finishedAt"].is_string()&&g["receipt"].is_object());
    goals.sort_by(|a,b|b["finishedAt"].as_str().cmp(&a["finishedAt"].as_str()));
    goals.truncate(64);
    aggregate(&goals)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn only_parent_completed_receipts_count_and_failures_never_prove_rsi_gain(){
        let good=json!({"id":"success","status":"completed","receipt":{
            "status":"completed","verified":true,"verificationOwner":"goal-parent",
            "iterations":2,"replans":1,"tokenUsage":{"totalTokens":400}}});
        let fail=json!({"id":"broken","status":"needs_review",
            "gaps":["도구 실패: timeout"],"receipt":{
            "status":"needs_review","verified":false,"verificationOwner":"goal-parent",
            "iterations":3,"tokenUsage":{"totalTokens":60}}});
        let spoof=json!({"id":"fake","status":"completed","receipt":{
            "status":"failed","verified":true,"verificationOwner":"goal-parent"}});
        let report=aggregate(&[good,fail,spoof]);
        assert_eq!(report["sampleCount"],2);
        assert_eq!(report["completedCount"],1);
        assert_eq!(report["unresolvedCount"],1);
        assert_eq!(report["recordedExecutorTokens"],460);
        assert_eq!(report["signals"][0]["categoryHypothesis"],"tool");
        assert_eq!(report["signals"][0]["qualityProven"],false);
        assert_eq!(report["pairedEvaluation"],false);
    }
}
