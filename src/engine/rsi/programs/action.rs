//! Evidence-preserving action trace and revision of failed editing procedures.
//! Child of programs.rs; all executable methods remain in the compiled successor.
use super::*;

/// Source-independent record of what the engine really did. Preserve
/// action order and failures: an aggregate tool-call count cannot identify
/// recoveries, and a model saying it followed a procedure is not evidence.
pub(in super::super) fn session_action_trace(state:&AppState,session_id:&str)->Vec<Value>{
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
pub(in super::super) fn synthesize_action_program(rows:&[Value])->Value {
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
pub(in super::super) fn complete_read_only_action_program(mut plan:Value, preferred:&str)->Value {
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
pub(in super::super) fn evolve_stalled_action_program(mut plan:Value,rows:&[Value],fallback:&str)->Value {
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

