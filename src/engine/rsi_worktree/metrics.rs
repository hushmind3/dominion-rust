//! metrics: isolated RSI worktree responsibility; no automatic production deployment.
use super::*;

pub fn metric_cases()->Vec<Value>{
    [
        ("memory-quality","memory","memory_ranking",false),
        ("long-recall-quality","memory","long_owner_recall",true),
        ("planning-quality","planning","planning_quality",true),
        ("search-quality","improver","improvement_search",true),
        ("group-chat-input-cost","context","group_chat_context_efficiency",true),
    ].into_iter().map(|(id,domain,name,holdout)|json!({
        "id":id,"domain":domain,"holdout":holdout,"filter":format!("rsi_frozen_oracle::{name}"),
        "metrics":[
            {"key":"quality","direction":"higher","minRelativeGain":0.02,"minAbsoluteGain":0.025},
            {"key":"elapsedMs","direction":"lower","minRelativeGain":0.2,"minAbsoluteGain":1.0,"maxRelativeRegression":0.25}
        ]
    })).map(|mut case|{
        if case["id"]=="long-recall-quality"{
            case["metrics"].as_array_mut().unwrap().push(json!({"key":"contextChars","direction":"lower",
                "minRelativeGain":0.1,"minAbsoluteGain":20.0,"maxRelativeRegression":0.1}));
        }
        if case["id"]=="group-chat-input-cost"{
            case["metrics"]=json!([
                {"key":"quality","direction":"higher","minRelativeGain":0.02},
                {"key":"contextChars","direction":"lower","minRelativeGain":0.05,"minAbsoluteGain":50.0,"maxRelativeRegression":0.05}
            ]);
        }
        case
    }).collect()
}

/// Evidence from earlier autonomous source runs, reintroduced to the next
/// generation. A successful build is explicitly NOT behavioral improvement.
pub fn recent_feedback(state:&crate::state::AppState)->String{
    let mut runs=state.store.all("rsi_run");
    runs.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    let mut lines=Vec::<String>::new();
    for run in runs.into_iter().take(5){
        let job_id=run["jobId"].as_str().unwrap_or("");
        let objective=state.store.get("job",job_id)
            .and_then(|job|job["input"]["rsiObjective"].as_str().map(str::to_string))
            .unwrap_or_else(||"목표 기록 없음".into());
        let checks_ok=run["checksPassed"]==true;
        let invalid=run["captureError"].as_str().unwrap_or("");
        let model_err=run["modelError"].as_str().unwrap_or("");
        let broken=run["checks"].as_array().map(|checks|checks.iter()
            .filter(|check|check["passed"]!=true)
            .filter_map(|check|check["label"].as_str())
            .take(3).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let result=if checks_ok{"정적 검사 통과(실제 성능 향상 미검증)"}
            else {"정적 검사 실패 또는 실행되지 않음"};
        let paired_verdict=run["paired"]["verdict"].as_str().unwrap_or("비교 안 함");
        let error=if !invalid.is_empty(){invalid}
            else if !model_err.is_empty(){model_err}
            else if !broken.is_empty(){broken.as_str()}else{"증거 없음"};
        lines.push(format!(
            "- 과거 목표: {}; 결과: {}; A/B 검증: {}; 오류: {}; 후보: {}",
            objective.chars().take(220).collect::<String>(),
            result,paired_verdict,error.chars().take(220).collect::<String>(),
            run["candidateId"].as_str().unwrap_or("없음"),
        ));
    }
    if lines.is_empty(){"과거 자기개선 실행 증거 없음".into()}
    else {lines.join("\n")}
}

