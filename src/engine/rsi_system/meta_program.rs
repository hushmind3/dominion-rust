//! Strategy feedback, executable method program compilation and inheritance.
//! Evolvable policy logic; the frozen evaluator alone judges source adoption.
use super::*;

pub(crate) fn improvement_directive(history:&[Value])->Value {
    let Some(previous)=history.last() else {
        return json!({"mode":"discover","sourceGenerationId":null,
            "reason":"no_recorded_generation"});
    };
    let observed=previous["evaluation"]["metrics"]["eligible"]==true;
    let has_patch=previous["candidateId"].is_string();
    let new_findings=previous["newFindings"].as_u64().unwrap_or(0);
    let prior=previous["improvementDirective"]["mode"].as_str().unwrap_or("");
    let failed_build=previous["evaluation"]["verdict"]=="build_failed"
        || previous["error"].as_str().is_some_and(|s|
            s.contains("build") || s.contains("compiler") || s.contains("compile"));
    let matched_control=previous["accepted"]==true && observed
        && previous["methodAblation"]["source"]
            =="compiled_method_on_off_real_worker_v1"
        && previous["methodAblation"]["methodAssociationObserved"]==true
        && previous["methodAblation"]["qualityProven"]==true
        && previous["methodAblation"]["controlEvidenceSha256"].is_string()
        && previous["methodAblation"]["treatmentEvidenceSha256"]
            ==previous["evaluation"]["evidenceSha256"];
    let mode=if matched_control {
        "evolve_executed_method_from_feedback"
    }else if previous["accepted"]==true && observed {
        "extend_verified_capability"
    }else if failed_build {
        if prior=="repair_counterexample"{"change_approach"}else{"repair_counterexample"}
    }else if has_patch {
        if prior=="revise_candidate"{"change_approach"}else{"revise_candidate"}
    }else if new_findings>0 {
        if prior=="implement_research"{"change_approach"}else{"implement_research"}
    }else{"change_approach"};
    json!({
        "sourceGenerationId":previous["id"],
        "mode":mode,"domain":previous["domain"],
        "previousAccepted":previous["accepted"]==true && observed,
        "previousVerdict":previous["evaluation"]["verdict"],
        "previousCandidateId":previous["candidateId"],
        "methodFeedback":if matched_control{json!({
            "source":"replayed_real_method_control_v1",
            "sourceGenerationId":previous["id"],
            "methodFamilyId":previous["methodAblation"]["methodFamilyId"],
            "originalMethodSourceSha256":previous["methodAblation"]["methodSourceSha256"],
            "controlId":previous["methodAblation"]["controlId"],
            "controlEvaluationId":previous["methodAblation"]["controlEvaluationId"],
            "treatmentEvaluationId":previous["methodAblation"]["treatmentEvaluationId"],
            "controlEvidenceSha256":previous["methodAblation"]["controlEvidenceSha256"],
            "treatmentEvidenceSha256":previous["methodAblation"]["treatmentEvidenceSha256"],
            "treatmentOnlyWins":previous["methodAblation"]["treatmentOnlyWins"],
            "treatmentQualityWins":previous["methodAblation"]["treatmentQualityWins"],
            "requiresNewRevisionAndSuccessorExecution":true
        })}else{Value::Null},
        "previousFocusFiles":previous["focusFiles"].as_array()
            .map(|a|a.iter().take(4).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "previousError":previous["error"].as_str().unwrap_or("")
            .chars().take(500).collect::<String>(),
        "researchArtifactHashes":previous["knowledgeArtifactHashes"].as_array()
            .map(|a|a.iter().take(6).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "newFindings":new_findings
    })
}

/// A model-authored improvement program is translated into the same executable
/// stages consumed by the real chat/runtime.rs tool router. It cannot bypass
/// hash-grounded reads, turn a proposal into an accepted result, or execute
/// unreviewed arbitrary shell code.
pub(crate) fn compile_action_program(sequence:&Value)->Option<Value> {
    let steps=sequence.as_array()?.iter().map(Value::as_str)
        .collect::<Option<Vec<_>>>()?;
    if !(2..=8).contains(&steps.len()) || steps.iter().any(|step|!matches!(*step,
        "code_search"|"workspace_read_range"|"workspace_replace"
        |"workspace_replace_range"|"workspace_write"|"rsi_system_plan"
        |"rsi_method_propose")) {return None;}
    let read=steps.iter().position(|step|*step=="workspace_read_range")?;
    let write=steps.iter().position(|step|matches!(*step,
        "workspace_replace"|"workspace_replace_range"|"workspace_write"))?;
    if read>=write {return None;}
    let mut stages=Vec::new();
    if steps[..read].contains(&"code_search"){stages.push("discover".to_string());}
    stages.push("ground".to_string());
    if steps[read+1..write].contains(&"code_search"){stages.push("contrast".to_string());}
    stages.push("edit".to_string());
    if steps[write+1..].contains(&"workspace_read_range"){
        stages.push("reground".to_string());
    }
    stages.push("verify".to_string());
    let executable=crate::engine::rsi_edit_program::normalize(&json!(stages));
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([steps,executable])).ok()?));
    Some(json!({"kind":"model_authored_executable_meta_program_v1",
        "revision":revision,"sequence":steps,"stages":executable,
        "qualityProven":false,"training":"verified_descendant_design_only"}))
}

/// The last *independently paired, adopted* model design that contains a
/// well-formed new improvement method may seed the next child. The child's
/// use of the method remains an UNMEASURED trial until later evaluation.
pub(crate) fn inherited_program(
    state:&crate::state::AppState,history:&[Value]
)->Option<Value> {
    for generation in history.iter().rev(){
        if generation["accepted"]!=true || generation["pairedRevalidated"]!=true
            || generation["candidateOrigin"]!="model_driven_detached_worktree"
            || generation["evaluation"]["metrics"]["eligible"]!=true {continue;}
        let design=&generation["systemDesign"];
        if design["source"]!="model_authored_system_design"
            || design["generatorCommit"]!=generation["generatorCommit"]
            || design["generationJobId"]!=generation["jobId"] {continue;}
        let Some(program)=compile_action_program(&design["actionProgram"]) else{continue};
        let Some(targets)=design["targetFiles"].as_array().filter(|t|!t.is_empty()) else{continue};
        let Some(id)=generation["candidateId"].as_str() else{continue};
        let Some(candidate)=state.store.get("rsi_candidate",id) else{continue};
        if candidate["status"]!="verified"
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        if !design_implemented(design,&changed)
            || !targets.iter().all(|file|file.as_str().is_some_and(|path|
                generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|hash|hash.len()==64
                        && hash.bytes().all(|b|b.is_ascii_hexdigit())))) {
            continue;
        }
        let mut trial=program;
        trial["inheritedFromCommit"]=generation["acceptedCommit"].clone();
        trial["inheritedFromDesignId"]=design["id"].clone();
        trial["inheritedFromCandidateId"]=candidate["id"].clone();
        trial["trialStatus"]=json!("unmeasured_executable_method_transfer");
        return Some(trial);
    }
    None
}


