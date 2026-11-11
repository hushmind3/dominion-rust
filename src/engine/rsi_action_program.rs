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


/// Meta-evaluator search. The rule that selects future improvement programs
/// is itself evolved from completed *independent* source evaluations.
/// Predictive concordance is not a claim that applying the rule caused gains.
fn episode_prediction(row:&Value,rule:&Value)->f64 {
    let writes=row["observedSourceEdits"].as_u64().unwrap_or(0).min(3) as f64;
    let capture=if row["candidateId"].is_string(){1.0}else{0.0};
    let findings=row["newFindings"].as_u64().unwrap_or(0).min(5) as f64;
    let failures=row["editOperations"].as_array().map(|ops|
        ops.iter().filter(|op|op["status"]=="failed"
            || op["error"].is_string()).count()).unwrap_or(0).min(5) as f64;
    let cost=(row["tokens"].as_u64().unwrap_or(0).min(100_000) as f64)/10_000.0;
    let weight=|name:&str|rule[name].as_f64().unwrap_or(0.0);
    weight("written")*writes+weight("captured")*capture
        +weight("findings")*findings-weight("failed")*failures
        -weight("cost")*cost
}

fn observed_quality(row:&Value)->Option<bool>{
    if row["jobId"].as_str().is_none_or(str::is_empty)
        || row["evaluation"]["metrics"]["eligible"]!=true
        || row["actionProgramTrial"]["arm"]!="candidate"
        || !row["actionProgramEffects"].as_array().is_some_and(|effects|
            effects.iter().any(|e|e["selected"]==true && e["performed"]==true)) {
        return None;
    }
    Some(row["accepted"]==true
        && row["evaluation"]["metrics"]["regressions"].as_u64()==Some(0)
        && row["evaluation"]["metrics"]["improvedHoldouts"]
            .as_u64().unwrap_or(0)>0)
}

/// Count correctly ordered independently evaluated better/worse episodes.
fn predictive_pairs(rows:&[&Value],rule:&Value)->Option<(u64,u64)>{
    let positives=rows.iter().filter(|row|observed_quality(row)==Some(true))
        .collect::<Vec<_>>();
    let negatives=rows.iter().filter(|row|observed_quality(row)==Some(false))
        .collect::<Vec<_>>();
    if positives.is_empty()||negatives.is_empty(){return None;}
    let mut agreement=0;
    let mut pairs=0;
    for positive in &positives {
        for negative in &negatives {
            pairs+=1;
            if episode_prediction(positive,rule)>episode_prediction(negative,rule) {
                agreement+=1;
            }
        }
    }
    Some((agreement,pairs))
}

/// Search candidate evaluator formulas on older outcomes, then accept a
/// change only when it predicts LATER unseen independent evaluations better
/// than the frozen original formula. Do not fit the rule on its validation set.
pub fn evolve_meta_evaluator(rows:&[Value])->Value{
    let baseline=json!({"written":1.0,"captured":4.0,"findings":2.0,
        "failed":1.0,"cost":0.5});
    let observations=rows.iter().filter(|row|observed_quality(row).is_some())
        .collect::<Vec<_>>();
    let unmeasured=json!({"status":"insufficient_independent_outcomes",
        "rule":baseline,"samples":observations.len(),"qualityProven":false});
    if observations.len()<8 {return unmeasured;}
    let split=(observations.len()*2/3).max(4);
    let train=&observations[..split];
    let validation=&observations[split..];
    let Some(base_train)=predictive_pairs(train,&baseline) else{return unmeasured;};
    let Some(base_validation)=predictive_pairs(validation,&baseline) else{return unmeasured;};
    let mut best=baseline.clone();
    let mut train_result=base_train;
    // These are not hardcoded action-program choices. They are evaluator
    // parameters explored over the measured outcome landscape.
    for written in [2.0,4.0,8.0,16.0] {
        for findings in [0.0,0.5,1.0,2.0] {
            for failed in [1.0,3.0,6.0] {
                let candidate=json!({"written":written,"captured":2.0,
                    "findings":findings,"failed":failed,"cost":0.5});
                let Some(score)=predictive_pairs(train,&candidate) else{continue;};
                if score.0>train_result.0 {
                    train_result=score;best=candidate;
                }
            }
        }
    }
    let Some(later)=predictive_pairs(validation,&best) else{return unmeasured;};
    if later.0<=base_validation.0 {
        return json!({"status":"heldout_rule_not_better",
            "rule":baseline,"samples":observations.len(),
            "trainPairs":base_train.1,"validationPairs":later.1,
            "baselineCorrect":base_validation.0,
            "candidateCorrect":later.0,"qualityProven":false});
    }
    let revision=memory::stable_id(&[
        "selfimprove.meta-evaluator-v1",&best.to_string()]);
    json!({"status":"temporal_predictive_gain_not_causal",
        "revision":revision,"rule":best,
        "trainingExamples":train.len(),"validationExamples":validation.len(),
        "trainingPairs":train_result.1,
        "baselineCorrect":base_validation.0,"candidateCorrect":later.0,
        "validationPairs":later.1,
        "qualityProven":false,"appliesTo":"future_execution_program_synthesis"})
}

