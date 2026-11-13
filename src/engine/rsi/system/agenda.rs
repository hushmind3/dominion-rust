//! Compiled, source-grounded RSI self-model: agenda responsibility.
use super::*;

/// entire source tree and measured history available to its own code.
/// Its design, ranking and future execution-method policy may change in the
/// next accepted revision; the supervising process merely authenticates facts.
pub(crate) fn system_agenda(state:&crate::state::AppState,model:&Value,
    history:&[Value],index_id:&str,root:&Path)->Value{
    let mut frontier=architecture_frontier(state,model,history,index_id,root);
    let archive=research_archive(state,model,index_id,history);
    let inherited=history.iter().rev().find_map(accepted_model_design);
    let files=source_inventory(root);
    // A successful previous agent may point to any actual source module,
    // including one it just created. Do not require that source to be the
    // human ancestor's first ranked representative.
    let mut inherited_targets=Vec::<Value>::new();
    if let Some(design)=inherited {
        for path in design["nextTargetFiles"].as_array().into_iter()
            .flatten().take(3).filter_map(Value::as_str){
            let existing=frontier.iter().find(|item|item["path"]==path).cloned();
            let proposed=existing.or_else(||{
                let file=files.iter().find(|row|row["path"]==path)?;
                if !source_entry_valid(state,root,index_id,file,"sha256"){return None;}
                let role=design["componentFiles"].as_object().and_then(|roles|
                    roles.iter().find(|(_,paths)|paths.as_array()
                        .is_some_and(|entries|entries.iter().any(|v|v==path))))
                    .map(|(role,_)|role.as_str()).unwrap_or_else(||source_role(path));
                let role=if valid_role(role){role}else{source_role(path)};
                Some(json!({
                    "role":role,"domain":design["nextDomain"],
                    "path":path,"sourceSha256":file["sha256"],
                    "sourceBytes":file["bytes"],
                    "attempts":0,"verifiedRedesigns":0,
                    "taskFailureSignalIds":[],"latestFailure":null,
                    "reason":"adopted_ancestor_next_system_design",
                    "qualityProven":false
                }))
            });
            if let Some(target)=proposed {
                if !frontier.contains(&target){frontier.push(target.clone());}
                if !inherited_targets.contains(&target){
                    inherited_targets.push(target);
                }
            }
        }
    }
    let staged=inherited_stage(history,&frontier);
    let selected=staged.as_ref().map(|row|row["target"].clone())
        .filter(Value::is_object)
        .or_else(||inherited_targets.first().cloned())
        .or_else(||frontier.first().cloned());
    let focus=if staged.is_some() {
        selected.as_ref().map(|t|vec![t.clone()]).unwrap_or_default()
    }else if !inherited_targets.is_empty() {
        inherited_targets
    }else{
        selected.as_ref().map(|t|vec![t.clone()]).unwrap_or_default()
    };
    json!({
        "source":"compiled_descendant_self_controller_v1",
        "generatorCommit":model["generatorCommit"],
        "sourceTreeSha256":model["sourceTreeSha256"],
        "frontier":frontier,
        "researchArchive":archive,
        "inheritedStage":staged,
        "target":selected,
        "focus":focus,
        "decisionOwner":"current_compiled_descendant",
        "nextHypothesis":inherited.map(|d|d["hypothesis"].clone())
            .unwrap_or(Value::Null),
        "nextImprovementMethod":inherited.and_then(|d|
            d["methodHypothesis"].as_str()).map(|s|json!(s)).unwrap_or(Value::Null)
    })
}

