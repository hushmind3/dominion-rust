//! Evidence-grounded executable program evolution for RSI descendants.
//! Tool ordering and recovery are generated from witnessed operations, not model prose.
//! A trial arm is never a quality improvement until matched frozen holdouts prove it.
use crate::{engine::{memory, tools}, state::AppState};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn edit(name:&str)->bool {
    matches!(name,"workspace_replace"|"workspace_replace_range"|"workspace_write")
}
fn witnessed(row:&Value)->bool {
    row["status"]=="completed" && row["result"]["error"].is_null()
        && (!row["tool"].as_str().is_some_and(edit)
            || (row["result"]["written"]==true && row["toolReceipt"]["verified"]==true))
}

/// The runner's durable operation ledger is the only source for program training.
pub fn action_trace(state:&AppState,session_id:&str)->Vec<Value> {
    let mut operations=state.store.all("execution_operation").into_iter()
        .filter(|row|row["sessionId"].as_str()==Some(session_id)
            && row["tool"].as_str().is_some_and(tools::rsi_allowed))
        .collect::<Vec<_>>();
    operations.sort_by(|a,b|a["createdAt"].as_str().cmp(&b["createdAt"].as_str())
        .then(a["runId"].as_str().cmp(&b["runId"].as_str()))
        .then(a["opSeq"].as_u64().cmp(&b["opSeq"].as_u64())));
    if operations.len()>64 {operations=operations.split_off(operations.len()-64);}
    operations.into_iter().map(|op|json!({
        "tool":op["tool"],"executed":witnessed(&op),
        "sourceChanged":op["tool"].as_str().is_some_and(edit) && witnessed(&op),
        "operationId":op["id"],"runId":op["runId"],
        "failure":op["status"]=="failed" || op["result"]["error"].is_string(),
    })).collect()
}