/// Compose discovery, mutation and recovery phases from distinct actual episodes.
pub fn synthesize(rows:&[Value])->Value {
    let evaluator=evolve_meta_evaluator(rows);
    let mut discovery=(i64::MIN,Vec::<String>::new());
    let mut mutation=(i64::MIN,Vec::<String>::new());
    let mut recoveries=BTreeMap::<String,BTreeMap<String,u64>>::new();
    // Recovery programs, unlike the old one-hop fallback, can re-observe a
    // stale source and then choose a witnessed mutation from another episode.
    let mut recovery_chains=BTreeMap::<String,BTreeMap<String,(u64,u64)>>::new();
    let mut episodes=0u64;
    for row in rows {
        let trace=row["actionTrace"].as_array().into_iter().flatten()
            .filter_map(|op|{
                let name=op["tool"].as_str()?;
                tools::rsi_allowed(name).then_some((name.to_string(),op["executed"]==true,
                    op["sourceChanged"]==true,op["failure"]==true,
                    op["runId"].as_str().unwrap_or("").to_string()))
            }).collect::<Vec<_>>();
        if trace.is_empty(){continue;}
        episodes+=1;
        for pair in trace.windows(2) {
            if pair[0].3 && pair[1].1 && pair[0].0!=pair[1].0
                && !pair[0].4.is_empty() && pair[0].4==pair[1].4 {
                *recoveries.entry(pair[0].0.clone()).or_default()
                    .entry(pair[1].0.clone()).or_insert(0)+=1;
            }
        }
        for (position,event) in trace.iter().enumerate() {
            if !event.3 {continue;}
            if event.4.is_empty(){continue;}
            let mut path=Vec::<String>::new();
            let mut changed=false;
            for successor in trace.iter().skip(position+1).take(5) {
                if successor.4!=event.4 {break;}
                // Failed retries cannot be taught as successful recovery.
                if successor.3 {break;}
                if !successor.1 {continue;}
                if path.last()!=Some(&successor.0) {
                    path.push(successor.0.clone());
                }
                changed|=successor.2;
                if path.len()>=3 || changed {break;}
            }
            if path.is_empty(){continue;}
            let key=json!(path).to_string();
            let count=recovery_chains.entry(event.0.clone()).or_default()
                .entry(key).or_insert((0,0));
            count.0+=1;
            count.1+=u64::from(changed);
        }
        let mut prefix=Vec::new();
        let mut suffix=Vec::new();
        let mut saw_edit=false;
        for (name,ok,changed,_,_) in trace {
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
        let base=if proven {100} else if eligible {20}
            else if row["newFindings"].as_u64().unwrap_or(0)>0 {5} else {1};
        // The meta-evaluator changes which witnessed episodes supply the
        // next executable method. Independent quality still outranks heuristics.
        let bonus=if evaluator["status"]=="temporal_predictive_gain_not_causal" {
            episode_prediction(row,&evaluator["rule"]).round()
                .clamp(-15.0,15.0) as i64
        } else {0};
        let reward=base+bonus;
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
    // Compose the observed repair prefix with an independently witnessed edit
    // when the failed generation recovered the source but did not write it.
    let successful_edit=sequence.iter()
        .find(|tool|edit(tool.as_str())).cloned();
    let mut recovery_paths=Vec::<Value>::new();
    for (failed,variants) in recovery_chains {
        let mut ranked=variants.into_iter().filter_map(|(steps,(observations,writes))|{
            let mut path=serde_json::from_str::<Vec<String>>(&steps).ok()?;
            let mut cross_episode=false;
            if !path.iter().any(|tool|edit(tool)) {
                if let Some(edit_tool)=successful_edit.as_ref() {
                    if path.len()<3 {
                        path.push(edit_tool.clone());
                        cross_episode=true;
                    }
                }
            }
            let score=(writes as i64)*20+(observations as i64)*3
                -(path.len() as i64);
            Some((score,observations,writes,cross_episode,path))
        }).collect::<Vec<_>>();
        ranked.sort_by(|a,b|b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.4.cmp(&b.4)));
        if let Some((_,observations,writes,cross_episode,steps))=ranked.first() {
            recovery_paths.push(json!({"onFailure":failed,"steps":steps,
                "witnessedRecoveries":observations,
                "verifiedRecoveryWrites":writes,
                "crossEpisodeCompletion":cross_episode,
                "status":"trial_unverified"}));
        }
    }
    let revision=memory::stable_id(&[
        &json!([&sequence,&on_failure,&recovery_paths]).to_string()]);
    json!({"kind":"selfimprove.composed-tool-program","revision":revision,
        "status":if sequence.is_empty(){"no_witnessed_steps"}else{"trial_unproven"},
        "sequence":sequence,"onFailure":on_failure,"recoveryPaths":recovery_paths,
        "origin":"cross_episode_witnessed_composition",
        "evidence":{"episodes":episodes},"metaEvaluator":evaluator,
        "qualityProven":false})
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
        &json!([&program["sequence"],&program["onFailure"],
            &program["recoveryPaths"],checkpoint]).to_string()]);
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
        &plan["recoveryPaths"],plan["controller"]["checkpointAfter"]]).to_string()]);
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
    // A failed edit can now create an ordered recovery state machine. Advance
    // only when each chosen recovery step really succeeded in this run.
    for (position,failure) in executed.iter().enumerate().rev() {
        if failure["status"]!="failed" && !failure["result"]["error"].is_string() {
            continue;
        }
        let Some(path)=program["recoveryPaths"].as_array().into_iter().flatten()
            .find(|row|row["onFailure"]==failure["tool"]) else{continue;};
        let Some(steps)=path["steps"].as_array() else{continue;};
        let mut cursor=0usize;
        for subsequent in executed.iter().skip(position+1) {
            if cursor>=steps.len(){break;}
            if subsequent["tool"]==steps[cursor] && witnessed(subsequent) {
                cursor+=1;
            }
        }
        if let Some(next)=steps.get(cursor).and_then(Value::as_str)
            .filter(|name|tools::rsi_allowed(name)) {
            return Some((next.to_string(),"learned_multi_step_recovery"));
        }
        // All the witnessed recovery steps were completed. A verified write
        // ends the procedure; do not restart the failed original edit.
        if cursor==steps.len() && steps.iter().any(|step|
            step.as_str().is_some_and(edit)) {
            return None;
        }
    }
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