/// Only immutable path+hash evidence is verified by the resident parent.
/// It MUST NOT recompute the last generation's frontier order: that would
/// freeze intelligence selection and reject a genuinely new planner policy.
pub(crate) fn validate_system_agenda(state:&crate::state::AppState,
    model:&Value,agenda:&Value,index_id:&str,root:&Path,
    commit:&str,history_len:usize)->Result<(),String>{
    // The external supervisor cannot require its OWN source inventory
    // algorithm to equal the descendant's: that would freeze the
    // self-modifying architecture analyzer and block legitimate revisions.
    // The measured binary commit and each selected actual source SHA bind
    // the controller to real code without replaying its ranking algorithm.
    let fingerprint=model["sourceTreeSha256"].as_str()
        .is_some_and(|sha|sha.len()==64
            && sha.bytes().all(|byte|byte.is_ascii_hexdigit()));
    if model["generatorCommit"]!=commit
        || model["historyCount"].as_u64()!=Some(history_len as u64)
        || !fingerprint
        || !model["sourceFileCount"].as_u64()
            .is_some_and(|count|count>0 && count<=2048)
        || model["architecture"].as_array().is_none_or(|roles|
            roles.is_empty()||roles.len()>64)
        || agenda["source"]!="compiled_descendant_self_controller_v1"
        || agenda["generatorCommit"]!=commit
        || agenda["sourceTreeSha256"]!=model["sourceTreeSha256"] {
        return Err("후손의 전체 소스 구조와 세대 ID가 일치하지 않습니다.".into());
    }
    let preview=model["sourceInventory"].as_array()
        .filter(|rows|rows.len()<=64)
        .ok_or("후손 자기모형의 소스 표본 형식 오류")?;
    for row in preview {
        if !source_entry_valid(state,root,index_id,row,"sha256") {
            return Err("후손이 보고한 실제 소스 표본의 해시가 다릅니다.".into());
        }
    }
    let rows=agenda["frontier"].as_array()
        .filter(|rows|rows.len()<=64)
        .ok_or("후손 자기재설계 후보가 잘못됐습니다.")?;
    for row in rows {
        if !row["role"].as_str().is_some_and(valid_role)
            || !row["domain"].as_str().is_some_and(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(&d))
            || !source_entry_valid(state,root,index_id,row,"sourceSha256") {
            return Err("후손이 선택한 개선 대상의 실제 소스 증거가 일치하지 않습니다.".into());
        }
    }
    let chosen=&agenda["target"];
    if !chosen.is_null() && !rows.iter().any(|row|row==chosen){
        return Err("후손의 선택 목표가 자체 소스 탐색 결과와 다릅니다.".into());
    }
    let focus=agenda["focus"].as_array()
        .filter(|focus|focus.len()<=3)
        .ok_or("후손의 실제 수정 범위가 너무 큽니다.")?;
    if focus.iter().any(|row|!rows.contains(row)){
        return Err("후손의 집중 수정 대상이 실제 인덱스 증거와 다릅니다.".into());
    }
    Ok(())
}

/// A model may propose a new executable source-edit procedure. It inherits
/// only after the associated source redesign is independently adopted.
/// This is a new *trial* for the next generation, not proof that the proposed
/// program already improved performance.
pub(crate) fn inherited_program(history:&[Value])->Option<Value>{
    // A newer independently adopted source revision supersedes an older
    // learned method, even if it chose to remove the old method. Reviving an
    // earlier program after a new source adoption is stale policy inheritance.
    let last=history.iter().rev().find(|row|row["accepted"]==true)?;
    let design=accepted_model_design(last)?;
    let sequence=design["actionProgram"].as_array()
        .filter(|seq|seq.len()>=2&&seq.len()<=8)?;
    let allowed=|tool:&str|matches!(tool,
        "code_search"|"workspace_read_range"|"workspace_replace"
        |"workspace_replace_range"|"workspace_replace_function"
        |"workspace_write"|"rsi_system_plan"|"rsi_method_propose");
    if !sequence.iter().all(|tool|tool.as_str().is_some_and(allowed)){return None;}
    let read=sequence.iter().position(|tool|tool=="workspace_read_range")?;
    let write=sequence.iter().position(|tool|matches!(tool.as_str(),
        Some("workspace_replace"|"workspace_replace_range"
            |"workspace_replace_function"|"workspace_write")))?;
    if read>=write{return None;}
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&sequence).ok()?));
    Some(json!({
        "kind":"rsi.induced_action_program","revision":revision,
        "sequence":sequence,"onFailure":[],"recoveryPaths":[],
        "status":"trial_inherited_from_verified_descendant_design",
        "origin":"model_authored_self_modification",
        "evidence":{
            "adoptedCommit":last["acceptedCommit"],"designId":design["id"],
            "generatorCommit":last["generatorCommit"],
            "pairedRevalidated":true,
            "methodTrialUnverified":true
        }
    }))
}


