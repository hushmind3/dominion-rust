//! Frozen source-linked comparison of independent ON/OFF descendant tasks.
//! The control may fail or regress; success flags never substitute for its
//! original paired-run receipt, observed tool trace and matched task inputs.
use super::*;

#[cfg(test)]
#[path = "method_contrast/tests.rs"]
mod tests;

fn actual_method_use(g:&Value)->bool {
    let verified_procedure=g["methodExecutions"].as_array().is_some_and(|rows|
        rows.iter().any(|row|row["result"]["verified"]==true
            && row["result"]["followed"]==true));
    let induced_program=g["methodActions"].as_array().is_some_and(|rows|
        rows.iter().any(|row|row["decision"].is_object()
            || row["inducedProgramDecision"].is_object()));
    verified_procedure||induced_program
}

/// Replay the method decision against original execution + tool records.
/// A generated summary alone cannot claim that a method performed a real edit.
fn original_method_executed(state:&AppState,g:&Value,job:&Value)->bool {
    if !actual_method_use(g){return false;}
    let Some(session)=job["sessionId"].as_str() else{return false};
    let executions=state.store.all("execution").into_iter()
        .filter(|e|e["sessionId"]==session).collect::<Vec<_>>();
    let operations=state.store.all("execution_operation").into_iter()
        .filter(|op|op["sessionId"]==session).collect::<Vec<_>>();
    let logged=g["methodActions"].as_array().is_some_and(|actions|actions.iter().any(|a|{
        let Some(execution)=executions.iter().find(|e|e["id"]==a["executionId"])
            else{return false};
        let decision=a["decision"].is_object()
            && a["decision"]==execution["rsiMethodDecision"];
        let induced=a["inducedProgramDecision"].is_object()
            && (a["inducedProgramDecision"]==execution["rsiActionProgramDecision"]
                || execution["rsiActionProgramDecisionHistory"].as_array()
                    .is_some_and(|rows|rows.contains(&a["inducedProgramDecision"])));
        (decision||induced) && operations.iter().any(|op|
            op["runId"]==execution["id"]
                && matches!(op["tool"].as_str(),Some(
                    "workspace_replace"|"workspace_replace_range"
                    |"workspace_replace_function"|"workspace_write"))
                && op["outcomeUnknown"]!=true
                && op["status"]!="interrupted")
    }));
    let procedures=job["sessionId"].as_str()
        .and_then(|id|state.store.get("session",id))
        .is_some_and(|session|g["methodExecutions"].as_array()
            .is_some_and(|rows|rows.iter().any(|row|
                session["rsiMethodExecutions"].as_array()
                    .is_some_and(|logged|logged.contains(row))
                && row["result"]["verified"]==true
                && row["result"]["followed"]==true
                && row["operationIds"].as_array().is_some_and(|ids|
                    !ids.is_empty()&&ids.iter().all(|id|operations.iter().any(|op|
                        op["id"]==*id&&op["origin"]=="procedure"
                        && op["runId"]==row["executionId"]))))));
    logged||procedures
}