/// Cold-start a model's first actual source edit using a SHA-grounded source
/// target. The bootstrapped grammar is only a TRIAL; subsequent programs are
/// synthesized from the run's real operations. Never manufacture edit text or
/// bypass the ordinary independent candidate capture and holdout evaluator.
pub fn seed_grounded_source_edit(mut program:Value,path:&str,sha:&str,
    preferred:&str)->Value {
    if !crate::engine::rsi::safe_path(path)
        || sha.len()!=64 || !sha.bytes().all(|b|b.is_ascii_hexdigit()){
        return program;
    }
    let mut steps=program["sequence"].as_array().cloned().unwrap_or_default();
    if steps.iter().any(|step|step.as_str().is_some_and(edit)){
        return program;
    }
    // Reading the exact indexed source is a prerequisite for every cold
    // source modification. Repeated read-only episodes are not rewarded.
    steps.retain(|step|step=="workspace_read_range");
    if steps.is_empty(){steps.push(json!("workspace_read_range"));}
    let tool=if matches!(preferred,"workspace_replace"|"workspace_replace_range"){
        preferred
    }else{"workspace_replace"};
    steps.push(json!(tool));
    let checkpoint=2u64;
    if !program["onFailure"].is_array(){program["onFailure"]=json!([]);}
    if !program["recoveryPaths"].is_array(){program["recoveryPaths"]=json!([]);}
    let old=program["revision"].clone();
    let revision=memory::stable_id(&[
        "grounded-source-bootstrap-v1",
        &json!([&steps,&program["onFailure"],&program["recoveryPaths"],
            checkpoint]).to_string()]);
    program["kind"]=json!("selfimprove.composed-tool-program");
    program["revision"]=json!(revision);
    program["sequence"]=json!(steps);
    program["status"]=json!("grounded_source_execution_trial_unproven");
    program["qualityProven"]=json!(false);
    program["controller"]=json!({"checkpointAfter":checkpoint,
        "mutation":"repair_read_only_cold_start",
        "parentRevision":old,"qualityProven":false});
    program["sourceGrounding"]=json!({"path":path,"sourceSha256":sha,
        "origin":"current_index_verified_source",
        "synthesisStatus":"runnable_unverified_not_an_adopted_improvement"});
    program
}

