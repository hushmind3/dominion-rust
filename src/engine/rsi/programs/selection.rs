//! Actual runtime attribution and paired action-program comparison.
//! Child of programs.rs; all executable methods remain in the compiled successor.
use super::*;

/// Attribution for learned action programs requires both a native policy
/// decision *and* the matching later tool operation. A displayed choice with
/// no executed tool is not a policy trial and cannot earn quality credit.
pub(in super::super) fn observed_induced_program(row:&Value)->Option<&str> {
    let revision=row["executionMethod"]["actionProgram"]["revision"].as_str()?;
    let trace=row["actionTrace"].as_array()?;
    let applied=row["methodActions"].as_array()?.iter().any(|receipt|{
        let choice=&receipt["inducedProgramDecision"];
        if choice["programRevision"].as_str()!=Some(revision){return false;}
        let Some(selected)=choice["chosenTool"].as_str() else {return false;};
        let Some(action_count)=choice["actionCount"].as_u64() else {return false;};
        trace.iter().any(|op|
            op["runId"]==receipt["executionId"]
                && op["opSeq"].as_u64().is_some_and(|seq|seq>action_count)
                && op["tool"].as_str()==Some(selected)
                && matches!(op["status"].as_str(),Some("completed"|"failed")))
    });
    applied.then_some(revision)
}

/// Only the same generator source, objective, domain, provider, model and
/// concrete focus can form a program comparison. Different source versions
/// are not an A/B experiment even if the task title is unchanged.
pub(in super::super) fn induced_program_context(row:&Value)->Option<String> {
    for key in ["generatorCommit","objectiveId","domain","generatorModel","generatorProviderId"] {
        if row[key].as_str().is_none_or(str::is_empty) {return None;}
    }
    let focus=row["focusFiles"].as_array()?;
    if focus.is_empty(){return None;}
    Some(json!([
        row["generatorCommit"],row["objectiveId"],row["domain"],
        row["generatorModel"],row["generatorProviderId"],focus
    ]).to_string())
}

/// Pairwise policy result: only an independently revalidated adopted source
/// gain counts as quality. If both policies improved, lower measured cost can
/// break the tie without disguising a failed implementation as a gain.
pub(in super::super) fn induced_program_pair_outcome(challenger:&Value,control:&Value)->i8 {
    let challenge_gain=generation_is_verified(challenger);
    let control_gain=generation_is_verified(control);
    if challenge_gain && !control_gain{return 1;}
    if control_gain && !challenge_gain{return -1;}
    if !challenge_gain{return 0;}
    let (Some(ct),Some(bt),Some(cm),Some(bm))=(
        challenger["tokens"].as_u64(),control["tokens"].as_u64(),
        challenger["elapsedMs"].as_u64(),control["elapsedMs"].as_u64()
    ) else {return 0;};
    if ct==0||bt==0||cm==0||bm==0{return 0;}
    if ct.saturating_mul(10)<=bt.saturating_mul(9)
        && cm.saturating_mul(10)<=bm.saturating_mul(11) {return 1;}
    if bt.saturating_mul(10)<=ct.saturating_mul(9)
        && bm.saturating_mul(10)<=cm.saturating_mul(11) {return -1;}
    0
}