/// This proves a bounded observed advantage for the same frozen task set.
/// It is NOT proof of a statistical or isolated causal effect of the entire
/// source revision. One model sample per arm can differ stochastically.
pub(super) fn evidence(state:&AppState,generation:&Value)->Value {
    let no=||json!({"source":"replayed_method_on_off_source_tasks_v1",
        "methodAssociationObserved":false,"qualityProven":false,
        "causalAblationProven":false});
    if generation["accepted"]!=true
        || generation["candidateOrigin"]!="model_driven_detached_worktree"
        || !generation_is_verified(generation)
    {return no();}
    let Some(on)=generation["jobId"].as_str()
        .and_then(|id|state.store.get("job",id)) else{return no()};
    let Some(control_id)=on["input"]["rsiMethodAblationControlId"].as_str() else{return no()};
    let Some(control)=state.store.get("rsi_method_control",control_id) else{return no()};
    let Some(off)=control["controlJobId"].as_str()
        .and_then(|id|state.store.get("job",id)) else{return no()};
    let Some(on_candidate)=generation["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{return no()};
    let Some(off_candidate)=control["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{return no()};
    if control["source"]!="isolated_actual_worker_method_off_v1"
        || control["campaignId"]!=generation["campaignId"]
        || control["controlJobId"]!=off["id"]
        || control["adopted"]!=false
        || control["runtimeDisabledMethods"]!=true
        || control["procedureOriginObserved"]!=false
        || off["rsiControlExperiment"]!=true
        || off["rsiMethodAblationDisabled"]!=true
        || off["input"]["rsiMethodAblationArm"]!="without_methods"
        || on["input"]["rsiMethodAblationArm"]!="with_methods"
        || off["input"]["rsiMethodAblationInputSha256"]!=control["promptAndCasesSha256"]
        || on["input"]["rsiMethodAblationInputSha256"]!=control["promptAndCasesSha256"]
        || control["generatorCommit"]!=generation["generatorCommit"]
        || on["input"]["rsiGeneratorCommit"]!=generation["generatorCommit"]
        || off["input"]["rsiGeneratorCommit"]!=generation["generatorCommit"]
        || off["input"]["rsiNativePlan"]["systemAgenda"]["target"]
            !=on["input"]["rsiNativePlan"]["systemAgenda"]["target"]
        || off["input"]["rsiMethodSnapshot"]!=on["input"]["rsiMethodSnapshot"]
        || control["methodSnapshot"]!=generation["methodSnapshot"]
        || !generation["methodSnapshot"].is_object()
        || control["providerId"]!=on["input"]["providerId"]
        || control["model"]!=on["input"]["model"]
        || off["input"]["providerId"]!=on["input"]["providerId"]
        || off["input"]["model"]!=on["input"]["model"]
        || control["domain"]!=generation["domain"]
        || on["rsiDomain"]!=generation["domain"]
        || off["rsiDomain"]!=generation["domain"]
        || off["input"]["rsiFrozenCases"]!=on["input"]["rsiFrozenCases"]
        || off_candidate["baseCommit"]!=on_candidate["baseCommit"]
        || off_candidate["cases"]!=on_candidate["cases"]
        || off_candidate["oracleVersion"]!=on_candidate["oracleVersion"]
        || off_candidate["experimentAdoption"].is_object()
        || control["evaluation"]["candidateId"]!=off_candidate["id"]
        || control["evaluation"]["evidenceSha256"]!=off_candidate["evaluation"]["evidenceSha256"]
        || on_candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
        || on_candidate["evaluation"]["metrics"]!=generation["evaluation"]["metrics"]
        || crate::engine::rsi::verify_paired_receipt(state,&on_candidate).is_err()
        || crate::engine::rsi::verify_control_receipt(state,&off_candidate).is_err()
    {return no();}
    let on_cases=on_candidate["evaluation"]["metrics"]["holdoutEvidence"]
        .as_array().cloned().unwrap_or_default();
    let off_cases=off_candidate["evaluation"]["metrics"]["holdoutEvidence"]
        .as_array().cloned().unwrap_or_default();
    if on_cases.len()<2||off_cases.len()!=on_cases.len()
        || !original_method_executed(state,generation,&on) {return no();}
    let mut seen=BTreeSet::new();
    let mut gains=0usize;let mut losses=0usize;
    let mut task_pairs=0usize;
    for old in &off_cases {
        let Some(id)=old["id"].as_str() else{return no()};
        if !seen.insert(id){return no();}
        let Some(new)=on_cases.iter().find(|r|
            r["id"]==old["id"]&&r["domain"]==old["domain"]
                && r["baselinePass"]==old["baselinePass"]) else{return no()};
        if old["domain"]!=generation["domain"] {continue;}
        task_pairs+=1;
        let (Some(before),Some(after))=
            (old["candidatePass"].as_bool(),new["candidatePass"].as_bool())
            else{return no()};
        let (Some(old_metric),Some(new_metric))=
            (old["metricGains"].as_u64(),new["metricGains"].as_u64())
            else{return no()};
        if !before&&after {gains+=1;}
        if before&&!after {losses+=1;}
        if before&&after {
            if new_metric>old_metric {gains+=1;}
            if new_metric<old_metric {losses+=1;}
        }
        if new["metricRegressions"].as_u64().unwrap_or(u64::MAX)>0 {
            losses+=1;
        }
    }
    let observed=task_pairs>=2&&gains>0&&losses==0;
    json!({
        "source":"replayed_method_on_off_source_tasks_v1",
        "methodAssociationObserved":observed,
        "qualityProven":observed,
        "causalAblationProven":false,
        "taskHoldoutPairs":task_pairs,
        "treatmentOnlyGains":gains,
        "controlOnlyGains":losses,
        "methodSourceSha256":generation["methodSnapshot"]["methodSourceSha256"],
        "methodPath":generation["methodSnapshot"]["methodPath"],
        "controlId":control["id"],"controlJobId":off["id"],
        "treatmentJobId":on["id"],
        "controlCandidateId":off_candidate["id"],
        "treatmentCandidateId":on_candidate["id"],
        "controlEvaluationSha256":off_candidate["evaluation"]["evidenceSha256"],
        "treatmentEvaluationSha256":on_candidate["evaluation"]["evidenceSha256"],
        "sameOriginalSourceAndFrozenCases":true,
        "methodActuallyUsed":true,
        "limitation":"single independent model run per arm; source planner held fixed"
    })
}