/// Reproduction is an executable transfer, not merely inheriting the
/// predecessor's source. A genuinely adopted candidate can change the
/// *next generation's own improvement procedure*. The new program is always
/// a fresh trial; an inherited successful patch does not prove this mutation.
pub fn offspring_from_adoption(rows:&[Value],head:&str,objective:&str,domain:&str)->Option<Value>{
    let parent=rows.iter().rev().find(|row|{
        row["accepted"]==true
            && row["acceptedCommit"]==head
            && row["objectiveId"]==objective
            && row["domain"]==domain
            && row["acceptedProof"]["status"]=="verified"
            && row["acceptedProof"]["evaluation"]["metrics"]["eligible"]==true
            && row["actionProgramTrial"]["arm"]=="candidate"
            && row["actionProgramEffects"].as_array().is_some_and(|effects|
                effects.iter().any(|effect|
                    effect["programRevision"]==row["actionProgramTrial"]["revision"]
                    && effect["performed"]==true
                    && effect["verifiedSourceWrite"]==true))
            && row["actionProgramTrial"]["program"]["sequence"].as_array()
                .is_some_and(|sequence|!sequence.is_empty())
    })?;
    let mut offspring=parent["actionProgramTrial"]["program"].clone();
    let old_revision=offspring["revision"].as_str()?.to_string();
    let prior_checkpoint=offspring["controller"]["checkpointAfter"]
        .as_u64().unwrap_or(6).clamp(2,8);
    let mut sequence=offspring["sequence"].as_array()?.clone();
    let first_edit=sequence.iter().position(|tool|tool.as_str().is_some_and(edit));
    let mut next_checkpoint=prior_checkpoint;
    let transform=if let Some(position)=first_edit {
        if !sequence[..position].iter().any(|tool|tool=="workspace_read_range") {
            // The original adopted patch succeeded, but the descendant gets
            // an explicit versioned observation before another source edit.
            sequence.insert(position,json!("workspace_read_range"));
            "insert_fresh_source_observation"
        } else {
            next_checkpoint=prior_checkpoint.saturating_sub(1).max(2);
            "shorten_discovery_after_verified_adoption"
        }
    } else {
        sequence.push(json!("workspace_read_range"));
        sequence.push(json!("workspace_replace_range"));
        "add_versioned_mutation_to_verified_discovery"
    };
    let revision=memory::stable_id(&[
        "offspring-program-v1",
        &json!([&sequence,&offspring["onFailure"],&offspring["recoveryPaths"],
            next_checkpoint]).to_string()]);
    if revision==old_revision {return None;}
    let prior_depth=offspring["lineage"]["depth"].as_u64().unwrap_or(0);
    offspring["revision"]=json!(revision);
    offspring["sequence"]=json!(sequence);
    offspring["controller"]["checkpointAfter"]=json!(next_checkpoint);
    offspring["controller"]["parentRevision"]=json!(old_revision);
    offspring["status"]=json!("descendant_trial_unproven");
    offspring["qualityProven"]=json!(false);
    offspring["lineage"]=json!({
        "depth":prior_depth.saturating_add(1),
        "parentRevision":old_revision,
        "sourceParentCommit":parent["generatorCommit"],
        "adoptedParentCommit":head,
        "parentJobId":parent["jobId"],
        "mutation":transform,
        "verifiedParentSourceGain":true,
        "offspringQualityProven":false
    });
    Some(offspring)
}

