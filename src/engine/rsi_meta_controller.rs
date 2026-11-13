//! Editable metacognitive controller compiled into each accepted descendant.
//! Inspired by HyperAgents, predictive improvement dynamics, and comparative
//! cross-trajectory evolution. It proposes search experiments; frozen independent
//! evaluation is the sole authority for adoption. Every forecast is calibrated
//! against the *next* observed, separately verified outcome.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

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
        || g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
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
            && child["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
            && independent_adoption(child)
    ).count()
}
fn confidence_adjustment(history:&[Value])->f64{
    // Both source ranking AND forecast repair must use the same replayable
    // observations. A fabricated summary cannot dampen exploration.
    let observed=crate::engine::rsi_prequential::drift(history,12);
    let Some(mean)=observed["meanBrierError"].as_f64() else{return 1.0};
    (1.0-0.6*mean).clamp(0.4,1.0)
}

/// Record cross-subsystem *successor* opportunities from the actual compiled
/// genealogy. An accepted redesign of A is followed by B only if the next
/// executable is built from A's accepted commit. The downstream improvement
/// must itself be independently accepted to count as a successful handoff.
/// Temporal association is not a causal A/B claim: this graph is an editable
/// exploration policy, while frozen evaluation alone grants quality credit.
fn observed_interaction_graph(history:&[Value])->Value{
    let mut links=BTreeMap::<(String,String),(u64,u64,BTreeSet<String>)>::new();
    for parent in history.iter().filter(|row|independent_adoption(row)){
        let Some(parent_commit)=parent["acceptedCommit"].as_str() else{continue};
        let parent_roles=model_design_roles(parent);
        for child in history.iter().filter(|row|
            row["generatorCommit"].as_str()==Some(parent_commit)
            && row["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
            && row["jobId"].as_str().is_some_and(|id|!id.is_empty())) {
            let mut next_roles=model_design_roles(child);
            if next_roles.is_empty(){
                if let Some(role)=child["metaPolicy"]["target"]["role"].as_str()
                    .filter(|r|!r.is_empty() && r.len()<=64
                        && r.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-')) {
                    next_roles.push(role.to_owned());
                }
            }
            let successful=independent_adoption(child);
            for from in &parent_roles{
                for to in &next_roles{
                    if from==to {continue;}
                    let entry=links.entry((from.clone(),to.clone())).or_default();
                    entry.0+=1;
                    if successful {entry.1+=1;}
                    if let Some(id)=child["jobId"].as_str(){
                        entry.2.insert(id.to_owned());
                    }
                }
            }
        }
    }
    let edges=links.into_iter().map(|((from,to),(trials,accepted,jobs))|json!({
        "fromRole":from,"toRole":to,
        "compiledSuccessorAttempts":trials,
        "independentlyAdoptedSuccessors":accepted,
        "successorJobIds":jobs.into_iter().take(6).collect::<Vec<_>>(),
        "observationKind":"linked_rebuilt_binary_successor",
        "crossRoleCausalityProven":false
    })).collect::<Vec<_>>();
    json!({
        "source":"editable_cross_subsystem_successor_graph_v1",
        "historyCount":history.len(),
        "edges":edges,
        "scoreMeaning":"observational_transfer_priority_not_verified_causal_effect",
        "qualityPromotionsRemainExternal":true
    })
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
    let graph=observed_interaction_graph(history);
    // A recent genuine redesign is a source of new transfer hypotheses.
    // Prefer trying other subsystems without pre-labeling that transfer useful.
    let recent_sources=history.iter().rev().filter(|g|independent_adoption(g))
        .take(3).flat_map(model_design_roles)
        .collect::<BTreeSet<String>>();
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
        let raw_probability=(adopted as f64+1.0)/(attempts as f64+2.0);
        let prequential=crate::engine::rsi_prequential::calibrate(
            history,role,target["path"].as_str().unwrap_or(""),
            target["sourceSha256"].as_str().unwrap_or(""),raw_probability);
        let probability=prequential["probability"].as_f64().unwrap_or(raw_probability);
        let epistemic=prequential["uncertainty"].as_f64()
            .unwrap_or((probability*(1.0-probability)/(attempts as f64+3.0)).sqrt());
        let coverage=if target["verifiedRoleChanged"]==true{0.0}else{0.22};
        let inherited_bonus=if inherited_domain==target["domain"].as_str(){0.12}else{0.0};
        let size=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(size/12_000.0).min(1.0);
        let penalty=regressions as f64/(attempts.max(1) as f64);
        let stall=fruitless as f64/(attempts.max(1) as f64);
        let clade=(productivity as f64/(adopted.max(1) as f64)).min(3.0);
        let cross_role_exploration=if !recent_sources.is_empty()
            && !recent_sources.contains(role){0.08}else{0.0};
        let transfer_prior=graph["edges"].as_array().into_iter().flatten()
            .filter(|edge|edge["toRole"]==role
                && edge["fromRole"].as_str().is_some_and(|from|
                    recent_sources.contains(from)))
            .map(|edge|{
                let trials=edge["compiledSuccessorAttempts"].as_u64().unwrap_or(0) as f64;
                let adopted=edge["independentlyAdoptedSuccessors"].as_u64().unwrap_or(0) as f64;
                0.20*((adopted+0.5)/(trials+1.0))*((trials/(trials+2.0)).min(1.0))
            }).fold(0.0_f64,f64::max);
        // UCB-style uncertainty, Bayesian shrinkage, and descendant
        // productivity provide a bounded, auditable choice. A failed edit is
        // evidence about execution feasibility, not proof the design is bad.
        let score=0.55*probability+0.48*epistemic*calibration
            +0.14*((trials+2.0).ln()/(attempts as f64+1.0)).sqrt()
            +coverage+inherited_bonus+0.14*clade
            +cross_role_exploration+transfer_prior
            -0.12*penalty-0.07*stall-0.035*cost;
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "sourceSha256":target["sourceSha256"],
            "score":score,"predictedAdoptionProbability":probability,
            "uncalibratedProbability":raw_probability,
            "prequentialEvidence":prequential,
            "uncertainty":epistemic*calibration,"attempts":attempts,
            "verifiedModelAdoptions":adopted,"regressions":regressions,
            "editsWithoutCandidate":fruitless,
            "productiveDescendantAdoptions":productivity,
            "crossRoleExplorationBonus":cross_role_exploration,
            "recordedTransferPriority":transfer_prior,
            "newComponent":target["verifiedRoleChanged"]!=true
        }),score));
    }
    // Repeated source-bound Brier errors make the prediction algorithm
    // itself a real executable redesign target, with cooldown.
    // Forecast drift is inferred from the independently replayed policy,
    // candidate and source path. The mutable historical Brier field alone
    // must never direct the descendant's next source edit.
    let drift=crate::engine::rsi_prequential::drift(history,6);
    let errors=drift["recentMeasuredPredictions"].as_u64().unwrap_or(0);
    let mean_error=drift["meanBrierError"].as_f64();
    let cooldown=history.iter().rev().take(4).any(|g|
        crate::engine::rsi_prequential::measured_target_attempt(
            g,"src/engine/rsi_prequential.rs"));
    let predictor_available=ranked.iter().any(|(row,_)|
        row["target"]["path"]=="src/engine/rsi_prequential.rs"
        && row["domain"]=="improver");
    let repair=errors>=4 && mean_error.is_some_and(|v|v>0.35)
        && !cooldown && predictor_available;
    if repair {
        for (row,score) in &mut ranked {
            if row["target"]["path"]=="src/engine/rsi_prequential.rs" {
                *score+=2.0;
                row["score"]=json!(*score);
                row["predictionRuleRepairBonus"]=json!(2.0);
            }
        }
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
    let mode=if repair && target["path"]=="src/engine/rsi_prequential.rs" {
        "repair_own_source_bound_prediction_rule"
    }else if counterexamples.iter().any(|row|
        row["failureKind"]=="compiler_failure"){"causal_repair_and_new_design"}
        else if !reference.is_empty(){"comparative_hybridization"}
        else{"open_ended_mechanism_discovery"};
    let alternatives=ranked.iter().take(32)
        .map(|(row,_)|row.clone()).collect::<Vec<_>>();
    let prediction_repair=json!({
        "source":"replayed_prequential_error_feedback_v1",
        "recentMeasuredPredictions":errors,"meanBrierError":mean_error,
        "replayedDriftEvidence":drift,
        "cooldown":cooldown,"repairRuleSourceSelected":repair,
        "targetSource":"src/engine/rsi_prequential.rs",
        "requiresActualDescendantSourceEdit":repair,"improvementProven":false
    });
    let serialized=json!([target,alternatives,reference,counterexamples,mode,
        prediction_repair,inherited_domain,history.len(),graph]);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&serialized).unwrap_or_default()));
    json!({
        "source":"compiled_editable_meta_controller_v1",
        "revision":revision,"historyCount":history.len(),
        "mode":mode,"target":target,"selectionEvidence":winner,
        "alternatives":alternatives,
        "crossComponentGraph":graph,
        "predictionRuleRepair":prediction_repair,
        "recentRedesignedRoles":recent_sources.into_iter().collect::<Vec<_>>(),
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

/// A successful donor from another verified campaign can influence the
/// offspring's NEXT exploration target. It never supplies an adoption label
/// or a copied executable patch.
pub(crate) fn integrate_peer_evidence(mut policy:Value,peers:&Value)->Value{
    let Some(donors)=peers["references"].as_array() else{return policy};
    let Some(mut alternatives)=policy["alternatives"].as_array().cloned() else{return policy};
    for candidate in &mut alternatives{
        let Some(role)=candidate["role"].as_str() else{continue};
        let mut bonus=0.0_f64;
        let mut references=Vec::new();
        for donor in donors {
            if donor["source"]!="paired_cross_campaign_design_reference_v1"
                || donor["automaticPatchReuse"]!=false
                || donor["roles"].as_array().is_none_or(|rows|
                    !rows.iter().any(|value|value.as_str()==Some(role))) {continue;}
            let identical=donor["identicalSourceFiles"].as_u64().unwrap_or(0).min(4);
            let candidate_bonus=0.10+0.015*identical as f64;
            bonus=bonus.max(candidate_bonus);
            if let Some(id)=donor["candidateId"].as_str(){
                references.push(id.to_owned());
            }
        }
        let prior=candidate["score"].as_f64().unwrap_or(0.0);
        candidate["score"]=json!(prior+bonus);
        candidate["crossLineageExplorationBonus"]=json!(bonus);
        candidate["peerDesignReferences"]=json!(references.into_iter().take(4).collect::<Vec<_>>());
    }
    alternatives.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
        .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
        .then_with(||a["role"].as_str().cmp(&b["role"].as_str())));
    let Some(winner)=alternatives.first().cloned() else{return policy};
    let prior=policy["revision"].clone();
    policy["target"]=winner["target"].clone();
    policy["selectionEvidence"]=winner.clone();
    policy["alternatives"]=json!(alternatives);
    policy["forecast"]["acceptanceProbability"]=winner["predictedAdoptionProbability"].clone();
    policy["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    policy["peerEvidenceSha256"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(peers).unwrap_or_default())));
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([prior,policy["alternatives"],
            policy["peerEvidenceSha256"]])).unwrap_or_default())));
    policy
}

