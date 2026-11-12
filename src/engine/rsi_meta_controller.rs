//! Editable metacognitive controller compiled into each accepted descendant.
//! Inspired by HyperAgents, predictive improvement dynamics, and comparative
//! cross-trajectory evolution. It proposes search experiments; frozen independent
//! evaluation is the sole authority for adoption. Every forecast is calibrated
//! against the *next* observed, separately verified outcome.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn model_design_roles(g:&Value)->Vec<String>{
    let design=&g["systemDesign"];
    if design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=g["generatorCommit"]
        || design["generationJobId"]!=g["jobId"] {return Vec::new();}
    let names=if design["component"]=="system"{
        design["components"].as_array().cloned().unwrap_or_default()
    }else{vec![design["component"].clone()]};
    names.iter().filter_map(Value::as_str)
        .filter(|name|!name.is_empty() && name.len()<=64
            && name.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-')
            && *name!="system")
        .map(str::to_string).collect()
}
fn independent_adoption(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && !model_design_roles(g).is_empty()
        && g["systemDesign"]["targetFiles"].as_array()
            .is_some_and(|files|!files.is_empty() && files.iter().all(|path|
                path.as_str().is_some_and(|p|
                    g["acceptedSourceHashes"][p].as_str().is_some_and(|sha|
                        sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
}
fn role_touched(g:&Value,role:&str)->bool{
    model_design_roles(g).iter().any(|name|name==role)
        || g["selfControllerVerified"]==true
            && g["metaPolicy"]["target"]["role"]==role
}
fn failure_kind(g:&Value)->&'static str{
    if g["accepted"]==true{return "adopted";}
    if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|error|
            error.contains("compile")||error.contains("compiler")) {
        return "compiler_failure";
    }
    if g["candidateId"].is_null() {
        return "no_verified_code_candidate";
    }
    if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0{
        return "behavior_regression";
    }
    if g["evaluation"]["verdict"].is_null(){return "evaluation_unavailable";}
    "no_independent_gain"
}
fn next_generation_productivity(history:&[Value],generation:&Value)->usize{
    if !independent_adoption(generation){return 0;}
    let Some(child_commit)=generation["acceptedCommit"].as_str() else{return 0};
    history.iter().filter(|child|
        child["generatorCommit"]==child_commit
            && child["selfControllerVerified"]==true
            && independent_adoption(child)
    ).count()
}
fn confidence_adjustment(history:&[Value])->f64{
    let errors=history.iter().filter_map(|row|
        row["metaLearning"]["brierError"].as_f64()
            .filter(|value|value.is_finite() && *value>=0.0 && *value<=1.0))
        .rev().take(12).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.6*mean).clamp(0.4,1.0)
}

/// Compare independent outcomes across generations, rather than choosing
/// an arbitrary first unmodified module. Success counts only when a
/// model-authored system design changed its declared source and the original
/// paired evaluator adopted that change. Future-generation productivity is a
/// distinct observation from immediate benchmark gain.
pub(crate) fn synthesize(
    history:&[Value],frontier:&[Value],allowed_domains:&[&str],
    inherited_domain:Option<&str>
)->Value{
    let calibration=confidence_adjustment(history);
    let mut ranked=Vec::<(Value,f64)>::new();
    let trials=history.len() as f64;
    for target in frontier.iter().filter(|row|
        row["domain"].as_str().is_some_and(|domain|allowed_domains.contains(&domain))) {
        let Some(role)=target["role"].as_str() else{continue};
        let relevant=history.iter().filter(|g|role_touched(g,role)).collect::<Vec<_>>();
        let attempts=relevant.len();
        let adopted=relevant.iter().filter(|g|independent_adoption(g)).count();
        let regressions=relevant.iter().filter(|g|
            g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0).count();
        let fruitless=relevant.iter().filter(|g|
            g["candidateId"].is_null() && g["accepted"]!=true).count();
        let productivity=relevant.iter()
            .map(|g|next_generation_productivity(history,g)).sum::<usize>();
        let probability=(adopted as f64+1.0)/(attempts as f64+2.0);
        let epistemic=(probability*(1.0-probability)/(attempts as f64+3.0)).sqrt();
        let coverage=if target["verifiedRedesigns"].as_u64().unwrap_or(0)>0{0.0}else{0.22};
        let inherited_bonus=if inherited_domain==target["domain"].as_str(){0.12}else{0.0};
        let size=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(size/12_000.0).min(1.0);
        let penalty=regressions as f64/(attempts.max(1) as f64);
        let stall=fruitless as f64/(attempts.max(1) as f64);
        let clade=(productivity as f64/(adopted.max(1) as f64)).min(3.0);
        // UCB-style uncertainty, Bayesian shrinkage, and descendant
        // productivity provide a bounded, auditable choice. A failed edit is
        // evidence about execution feasibility, not proof the design is bad.
        let score=0.55*probability+0.48*epistemic*calibration
            +0.14*((trials+2.0).ln()/(attempts as f64+1.0)).sqrt()
            +coverage+inherited_bonus+0.14*clade
            -0.12*penalty-0.07*stall-0.035*cost;
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "sourceSha256":target["sourceSha256"],
            "score":score,"predictedAdoptionProbability":probability,
            "uncertainty":epistemic*calibration,"attempts":attempts,
            "verifiedModelAdoptions":adopted,"regressions":regressions,
            "editsWithoutCandidate":fruitless,
            "productiveDescendantAdoptions":productivity,
            "newComponent":target["verifiedRedesigns"].as_u64().unwrap_or(0)==0
        }),score));
    }
    ranked.sort_by(|(left,a),(right,b)|
        b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||left["role"].as_str().cmp(&right["role"].as_str())));
    let winner=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    let target=winner["target"].clone();
    let reference=history.iter().rev().filter(|g|independent_adoption(g))
        .take(2).map(|g|json!({
            "generationJobId":g["jobId"],"acceptedCommit":g["acceptedCommit"],
            "roles":model_design_roles(g),
            "hypothesis":g["systemDesign"]["hypothesis"].as_str().unwrap_or("")
                .chars().take(400).collect::<String>(),
            "design":g["systemDesign"]["design"].as_str().unwrap_or("")
                .chars().take(700).collect::<String>(),
            "targetFiles":g["systemDesign"]["targetFiles"],
            "metaGain":g["evaluation"]["metrics"]["metaHoldoutGains"],
            "productiveDescendants":next_generation_productivity(history,g)
        })).collect::<Vec<_>>();
    let counterexamples=history.iter().rev().filter(|g|
        g["accepted"]!=true && !model_design_roles(g).is_empty())
        .take(3).map(|g|json!({
            "generationJobId":g["jobId"],"roles":model_design_roles(g),
            "failureKind":failure_kind(g),
            "sourceTarget":g["systemDesign"]["targetFiles"],
            "hypothesis":g["systemDesign"]["hypothesis"].as_str().unwrap_or("")
                .chars().take(250).collect::<String>(),
            "error":g["error"].as_str().unwrap_or("")
                .chars().take(500).collect::<String>()
        })).collect::<Vec<_>>();
    let mode=if counterexamples.iter().any(|row|
        row["failureKind"]=="compiler_failure"){"causal_repair_and_new_design"}
        else if !reference.is_empty(){"comparative_hybridization"}
        else{"open_ended_mechanism_discovery"};
    let alternatives=ranked.iter().take(6)
        .map(|(row,_)|row.clone()).collect::<Vec<_>>();
    let serialized=json!([target,alternatives,reference,counterexamples,mode,
        inherited_domain,history.len()]);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&serialized).unwrap_or_default()));
    json!({
        "source":"compiled_editable_meta_controller_v1",
        "revision":revision,"historyCount":history.len(),
        "mode":mode,"target":target,"selectionEvidence":winner,
        "alternatives":alternatives,
        "referenceDesigns":reference,"counterexamples":counterexamples,
        "forecast":{"acceptanceProbability":winner["predictedAdoptionProbability"],
            "uncertainty":winner["uncertainty"],
            "calibrationFactor":calibration,
            "qualityProven":false},
        "metaMechanismSource":"src/engine/rsi_meta_controller.rs",
        "successorCanRewriteMetaMechanism":true,
        "frozenEvaluatorUnchanged":true
    })
}

