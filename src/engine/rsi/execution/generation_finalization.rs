//! Commit the independently evaluated generation, publish proof and decide
//! whether the next descendant receives work. The campaign runtime owns only
//! orchestration; this immutable supervisor owns terminal decision evidence.
use super::*;

pub(super) struct GenerationObservation<'a> {
    pub input:&'a Value,
    pub current:&'a Value,
    pub stopped:Option<&'a str>,
    pub attempt:u64,
    pub prior_failure:bool,
    pub has_candidate:bool,
    pub has_research:bool,
    pub new_findings:usize,
    pub started:Instant,
}

pub(super) fn finalize_generation(
    context:&JobContext,
    record:&mut Value,
    mut generation:Value,
    observed:GenerationObservation<'_>,
)->Result<Option<String>,String>{
    let GenerationObservation{input,current,stopped,attempt,prior_failure,
        has_candidate,has_research,new_findings,started}=observed;
    let generation_accepted = generation["accepted"] == true;
    // A real, independently adopted NATIVE source mutation can succeed
    // even when a previous model turn exhausted its own output budget.
    // Do not discard that measured descendant merely because the model
    // ended badly. The coordinator's aggregate token/time budgets are
    // enforced again before launching any later generation.
    let stopped = if generation_accepted
        && generation["nativeMutation"]["origin"]=="native_inductive_source_mutation"
        && stopped==Some("token_budget") {
        None
    } else {stopped};
    let accepted_streak = if generation_accepted {
        record["acceptedStreak"].as_u64().unwrap_or(0) + 1
    } else {
        // Research-only or failed generations interrupt a consecutive
        // adoption chain, even when no evaluation was produced.
        0
    };
    record["acceptedStreak"] = json!(accepted_streak);
    let max_streak = record["maxAcceptedStreak"]
        .as_u64()
        .unwrap_or(0)
        .max(accepted_streak);
    record["maxAcceptedStreak"] = json!(max_streak);
    // Three consecutive adopted generations are the documented threshold
    // for calling the capability demonstrated, not a single lucky patch.
    let mut evidence_record=record.clone();
    evidence_record["history"].as_array_mut()
        .ok_or("세대 이력 손상")?.push(generation.clone());
    evidence_record["systemEvolution"]=input["rsiSystemEvolution"].clone();
    let proof=crate::engine::rsi::recursive_proof_report(&context.state,&evidence_record);
    crate::engine::rsi::attach_recursive_proof(record,&proof);
    let generation_knowledge = generation["knowledgeId"].clone();
    let generation_tokens = generation["tokens"].clone();
    let generation_elapsed = generation["elapsedMs"].clone();
    let generation_verdict = generation["evaluation"]["verdict"].clone();
    // Workers read this durable table, not the coordinator's in-memory
    // history. Publish only after research, method feedback and trial
    // outcomes are complete so the next worker can actually continue.
    // A real completion timestamp is also necessary to evolve the method
    // from the latest measured outcome across restarted campaigns.
    generation["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
    context.state.store.put("rsi_generation", &generation)
        .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
    record["history"]
        .as_array_mut()
        .ok_or("세대 목록 손상")?
        .push(generation);
    // New findings, not a write count or query spelling, justify continuing
    // a research-only generation within the configured campaign budgets.
    let no_edit = !has_candidate
        && current["result"]["rsi"]["diffBytes"].as_u64().unwrap_or(0) == 0;
    let research = has_research;
    let changed = new_findings>0;
    let streak = if no_edit {
        record["noProgressStreak"].as_u64().unwrap_or(0) + 1
    } else {
        0
    };
    record["noProgressStreak"] = json!(streak);
    record["activeGenerationJobId"] = Value::Null;
    let decision_objective = selection_objective(input);
    let decision_history = record["history"].as_array().cloned().unwrap_or_default();
    let decision_basis = if let Some(reason)=stopped {
        reason
    } else if prior_failure {
        "duplicate_unproductive_patch"
    } else if generation_accepted {
        "accepted_implementation"
    } else if !has_candidate
        && record["history"].as_array().and_then(|rows|rows.last())
            .and_then(|row|row["editEvidence"]["attempts"].as_u64())
            .unwrap_or(0)>0 {
        "edit_attempt_without_candidate"
    } else if new_findings > 0 {
        "research_findings"
    } else if no_edit {
        "no_progress"
    } else {
        "implementation_not_adopted"
    };
    let next_domain = select_domain_for_state(&context.state, &decision_history, &decision_objective);
    let reuse = method_reuse_decision(&context.state, input, &next_domain);
    record["nextDecision"] = json!({
        "basis": decision_basis,
        "fromGeneration": attempt,
        "nextDomain": next_domain,
        "knowledgeId": generation_knowledge,
        "headCommit": record["headCommit"],
        "trialAlgorithmId": record["nextTrialAlgorithmId"],
        "observed": {
            "accepted": generation_accepted,
            "tokens": generation_tokens,
            "elapsedMs": generation_elapsed,
            "verdict": generation_verdict,
            "editEvidence":record["history"].as_array()
                .and_then(|rows|rows.last()).map(|row|row["editEvidence"].clone()),
        },
        "reuseDecision": reuse,
    });
    elapsed(record, started);
    save(context, record)?;
    if proof_goal_reached(&context.state,input,record) {
        return Ok(Some("capability_demonstrated".into()));
    }
    if let Some(reason)=stopped {return Ok(Some(reason.into()));}
    if let Some(error)=generation_model_error(&current) {
        // A late provider failure after source bytes were written,
        // compiled and independently paired-evaluated is not a reason
        // to terminate the lineage before its next measured mutation.
        // An unmeasured error still stops this campaign.
        let paired=record["history"].as_array()
            .and_then(|rows|rows.last())
            .is_some_and(|last|last["evaluation"]["source"]
                =="server_invoked_local_paired_git_worktrees");
        if !paired {return Err(error.to_string());}
    }
    if no_edit && should_stop_no_progress(streak, research, changed,
        record["nextTrialAlgorithmId"].is_string(),
        input["rsiAutonomous"]==true) {
        return Ok(Some("no_progress".into()));
    }
    Ok(None)
}
