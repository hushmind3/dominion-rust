//! Cohesive source-level RSI coordinator subsystem.
//! This is code that runs in the compiled child, not model-weight training.
use super::*;

/// Source-independent record of what the engine really did. Preserve
/// action order and failures: an aggregate tool-call count cannot identify
/// recoveries, and a model saying it followed a procedure is not evidence.
pub(super) fn session_action_trace(state:&AppState,session_id:&str)->Vec<Value>{
    let mut ops=state.store.all("execution_operation").into_iter()
        .filter(|op|op["sessionId"].as_str()==Some(session_id)
            && tools::rsi_allowed(op["tool"].as_str().unwrap_or("")))
        .collect::<Vec<_>>();
    crate::engine::procedure::sort_operations(&mut ops);
    // Preserve the most recent actions: candidate writes often happen after
    // dozens of discovery calls, not at the beginning of the session.
    if ops.len()>48 { ops=ops.split_off(ops.len()-48); }
    ops.into_iter().map(|op|{
        let source_edit=matches!(op["tool"].as_str(),
            Some("workspace_write"|"workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_replace_function"));
        let verified=op["status"]=="completed"
            && op["result"]["written"]==true
            && op["toolReceipt"]["verified"]==true;
        let executed=op["status"]=="completed"
            && op["result"]["error"].is_null()
            && (!source_edit || verified);
        json!({
            "tool":op["tool"],"executed":executed,"sourceChanged":verified,
            "status":op["status"],
            "error":op["result"]["error"].as_str()
                .or_else(||op["error"].as_str()).unwrap_or("")
                .chars().take(200).collect::<String>(),
            "operationId":op["id"],"runId":op["runId"],"opSeq":op["opSeq"]
        })
    }).collect()
}

/// Compose a new, revisioned harness action program from different observed
/// episodes. A researched prefix and a candidate-producing suffix can come
/// from different generations: the result need not be a verbatim replay of
/// any previous model's tool calls. This changes tool availability in the
/// descendant's native runtime, not the language model's parameters.
pub(super) fn synthesize_action_program(rows:&[Value])->Value {
    let mut best_discovery:(i64,Vec<String>)=(i64::MIN,Vec::new());
    let mut best_mutation:(i64,Vec<String>)=(i64::MIN,Vec::new());
    let mut recovery=BTreeMap::<String,BTreeMap<String,u64>>::new();
    // Each path is a genuinely executed repair: failed tool -> successful
    // observations -> receipt-verified source mutation. No prose-based credit.
    let mut recovery_paths=BTreeMap::<String,BTreeMap<Vec<String>,(u64,u64)>>::new();
    let mut episode_count=0u64;
    for generation in rows {
        let steps=generation["actionTrace"].as_array().into_iter()
            .flatten().filter_map(|step|{
                let name=step["tool"].as_str()?;
                if !tools::rsi_allowed(name) {return None;}
                Some((name.to_string(),step["executed"]==true,
                    step["sourceChanged"]==true))
            }).collect::<Vec<_>>();
        if steps.is_empty(){continue;}
        episode_count+=1;
        for pair in steps.windows(2) {
            if !pair[0].1 && pair[1].1 && pair[0].0!=pair[1].0 {
                *recovery.entry(pair[0].0.clone()).or_default()
                    .entry(pair[1].0.clone()).or_insert(0)+=1;
            }
        }
        // Learn bounded, executable *multi-step* recovery programs. Training
        // requires a real verified edit after the failure; a read-only run
        // cannot become a successful repair. A failed intermediate step
        // terminates this path instead of laundering it as success.
        for (index,step) in steps.iter().enumerate() {
            if step.1 {continue;}
            let mut path=Vec::<String>::new();
            for next in steps.iter().skip(index+1).take(3) {
                if !next.1 || next.0==step.0 {break;}
                if path.last()!=Some(&next.0) {path.push(next.0.clone());}
                if next.2 && matches!(next.0.as_str(),
                    "workspace_write"|"workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_replace_function") {
                    let candidate=generation["candidateCapture"]["checksPassed"]==true;
                    let paired=generation_is_verified(generation);
                    let weight=if paired {8} else if candidate {3} else {1};
                    let evidence=recovery_paths.entry(step.0.clone()).or_default()
                        .entry(path).or_insert((0,0));
                    evidence.0+=1;
                    evidence.1+=weight;
                    break;
                }
            }
        }
        let mut discovery=Vec::new();
        let mut mutation=Vec::new();
        let mut saw_mutation=false;
        for (tool,ok,_) in steps {
            if !ok {continue;}
            let changes=matches!(tool.as_str(),
                "workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_write"|"workspace_replace_function");
            if changes {saw_mutation=true;}
            let segment=if saw_mutation {&mut mutation}else{&mut discovery};
            if segment.last()!=Some(&tool) && segment.len()<5 {segment.push(tool);}
        }
        let findings=generation["newFindings"].as_u64().unwrap_or(0);
        let eligible=generation["candidateCapture"]["checksPassed"]==true;
        let accepted=generation_is_verified(generation);
        if !discovery.is_empty() {
            let reward=if accepted {40}
                else if eligible {18}
                else if findings>0 {5+findings.min(4) as i64}
                else {1};
            if reward>best_discovery.0 {best_discovery=(reward,discovery);}
        }
        if !mutation.is_empty() {
            let reward=if accepted {100}
                else if eligible {32}
                else if generation["attemptedCandidateId"].is_string(){12}
                else {3};
            if reward>best_mutation.0 {best_mutation=(reward,mutation);}
        }
    }
    let mut sequence=best_discovery.1;
    for tool in best_mutation.1 {
        if sequence.last()!=Some(&tool) && sequence.len()<8 {
            sequence.push(tool);
        }
    }
    let rules=recovery.into_iter().filter_map(|(failed,candidates)|{
        let (next,observed)=candidates.into_iter()
            .max_by(|a,b|a.1.cmp(&b.1).then_with(||b.0.cmp(&a.0)))?;
        Some(json!({"onFailure":failed,"tryTool":next,"observedRecoveries":observed}))
    }).collect::<Vec<_>>();
    let paths=recovery_paths.into_iter().filter_map(|(failed,candidates)|{
        let (steps,(observed,weighted))=candidates.into_iter()
            .max_by(|a,b|a.1.1.cmp(&b.1.1)
                .then_with(||a.1.0.cmp(&b.1.0))
                .then_with(||b.0.cmp(&a.0)))?;
        Some(json!({"onFailure":failed,"steps":steps,
            "observedRecoveries":observed,"weightedEvidence":weighted}))
    }).collect::<Vec<_>>();
    let evidence=json!({"episodes":episode_count,
        "discoveryEvidence":best_discovery.0.max(0),
        "mutationEvidence":best_mutation.0.max(0),
        "recoveryRules":rules.len(),"verifiedEditRecoveryPaths":paths.len()});
    let revision=crate::engine::memory::stable_id(&[
        &json!([&sequence,&rules,&paths]).to_string()]);
    let evidence_revision=crate::engine::memory::stable_id(&[&evidence.to_string()]);
    json!({"revision":revision,"evidenceRevision":evidence_revision,
        "kind":"rsi.induced_action_program",
        "status":"trial_unverified","sequence":sequence,
        "onFailure":rules,"recoveryPaths":paths,"evidence":evidence,
        "origin":"cross_episode_composition_with_verified_edit_recovery"})
}