/// A frozen-case digest distinguishes compatible comparisons from coincidental
/// success labels. An absent suite never admits peers into the archive.
pub(crate) fn comparison_profile(input:&Value)->Option<String>{
    let cases=input["rsiFrozenCases"].as_array().filter(|cases|!cases.is_empty())?;
    Some(format!("{:x}",Sha256::digest(serde_json::to_vec(cases).ok()?)))
}

/// MGM-style other-lineage references: the built successor may inspect
/// independently paired designs from other campaigns sharing the same frozen
/// suite. No foreign patch is imported or credited to this descendant.
pub(crate) fn peer_lineages(
    state:&crate::state::AppState,campaign:&str,input:&Value,
    preferred_role:Option<&str>,
)->Value {
    let profile=comparison_profile(input);
    let mut ranked=Vec::<(i64,Value)>::new();
    let mut patches=BTreeSet::new();
    if let Some(sha)=profile.as_deref(){
        for generation in state.store.all("rsi_generation").into_iter().take(256) {
            if generation["campaignId"].as_str()==Some(campaign)
                || generation["comparisonProfileSha256"].as_str()!=Some(sha)
                || !independent_adoption(&generation) {continue;}
            let Some(candidate)=generation["candidateId"].as_str()
                .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
            if candidate["status"]!="verified"
                || candidate["evaluation"]["metrics"]["eligible"]!=true
                || candidate["baseCommit"]!=generation["generatorCommit"]
                || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
                || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
            let Some(patch)=candidate["patchSha256"].as_str() else{continue};
            if !patches.insert(patch.to_string()){continue;}
            let mut aligned=Vec::new();let mut same=0usize;
            for file in generation["systemDesign"]["targetFiles"].as_array()
                .into_iter().flatten().filter_map(Value::as_str).take(24) {
                if !crate::engine::rsi::safe_path(file){continue;}
                if let Some(current)=crate::engine::code_index::indexed_source_hash(
                    state,input["rsiIndexId"].as_str().unwrap_or(""),file) {
                    same+=usize::from(
                        generation["acceptedSourceHashes"][file].as_str()==Some(current.as_str()));
                    aligned.push(file.to_owned());
                }
            }
            if aligned.is_empty() || same==0 {continue;}
            // An unrelated lineage with a different current source revision
            // is not transferable merely because the filename overlaps.
            let roles=model_design_roles(&generation);
            let match_role=preferred_role.is_some_and(|role|roles.iter().any(|r|r==role));
            let gain=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0);
            let score=i64::from(match_role)*1000+gain.min(20) as i64*30
                +same.min(8) as i64*3;
            ranked.push((score,json!({
                "source":"paired_cross_campaign_design_reference_v1",
                "campaignId":generation["campaignId"],
                "generationId":generation["id"],
                "candidateId":candidate["id"],
                "patchSha256":patch,
                "generatorCommit":generation["generatorCommit"],
                "acceptedCommit":generation["acceptedCommit"],
                "comparisonProfileSha256":sha,
                "roles":roles,"alignedSourceFiles":aligned,
                "identicalSourceFiles":same,
                "nextDomain":generation["systemDesign"]["nextDomain"],
                "hypothesis":generation["systemDesign"]["hypothesis"].as_str()
                    .unwrap_or("").chars().take(400).collect::<String>(),
                "designSummary":generation["systemDesign"]["design"].as_str()
                    .unwrap_or("").chars().take(600).collect::<String>(),
                "automaticPatchReuse":false,
                "qualityProvenForThisDescendant":false
            })));
        }
    }
    ranked.sort_by(|a,b|b.0.cmp(&a.0).then_with(||
        a.1["candidateId"].as_str().cmp(&b.1["candidateId"].as_str())));
    json!({
        "source":"paired_cross_campaign_reference_archive_v1",
        "comparisonProfileSha256":profile,
        "references":ranked.into_iter().take(4).map(|(_,row)|row).collect::<Vec<_>>(),
        "automaticPatchReuse":false
    })
}

