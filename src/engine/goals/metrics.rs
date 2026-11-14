//! Runtime goal outcome and token telemetry, derived only from durable records.
//! No simulated success counts, no guessed API price, no claimed benchmark score.
use crate::state::AppState;
use serde_json::{json,Value};
use std::collections::{BTreeMap,HashMap,HashSet};

fn count(value:&Value,key:&str)->u64{value[key].as_u64().unwrap_or(0)}
fn observed_success(goal:&Value, job:Option<&Value>)->bool{
    goal["status"]=="completed"
        && goal["finalReport"]["accepted"]==true
        && job.is_some_and(|job|job["kind"]=="goal"
            && job["status"]=="completed"
            && job["input"]["goalId"]==goal["id"])
}
fn settled(status:&str)->bool{
    matches!(status,"completed"|"failed"|"cancelled"|"needs_review")
}

/// The same finite audit cursor is used for every goal, so the endpoint
/// cannot race a later provider log into only part of its summary.
pub fn snapshot(state:&AppState)->Result<Value,String>{
    let goals=state.store.try_all("goal")?;
    let jobs=state.store.try_all("job")?;
    let jobs_by_id=jobs.iter().filter_map(|job|
        job["id"].as_str().map(|id|(id.to_string(),job))
    ).collect::<HashMap<_,_>>();
    let through=state.store.audit_rowid_cursor()?;
    let mut status_counts=BTreeMap::<String,u64>::new();
    let mut rows=Vec::<Value>::new();
    let mut seen_sessions=HashSet::<String>::new();
    let mut accepted=0_u64;
    let mut delivered=0_u64;
    let mut accepted_waiting_for_apply=0_u64;
    let mut settled_count=0_u64;
    let mut incomplete_acceptance=0_u64;
    let mut usage_sessions=0_u64;
    let mut missing_session=0_u64;
    let mut no_usage_audit=0_u64;
    let mut shared_session_links=0_u64;
    let mut calls=0_u64;
    let mut estimated_calls=0_u64;
    let mut prompt_tokens=0_u64;
    let mut completion_tokens=0_u64;
    let mut total_tokens=0_u64;
    let mut cached_prompt_tokens=0_u64;
    let mut cache_eligible_prompt_tokens=0_u64;
    let mut cache_reported_calls=0_u64;
    let mut total_replans=0_u64;
    let mut total_iterations=0_u64;
    for goal in &goals {
        let status=goal["status"].as_str().unwrap_or("unknown");
        *status_counts.entry(status.into()).or_insert(0)+=1;
        let is_settled=settled(status);
        if is_settled{settled_count+=1;}
        let job=goal["jobId"].as_str().and_then(|id|jobs_by_id.get(id).copied());
        let checked=observed_success(goal,job);
        if checked {
            accepted+=1;
            // A verified diff in an isolated worktree is not yet the delivered
            // source. Keep user-approved Git adoption separate.
            if goal["isolateWorktree"]==true && goal["gitWorktree"]["applied"]!=true {
                accepted_waiting_for_apply+=1;
            }else{delivered+=1;}
        }
        if status=="completed" && !checked {incomplete_acceptance+=1;}
        total_replans=total_replans.saturating_add(count(goal,"replans"));
        total_iterations=total_iterations.saturating_add(count(goal,"iterations"));
        let session=goal["sessionId"].as_str().filter(|s|!s.is_empty());
        let mut row_calls=None::<u64>;
        let mut row_tokens=None::<u64>;
        let mut row_estimated=None::<u64>;
        if let Some(session)=session {
            if !seen_sessions.insert(session.to_string()){
                shared_session_links+=1;
            } else {
                // The error-preserving audit query must not turn a storage error
                // into a false zero-token run, as session_usage() can.
                let events=state.store.model_usage_for_session_in_window(session,0,through)?;
                if events.is_empty() {no_usage_audit+=1;}
                else {usage_sessions+=1;}
                let mut this_calls=0_u64;
                let mut this_tokens=0_u64;
                let mut this_estimated=0_u64;
                for event in &events {
                    let detail=&event["detail"];
                    this_calls+=1;
                    let estimated=detail["estimated"]==true;
                    if estimated {this_estimated+=1;}
                    let prompt=count(detail,"promptTokens");
                    let completion=count(detail,"completionTokens");
                    let token=count(detail,"totalTokens");
                    prompt_tokens=prompt_tokens.saturating_add(prompt);
                    completion_tokens=completion_tokens.saturating_add(completion);
                    total_tokens=total_tokens.saturating_add(token);
                    this_tokens=this_tokens.saturating_add(token);
                    if !estimated {
                        if let Some(cached)=detail["cachedPromptTokens"].as_u64()
                            .filter(|cached|*cached<=prompt) {
                            cached_prompt_tokens=cached_prompt_tokens.saturating_add(cached);
                            cache_eligible_prompt_tokens=cache_eligible_prompt_tokens.saturating_add(prompt);
                            cache_reported_calls+=1;
                        }
                    }
                }
                calls=calls.saturating_add(this_calls);
                estimated_calls=estimated_calls.saturating_add(this_estimated);
                row_calls=Some(this_calls);
                row_tokens=Some(this_tokens);
                row_estimated=Some(this_estimated);
            }
        }else if is_settled || count(goal,"iterations")>0 {
            // A newly queued goal has not yet generated any provider calls;
            // absence of a session at admission is not missing telemetry.
            missing_session+=1;
        }
        rows.push(json!({
            "goalId":goal["id"],"status":status,"jobId":goal["jobId"],
            "jobStatus":job.map(|job|job["status"].clone()),
            "internalAccepted":checked,
            "deliveredToSource":checked
                && (goal["isolateWorktree"]!=true ||goal["gitWorktree"]["applied"]==true),
            "verificationRecorded":goal["finalReport"].is_object(),
            "iterations":goal["iterations"],"replans":goal["replans"],
            "sessionId":session,
            "modelCalls":row_calls,"recordedTokens":row_tokens,
            "estimatedCalls":row_estimated,
            "startedAt":goal["startedAt"],"finishedAt":goal["finishedAt"]
        }));
    }
    rows.sort_by(|a,b|b["finishedAt"].as_str().unwrap_or("")
        .cmp(a["finishedAt"].as_str().unwrap_or("")));
    let denom_covered=missing_session==0 && no_usage_audit==0 && shared_session_links==0;
    let rate=if settled_count>0 {
        Some(accepted as f64/settled_count as f64)
    }else{None};
    // This is a *recorded-token efficiency* statistic, not a billed price.
    // It includes failed and pending attempts to avoid selective omission.
    let tokens_per_accepted=if accepted>0 && denom_covered {
        Some(total_tokens as f64/accepted as f64)
    }else{None};
    Ok(json!({
        "schemaVersion":1,
        "source":"local_sqlite_goals_and_model_usage_audit",
        "proofLevel":"runtime_observation_not_independent_benchmark",
        "externalOutcomeVerified":false,"paidModelExecutionVerified":false,
        "createdAt":chrono::Utc::now().to_rfc3339(),
        "auditThroughRowId":through,
        "totalGoals":goals.len(),
        "statusCounts":status_counts,
        "settledGoals":settled_count,
        "internalAcceptedGoals":accepted,
        "deliveredToSourceGoals":delivered,
        "acceptedAwaitingGitApply":accepted_waiting_for_apply,
        "completedWithoutValidJobOrAcceptance":incomplete_acceptance,
        "pendingOrRunningGoals":goals.len().saturating_sub(settled_count as usize),
        "internalAcceptanceRateAmongSettled":rate,
        "totalIterations":total_iterations,"totalReplans":total_replans,
        "usage":{
            "sessionsWithAudit":usage_sessions,
            "goalsWithoutSession":missing_session,
            "sessionsWithoutUsageAudit":no_usage_audit,
            "sharedSessionLinks":shared_session_links,
            "calls":calls,"estimatedCalls":estimated_calls,
            "promptTokens":prompt_tokens,"completionTokens":completion_tokens,
            "totalTokens":total_tokens,
            "cachedPromptTokens":cached_prompt_tokens,
            "cacheEligiblePromptTokens":cache_eligible_prompt_tokens,
            "cacheReportedCalls":cache_reported_calls,
            "completeAttribution":denom_covered,
            "recordedTokensPerInternalAccepted":tokens_per_accepted,
            "actualBilledKrw":null,
            "actualBilledKrwPerSuccess":null
        },
        "rows":rows.into_iter().take(80).collect::<Vec<_>>(),
        "rowsTruncated":goals.len()>80,
        "notes":[
            "완수율은 저장된 목표의 현재 상태 중 완료·실패·취소·검토 대기만 분모로 계산합니다. 대기·실행 중 목표는 별도로 표시합니다.",
            "내부 성공은 목표 최종 검증과 일치하는 완료된 실행 작업이 모두 있어야 합니다. 격리된 Git 변경은 사용자 원본 반영 전까지 납품 성과로 세지 않습니다.",
            "외부 벤치마크 또는 실모델 완주 인증이 아닙니다.",
            "토큰은 세션별 model.usage 감사 로그 기반입니다. 추정 사용량은 별도 집계하며 공급자 실제 청구액은 알 수 없습니다.",
            "토큰/내부성공은 실패·진행 중 작업의 기록 토큰도 포함합니다. 세션·사용량이 누락되면 분모 보장이 없어 null로 표시합니다."
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-goal-metrics-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        (AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path())),dir)
    }
    #[test]
    fn zero_records_are_not_a_fake_success_rate(){
        let (state,dir)=fixture();
        let result=snapshot(&state).unwrap();
        assert_eq!(result["totalGoals"],0);
        assert!(result["internalAcceptanceRateAmongSettled"].is_null());
        assert!(result["usage"]["actualBilledKrw"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn valid_accepted_goal_needs_matching_finished_job_and_report(){
        let (state,dir)=fixture();
        state.store.put("goal",&json!({"id":"g1","status":"completed",
            "jobId":"j1","sessionId":"s1","finalReport":{"accepted":true},"replans":1})).unwrap();
        state.store.put("job",&json!({"id":"j1","kind":"goal","status":"completed",
            "input":{"goalId":"g1"}})).unwrap();
        state.store.put("goal",&json!({"id":"g2","status":"completed",
            "jobId":"j2","sessionId":"s2","finalReport":{"accepted":true}})).unwrap();
        state.store.put("job",&json!({"id":"j2","kind":"goal","status":"failed",
            "input":{"goalId":"g2"}})).unwrap();
        state.store.put("goal",&json!({"id":"g3","status":"needs_review",
            "jobId":"j3","sessionId":"s3"})).unwrap();
        state.store.put("goal",&json!({"id":"g4","status":"running",
            "jobId":"j4","sessionId":"s4"})).unwrap();
        for sid in ["s1","s2","s3","s4"]{
            state.store.audit("model.usage",json!({"sessionId":sid,
                "promptTokens":10,"completionTokens":5,"totalTokens":15,
                "estimated":sid=="s3",
                "cachedPromptTokens":if sid=="s1"{json!(3)}else{Value::Null}}));
        }
        let result=snapshot(&state).unwrap();
        assert_eq!(result["totalGoals"],4);
        assert_eq!(result["settledGoals"],3);
        assert_eq!(result["internalAcceptedGoals"],1);
        assert_eq!(result["completedWithoutValidJobOrAcceptance"],1);
        assert_eq!(result["internalAcceptanceRateAmongSettled"],1.0/3.0);
        assert_eq!(result["usage"]["totalTokens"],60);
        assert_eq!(result["usage"]["calls"],4);
        assert_eq!(result["usage"]["estimatedCalls"],1);
        assert_eq!(result["usage"]["cachedPromptTokens"],3);
        assert_eq!(result["usage"]["cacheEligiblePromptTokens"],10);
        assert_eq!(result["usage"]["recordedTokensPerInternalAccepted"],60.0);
        assert!(result["usage"]["actualBilledKrw"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn git_isolation_acceptance_does_not_automatically_mean_delivered_code(){
        let (state,dir)=fixture();
        state.store.put("goal",&json!({"id":"isolate","status":"completed",
            "isolateWorktree":true,"gitWorktree":{"applied":false},
            "jobId":"j","sessionId":"s","finalReport":{"accepted":true}})).unwrap();
        state.store.put("job",&json!({"id":"j","kind":"goal",
            "status":"completed","input":{"goalId":"isolate"}})).unwrap();
        state.store.audit("model.usage",json!({"sessionId":"s",
            "totalTokens":40,"promptTokens":30,"completionTokens":10}));
        let before=snapshot(&state).unwrap();
        assert_eq!(before["internalAcceptedGoals"],1);
        assert_eq!(before["deliveredToSourceGoals"],0);
        assert_eq!(before["acceptedAwaitingGitApply"],1);
        state.store.patch_if("goal","isolate","status",&json!("completed"),
            &json!({"gitWorktree":{"applied":true}})).unwrap();
        let after=snapshot(&state).unwrap();
        assert_eq!(after["deliveredToSourceGoals"],1);
        assert_eq!(after["acceptedAwaitingGitApply"],0);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn newly_queued_unstarted_goal_is_not_missing_token_telemetry(){
        let (state,dir)=fixture();
        state.store.put("goal",&json!({"id":"queued","status":"queued",
            "jobId":"j","iterations":0})).unwrap();
        let result=snapshot(&state).unwrap();
        assert_eq!(result["usage"]["goalsWithoutSession"],0);
        assert_eq!(result["usage"]["completeAttribution"],true);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn missing_provenance_disables_cost_per_success_even_when_goal_says_complete(){
        let (state,dir)=fixture();
        state.store.put("goal",&json!({"id":"missing","status":"completed",
            "jobId":"j","finalReport":{"accepted":true}})).unwrap();
        state.store.put("job",&json!({"id":"j","kind":"goal",
            "status":"completed","input":{"goalId":"missing"}})).unwrap();
        let result=snapshot(&state).unwrap();
        assert_eq!(result["internalAcceptedGoals"],1);
        assert_eq!(result["usage"]["goalsWithoutSession"],1);
        assert_eq!(result["usage"]["completeAttribution"],false);
        assert!(result["usage"]["recordedTokensPerInternalAccepted"].is_null());
        let _=std::fs::remove_dir_all(dir);
    }
}