/// A read-only success is a usable *prefix*, not a complete autonomous
/// implementation algorithm. One generation of grounded discovery suffices
/// to create the next trial edit edge; waiting for two more read-only attempts
/// wastes the campaign budget before an executable improvement exists.
/// This invents a *trial procedure*, never a forged source edit or gain.
pub(super) fn complete_read_only_action_program(mut plan:Value, preferred:&str)->Value {
    let sequence=plan["sequence"].as_array().cloned().unwrap_or_default();
    let grounded=sequence.iter().any(|tool|
        tool=="workspace_read_range");
    let editable=sequence.iter().any(|tool|
        matches!(tool.as_str(),
            Some("workspace_write"|"workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_replace_function")));
    if !grounded || editable || !matches!(preferred,"workspace_replace"|"workspace_replace_range" | "workspace_replace_function") {
        return plan;
    }
    let mut next=sequence.clone();
    if next.len()>=8{return plan;}
    next.push(json!(preferred));
    let original=plan["revision"].as_str().unwrap_or("").to_string();
    let revised=crate::engine::memory::stable_id(&[
        &json!([&next,&plan["onFailure"],&plan["recoveryPaths"]]).to_string()]);
    if revised==original{return plan;}
    plan["revision"]=json!(revised);
    plan["sequence"]=json!(next);
    plan["mutatedFrom"]=json!(original);
    plan["mutationOperator"]=json!("complete_grounded_read_only_experience");
    plan["status"]=json!("trial_from_successful_read_unverified_edit");
    plan["evidence"]["originalReadOnlySequence"]=json!(sequence);
    plan["evidence"]["expectedNextEffect"]=json!("receipt_verified_source_edit");
    plan
}