/// Compose discovery, mutation and recovery phases from distinct actual episodes.
pub fn synthesize(rows:&[Value])->Value {
    let mut discovery=(i64::MIN,Vec::<String>::new());
    let mut mutation=(i64::MIN,Vec::<String>::new());
    let mut recoveries=BTreeMap::<String,BTreeMap<String,u64>>::new();
    let mut episodes=0u64;
    for row in rows {
        let trace=row["actionTrace"].as_array().into_iter().flatten()
            .filter_map(|op|{
                let name=op["tool"].as_str()?;
                tools::rsi_allowed(name).then_some((name.to_string(),op["executed"]==true,
                    op["sourceChanged"]==true,op["failure"]==true))
            }).collect::<Vec<_>>();
        if trace.is_empty(){continue;}
        episodes+=1;
        for pair in trace.windows(2) {
            if pair[0].3 && pair[1].1 && pair[0].0!=pair[1].0 {
                *recoveries.entry(pair[0].0.clone()).or_default()
                    .entry(pair[1].0.clone()).or_insert(0)+=1;
            }
        }
        let mut prefix=Vec::new();
        let mut suffix=Vec::new();
        let mut saw_edit=false;
        for (name,ok,changed,_) in trace {
            if !ok {continue;}
            if edit(&name) {saw_edit=true;}
            let selected=if saw_edit {&mut suffix}else{&mut prefix};
            if selected.last()!=Some(&name) && selected.len()<5 {
                // Failed or unchanged writes are never part of a successful program.
                if !edit(&name) || changed {selected.push(name);}
            }
        }
        let proven=row["accepted"]==true
            && row["evaluation"]["metrics"]["eligible"]==true
            && row["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0)>0;
        let eligible=row["candidateId"].is_string();
        let reward=if proven {100} else if eligible {20}
            else if row["newFindings"].as_u64().unwrap_or(0)>0 {5} else {1};
        if !prefix.is_empty() && reward>discovery.0 {discovery=(reward,prefix);}
        if !suffix.is_empty() && reward>mutation.0 {mutation=(reward,suffix);}
    }
    let mut sequence=discovery.1;
    for name in mutation.1 {
        if sequence.len()<8 && sequence.last()!=Some(&name) {sequence.push(name);}
    }
    let on_failure=recoveries.into_iter().filter_map(|(failed,nexts)| {
        let (next,count)=nexts.into_iter().max_by(|a,b|
            a.1.cmp(&b.1).then_with(||b.0.cmp(&a.0)))?;
        Some(json!({"onFailure":failed,"tryTool":next,"observedRecoveries":count}))
    }).collect::<Vec<_>>();
    let revision=memory::stable_id(&[&json!([&sequence,&on_failure]).to_string()]);
    json!({"kind":"selfimprove.composed-tool-program","revision":revision,
        "status":if sequence.is_empty(){"no_witnessed_steps"}else{"trial_unproven"},
        "sequence":sequence,"onFailure":on_failure,"origin":"cross_episode_witnessed_composition",
        "evidence":{"episodes":episodes},"qualityProven":false})
}


/// Descendant controller mutation: experienced tool outcomes change when the
/// native harness stops broad discovery. This is an executable policy revision,
/// not a model instruction or evidence of a quality gain.
pub fn evolve_controller(rows:&[Value], mut program:Value)->Value {
    if program["sequence"].as_array().is_none_or(Vec::is_empty) {return program;}
    let source=rows.iter().rev().find(|row|
        row["actionTrace"].as_array().is_some_and(|trace|!trace.is_empty())
        && row["jobId"].as_str().is_some_and(|id|!id.is_empty()));
    let Some(last)=source else {
        program["controller"]=json!({"checkpointAfter":6,
            "mutation":"cold_start","qualityProven":false});
        return program;
    };
    let previous=rows.iter().rev().find(|row|
        row["actionProgramTrial"]["arm"]=="candidate"
        && row["actionProgramTrial"]["program"]["controller"]["checkpointAfter"].is_u64());
    let earlier=previous.and_then(|row|
        row["actionProgramTrial"]["program"]["controller"]["checkpointAfter"].as_u64())
        .unwrap_or(6);
    let mut checkpoint=earlier.clamp(2,8);
    let failed_edits=last["editOperations"].as_array().is_some_and(|ops|ops.iter().any(|op|
        op["written"]!=true && (op["status"]=="failed" || op["error"].is_string())));
    let changed=last["observedSourceEdits"].as_u64().unwrap_or(0)>0;
    let candidate=last["candidateId"].is_string();
    let stopped=last["stopReason"].as_str().unwrap_or("");
    let reason=if last["accepted"]==true {
        "preserve_verified_repair_window"
    } else if stopped.contains("token") || last["error"].as_str()
        .is_some_and(|text|text.contains("token_budget")) {
        checkpoint=checkpoint.saturating_sub(2).max(2);
        "reduce_discovery_after_budget_exhaustion"
    } else if failed_edits {
        checkpoint=checkpoint.saturating_sub(1).max(2);
        "reach_hash_grounded_edit_earlier_after_failed_tool"
    } else if changed && !candidate {
        checkpoint=checkpoint.saturating_sub(1).max(2);
        "reserve_more_cycles_for_candidate_capture"
    } else if candidate && last["evaluation"]["metrics"]["improvedHoldouts"]
        .as_u64().unwrap_or(0)==0 {
        checkpoint=(checkpoint+1).min(8);
        "expand_evidence_after_neutral_candidate"
    } else if !changed && last["newFindings"].as_u64().unwrap_or(0)==0 {
        checkpoint=checkpoint.saturating_sub(1).max(2);
        "shorten_unproductive_discovery"
    } else {"retain_observed_discovery_budget"};
    let revision=memory::stable_id(&[
        &json!([&program["sequence"],&program["onFailure"],checkpoint]).to_string()]);
    let parent=program["revision"].clone();
    program["revision"]=json!(revision);
    program["controller"]=json!({"checkpointAfter":checkpoint,
        "mutation":reason,"sourceJobId":last["jobId"],
        "parentRevision":previous.map(|row|row["actionProgramTrial"]["revision"].clone())
            .unwrap_or(parent),
        "qualityProven":false});
    program
}

/// A policy that reached the native tool menu twice without producing a real edit
/// evolves its procedure. Merely sending an instruction is not an attempted trial.
pub fn evolve_stalled(rows:&[Value],mut plan:Value)->Value {
    let Some(parent)=plan["revision"].as_str().map(str::to_string) else{return plan;};
    let mut stalled=0u64;
    let mut jobs=BTreeSet::<String>::new();
    for row in rows {
        if row["actionProgramTrial"]["arm"]!="candidate"
            || row["actionProgramTrial"]["revision"]!=parent {continue;}
        let job=row["jobId"].as_str().unwrap_or("");
        if job.is_empty() || !jobs.insert(job.to_string()) {continue;}
        let attempted=row["actionProgramEffects"].as_array().is_some_and(|effects|
            effects.iter().any(|effect|effect["selected"]==true));
        if !attempted {continue;}
        let changed=row["actionProgramEffects"].as_array().is_some_and(|effects|
            effects.iter().any(|effect|effect["verifiedSourceWrite"]==true));
        if changed || row["candidateId"].is_string() || row["accepted"]==true {continue;}
        stalled+=1;
    }
    if stalled<2 {return plan;}
    let mut seq=plan["sequence"].as_array().cloned().unwrap_or_default();
    if seq.is_empty(){return plan;}
    let operator=if !seq.iter().any(|v|v.as_str().is_some_and(edit)){
        seq.push(json!("workspace_replace_range"));
        "append_missing_edit_phase"
    } else if seq.len()>2 && !seq[0].as_str().is_some_and(edit) {
        seq.remove(0);
        "remove_stalled_discovery_phase"
    } else {
        let replacement=if seq.last().and_then(Value::as_str)==Some("workspace_replace") {
            "workspace_replace_range"
        } else {"workspace_replace"};
        if seq.len()<8 {seq.push(json!(replacement));}
        else if let Some(last)=seq.last_mut(){*last=json!(replacement);}
        "switch_failed_edit_primitive"
    };
    let revision=memory::stable_id(&[&json!([&seq,&plan["onFailure"],
        plan["controller"]["checkpointAfter"]]).to_string()]);
    if revision!=parent {
        plan["sequence"]=json!(seq);
        plan["revision"]=json!(revision);
        plan["mutatedFrom"]=json!(parent);
        plan["mutationOperator"]=json!(operator);
        plan["failedTrials"]=json!(stalled);
        plan["status"]=json!("evolved_trial_unproven");
    }
    plan
}

/// The currently executing native worker uses the generated procedure to choose
/// a *different tool menu*. Steps are skipped only after witnessed execution.
pub fn next_tool(program:&Value,ops:&[Value],execution_id:&str)->Option<(String,&'static str)> {
    let seq=program["sequence"].as_array()?;
    if seq.is_empty(){return None;}
    let mut executed=ops.iter()
        .filter(|op|op["runId"].as_str()==Some(execution_id))
        .collect::<Vec<_>>();
    executed.sort_by_key(|op|op["opSeq"].as_u64().unwrap_or(u64::MAX));
    if let Some(last)=executed.last().filter(|row|
        row["status"]=="failed" || row["result"]["error"].is_string()) {
        if let Some(recovery)=program["onFailure"].as_array().into_iter().flatten()
            .find(|row|row["onFailure"]==last["tool"]) {
            let next=recovery["tryTool"].as_str()?;
            if tools::rsi_allowed(next) {
                return Some((next.to_string(),"observed_failure_recovery"));
            }
        }
    }
    let mut cursor=0usize;
    for operation in executed {
        if !witnessed(operation){continue;}
        if let Some(offset)=seq.iter().skip(cursor)
            .position(|step|step==&operation["tool"]) {
            cursor+=offset+1;
        }
    }
    seq.get(cursor).and_then(Value::as_str)
        .filter(|name|tools::rsi_allowed(name))
        .map(|name|(name.to_string(),"composed_cross_episode_step"))
}

pub fn tool_allowed(name:&str,chosen:&str)->bool {
    name==chosen || name=="workspace_read_range"
}

/// Every chosen tool must be witnessed *after* the choice on the same execution.
/// Selection alone is not source improvement or a valid training reward.
pub fn observed_effects(state:&AppState,session_id:&str)->Vec<Value> {
    let operations=state.store.all("execution_operation");
    let focus=state.store.get("session",session_id)
        .and_then(|session|session["rsiFocusFiles"].as_array().cloned())
        .unwrap_or_default();
    let mut effects=Vec::new();
    for execution in state.store.all("execution").into_iter()
        .filter(|row|row["sessionId"].as_str()==Some(session_id)) {
        let Some(run_id)=execution["id"].as_str() else{continue;};
        for choice in execution["rsiActionProgramChoices"].as_array().into_iter().flatten() {
            let Some(tool)=choice["chosenTool"].as_str() else{continue;};
            let after=choice["afterOpSeq"].as_u64().unwrap_or(u64::MAX);
            let matched=operations.iter().filter(|op|
                op["runId"].as_str()==Some(run_id)
                && op["tool"].as_str()==Some(tool)
                && op["opSeq"].as_u64().is_some_and(|n|n>after)
                && witnessed(op)).collect::<Vec<_>>();
            effects.push(json!({"programRevision":choice["programRevision"],
                "executionId":run_id,"chosenTool":tool,"afterOpSeq":after,
                "selected":true,"performed":!matched.is_empty(),
                "verifiedSourceWrite":matched.iter().any(|op|
                    op["tool"].as_str().is_some_and(edit) && witnessed(op)
                    && (focus.is_empty() || focus.contains(&op["arguments"]["path"]))),
                "operationIds":matched.iter().map(|op|op["id"].clone()).collect::<Vec<_>>(),
                "qualityGain":"not_inferred_from_tool_selection"}));
            if effects.len()>=64{return effects;}
        }
    }
    effects
}

/// Reuse the SAME executable candidate while a matched comparison is still
/// running. Re-synthesizing every generation silently invalidates the cohort
/// before its baseline arm can be measured.
pub fn pinned_program(rows:&[Value],source:&str,objective:&str,model:&str,
    provider:&str,domain:&str)->Option<Value>{
    let incumbent=rows.iter().rev().find(|row|{
        let frozen=&row["actionProgramTrial"]["frozen"];
        frozen["sourceCommit"]==source && frozen["objectiveId"]==objective
            && frozen["model"]==model && frozen["provider"]==provider
            && frozen["domain"]==domain
            && row["actionProgramTrial"]["program"]["sequence"].as_array()
                .is_some_and(|steps|!steps.is_empty())
    })?;
    let proposal=incumbent["actionProgramTrial"]["program"].clone();
    let report=trial_plan(rows,proposal.clone(),source,objective,model,provider,domain);
    (report["status"]=="collecting_frozen_cohorts").then_some(proposal)
}

/// Frozen cohort: same program revision, source commit, objective, model and
/// provider. An actual holdout gain, not a write, decides a candidate promotion.
pub fn trial_plan(rows:&[Value],program:Value,source:&str,objective:&str,
    model:&str,provider:&str,domain:&str)->Value {
    if program["sequence"].as_array().is_none_or(Vec::is_empty) {
        return json!({"arm":"baseline","status":"no_executable_program","program":program});
    }
    let revision=program["revision"].as_str().unwrap_or("").to_string();
    let mut trial=0u64;
    let mut control=0u64;
    let mut trial_gains=0u64;
    let mut control_gains=0u64;
    let mut trial_regressions=0u64;
    let mut unattributed_adoptions=0u64;
    let mut actually_executed=0u64;
    for row in rows {
        if row["actionProgramTrial"]["revision"]!=revision
            || row["generatorCommit"]!=source
            || row["objectiveId"]!=objective
            || row["generatorModel"]!=model
            || row["generatorProviderId"]!=provider
            || row["domain"]!=domain
            || row["actionProgramTrial"]["frozen"]["domain"]!=domain {continue;}
        let gain=row["accepted"]==true
            && row["evaluation"]["metrics"]["eligible"]==true
            && row["evaluation"]["metrics"]["regressions"].as_u64()==Some(0)
            && row["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0)>0;
        let regression=row["evaluation"]["verdict"]=="regressed";
        // A good patch written independently of the composed program cannot
        // give that program a quality reward. Require a post-selection,
        // verified source write from the selected tool on the focused file.
        let executed=row["actionProgramEffects"].as_array().is_some_and(|effects|
            effects.iter().any(|e|e["programRevision"]==revision
                && e["performed"]==true));
        let attributed_write=row["actionProgramEffects"].as_array().is_some_and(|effects|
            effects.iter().any(|e|e["programRevision"]==revision
                && e["performed"]==true && e["verifiedSourceWrite"]==true));
        if row["actionProgramTrial"]["arm"]=="candidate" {
            trial+=1;
            actually_executed+=u64::from(executed);
            unattributed_adoptions+=u64::from(gain && !attributed_write);
            trial_gains+=u64::from(gain && attributed_write);
            trial_regressions+=u64::from(regression);
        } else if row["actionProgramTrial"]["arm"]=="baseline" {
            control+=1;
            control_gains+=u64::from(gain);
        }
    }
    let paired=trial>=2 && control>=2;
    let status=if !paired{"collecting_frozen_cohorts"}
        else if trial_regressions==0 && trial_gains>control_gains {
            "candidate_supported_by_holdout"
        } else if trial_gains<control_gains || trial_regressions>0 {
            "baseline_preferred_by_holdout"
        } else {"paired_no_quality_gain"};
    let arm=if status=="candidate_supported_by_holdout" {"candidate"}
        else if status=="baseline_preferred_by_holdout"
            || status=="paired_no_quality_gain" {"baseline"}
        else if trial<=control {"candidate"} else {"baseline"};
    json!({"arm":arm,"status":status,"revision":revision,"program":program,
        "frozen":{"sourceCommit":source,"objectiveId":objective,
            "model":model,"provider":provider,"domain":domain},
        "comparison":{"candidateGenerations":trial,"baselineGenerations":control,
            "candidateHoldoutGains":trial_gains,"baselineHoldoutGains":control_gains,
            "candidateRegressions":trial_regressions,
            "candidatePolicyActionsExecuted":actually_executed,
            "adoptionsWithoutAttributableProgramWrite":unattributed_adoptions,
            "qualityProven":status=="candidate_supported_by_holdout",
            "causalQualityGain":"not_estimated_by_sequential_cohorts"}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combines_witnessed_search_and_edit_from_independent_episodes(){
        let plan=synthesize(&[
            json!({"actionTrace":[{"tool":"code_search","executed":true}],
                "newFindings":3}),
            json!({"actionTrace":[{"tool":"workspace_replace","executed":true,
                "sourceChanged":true}],"candidateId":"candidate"}),
        ]);
        assert_eq!(plan["sequence"],json!(["code_search","workspace_replace"]));
        assert_eq!(plan["qualityProven"],false);
    }
    #[test]
    fn executable_program_evolves_after_independent_unproductive_trials(){
        let plan=synthesize(&[json!({"actionTrace":[
            {"tool":"code_search","executed":true}],"newFindings":1})]);
        let revision=plan["revision"].clone();
        let trials=["job1","job2"].iter().map(|id|json!({"jobId":id,
            "actionProgramTrial":{"revision":revision,"arm":"candidate"},
            "actionProgramEffects":[{"selected":true,"performed":false}],
            "accepted":false})).collect::<Vec<_>>();
        let changed=evolve_stalled(&trials,plan);
        assert_ne!(changed["revision"],revision);
        assert_eq!(changed["mutationOperator"],"append_missing_edit_phase");
        assert_eq!(changed["sequence"],json!(["code_search","workspace_replace_range"]));
    }
    #[test]
    fn worker_advances_only_on_actual_tool_operations(){
        let program=json!({"sequence":["code_search","workspace_replace"]});
        let fake=json!({"runId":"run","opSeq":1,"tool":"code_search",
            "status":"failed","result":{"error":"bad"}});
        assert_eq!(next_tool(&program,&[fake],"run").unwrap().0,"code_search");
        let real=json!({"runId":"run","opSeq":1,"tool":"code_search",
            "status":"completed","result":{}});
        assert_eq!(next_tool(&program,&[real],"run").unwrap().0,"workspace_replace");
    }
    #[test]
    fn controller_mutates_discovery_limit_from_real_failed_tools(){
        let baseline=json!({"sequence":["code_search","workspace_replace"],
            "onFailure":[],"revision":"base"});
        let failure=json!({"jobId":"worker","actionTrace":[{"tool":"code_search"}],
            "editOperations":[{"status":"failed","written":false}],
            "observedSourceEdits":0,"accepted":false});
        let evolved=evolve_controller(&[failure],baseline);
        assert_eq!(evolved["controller"]["checkpointAfter"],5);
        assert_eq!(evolved["controller"]["qualityProven"],false);
    }
    #[test]
    fn independent_model_edit_does_not_promote_an_unexecuted_program(){
        let program=json!({"sequence":["workspace_replace"],"revision":"r1"});
        let mk=|id:&str,arm:&str|json!({"jobId":id,"generatorCommit":"head",
            "objectiveId":"obj","generatorModel":"model","generatorProviderId":"provider",
            "domain":"memory","actionProgramTrial":{"revision":"r1",
                "arm":arm,"frozen":{"domain":"memory"}},
            "accepted":arm=="candidate","actionProgramEffects":[],
            "evaluation":{"metrics":{"eligible":true,
                "regressions":0,"improvedHoldouts":2}}});
        let rows=vec![mk("c1","candidate"),mk("b1","baseline"),
            mk("c2","candidate"),mk("b2","baseline")];
        let plan=trial_plan(&rows,program,"head","obj","model","provider","memory");
        assert_eq!(plan["comparison"]["candidateHoldoutGains"],0);
        assert_eq!(plan["comparison"]["adoptionsWithoutAttributableProgramWrite"],2);
        assert_eq!(plan["comparison"]["qualityProven"],false);
    }
    #[test]
    fn policy_cannot_gain_quality_from_tool_calls_or_mismatched_source(){
        let program=json!({"sequence":["workspace_replace"],"revision":"r1"});
        let old=json!({"generatorCommit":"other","objectiveId":"obj",
            "generatorModel":"model","generatorProviderId":"provider",
            "actionProgramTrial":{"revision":"r1","arm":"candidate"},
            "accepted":true,"evaluation":{"metrics":{"eligible":true,
                "regressions":0,"improvedHoldouts":2}}});
        let plan=trial_plan(&[old],program,"head","obj","model","provider","memory");
        assert_eq!(plan["comparison"]["candidateGenerations"],0);
        assert_eq!(plan["comparison"]["qualityProven"],false);
    }
}