/// A next-generation algorithm written by the model may override WHICH
/// source-grounded task the compiled child tries. The frozen supervisor
/// checks re-bound path/hash and cannot choose the new ranking policy.
pub(crate) fn apply_successor_stage(mut policy:Value,stage:&Value)->Value{
    // Repeated independently measured forecast failure is a concrete
    // self-repair obligation, not a model-authored stage that can silently
    // defer its own prediction-rule redesign.
    if policy["predictionRuleRepair"]["repairRuleSourceSelected"]==true {
        return policy;
    }
    if stage["source"]!="rebound_verified_model_successor_program_v1"{return policy;}
    let winner=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]["role"]==stage["role"]
            && row["target"]["domain"]==stage["domain"]
            && row["target"]["path"]==stage["path"]
            && row["target"]["sourceSha256"]==stage["sourceSha256"])
        .cloned();
    let Some(winner)=winner else{return policy};
    let previous=policy["revision"].clone();
    policy["target"]=winner["target"].clone();
    policy["selectionEvidence"]=winner.clone();
    policy["forecast"]["acceptanceProbability"]=winner["predictedAdoptionProbability"].clone();
    policy["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    policy["mode"]=json!("model_written_successor_method_executed_as_target");
    policy["successorStage"]=stage.clone();
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([previous,stage,winner])).unwrap_or_default())));
    policy
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
            if aligned.is_empty(){continue;}
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
    let transfers=policy["crossComponentGraph"]["edges"].as_array().into_iter().flatten()
        .filter(|edge|edge["independentlyAdoptedSuccessors"].as_u64().unwrap_or(0)>0)
        .take(4).map(|edge|format!(
            "{} -> {}: compiledNextAttempts={} independentlyAdoptedNext={} (association only)",
            edge["fromRole"],edge["toRole"],edge["compiledSuccessorAttempts"],
            edge["independentlyAdoptedSuccessors"]
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
        Observed source-to-successor improvement transfers (not causal proof):\n{}\n\
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
        refs,transfers,failures
    )
}

/// New prediction errors are durable observations available to later compiled
/// meta-controllers. No model statement can turn a prediction into evidence.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    crate::engine::rsi_prequential::feedback(policy,generation)
}

#[cfg(test)]
#[path = "rsi_meta_controller/tests.rs"]
mod tests;

