//! edit_policy: source-grounded compiled descendant planning responsibility.
use super::*;

pub(crate) fn sub2_edit_policy_reward(g:&Value)->f64 {
    // Never promote a source-edit controller on a request's claimed model
    // identity. The full, untruncated actual session audit must match.
    if g["modelAttributionConsistent"]!=true
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || g["editPolicy"].is_null(){return 0.0;}
    // Merely carrying an inherited policy in a plan is not execution.
    // Match its revision to a real program-stage or rule-fire receipt
    // from the same worker before assigning credit for source quality.
    let executable=g["programStages"].as_array().is_some_and(|rows|
        rows.iter().any(|row|
            row["policyRevision"]==g["editPolicy"]["revision"]
            && row["generationJobId"]==g["jobId"]
            && row["runId"].is_string()))
        || g["ruleExecutions"].as_array().is_some_and(|rows|
            rows.iter().any(|row|
                row["policyRevision"]==g["editPolicy"]["revision"]
                && row["generationJobId"]==g["jobId"]
                && row["runId"].is_string()));
    if !executable{return 0.0;}
    let accepted=g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["evidenceVerified"]==true;
    if accepted {
        let Some(rules)=g["editPolicy"]["rules"].as_array()
            else{return 0.0};
        let Some(timeline)=g["toolTimeline"].as_array()
            else{return 0.0};
        let adopted_after=|op:&Value,run:&str,before:u64|{
            let expected=op["path"].as_str().and_then(|path|
                g["acceptedSourceHashes"].get(path))
                .and_then(Value::as_str);
            op["runId"].as_str()==Some(run)
                && op["opSeq"].as_u64().is_some_and(|seq|seq>before)
                && matches!(op["tool"].as_str(),
                    Some("workspace_replace"|"workspace_replace_range"
                        |"workspace_replace_function"|"workspace_write"))
                && op["status"]=="completed"
                && op["result"]["written"]==true
                && op["toolReceipt"]["verified"]==true
                && expected.is_some_and(|sha|sha.len()==64
                    && op["result"]["postSha256"].as_str()==Some(sha))
        };
        if rules.is_empty() {
            // A source improvement can be attributed to the inherited edit
            // PROGRAM only if its edit phase preceded the actual accepted
            // source bytes. A discovery or grounding stage alone is not
            // proof that a rewritten procedure changed the implementation.
            let witnessed=g["programStages"].as_array().is_some_and(|events|
                events.iter().any(|event|{
                    let Some(run)=event["runId"].as_str() else{return false};
                    let Some(before)=event["opSeqBefore"].as_u64() else{return false};
                    event["generationJobId"]==g["jobId"]
                        && event["policyRevision"]==g["editPolicy"]["revision"]
                        && event["phase"]=="edit"
                        && timeline.iter().any(|op|adopted_after(op,run,before))
                }));
            return if witnessed {1.0}else{0.0};
        }
        let proved=g["ruleExecutions"].as_array().is_some_and(|events|
            events.iter().any(|event|{
                let Some(run)=event["runId"].as_str() else{return false};
                let Some(before)=event["opSeqBefore"].as_u64() else{return false};
                let matched_rule=event["generationJobId"]==g["jobId"]
                    && event["policyRevision"]==g["editPolicy"]["revision"]
                    && rules.iter().any(|r|r["id"]==event["ruleId"]);
                let acted=timeline.iter().any(|op|
                    op["runId"]==run
                    && op["opSeq"].as_u64().is_some_and(|seq|seq>before)
                    && op["status"]=="completed"
                    && event["allow"].as_array().is_some_and(|tools|
                        tools.iter().any(|name|name==&op["tool"])));
                let adopted=timeline.iter().any(|op|adopted_after(op,run,before));
                matched_rule && acted && adopted
            }));
        return if proved {1.0}else{0.0};
    }
    if g["pairedRevalidated"]==true
        && g["candidateId"].is_string()
        && g["evaluation"]["metrics"]["regressions"].as_u64()==Some(0) {
        return 0.1;
    }
    0.0
}
/// The old supervisor does not synthesize source-edit rules. The current
/// compiled child learns from its own executed receipts AND persisted earlier
/// campaigns with the same branch, objective, partition and model identity.
pub(super) fn inherited_edit_policy(
    state:&AppState,input:&Value,history:&[Value],
    selected_model:&Value,domain:&str,
)->Value{
    let objective=crate::engine::memory::stable_id(&[
        input["rsiObjective"].as_str().unwrap_or("")
    ]);
    let partition=research_scope(input).partition_key;
    let provider=selected_model["providerId"].as_str()
        .or_else(||input["providerId"].as_str()).unwrap_or("");
    let model=selected_model["model"].as_str()
        .or_else(||input["model"].as_str()).unwrap_or("");
    let branch=input["rsiSourceBranch"].as_str().unwrap_or("");
    let mut seen=BTreeSet::<String>::new();
    let mut rows=Vec::<Value>::new();
    for g in history.iter().chain(state.store.all("rsi_generation").iter()) {
        let Some(id)=g["jobId"].as_str() else{continue};
        if !seen.insert(id.to_string())
            || g["objectiveId"]!=objective
            || g["knowledgePartition"]!=partition
            || g["sourceBranch"]!=branch
            || g["generatorProviderId"]!=provider
            || g["generatorModel"]!=model {continue;}
        rows.push(g.clone());
    }
    rows.sort_by(|a,b|a["createdAt"].as_str()
        .cmp(&b["createdAt"].as_str())
        .then_with(||a["jobId"].as_str().cmp(&b["jobId"].as_str())));
    // Local rule mutation is bounded to recent executions. Confirmed
    // productivity evidence is a DIFFERENT, durable learning horizon:
    // truncating it to 128 rows can erase both arms of earlier independently
    // matched source trials after many campaigns.
    let recent=if rows.len()>128{
        &rows[rows.len()-128..]
    }else{rows.as_slice()};
    let generated=crate::engine::rsi_edit_genome::evolve(recent,sub2_edit_policy_reward);
    // Prefer a policy only after it produced more independently adopted
    // FIRST rebuilt successors than a control in >=2 matched source contexts.
    // This decision is compiled into the current child's executable policy,
    // not a recommendation authored by the older campaign coordinator.
    if let Some(profile)=crate::engine::rsi_meta_controller::comparison_profile(input){
        crate::engine::rsi_successor_productivity::choose(
            state,&rows,generated,&profile,domain)
    }else{generated}
}

