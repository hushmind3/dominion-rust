//! Cross-generation mutation operator transfer and learned execution method.
//! Child of programs.rs; all executable methods remain in the compiled successor.
use super::*;

/// Second-order transfer: an independently adopted descendant can teach
/// DOMINION which *program mutation operator* works across tasks. The learner
/// below changes the next generation's executable sequence; it never copies
/// a prior patch or treats source writes as measured quality. The transferred
/// method remains a trial until its own matched evaluation succeeds.
pub(in super::super) fn transfer_verified_mutation_operator(mut program:Value,rows:&[Value])->Value {
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
pub(in super::super) fn durable_edit_policy_history(
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

pub(in super::super) fn verified_genome_reward(g:&Value)->f64{
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

pub(in super::super) fn learn_execution_method(state:&AppState, input:&Value, history:&[Value]) -> Value {
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