/// Compare a learned descendant against its *executed* parent on matched
/// source/model/task contexts. Two independent, non-overlapping wins are
/// needed before promotion. A failed or unexecuted child cannot silently
/// replace a proven parent. All other plans remain experiments.
pub(in super::super) fn select_evaluated_action_program(rows:&[Value],proposed:Value)->(Value,Value) {
    let mut observed=BTreeMap::<String,Vec<&Value>>::new();
    let mut programs=BTreeMap::<String,Value>::new();
    for row in rows {
        let Some(revision)=observed_induced_program(row) else {continue;};
        let Some(_)=induced_program_context(row) else {continue;};
        let Some(job_id)=row["jobId"].as_str().filter(|id|!id.is_empty()) else {continue;};
        if row["error"].as_str().is_some_and(|e|!e.is_empty())
            || row["improvementActionObservation"]["providerInterrupted"]==true {
            continue;
        }
        // Duplicate store/history copies of the same job are not independent.
        if !observed.get(revision).is_some_and(|items|
            items.iter().any(|prior|prior["jobId"].as_str()==Some(job_id))) {
            observed.entry(revision.to_string()).or_default().push(row);
            programs.insert(revision.to_string(),
                row["executionMethod"]["actionProgram"].clone());
        }
    }
    let mut reports=Vec::<Value>::new();
    let mut best:Option<(u64,String,Value)>=None;
    let mut rejected=BTreeMap::<String,String>::new();
    for (revision,program) in &programs {
        let Some(parent)=program["mutatedFrom"].as_str() else {continue;};
        let (Some(challengers),Some(controls))=(observed.get(revision),observed.get(parent))
            else {continue;};
        let mut used_controls=BTreeSet::<String>::new();
        let mut wins=0u64;
        let mut losses=0u64;
        let mut ties=0u64;
        let mut matched=Vec::<Value>::new();
        for challenge in challengers {
            let Some(context)=induced_program_context(challenge) else {continue;};
            let baseline=controls.iter().find(|row|{
                let Some(job)=row["jobId"].as_str() else {return false;};
                !used_controls.contains(job)
                    && row["jobId"]!=challenge["jobId"]
                    && induced_program_context(row).as_deref()==Some(context.as_str())
            });
            let Some(control)=baseline else {continue;};
            let job=control["jobId"].as_str().unwrap_or("").to_string();
            used_controls.insert(job);
            let verdict=induced_program_pair_outcome(challenge,control);
            if verdict>0 {wins+=1;}
            else if verdict<0 {losses+=1;}
            else {ties+=1;}
            matched.push(json!({"challengeJob":challenge["jobId"],
                "controlJob":control["jobId"],"verdict":match verdict {
                    1=>"supported",-1=>"rejected",_=>"inconclusive"
                }}));
        }
        let status=if wins>=2 && losses==0 && ties==0 {
            "quality_promoted"
        }else if losses>=2 && wins==0 {
            "quality_rejected"
        }else {"trial_unverified"};
        if status=="quality_promoted"
            && best.as_ref().is_none_or(|(previous,id,_)|
                wins>*previous || (wins==*previous && revision<id)) {
            best=Some((wins,revision.clone(),program.clone()));
        }
        if status=="quality_rejected" {
            rejected.insert(revision.clone(),parent.to_string());
        }
        reports.push(json!({"revision":revision,"parentRevision":parent,
            "status":status,"wins":wins,"losses":losses,"inconclusive":ties,
            "independentPairs":matched}));
    }
    if let Some((wins,revision,mut winner))=best {
        winner["status"]=json!("quality_promoted_from_matched_program_trials");
        return (winner,json!({"status":"quality_promoted",
            "chosenRevision":revision,"independentWins":wins,
            "comparisons":reports}));
    }
    let proposed_revision=proposed["revision"].as_str().unwrap_or("");
    if let Some(parent)=rejected.get(proposed_revision) {
        if let Some(mut fallback)=programs.get(parent).cloned() {
            fallback["status"]=json!("quality_rejected_child_fallback");
            return (fallback,json!({"status":"quality_rejected",
                "rejectedRevision":proposed_revision,
                "chosenRevision":parent,"comparisons":reports}));
        }
    }
    (proposed,json!({"status":"trial_unverified",
        "reason":"no_two_independent_matched_quality_wins",
        "comparisons":reports}))
}

/// Success-derived algorithm invention: when independently adopted edits
/// repeatedly bypass an investigation tool yet still use a real source read,
/// compose a *shorter* trial method. This is program ablation inferred from
/// observed execution, not a claim that shortening improves holdout quality.
/// The existing matched parent/child evaluator determines later promotion.
pub(in super::super) fn compress_successful_action_program(mut program:Value, rows:&[Value])->Value {
    // Successful source-verified descendants may themselves be compressed.
    // Only skip an already compressed revision, preventing no-op mutation loops.
    if program["mutationOperator"]=="success_derived_discovery_ablation" {
        return program;
    }
    let Some(revision)=program["revision"].as_str().map(str::to_string) else{return program;};
    let mut sequence=program["sequence"].as_array().cloned().unwrap_or_default();
    let Some(search_position)=sequence.iter().position(|step|step=="code_search") else{return program;};
    // A read must remain before source changes: it grounds edit ranges and
    // their SHA, so compression only removes genuinely redundant discovery.
    if !sequence.iter().skip(search_position+1).any(|step|step=="workspace_read_range") {
        return program;
    }
    let mut verified_jobs=BTreeSet::<String>::new();
    let mut witnessed_domains=BTreeSet::<String>::new();
    for row in rows {
        if observed_induced_program(row)!=Some(revision.as_str())
            || !generation_is_verified(row)
            || row["sourceContextChars"].as_u64().unwrap_or(0)==0
            || row["parentContextFit"]["sourceExcerptDeferred"]==true
        {continue;}
        let Some(job)=row["jobId"].as_str().filter(|id|!id.is_empty()) else{continue;};
        let Some(trace)=row["actionTrace"].as_array() else{continue;};
        let Some(edit_position)=trace.iter().position(|step|
            step["executed"]==true && step["sourceChanged"]==true
            && matches!(step["tool"].as_str(),
                Some("workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_write"|"workspace_replace_function")))
        else{continue;};
        let before=&trace[..edit_position];
        let grounded=before.iter().any(|step|
            step["tool"]=="workspace_read_range" && step["executed"]==true);
        let searched=before.iter().any(|step|
            step["tool"]=="code_search" && step["executed"]==true);
        if grounded && !searched {
            verified_jobs.insert(job.to_string());
            if let Some(domain)=row["domain"].as_str(){witnessed_domains.insert(domain.to_string());}
        }
    }
    if verified_jobs.len()<2 {return program;}
    sequence.remove(search_position);
    let updated=crate::engine::memory::stable_id(&[
        &json!([&sequence,&program["onFailure"],&program["recoveryPaths"]]).to_string()]);
    if updated==revision{return program;}
    program["revision"]=json!(updated);
    program["sequence"]=json!(sequence);
    program["mutatedFrom"]=json!(revision);
    program["mutationOperator"]=json!("success_derived_discovery_ablation");
    program["status"]=json!("trial_success_compression_unverified");
    program["successfulShortcutEvidence"]=json!({
        "verifiedIndependentJobs":verified_jobs.len(),
        "observedDomains":witnessed_domains.into_iter().collect::<Vec<_>>(),
        "removed":"code_search","preserved":"workspace_read_range"
    });
    program
}