/// An independently accepted model may teach its next compiled descendant
/// a branching, failure-aware METHOD rather than a fixed editing tool list.
/// The compiled child itself chooses between staged redesign hypotheses.
pub(crate) fn inherited_stage(history:&[Value],frontier:&[Value])->Option<Value>{
    let last_adopted=history.iter().rposition(|g|g["accepted"]==true)?;
    let ancestor=&history[last_adopted];
    let design=accepted_model_design(ancestor)?;
    let stages=design["successorStages"].as_array()?;
    let later=&history[last_adopted+1..];
    stages.iter().enumerate().filter_map(|(index,stage)|{
        let path=stage["path"].as_str()?;
        let role=stage["role"].as_str()?;
        let domain=stage["domain"].as_str()?;
        let target=frontier.iter().find(|row|
            row["path"]==path && row["role"]==role && row["domain"]==domain)?;
        let failures=later.iter().filter(|g|g["accepted"]!=true
            && g["focusFiles"].as_array().is_some_and(|files|
                files.iter().any(|file|file.as_str()==Some(path)))).count() as i64;
        let wins=later.iter().filter(|g|g["accepted"]==true
            && g["focusFiles"].as_array().is_some_and(|files|
                files.iter().any(|file|file.as_str()==Some(path)))).count() as i64;
        let policy=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        let max_retries=stage["maxRetries"].as_i64().unwrap_or(2).clamp(0,8);
        if policy=="repair_failed" && failures>max_retries{return None;}
        let prior=stage["priority"].as_i64().unwrap_or(0).clamp(-20,20);
        let gain=match policy {
            "avoid_failed"=>-3*failures,
            "repair_failed"=>if failures==0{0}else{2*failures},
            "explore_novel"=>if wins>0{-4*wins}else{3},
            "repeat_verified"=>4*wins,
            _=>return None
        };
        let score=prior+gain-2*failures.min(3);
        Some((score,index,json!({
            "source":"adopted_descendant_method_stage_v1",
            "sourceDesignId":design["id"],
            "predecessorCommit":ancestor["acceptedCommit"],
            "stageIndex":index,
            "target":target,
            "role":role,"domain":domain,
            "path":path,"sourceSha256":target["sourceSha256"],
            "objective":stage["objective"],
            "learningPolicy":policy,
            "executionPolicy":stage["executionPolicy"],
            "failedAttemptsSinceAdoption":failures,
            "adoptedAttemptsSinceAdoption":wins,
            "qualityProven":false
        })))
    }).max_by(|a,b|a.0.cmp(&b.0).then_with(||b.1.cmp(&a.1)))
        .map(|(_,_,plan)|plan)
}

/// The next compiled intelligence worker receives its own source-grounded
/// research agenda and actual inherited experimental editing method. The
/// model remains free to reject the hypothesis and implement a better one.
pub(crate) fn agenda_instruction(agenda:&Value)->String{
    let target=&agenda["target"];
    let focus=agenda["focus"].as_array().into_iter().flatten()
        .filter_map(|row|row["path"].as_str())
        .collect::<Vec<_>>().join(", ");
    let frontier=agenda["frontier"].as_array().into_iter().flatten()
        .take(7).map(|row|format!(
            "{}: path={} priorAttempts={} verifiedRedesigns={} lastFailure={} reason={}",
            row["role"],row["path"],row["attempts"],
            row["verifiedRedesigns"],row["latestFailure"],row["reason"]
        )).collect::<Vec<_>>().join("\n");
    let stage=&agenda["inheritedStage"];
    let stage_instruction=if stage.is_object(){
        format!(
            "INHERITED EXECUTABLE METHOD STAGE: role={} domain={} file={} sha={} objective={} learning={} execution={} failures={} sourceDesign={}",
            stage["role"],stage["domain"],stage["path"],
            stage["sourceSha256"],stage["objective"],
            stage["learningPolicy"],stage["executionPolicy"],
            stage["failedAttemptsSinceAdoption"],stage["sourceDesignId"])
    }else{String::new()};
    let archive=agenda["researchArchive"]["verifiedAdoptedDesigns"]
        .as_array().into_iter().flatten().take(4).map(|row|
            format!("ADOPTED roles={} sources={} hypothesis={} method={}",
                row["roles"],row["targetFiles"],row["hypothesis"],row["methodHypothesis"]))
        .chain(agenda["researchArchive"]["unverifiedFailureLessons"]
            .as_array().into_iter().flatten().take(3).map(|row|
                format!("FAILED (unproven) domain={} hypothesis={} observed={}",
                    row["domain"],row["hypothesis"],row["observedFailure"])))
        .collect::<Vec<_>>().join("\n");
    format!(
        "[SELF-OWNED TASK+META RESEARCH CONTROLLER]\n\
        Your compiled agent chose component={} domain={} because {}.\n\
        Actual target source: {}, sourceSha256={}.\n\
        Grounded focus: {}. Source-tree revision SHA-256: {}.\n\
        Your model must inspect these true source bytes, evaluate the prior \
        limitations, and invent a causal change to the *same intelligence \
        system* which will design the next descendant. Use rsi_system_plan \
        with component=system, components, componentFiles and all targetFiles \
        for a coupled redesign; new source modules are permitted. Use the \
        optional actionProgram to teach the next accepted agent a DIFFERENT \
        executable method. The first action must still be actual source \
        observation and the method must write implementation, not prose.\n\
        Previous independently adopted hypothesis: {}.\n\
        If inheritedStage is present, implement its model-authored, failure-adapted         objective using the indicated executionPolicy rather than blindly         repeating the previous attempt. This whole method can itself be         edited inside the next compiled meta-controller.\n\
        Self-selected source research frontier:\n{}\n        Persisted previous research (only adopted source gains are verified):\n{}\n{}",
        target["role"],target["domain"],target["reason"],
        target["path"],target["sourceSha256"],focus,
        agenda["sourceTreeSha256"],agenda["nextHypothesis"],frontier,archive,stage_instruction
    )
}