/// Verify the peer's raw paired receipt and unchanged source identity;
/// reference text can advise an experiment, never waive independent testing.
pub(crate) fn validate_peer_lineages(
    state:&crate::state::AppState,peers:&Value,
    profile:Option<&str>,campaign:&str,
)->Result<(),String>{
    if peers["source"]!="paired_cross_campaign_reference_archive_v1"
        || peers["comparisonProfileSha256"].as_str()!=profile
        || peers["automaticPatchReuse"]!=false {
        return Err("교차 계보 비교의 평가 조건 또는 출처가 다릅니다.".into());
    }
    let rows=peers["references"].as_array().filter(|rows|rows.len()<=4)
        .ok_or("교차 계보 참고 수가 범위를 벗어났습니다.")?;
    let mut seen=BTreeSet::new();
    for row in rows {
        let id=row["candidateId"].as_str()
            .ok_or("교차 계보 후보 ID가 없습니다.")?;
        if !seen.insert(id)
            || row["source"]!="paired_cross_campaign_design_reference_v1"
            || row["comparisonProfileSha256"].as_str()!=profile
            || row["automaticPatchReuse"]!=false
            || row["campaignId"].as_str()==Some(campaign) {
            return Err("중복 또는 자기 계보 참고가 포함됐습니다.".into());
        }
        let generation_id=row["generationId"].as_str()
            .ok_or("교차 계보 기록 ID가 없습니다.")?;
        let generation=state.store.get("rsi_generation",generation_id)
            .ok_or("교차 계보 원본 세대가 없습니다.")?;
        if generation["campaignId"]!=row["campaignId"]
            || generation["candidateId"]!=id
            || generation["comparisonProfileSha256"].as_str()!=profile
            || generation["acceptedCommit"]!=row["acceptedCommit"]
            || !independent_adoption(&generation) {
            return Err("교차 계보 세대의 원본 검증 기록이 다릅니다.".into());
        }
        let candidate=state.store.get("rsi_candidate",id)
            .ok_or("교차 계보 원본 후보가 없습니다.")?;
        if candidate["status"]!="verified"
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || candidate["patchSha256"]!=row["patchSha256"]
            || candidate["baseCommit"]!=row["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=row["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate){
            return Err("교차 계보 후보의 독립 평가 영수증이 다릅니다.".into());
        }
    }
    Ok(())
}