/// An experimentally rejected method must not be reselected under a new
/// cosmetic ID. Regenerate its EXECUTABLE graph from the rejected cohort's
/// witnessed failure, and give the replacement a fresh independent trial.
pub fn recompose_after_failed_cohort(
    rows:&[Value],source:&str,objective:&str,model:&str,
    provider:&str,domain:&str,
)->Option<Value>{
    let mut seen=BTreeSet::<String>::new();
    let mut candidates=Vec::<Value>::new();
    for row in rows.iter().rev() {
        let trial=&row["actionProgramTrial"];
        let frozen=&trial["frozen"];
        let revision=trial["revision"].as_str().unwrap_or("");
        if revision.is_empty() || !seen.insert(revision.to_string())
            || frozen["sourceCommit"]!=source
            || frozen["objectiveId"]!=objective
            || frozen["model"]!=model
            || frozen["provider"]!=provider
            || frozen["domain"]!=domain {continue;}
        let report=trial_plan(rows,trial["program"].clone(),
            source,objective,model,provider,domain);
        if matches!(report["status"].as_str(),
            Some("baseline_preferred_by_holdout"|"paired_no_quality_gain"
                |"program_never_executed"|"program_insufficient_effects")) {
            candidates.push(trial["program"].clone());
        }
    }
    let mut parent=candidates.into_iter().next()?;
    let old=parent["revision"].as_str()?.to_string();
    let mut sequence=parent["sequence"].as_array()?.clone();
    if sequence.is_empty() {return None;}
    let transform=if let Some(at)=sequence.iter().position(|tool|tool.as_str().is_some_and(edit)) {
        if !sequence[..at].iter().any(|tool|tool=="workspace_read_range") {
            sequence.insert(at,json!("workspace_read_range"));
            "insert_observation_before_failed_mutation"
        } else {
            let current=sequence[at].as_str().unwrap_or("");
            sequence[at]=json!(if current=="workspace_replace" {
                "workspace_replace_range"
            } else {"workspace_replace"});
            "change_edit_operator_after_rejected_comparison"
        }
    } else {
        sequence.push(json!("workspace_read_range"));
        sequence.push(json!("workspace_replace_range"));
        "replace_research_only_policy_with_versioned_edit"
    };
    let checkpoint=parent["controller"]["checkpointAfter"]
        .as_u64().unwrap_or(6).saturating_sub(1).max(2);
    let revision=memory::stable_id(&[
        "recomposed-after-cohort-v1",
        &json!([&sequence,&parent["onFailure"],&parent["recoveryPaths"],
            checkpoint]).to_string()]);
    if revision==old || seen.contains(&revision) {return None;}
    parent["sequence"]=json!(sequence);
    parent["revision"]=json!(revision);
    parent["controller"]["checkpointAfter"]=json!(checkpoint);
    parent["controller"]["parentRevision"]=json!(old);
    parent["mutatedFrom"]=json!(old);
    parent["mutationOperator"]=json!(transform);
    parent["qualityProven"]=json!(false);
    parent["status"]=json!("post_comparison_recomposition_trial");
    Some(parent)
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
    matches!(report["status"].as_str(),
        Some("collecting_frozen_cohorts"|"collecting_executable_effects"))
        .then_some(proposal)
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
        else if actually_executed==0 {"program_never_executed"}
        else if actually_executed<2 && trial<4 {"collecting_executable_effects"}
        else if actually_executed<2 {"program_insufficient_effects"}
        else if trial_regressions==0 && trial_gains>control_gains {
            "candidate_supported_by_holdout"
        } else if trial_gains<control_gains || trial_regressions>0 {
            "baseline_preferred_by_holdout"
        } else {"paired_no_quality_gain"};
    let arm=if status=="candidate_supported_by_holdout" {"candidate"}
        else if matches!(status,"baseline_preferred_by_holdout"
            |"paired_no_quality_gain"|"program_never_executed"
            |"program_insufficient_effects") {"baseline"}
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
    fn evaluator_evolves_from_temporally_held_out_executed_evidence(){
        let mut rows=Vec::new();
        // Two cohorts of twelve independent evaluations, each containing
        // source-write successes and research-heavy evaluated non-gains.
        for i in 0..24 {
            let positive=i%2==0;
            rows.push(json!({"jobId":format!("job-{i}"),
                "actionProgramTrial":{"arm":"candidate"},
                "actionProgramEffects":[{"selected":true,"performed":true}],
                "evaluation":{"metrics":{"eligible":true,"regressions":0,
                    "improvedHoldouts":if positive{2}else{0}}},
                "accepted":positive,"observedSourceEdits":if positive{1}else{0},
                "candidateId":"candidate","newFindings":if positive{0}else{5},
                "tokens":1000}));
        }
        let evolved=evolve_meta_evaluator(&rows);
        assert_eq!(evolved["status"],"temporal_predictive_gain_not_causal");
        assert!(evolved["candidateCorrect"].as_u64().unwrap()
            >evolved["baselineCorrect"].as_u64().unwrap());
        assert_eq!(evolved["qualityProven"],false);
        let method=synthesize(&rows);
        assert_eq!(method["metaEvaluator"]["revision"],evolved["revision"]);
    }
    #[test]
    fn evaluator_refuses_unexecuted_or_self_claimed_success(){
        let rows=(0..24).map(|i|json!({"jobId":format!("claim-{i}"),
            "actionProgramTrial":{"arm":"candidate"},
            "accepted":true,"evaluation":{"metrics":{"eligible":true,
                "regressions":0,"improvedHoldouts":10}},
            "actionProgramEffects":[{"selected":true,"performed":false}]}))
            .collect::<Vec<_>>();
        assert_eq!(evolve_meta_evaluator(&rows)["status"],
            "insufficient_independent_outcomes");
    }

    #[test]
    fn adopted_method_can_mutate_for_three_rebuilt_generations(){
        let mut method=json!({"revision":"founder",
            "sequence":["code_search","workspace_replace"],"onFailure":[],
            "controller":{"checkpointAfter":6}});
        for depth in 1..=3u64 {
            let head=format!("adopted-{depth}");
            let revision=method["revision"].clone();
            let prior=format!("source-{}",depth-1);
            let row=json!({"jobId":format!("worker-{depth}"),
                "generatorCommit":prior,"acceptedCommit":head,
                "objectiveId":"goal","domain":"improver","accepted":true,
                "acceptedProof":{"status":"verified",
                    "evaluation":{"metrics":{"eligible":true}}},
                "actionProgramTrial":{"arm":"candidate","revision":revision,
                    "program":method},
                "actionProgramEffects":[{"programRevision":revision,
                    "performed":true,"verifiedSourceWrite":true}]});
            let successor=offspring_from_adoption(
                &[row],&head,"goal","improver").unwrap();
            assert_eq!(successor["lineage"]["depth"],depth);
            assert_ne!(successor["revision"],revision);
            assert_eq!(successor["qualityProven"],false);
            method=successor;
        }
    }
    #[test]
    fn unexecuted_program_is_replaced_not_wrongly_ranked_as_quality_loss(){
        let method=json!({"revision":"unused","sequence":["code_search"],
            "controller":{"checkpointAfter":6}});
        let rows=["candidate","baseline","candidate","baseline"].iter()
            .enumerate().map(|(i,arm)|json!({
                "jobId":format!("unused-{i}"),"generatorCommit":"head",
                "objectiveId":"goal","generatorModel":"model",
                "generatorProviderId":"provider","domain":"improver",
                "actionProgramTrial":{"arm":arm,"revision":"unused",
                    "program":method,"frozen":{"domain":"improver",
                        "sourceCommit":"head","objectiveId":"goal",
                        "model":"model","provider":"provider"}},
                "actionProgramEffects":[],"accepted":false
            })).collect::<Vec<_>>();
        let assessed=trial_plan(&rows,method,"head","goal","model","provider","improver");
        assert_eq!(assessed["status"],"program_never_executed");
        let recomposed=recompose_after_failed_cohort(&rows,"head","goal",
            "model","provider","improver").unwrap();
        assert_eq!(recomposed["sequence"],json!([
            "code_search","workspace_read_range","workspace_replace_range"]));
        assert_eq!(recomposed["qualityProven"],false);
    }
    #[test]
    fn independently_witnessed_recovery_steps_become_a_multi_step_native_program(){
        let row=json!({"jobId":"fault","actionTrace":[
            {"tool":"workspace_replace","executed":false,"failure":true,"runId":"run-a"},
            {"tool":"workspace_read_range","executed":true,"failure":false,"runId":"run-a"}
        ],"newFindings":1});
        let donor=json!({"jobId":"editor","actionTrace":[
            {"tool":"workspace_replace_range","executed":true,
                "sourceChanged":true,"failure":false}],"candidateId":"c"});
        let program=synthesize(&[row,donor]);
        assert_eq!(program["recoveryPaths"][0]["steps"],
            json!(["workspace_read_range","workspace_replace_range"]));
        assert_eq!(program["recoveryPaths"][0]["crossEpisodeCompletion"],true);
        let failure=json!({"runId":"run","opSeq":1,"tool":"workspace_replace",
            "status":"failed","result":{"error":"stale"}});
        let next=next_tool(&program,&[failure.clone()],"run").unwrap();
        assert_eq!(next.0,"workspace_read_range");
        assert_eq!(next.1,"learned_multi_step_recovery");
        let read=json!({"runId":"run","opSeq":2,"tool":"workspace_read_range",
            "status":"completed","result":{"sourceSha256":"fresh"}});
        assert_eq!(next_tool(&program,&[failure.clone(),read.clone()],"run").unwrap().0,
            "workspace_replace_range");
        let edit=json!({"runId":"run","opSeq":3,"tool":"workspace_replace_range",
            "status":"completed","result":{"written":true},
            "toolReceipt":{"verified":true}});
        assert!(next_tool(&program,&[failure,read,edit],"run").is_none());
    }

    #[test]
    fn index_grounded_cold_start_produces_executable_edit_not_a_quality_claim(){
        let no_training=json!({"revision":"no-training","sequence":[],
            "qualityProven":false});
        let seeded=seed_grounded_source_edit(no_training,
            "src/engine/decompose.rs",&"a".repeat(64),"workspace_replace");
        assert_eq!(seeded["sequence"],
            json!(["workspace_read_range","workspace_replace"]));
        assert_eq!(seeded["controller"]["checkpointAfter"],2);
        assert_eq!(seeded["qualityProven"],false);
        assert_eq!(seeded["sourceGrounding"]["sourceSha256"],"a".repeat(64));
        let again=seed_grounded_source_edit(seeded.clone(),"src/engine/decompose.rs",
            &"a".repeat(64),"workspace_replace_range");
        assert_eq!(again["revision"],seeded["revision"],
            "do not replace a learned edit with a new bootstrapped program");
    }
    #[test]
    fn bootstrap_never_bypasses_missing_index_or_unsafe_target(){
        let empty=json!({"revision":"zero","sequence":[]});
        assert_eq!(seed_grounded_source_edit(empty.clone(),
            "../outside.rs",&"a".repeat(64),"workspace_replace"),empty);
        assert_eq!(seed_grounded_source_edit(empty.clone(),
            "src/engine/decompose.rs","unverified","workspace_replace"),empty);
    }
    #[test]
    fn real_adopted_parent_reproduces_a_changed_descendant_program(){
        let parent_program=json!({"revision":"parent-program",
            "sequence":["code_search","workspace_replace"],"onFailure":[],
            "controller":{"checkpointAfter":6}});
        let first=json!({"jobId":"parent-job","accepted":true,
            "generatorCommit":"source-v0","acceptedCommit":"source-v1",
            "objectiveId":"goal","domain":"improver",
            "acceptedProof":{"status":"verified","evaluation":{"metrics":{"eligible":true}}},
            "actionProgramTrial":{"arm":"candidate","revision":"parent-program",
                "program":parent_program},
            "actionProgramEffects":[{"programRevision":"parent-program",
                "performed":true,"verifiedSourceWrite":true}]});
        let offspring=offspring_from_adoption(&[first],"source-v1","goal","improver").unwrap();
        assert_ne!(offspring["revision"],"parent-program");
        assert_eq!(offspring["lineage"]["depth"],1);
        assert_eq!(offspring["lineage"]["adoptedParentCommit"],"source-v1");
        assert_eq!(offspring["sequence"],
            json!(["code_search","workspace_read_range","workspace_replace"]));
        assert_eq!(offspring["qualityProven"],false);
    }
    #[test]
    fn unexecuted_or_unverified_parent_cannot_produce_an_offspring(){
        let first=json!({"jobId":"parent-job","accepted":true,
            "generatorCommit":"source-v0","acceptedCommit":"source-v1",
            "objectiveId":"goal","domain":"improver",
            "acceptedProof":{"status":"verified","evaluation":{"metrics":{"eligible":true}}},
            "actionProgramTrial":{"arm":"candidate","revision":"p",
                "program":{"revision":"p","sequence":["workspace_replace"]}},
            "actionProgramEffects":[]});
        assert!(offspring_from_adoption(&[first],"source-v1","goal","improver").is_none());
    }
    #[test]
    fn measured_rejection_rewrites_method_instead_of_repeating_it(){
        let parent=json!({"revision":"loser","sequence":["code_search","workspace_replace"],
            "onFailure":[],"controller":{"checkpointAfter":6}});
        let rows=["candidate","baseline","candidate","baseline"].iter().enumerate()
            .map(|(n,arm)|json!({"jobId":format!("job-{n}"),"generatorCommit":"source",
                "objectiveId":"goal","generatorModel":"model",
                "generatorProviderId":"provider","domain":"improver",
                "actionProgramTrial":{"arm":arm,"revision":"loser",
                    "program":parent,"frozen":{"sourceCommit":"source",
                    "objectiveId":"goal","model":"model","provider":"provider",
                    "domain":"improver"}},"accepted":false}))
            .collect::<Vec<_>>();
        let child=recompose_after_failed_cohort(&rows,"source","goal",
            "model","provider","improver").unwrap();
        assert_ne!(child["revision"],"loser");
        assert_eq!(child["sequence"],json!([
            "code_search","workspace_read_range","workspace_replace"]));
        assert_eq!(child["mutatedFrom"],"loser");
        assert_eq!(child["qualityProven"],false);
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