/// A system-generation *model* chooses the actual design and edits. The
/// descendant's source-grounded map is context, not a fixed list of allowed
/// mutations. The code index and source-edit tools still cover the repository.
pub(crate) fn instruction(self_model:&Value,owner_objective:&str)->String {
    let components=self_model["architecture"].as_array()
        .into_iter().flatten()
        .filter_map(|row|{
            let name=row["role"].as_str()?;
            let paths=row["representativeSources"].as_array()?
                .iter().filter_map(|item|item["path"].as_str()).take(3)
                .collect::<Vec<_>>().join(", ");
            Some(format!("{name}: {paths}"))
        }).collect::<Vec<_>>().join("\n");
    let inherited=self_model["inheritedDesign"].as_object().map(|_|format!("Previously accepted design: nextDomain={} hypothesis={}",self_model["inheritedDesign"]["nextDomain"],self_model["inheritedDesign"]["hypothesis"])).unwrap_or_default();
    let history=self_model["recentMeasuredGenerations"].as_array()
        .into_iter().flatten().take(4).map(|row|format!(
            "domain={} adopted={} verdict={} holdoutGain={} metaGain={}",
            row["domain"],row["accepted"],row["verdict"],
            row["improvedHoldouts"],row["metaHoldoutGains"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[COMPILED DESCENDANT: SYSTEM SELF-REDESIGN]\n\
        You are running from revision {}. The user goal is: {}.\n\
        You control design, source analysis, algorithm choice, method learning and \
        actual code edits for your next descendant. Do not treat a prewritten \
        source mutation or a single benchmark speedup as system evolution.\n\
        Inspect real source and identify a causal limitation in the current \
        agent's algorithm, harness, memory, learning, tools or improvement \
        method. Propose a genuinely new executable design, inspect source \
        hashes with code_search/workspace_read_range and WRITE the required \
        cross-module Rust changes with workspace_replace_range/function/write. \
        Before editing submit a structured, causal system design with rsi_system_plan (component, hypothesis, design, targetFiles, nextDomain), then actually WRITE code. A design alone earns no credit. Where beneficial, propose a reusable executable method using \
        rsi_method_propose; mere registration is not evidence of execution. \
        Make your next-generation planning and implementation behavior better, \
        not only an external product function. The independently frozen paired \
        evaluator alone decides adoption; do not edit the evaluator. \
        You may INVENT new intelligence components with newComponents: each \
        must include role, domain, purpose, sourcePaths AND changed \
        integrationPaths that actually call or register that code. Propose \
        interactionEdges with fromRole, toRole, fromPath, toPath, mechanism \
        and observable for verifiable mutual improvement. INVENT new \
        improvementPrinciples (name, methodRole, methodSourcePaths, \
        improvesRoles, mechanism, falsifiableOutcome, composedFrom). A \
        principle's quality is learned from what the next COMPILED agent \
        actually improves on paired holdouts, not just from source adoption. \
        These inventions \
        become inheritable roles only after independent source adoption. \
        The accepted source will be compiled as your successor, which must \
        design and execute the subsequent change.\n\
        Source architecture (representatives; the full repository is searchable):\n{}\n\
        Measured predecessor outcomes (not claims about the new generation):\n{}\n\
        Prior accepted design intent: {}\nReport source changes and receipts, not a claimed AGI or RSI completion.",
        self_model["generatorCommit"],owner_objective,components,history,inherited)
}