/// Hybrid innovation is an executable code-search objective, not the reuse of
/// an ancestor's patch. The next compiled task/meta agent gets a bounded
/// structured causal contract: what failed, what was predicted, and what
/// architectural part of the procedure it should experiment with.
pub(crate) fn next_method_hypothesis(policy:&Value,history:&[Value])->Value{
    let selected=&policy["selectionEvidence"];
    let target=&policy["target"];
    let role=target["role"].as_str().unwrap_or("improvement_methods");
    let mut observations=history.iter().rev().filter(|g|
        g["metaPolicy"]["target"]["role"]==role
            && g["metaLearning"]["independentlyMeasured"]==true)
        .take(12);
    let last=observations.next();
    let surprise=last.and_then(|g|g["metaLearning"]["brierError"].as_f64())
        .unwrap_or(0.25);
    let predicted=selected["predictedAdoptionProbability"].as_f64().unwrap_or(0.5);
    let proposed=if surprise>0.35 {
        "revise_uncertainty_and_counterfactual_evaluation"
    }else if selected["editsWithoutCandidate"].as_u64().unwrap_or(0)>1 {
        "invent_alternative_source_edit_method"
    }else if selected["productiveDescendantAdoptions"].as_u64().unwrap_or(0)==0
        && selected["verifiedModelAdoptions"].as_u64().unwrap_or(0)>0 {
        "improve_future_generator_productivity"
    }else{
        "combine_independently_adopted_meta_and_task_architecture"
    };
    json!({
        "source":"compiled_meta_method_proposal_v1",
        "targetRole":role,
        "metaRevision":policy["revision"],
        "methodFamily":proposed,
        "estimatedAdoptionProbability":predicted,
        "observedSurprise":surprise,
        "requiredImplementation":["task_agent_source","self_improvement_source"],
        "qualityProven":false,
        "agentOwnsImplementation":true
    })
}