/// Evolve the method that evolves the harness: after multiple independent
/// attempts of the *same policy* produce neither a candidate nor a verified
/// source edit, modify the executable program rather than retrying the same
/// ranked tools. The new revision becomes a trial; there is no quality claim.
pub(super) fn evolve_stalled_action_program(mut plan:Value,rows:&[Value],fallback:&str)->Value {
    // Follow previously *executed* descendants of this synthesized recipe.
    // Without lineage replay we would mutate the root once, then keep
    // returning that same first child forever even when the child also fails.
    // Only decisions recorded by the native runner count as a trial.
    let mut seen_revisions=BTreeSet::<String>::new();
    for _ in 0..8 {
        let Some(current)=plan["revision"].as_str() else {return plan;};
        if !seen_revisions.insert(current.to_string()) {break;}
        let child=rows.iter().filter(|row|{
            let method=&row["executionMethod"]["actionProgram"];
            method["mutatedFrom"].as_str()==Some(current)
                && method["revision"].as_str()
                    .is_some_and(|revision|revision!=current && !seen_revisions.contains(revision))
                && row["methodActions"].as_array().is_some_and(|actions|actions.iter()
                    .any(|action|action["inducedProgramDecision"]["programRevision"]
                        ==method["revision"]
                        || (action["inducedProgramDecision"].is_object()
                            && action["inducedProgramDecision"]["programRevision"].is_null())))
        }).max_by(|a,b|a["createdAt"].as_str().cmp(&b["createdAt"].as_str())
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
        let Some(child)=child else {break;};
        plan=child["executionMethod"]["actionProgram"].clone();
    }
    let Some(parent)=plan["revision"].as_str().map(str::to_string) else {return plan;};
    let mut stalled=0u64;
    let mut failed_compiles=0u64;
    let mut regressed_quality=0u64;
    let mut no_gain_candidates=0u64;
    let mut seen=BTreeSet::<String>::new();
    for row in rows {
        let method=&row["executionMethod"]["actionProgram"];
        if method["revision"].as_str()!=Some(parent.as_str()) {continue;}
        let job=row["jobId"].as_str().or_else(||row["id"].as_str()).unwrap_or("");
        if job.is_empty() || !seen.insert(job.to_string()) {continue;}
        // A research-only generation with no actual policy action is not a
        // failed trial of the synthesized program.
        if !row["methodActions"].as_array().is_some_and(|actions|
            actions.iter().any(|action|action["inducedProgramDecision"].is_object())) {
            continue;
        }
        // A real source edit that fails compilation or the frozen oracle is
        // *negative quality feedback*, not a successful method trial. The
        // previous learner skipped it forever and kept repeating that method.
        // Require operation-level attribution, not the model's policy narration.
        let executed=observed_induced_program(row)==Some(parent.as_str());
        if executed {
            if row["attemptedCandidateId"].is_string()
                && row["candidateCapture"]["checksPassed"]==false {
                failed_compiles+=1;
                continue;
            }
            if row["evaluation"]["verdict"]=="regressed"
                || row["evaluation"]["candidateStatus"]=="rejected" {
                regressed_quality+=1;
                continue;
            }
            // An independently measured, compiling patch with zero holdout
            // gain is a *method counterexample*, not a productive trial.
            // The old learner ignored every such candidate indefinitely.
            if row["attemptedCandidateId"].is_string()
                && row["candidateCapture"]["checksPassed"]==true
                && row["evaluation"]["verdict"]=="inconclusive"
                && row["evaluation"]["metrics"]["pairedCases"].as_u64().unwrap_or(0)
                    >= crate::engine::rsi::MIN_PAIRED_CASES as u64
                && row["evaluation"]["metrics"]["improvedHoldouts"].as_u64()==Some(0)
                && row["evaluation"]["metrics"]["regressions"].as_u64()==Some(0) {
                no_gain_candidates+=1;
                continue;
            }
        }
        if generation_is_verified(row) || row["attemptedCandidateId"].is_string()
            || row["writeReceipts"].as_array().is_some_and(|receipts|
                receipts.iter().any(|receipt|receipt["written"]==true)) {
            continue;
        }
        stalled+=1;
    }
    if stalled<2 && failed_compiles<2 && regressed_quality<2
        && no_gain_candidates<2 {return plan;}
    let mut sequence=plan["sequence"].as_array().cloned().unwrap_or_default();
    // The highest-value mutation is to turn a research-only program into
    // an implementation program. Dropping discovery while keeping *no edit*
    // repeats the very failure we were supposed to learn from.
    let has_edit=|sequence:&[Value]|sequence.iter().any(|step|matches!(
        step.as_str(),Some("workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_write"|"workspace_replace_function")));
    let transform=if failed_compiles>=2 || regressed_quality>=2 {
        // Rejected quality is a counterexample against this specific
        // execution algorithm. Compile failures demand refreshed grounded
        // source before a different edit operator; regressions demand a
        // fresh counterexample search rather than another blind write.
        let mut proposed=sequence.clone();
        if failed_compiles>=2 {
            if let Some(index)=proposed.iter().position(|step|
                matches!(step.as_str(),Some("workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_write"|"workspace_replace_function"))) {
                if index==0 || proposed[index-1]!="workspace_read_range" {
                    proposed.insert(index,json!("workspace_read_range"));
                }
            }
        } else if proposed.first().and_then(Value::as_str)!=Some("code_search") {
            proposed.insert(0,json!("code_search"));
        }
        if let Some(index)=proposed.iter().rposition(|step|
            matches!(step.as_str(),Some("workspace_replace"|"workspace_replace_range" | "workspace_replace_function"))) {
            let alternative=if proposed[index]=="workspace_replace" {
                "workspace_replace_range"
            } else {"workspace_replace"};
            proposed[index]=json!(alternative);
        } else if !has_edit(&proposed) {
            proposed.push(json!(fallback));
        }
        if proposed==sequence {
            // A counterexample must never create a no-op new "revision".
            proposed.insert(0,json!("workspace_read_range"));
        }
        sequence=proposed;
        if failed_compiles>=2 {"repair_failed_compilation_method"}
        else {"repair_regressed_quality_method"}
    } else if no_gain_candidates>=2 {
        // The source mutation was real and independently evaluated, but it
        // did not improve behavior. Change the *method*, not the verdict:
        // ensure a grounded read and switch the actual replacement operator.
        // The resulting child is a trial subject to the next frozen A/B.
        let mut proposed=sequence.clone();
        if !proposed.iter().any(|step|step=="workspace_read_range") {
            let edit_position=proposed.iter().position(|step|
                matches!(step.as_str(),Some("workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_write"|"workspace_replace_function")))
                .unwrap_or(proposed.len());
            proposed.insert(edit_position,json!("workspace_read_range"));
        }
        if let Some(index)=proposed.iter().rposition(|step|
            matches!(step.as_str(),Some("workspace_replace"|"workspace_replace_range" | "workspace_replace_function"))) {
            let alternative=if proposed[index]=="workspace_replace" {
                "workspace_replace_range"
            } else {"workspace_replace"};
            proposed[index]=json!(alternative);
        } else if !has_edit(&proposed) {
            proposed.push(json!(fallback));
        }
        if proposed==sequence {return plan;}
        sequence=proposed;
        "repair_zero_gain_candidate_method"
    } else if !has_edit(&sequence) {
        sequence.push(json!(fallback));
        "append_source_mutation"
    } else if let Some(repair)=plan["recoveryPaths"].as_array()
        .into_iter().flatten().filter_map(|row|{
            let steps=row["steps"].as_array()?.clone();
            if steps.is_empty() || !has_edit(&steps) {return None;}
            Some((row["weightedEvidence"].as_u64().unwrap_or(0),steps))
        }).max_by(|a,b|a.0.cmp(&b.0)) {
        // The next generation executes an observed successful repair recipe
        // instead of reusing the repeatedly unproductive predecessor.
        if repair.1!=sequence {
            sequence=repair.1;
            "promote_observed_recovery_path"
        } else if sequence.len()>2 {
            sequence.remove(0);
            "drop_unproductive_discovery"
        } else {
            let next=if sequence.last().and_then(Value::as_str)
                ==Some("workspace_replace") {"workspace_replace_range"}
                else {"workspace_replace"};
            sequence.push(json!(next));
            "expand_edit_operator"
        }
    } else if sequence.len()>2 {
        sequence.remove(0);
        "drop_unproductive_discovery"
    } else {
        let other=if sequence.last().and_then(Value::as_str)
            ==Some("workspace_replace") {
            "workspace_replace_range"
        } else {"workspace_replace"};
        sequence.push(json!(other));
        "expand_edit_operator"
    };
    let revision=crate::engine::memory::stable_id(&[
        &json!([&sequence,&plan["onFailure"],&plan["recoveryPaths"]]).to_string()]);
    if revision==parent {return plan;}
    plan["revision"]=json!(revision);
    plan["sequence"]=json!(sequence);
    plan["mutatedFrom"]=json!(parent);
    plan["mutationOperator"]=json!(transform);
    plan["failureEpisodes"]=json!(stalled);
    plan["compileFailureEpisodes"]=json!(failed_compiles);
    plan["qualityRegressionEpisodes"]=json!(regressed_quality);
    plan["qualityNoGainEpisodes"]=json!(no_gain_candidates);
    plan["status"]=json!(if failed_compiles>=2||regressed_quality>=2
        || no_gain_candidates>=2 {
        "trial_after_quality_counterexamples"
    } else {"trial_after_unproductive_predecessor"});
    plan
}

/// Attribution for learned action programs requires both a native policy
/// decision *and* the matching later tool operation. A displayed choice with
/// no executed tool is not a policy trial and cannot earn quality credit.
pub(super) fn observed_induced_program(row:&Value)->Option<&str> {
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
pub(super) fn induced_program_context(row:&Value)->Option<String> {
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
pub(super) fn induced_program_pair_outcome(challenger:&Value,control:&Value)->i8 {
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
pub(super) fn select_evaluated_action_program(rows:&[Value],proposed:Value)->(Value,Value) {
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
pub(super) fn compress_successful_action_program(mut program:Value, rows:&[Value])->Value {
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

/// Second-order transfer: an independently adopted descendant can teach
/// DOMINION which *program mutation operator* works across tasks. The learner
/// below changes the next generation's executable sequence; it never copies
/// a prior patch or treats source writes as measured quality. The transferred
/// method remains a trial until its own matched evaluation succeeds.
pub(super) fn transfer_verified_mutation_operator(mut program:Value,rows:&[Value])->Value {
    let mut operators=BTreeMap::<String,(BTreeSet<String>,BTreeSet<String>)>::new();
    for row in rows {
        if !generation_is_verified(row) || observed_induced_program(row).is_none() {
            continue;
        }
        let Some(job)=row["jobId"].as_str().filter(|job|!job.is_empty()) else {continue;};
        let Some(domain)=row["domain"].as_str().filter(|domain|!domain.is_empty()) else {continue;};
        let operator=row["executionMethod"]["actionProgram"]["mutationOperator"].as_str()
            .unwrap_or("");
        let family=match operator {
            "success_derived_discovery_ablation"=>"omit_redundant_discovery",
            "repair_failed_compilation_method"|"repair_zero_gain_candidate_method"
                =>"reobserve_and_change_editor",
            _=>continue,
        };
        let evidence=operators.entry(family.into()).or_default();
        evidence.0.insert(job.to_string());
        evidence.1.insert(domain.to_string());
    }
    let sequence=program["sequence"].as_array().cloned().unwrap_or_default();
    let prior=program["revision"].as_str().unwrap_or("").to_string();
    if sequence.is_empty() || prior.is_empty(){return program;}
    let mut candidates=operators.into_iter().filter(|(_, (jobs,domains))|
        jobs.len()>=2 && domains.len()>=2).collect::<Vec<_>>();
    candidates.sort_by(|a,b|b.1.0.len().cmp(&a.1.0.len()).then(a.0.cmp(&b.0)));
    for (operator,(jobs,domains)) in candidates {
        let mut proposed=sequence.clone();
        let edit=|steps:&[Value]|steps.iter().position(|step|
            matches!(step.as_str(),Some("workspace_replace"|"workspace_replace_range" | "workspace_replace_function")));
        let Some(mut edit_at)=edit(&proposed) else{continue;};
        match operator.as_str() {
            "omit_redundant_discovery" => {
                let Some(read_at)=proposed.iter().position(|step|step=="workspace_read_range") else{continue;};
                let Some(search_at)=proposed.iter().position(|step|step=="code_search") else{continue;};
                if search_at>=read_at || read_at>=edit_at {continue;}
                proposed.remove(search_at);
            }
            "reobserve_and_change_editor" => {
                if !proposed[..edit_at].iter().any(|step|step=="workspace_read_range") {
                    if proposed.len()>=8 {continue;}
                    proposed.insert(edit_at,json!("workspace_read_range"));
                    edit_at+=1;
                }
                proposed[edit_at]=json!(if proposed[edit_at]=="workspace_replace" {
                    "workspace_replace_range"
                }else{"workspace_replace"});
            }
            _=>continue
        }
        if proposed==sequence {continue;}
        let revision=crate::engine::memory::stable_id(&[
            &json!([&proposed,&program["onFailure"],&program["recoveryPaths"]]).to_string()
        ]);
        if revision==prior {continue;}
        program["sequence"]=json!(proposed);
        program["revision"]=json!(revision);
        program["mutatedFrom"]=json!(prior);
        program["mutationOperator"]=json!("transfer_verified_meta_operator");
        program["metaTransfer"]=json!({
            "operator":operator,"evidenceJobs":jobs.into_iter().take(8).collect::<Vec<_>>(),
            "evidenceDomains":domains.into_iter().collect::<Vec<_>>(),
            "source":"executed_descendant_and_independent_paired_adoption",
            "claim":"unverified_new_task_trial"
        });
        program["status"]=json!("trial_cross_domain_meta_operator");
        break;
    }
    program
}

/// Induce a new *execution method*, not a new model weight or a replayed
/// patch. Concrete edit receipts are training examples; independent paired
/// evidence, when present, outweighs mere file writes. The result controls
/// the next generation's native tool ordering and retry selection.
/// Long-lived edit-method learning must NOT use improvement_history():
/// that routine intentionally drops completed-candidate history when a new
/// research campaign starts. Preserve every comparable, genuinely measured
/// descendant's rule executions after a restart, while replaying the old
/// evaluator receipt before giving a successful method any reward.
pub(super) fn durable_edit_policy_history(
    state:&AppState,input:&Value,campaign:&[Value]
)->Vec<Value>{
    let partition=research_scope(input).partition_key;
    let objective=crate::engine::memory::stable_id(&[
        input["rsiObjective"].as_str().unwrap_or("")]);
    let mut rows=std::collections::BTreeMap::<String,Value>::new();
    for mut generation in state.store.all("rsi_generation").into_iter()
        .chain(campaign.iter().cloned()){
        if generation["knowledgePartition"]!=partition
            ||generation["objectiveId"]!=objective{
            continue;
        }
        let Some(job)=generation["jobId"].as_str()
            .filter(|id|!id.is_empty()).map(str::to_owned)else{continue};
        if generation["accepted"]==true{
            let independently_adopted= generation["candidateId"].as_str()
                .and_then(|id|state.store.get("rsi_candidate",id))
                .is_some_and(|candidate|
                    candidate["baseCommit"]==generation["generatorCommit"]
                    &&candidate["evaluation"]["evidenceSha256"]
                        ==generation["evaluation"]["evidenceSha256"]
                    &&candidate["experimentAdoption"]["commit"]
                        ==generation["acceptedCommit"]
                    &&crate::engine::rsi::verify_paired_receipt(
                        state,&candidate).is_ok());
            generation["evidenceVerified"]=json!(independently_adopted);
        }else{
            generation["evidenceVerified"]=json!(false);
        }
        if let Some(current)=rows.get_mut(&job){
            if generation_completion_millis(&generation)
                >generation_completion_millis(current){
                *current=generation;
            }
        }else{
            rows.insert(job,generation);
        }
    }
    let mut output=rows.into_values().collect::<Vec<_>>();
    output.sort_by(|a,b|
        generation_completion_millis(a).cmp(&generation_completion_millis(b))
            .then_with(||a["jobId"].as_str().cmp(&b["jobId"].as_str())));
    output
}

pub(super) fn verified_genome_reward(g:&Value)->f64{
    if g["candidateOrigin"]=="native_inductive_source_mutation"{
        return 0.0;
    }
    if g["accepted"]==true && g["evidenceVerified"]==true
        &&g["pairedRevalidated"]==true
        &&g["evaluation"]["metrics"]["eligible"]==true {
        return 1.0;
    }
    if g["candidateId"].is_string()
        &&g["pairedRevalidated"]==true
        &&g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"{
        return 0.10;
    }
    0.0
}

pub(super) fn learn_execution_method(state:&AppState, input:&Value, history:&[Value]) -> Value {
    #[derive(Default)]
    struct Arm { attempts:u64, writes:u64, candidates:u64, accepted:u64, rejected:u64, tokens:u64 }
    let partition=research_scope(input).partition_key;
    let objective=crate::engine::memory::stable_id(&[
        input["rsiObjective"].as_str().unwrap_or("")]);
    let mut episodes=BTreeMap::<String,Value>::new();
    for generation in state.store.all("rsi_generation").into_iter().chain(history.iter().cloned()) {
        if generation["knowledgePartition"]!=partition {continue;}
        let same_objective=generation["objectiveId"]==objective;
        // The tool method itself is source-edit-agnostic. Transfer across
        // unrelated tasks/domains is allowed only for independently adopted
        // source changes; failed or research-only attempts remain scoped.
        if !same_objective && generation["accepted"]!=true {continue;}
        let key=generation["id"].as_str().unwrap_or("");
        if !key.is_empty() {episodes.insert(key.to_string(),generation);}
    }
    let rows=revalidated_history(state,&episodes.into_values().collect::<Vec<_>>());
    let base_action_program=synthesize_action_program(&rows);
    let mut exact=Arm::default();
    let mut range=Arm::default();
    let mut function=Arm::default();
    let mut sources=BTreeSet::<String>::new();
    let mut transfer_domains=BTreeSet::<String>::new();
    let mut productive_reads=Vec::<u64>::new();
    let mut stalled_reads=0u64;
    for row in &rows {
        let same_objective=row["objectiveId"]==objective;
        if !same_objective && !generation_is_verified(row) {continue;}
        let receipts=row["writeReceipts"].as_array();
        let mut working=BTreeSet::<String>::new();
        for receipt in receipts.into_iter().flatten() {
            let arm=match receipt["tool"].as_str() {
                Some("workspace_replace")=>&mut exact,
                Some("workspace_replace_range")=>&mut range,
                Some("workspace_replace_function")=>&mut function,
                _=>continue
            };
            arm.attempts+=1;
            if receipt["written"]==true {
                arm.writes+=1;
                working.insert(receipt["tool"].as_str().unwrap().to_string());
            } else {arm.rejected+=1;}
        }
        // Evaluate exploration for *all* generations, including read-only
        // stalls and rejected source edits. Previously the single-writer
        // guard below skipped these failures, leaving the learned exploration
        // limit permanently unchanged precisely when it should improve.
        // A candidate is real only when its original worker receipt agrees.
        let compiled_candidate=row["jobId"].as_str().and_then(|job_id|
            state.store.get("rsi_run",&format!("rsi-run-{job_id}")))
            .is_some_and(|receipt|
                receipt["checksPassed"]==true
                && receipt["candidateId"].is_string()
                && (receipt["candidateId"]==row["attemptedCandidateId"]
                    || receipt["candidateId"]==row["candidateId"]));
        let reads=row["toolsUsed"].as_array().into_iter().flatten()
            .filter(|usage|matches!(usage["tool"].as_str(),
                Some("workspace_read_range"|"code_search")))
            .map(|usage|usage["calls"].as_u64().unwrap_or(0))
            .sum::<u64>();
        if reads>0 && compiled_candidate {
            productive_reads.push(reads);
        } else if reads>0 && row["attemptedCandidateId"].is_null()
            && (matches!(row["improvementDirective"]["mode"].as_str(),
                Some("implement_research"|"revise_candidate"|"repair_counterexample"|"change_approach"))
                || (reads>=3 && row["newFindings"].as_u64().unwrap_or(0)==0)) {
            // A model that was explicitly supposed to WRITE source but spent
            // tokens researching and produced zero candidates is a stalled
            // implementation even when its research produced "new findings".
            // Three live generations each read once, found something, yet
            // never edited: the old >=3-reads/no-findings condition counted
            // zero failures and never updated the next generation's policy.
            // Read-only discovery phases remain excluded from this signal.
            stalled_reads+=1;
        }
        // Independently adopted source gains teach cross-domain transfer
        // even when the model needed both editing operators. Only the
        // *tool-specific* effect attribution requires a single writer.
        if generation_is_verified(row) {
            if let Some(id)=row["objectiveId"].as_str() {sources.insert(id.to_string());}
            if let Some(domain)=row["domain"].as_str() {transfer_domains.insert(domain.to_string());}
        }
        // Attribute candidate quality to one editing arm only when exactly
        // one arm produced a confirmed mutation. Multi-tool attempts still
        // teach exploration without falsely proving either edit operator.
        if working.len()!=1 {continue;}
        let name=working.iter().next().unwrap();
        let arm=match name.as_str() {
            "workspace_replace"=>&mut exact,
            "workspace_replace_range"=>&mut range,
            "workspace_replace_function"=>&mut function,
            _=>continue,
        };
        if compiled_candidate {arm.candidates+=1;}
        if generation_is_verified(row) {arm.accepted+=1;}
        arm.tokens=arm.tokens.saturating_add(row["tokens"].as_u64().unwrap_or(0));
    }
    // Comparable outcomes rank the method. File writes are only evidence
    // that it can execute, not evidence of quality improvement.
    // Score by measured yield per attempt; a greater number of rejected
    // calls is never a positive tie-breaker.
    let score=|arm:&Arm| -> i64 {
        let gain=arm.accepted.saturating_mul(1000)
            .saturating_add(arm.candidates.saturating_mul(200))
            .saturating_add(arm.writes.saturating_mul(50));
        (gain as i64 - arm.rejected.saturating_mul(60) as i64)
            / arm.attempts.max(1) as i64
    };
    let exact_score=score(&exact);
    let range_score=score(&range);
    let function_score=score(&function);
    let (base_preferred,base_reason)=if exact.accepted>range.accepted {
        ("workspace_replace","measured_adoption")
    } else if range.accepted>exact.accepted {
        ("workspace_replace_range","measured_adoption")
    } else if exact.candidates>range.candidates {
        ("workspace_replace","candidate_verification")
    } else if range.candidates>exact.candidates {
        ("workspace_replace_range","candidate_verification")
    } else if exact.writes>0 && range.writes==0 {
        ("workspace_replace","confirmed_write_only")
    } else if range.writes>0 && exact.writes==0 {
        ("workspace_replace_range","confirmed_write_only")
    } else if exact.rejected>range.rejected && exact.writes==0 {
        ("workspace_replace_range","counterexample_exploration")
    } else if range.rejected>exact.rejected && range.writes==0 {
        ("workspace_replace","counterexample_exploration")
    } else if exact_score>range_score {
        ("workspace_replace","observed_edit_throughput")
    } else {
        ("workspace_replace_range","default_unmeasured")
    };
    // Function-scoped changes are a third, independently measured arm.
    // Never credit a function patch to the old line-range arm: it would
    // teach future generations to choose the wrong editing primitive.
    let base=if base_preferred=="workspace_replace" {&exact}else{&range};
    let function_preferred=function.writes>0 && (
        function.accepted>base.accepted
        || (function.accepted==base.accepted && function.candidates>base.candidates)
        || (function.accepted==base.accepted
            && function.candidates==base.candidates
            && function_score>score(base))
    );
    let (preferred,why)=if function_preferred {
        ("workspace_replace_function",
            if function.accepted>0 {"measured_function_adoption"}
            else if function.candidates>0 {"verified_function_candidate"}
            else {"observed_scoped_function_throughput"})
    }else{(base_preferred,base_reason)};
    let fallback=if preferred=="workspace_replace_function" {
        if exact_score>range_score {"workspace_replace"}else{"workspace_replace_range"}
    }else if preferred=="workspace_replace" {
        "workspace_replace_range"
    } else {"workspace_replace"};
    // A single real read-only episode can produce an executable edit trial.
    // Repeated policy failures then evolve that trial's recovery structure.
    let executable=complete_read_only_action_program(base_action_program,preferred);
    let proposal=evolve_stalled_action_program(executable,&rows,preferred);
    let proposed=compress_successful_action_program(proposal,&rows);
    // Transfer a proven mutation *operator*, not a model-written source patch.
    // The next generation executes this new plan under the same independent
    // evaluation and must earn its own adopted improvement.
    let proposed=transfer_verified_mutation_operator(proposed,&rows);
    let (action_program,program_comparison)=select_evaluated_action_program(&rows,proposed);
    // General autonomous research (without an explicit source focus) uses
    // this learned call limit to exit unproductive discovery. A single
    // episode is not an improvement claim; later generations measure it.
    let exploration_limit=if let Some(best)=productive_reads.iter().min() {
        best.saturating_add(2).clamp(2,8)
    } else if stalled_reads>=2 {
        6u64.saturating_sub(stalled_reads.min(4)).max(2)
    } else {6};
    let evidence=json!({
        "workspace_replace":{"attempts":exact.attempts,"confirmedWrites":exact.writes,
            "checkedCandidates":exact.candidates,"verifiedAdoptions":exact.accepted,
            "rejectedCalls":exact.rejected,"tokens":exact.tokens},
        "workspace_replace_range":{"attempts":range.attempts,"confirmedWrites":range.writes,
            "checkedCandidates":range.candidates,"verifiedAdoptions":range.accepted,
            "rejectedCalls":range.rejected,"tokens":range.tokens},
        "workspace_replace_function":{"attempts":function.attempts,"confirmedWrites":function.writes,
            "checkedCandidates":function.candidates,"verifiedAdoptions":function.accepted,
            "rejectedCalls":function.rejected,"tokens":function.tokens},
        "verifiedTransferObjectives":sources.len(),
        "verifiedTransferDomains":transfer_domains.len(),
        "productiveReadSamples":productive_reads.len(),
        "stalledResearchEpisodes":stalled_reads,
        "sourceGenerationCount":rows.len()
    });
    let revision=crate::engine::memory::stable_id(&[
        &partition,&objective,preferred,&exploration_limit.to_string(),&evidence.to_string()]);
    let learned=(exact.attempts+range.attempts+function.attempts)>0;
    json!({
        "id":format!("rsi-execution-method-{}",crate::engine::memory::stable_id(&[&partition,&objective])),
        "revision":revision,"kind":"rsi.execution_method",
        "ownerObjectiveId":objective,"knowledgePartition":partition,
        "preferredTool":preferred,"fallbackTool":fallback,
        "explorationCallLimit":exploration_limit,
        "actionProgram":action_program,
        "programComparison":program_comparison,
        "reason":why,
        "status":if sources.len()>=2 && transfer_domains.len()>=2
            && (exact.accepted+range.accepted+function.accepted)>=2 {
            "measured_transfer_observed"
        } else if exact.accepted+range.accepted+function.accepted>0 {"quality_evidence_observed"}
          else if learned {"trial_unverified"}else{"unmeasured_default"},
        "program":[
            {"step":"discover","maxToolCalls":exploration_limit},
            {"step":"observe","tool":"workspace_read_range"},
            {"step":"mutate","tool":preferred,"onFailure":fallback},
            {"step":"assess","executor":"detached_candidate_compilation_and_frozen_pairing"},
            {"step":"learn","feedback":"independent_candidate_and_source_receipts"}
        ],
        "evidence":evidence,
        "note":"Runtime tool ordering is a testable action. Recorded associations do not prove this method caused a quality gain."
    })
}

