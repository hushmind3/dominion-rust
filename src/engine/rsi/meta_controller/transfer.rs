//! transfer stage of the compiled, revisable meta-improvement controller.
//! These observations propose experiments; only frozen independent evaluation may adopt them.
use super::*;

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
                || crate::engine::rsi::verify_paired_receipt(state,&candidate).is_err() {continue;}
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
            || crate::engine::rsi::verify_paired_receipt(state,&candidate).is_err(){
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
    let failures=&selected["failurePosterior"];
    let compiler=failures["compiler_failure"].as_f64().unwrap_or(0.2);
    let missing_edit=failures["no_verified_code_candidate"].as_f64().unwrap_or(0.2);
    let regression=failures["behavior_regression"].as_f64().unwrap_or(0.2);
    let plateau=failures["no_independent_gain"].as_f64().unwrap_or(0.2);
    let proposed=if compiler>=0.32 {
        "compiler_guided_meta_repair_and_source_contract_evolution"
    }else if missing_edit>=0.32 {
        "invent_a_new_executable_code_editing_method"
    }else if regression>=0.32 {
        "counterexample_guided_causal_meta_rewrite"
    }else if plateau>=0.36 {
        "diverse_cross_task_architecture_hybridization"
    }else if surprise>0.35 {
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
        "failurePosterior":failures,
        "requiredImplementation":["task_agent_source","self_improvement_source"],
        "qualityProven":false,
        "agentOwnsImplementation":true
    })
}


/// Mendelian recombination at the *hypothesis level*: no foreign code or
/// patches are copied. A current compiled successor fuses distinct verified
/// causal mechanisms into one falsifiable cross-component design contract.
/// Parent and worker can recompute the same contract from the checked peers.
pub(crate) fn compositional_hypothesis(peers:&Value,policy:&Value)->Value{
    let rows=peers["references"].as_array().cloned().unwrap_or_default();
    let primary=policy["target"]["role"].as_str().unwrap_or("");
    let mut ranked=rows.into_iter().filter(|row|
        row["source"]=="paired_cross_campaign_design_reference_v1"
            && row["automaticPatchReuse"]==false
            && row["identicalSourceFiles"].as_u64().unwrap_or(0)>0)
        .collect::<Vec<_>>();
    ranked.sort_by(|a,b|{
        let matches=|row:&Value|row["roles"].as_array().is_some_and(|roles|
            roles.iter().any(|v|v.as_str()==Some(primary)));
        matches(b).cmp(&matches(a))
            .then(b["identicalSourceFiles"].as_u64()
                .cmp(&a["identicalSourceFiles"].as_u64()))
            .then(a["candidateId"].as_str().cmp(&b["candidateId"].as_str()))
    });
    let Some(anchor)=ranked.first() else{return Value::Null};
    let anchor_roles=anchor["roles"].as_array().cloned().unwrap_or_default();
    let complementary=ranked.iter().skip(1).find(|other|
        other["patchSha256"]!=anchor["patchSha256"]
            && other["roles"].as_array().is_some_and(|other_roles|
                other_roles.iter().any(|role|!anchor_roles.contains(role))));
    let Some(second)=complementary else{return Value::Null};
    let mut roles=BTreeSet::new();
    for row in [anchor,second]{
        for role in row["roles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str){
            roles.insert(role.to_string());
        }
    }
    let mut target_sources=BTreeSet::new();
    for row in [anchor,second] {
        for path in row["alignedSourceFiles"].as_array().into_iter()
            .flatten().filter_map(Value::as_str) {
            target_sources.insert(path.to_string());
        }
    }
    let design=json!({
        "source":"compiled_comparative_meta_design_hypothesis_v1",
        "parentMetaRevision":policy["revision"],
        "peerCandidateIds":[anchor["candidateId"],second["candidateId"]],
        "peerPatchHashes":[anchor["patchSha256"],second["patchSha256"]],
        "roles":roles.into_iter().collect::<Vec<_>>(),
        "sourceAnchors":target_sources.into_iter().take(8).collect::<Vec<_>>(),
        "competingHypotheses":[anchor["hypothesis"],second["hypothesis"]],
        "inheritedMethods":[anchor["designSummary"],second["designSummary"]],
        "causalQuestion":"Can a newly authored task+meta architecture combine the two independently useful mechanisms without their original failure modes?",
        "requiredNewWork":["inspect_exact_current_source","design_new_coupled_mechanism",
            "edit_actual_agent_and_meta_controller","evaluate_independently"],
        "automaticPatchReuse":false,"qualityProvenForThisDescendant":false
    });
    design
}