/// The model—not this controller—authors the actual design, programs and
/// reusable procedures. Comparative observations are bounded and source
/// grounded; they carry no adoption rights.
pub(crate) fn instruction(policy:&Value)->String {
    let target=&policy["target"];
    let refs=policy["referenceDesigns"].as_array().into_iter().flatten()
        .take(2).map(|g|format!(
            "verified roles={} causal hypothesis={} design={} next generation gains={}",
            g["roles"],g["hypothesis"],g["design"],g["productiveDescendants"]
        )).collect::<Vec<_>>().join("\n");
    let failures=policy["counterexamples"].as_array().into_iter().flatten()
        .take(3).map(|g|format!(
            "failed roles={} cause={} source={} observed error={}",
            g["roles"],g["failureKind"],g["sourceTarget"],g["error"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[EDITABLE META-IMPROVER revision={}]\n\
        Selection mode={}; chosen intelligence subsystem={} domain={} source={} hash={}. \
        Estimated adoption chance={} uncertainty={}; this is an unproven forecast.\n\
        Compare MULTIPLE independent earlier experiments, not one trajectory. \
        Valid successes and their descendant productivity:\n{}\n\
        Counterexamples to avoid repeating:\n{}\n\
        Author the novel executable redesign yourself, including new modules or \
        multi-component changes. You may also edit src/engine/rsi_meta_controller.rs, \
        src/engine/rsi_system.rs and the task/memory/learning/tool harness, so \
        the next compiled child changes HOW it designs improvements. \
        Explain a counterfactual or failure mode for your proposal and predict \
        observable changes before editing. After an accepted change the NEXT \
        binary must generate the new improvement plan. Frozen verification is \
        external and cannot be changed.",
        policy["revision"],policy["mode"],target["role"],target["domain"],
        target["path"],target["sourceSha256"],
        policy["forecast"]["acceptanceProbability"],policy["forecast"]["uncertainty"],
        refs,failures
    )
}

/// New prediction errors are durable observations available to later compiled
/// meta-controllers. No model statement can turn a prediction into evidence.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    if policy["source"]!="compiled_editable_meta_controller_v1" {
        return Value::Null;
    }
    let probability=policy["forecast"]["acceptanceProbability"]
        .as_f64().filter(|p|p.is_finite()&&*p>=0.0&&*p<=1.0);
    let assessed=generation["evaluation"]["verdict"].as_str().is_some()
        && generation["evaluation"]["metrics"]["eligible"].is_boolean();
    let observed=if assessed{Some(independent_adoption(generation))}else{None};
    let brier=match(probability,observed){
        (Some(p),Some(success))=>Some((p-if success{1.0}else{0.0}).powi(2)),
        _=>None
    };
    json!({
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "predictedProbability":probability,
        "independentlyMeasured":observed.is_some(),
        "actualModelAuthoredAdoption":observed,
        "brierError":brier,
        "failureKind":if independent_adoption(generation){"adopted"}
            else{failure_kind(generation)},
        "attemptedSourceChange":generation["candidateId"].is_string(),
        "nextPolicyCanTrainFromObservation":observed.is_some(),
        "qualityProven":observed==Some(true)
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn comparison_prioritizes_novelty_then_updates_after_grounded_failures(){
        let target=|role:&str,path:&str|json!({"role":role,"domain":if role=="memory"{"memory"}else{"learning"},
            "path":path,"sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "verifiedRoleChanged":false});
        let frontier=vec![target("memory","src/engine/memory.rs"),
            target("learning","src/engine/learning.rs")];
        let first=synthesize(&[],&frontier,&["memory","learning"],None);
        assert!(first["target"].is_object());
        assert_eq!(first["historyCount"],0);
        let mut failures=Vec::new();
        for n in 0..6{
            failures.push(json!({"jobId":format!("job-{n}"),"generatorCommit":"base",
                "accepted":false,"candidateId":null,
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":"base","generationJobId":format!("job-{n}"),
                    "component":"memory","targetFiles":["src/engine/memory.rs"]}}));
        }
        let later=synthesize(&failures,&frontier,&["memory","learning"],None);
        assert_eq!(later["target"]["role"],"learning");
        assert!(later["counterexamples"].as_array().unwrap().len()>0);
        assert_eq!(feedback(&later,&failures[0])["independentlyMeasured"],false);
    }
}
