//! Source-bound cross-lineage experimental method transfer.
//! Observational controls are evidence for exploration, never adoption authority.
use super::{independent_adoption,model_design_roles,TRANSFER_METHOD_SOURCE};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

// Attribution is to one executed donor method on one exact source version
// and frozen comparison profile. A donor mentioned among several references
// is not evidence that its method caused a recipient's outcome.
fn measured_recipient(g:&Value)->bool{
    g["evaluation"]["verdict"].is_string()
        && g["evaluation"]["metrics"]["eligible"].is_boolean()
}
pub(super) fn executed_recipient_context(
    g:&Value,role:&str,path:&str,source_sha:&str,profile:&Value
)->bool{
    g["metaPolicyExecuted"]==true
        && &g["comparisonProfileSha256"]==profile
        && g["metaPolicy"]["target"]["role"]==role
        && g["metaPolicy"]["target"]["path"]==path
        && g["metaPolicy"]["target"]["sourceSha256"]==source_sha
        && g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(path)))
}
pub(super) fn recipient_source_adopted(g:&Value,path:&str,previous_sha:&str)->bool{
    independent_adoption(g)
        && g["systemDesign"]["targetFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|sha|
            sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())
                && sha!=previous_sha)
}

/// Turn independently paired *other-lineage* results into a bounded
/// experimental hypothesis rather than copying a foreign patch. A donor's
/// nextDomain is a proposed transferable METHOD, not an observed improvement
/// of the new role. The compiled descendant itself chooses a source-bound
/// recipient and must still author, build and independently test its changes.
///
/// Source compatibility (at least one identical current indexed source file)
/// and the same frozen comparison profile are required. A donor cannot
/// promote its own role as a repeated success by reporting a nextDomain.
pub(crate) fn evolve_from_peer_methods(mut policy:Value,peers:&Value,history:&[Value])->Value{
    if policy["source"]!="compiled_editable_meta_controller_v1"
        || policy["metaRepairChosen"]==true
        || peers["source"]!="paired_cross_campaign_reference_archive_v1"
        || peers["automaticPatchReuse"]!=false
        || peers["comparisonProfileSha256"].as_str()
            .is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){
        return policy;
    }
    let Some(donors)=peers["references"].as_array()else{return policy;};
    let Some(mut alternatives)=policy["alternatives"].as_array().cloned()
        else{return policy;};
    let original_revision=policy["revision"].clone();
    let mut transferred=false;
    for candidate in &mut alternatives{
        let (Some(role),Some(domain),Some(path),Some(hash))=(
            candidate["target"]["role"].as_str(),
            candidate["target"]["domain"].as_str(),
            candidate["target"]["path"].as_str(),
            candidate["target"]["sourceSha256"].as_str()
        )else{continue};
        if !crate::engine::rsi::safe_path(path) || hash.len()!=64
            || !hash.bytes().all(|b|b.is_ascii_hexdigit()) {continue;}
        let mut seen=BTreeSet::new();
        let mut bridges=Vec::<Value>::new();
        let mut learned_factors=Vec::<(String,f64)>::new();
        // Observational matched control: same role, source revision, frozen
        // suite and executed focus, but no peer-method intervention.
        let mut control_seen=BTreeSet::<String>::new();
        let mut control_trials=0_u64;
        let mut control_adoptions=0_u64;
        for g in history.iter().rev().take(256){
            if !executed_recipient_context(g,role,path,hash,
                &peers["comparisonProfileSha256"])
                || g["metaPolicy"]["peerMethodTransfer"]["active"]==true
                || !measured_recipient(g){continue;}
            let Some(job)=g["jobId"].as_str()else{continue};
            if !control_seen.insert(job.to_owned()){continue;}
            control_trials+=1;
            control_adoptions+=u64::from(recipient_source_adopted(g,path,hash));
        }
        let control_p=(control_adoptions as f64+1.0)/(control_trials as f64+2.0);
        for donor in donors.iter().take(4){
            if donor["source"]!="paired_cross_campaign_design_reference_v1"
                || donor["automaticPatchReuse"]!=false
                || donor["comparisonProfileSha256"]!=peers["comparisonProfileSha256"]
                || donor["nextDomain"]!=domain
                || donor["identicalSourceFiles"].as_u64().unwrap_or(0)==0
                || donor["alignedSourceFiles"].as_array()
                    .is_none_or(|v|v.is_empty())
                || donor["roles"].as_array().is_none_or(|roles|
                    roles.is_empty() || roles.iter().any(|r|r.as_str()==Some(role))){
                continue;
            }
            let Some(patch)=donor["patchSha256"].as_str()
                .filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))
                else{continue};
            if !seen.insert(patch.to_string()){continue;}
            let Some(donor_id)=donor["candidateId"].as_str()
                .filter(|id|!id.is_empty()) else{continue};
            // Learn the portability of this donor's METHOD from subsequent
            // executable recipient attempts, not from the donor's own win.
            // Match role, real focused source path, frozen suite, and donor.
            // Count unique generations. No held-out case data enter this law.
            let mut observed=BTreeSet::<String>::new();
            let mut attempts=0_u64;
            let mut measured=0_u64;
            let mut adopted=0_u64;
            for g in history.iter().rev().take(256){
                let observed_transfer=&g["metaPolicy"]["peerMethodTransfer"];
                if !executed_recipient_context(g,role,path,hash,
                    &peers["comparisonProfileSha256"])
                    || observed_transfer["active"]!=true
                    || observed_transfer["activeDonorCandidateId"]!=donor_id
                    || observed_transfer["comparisonProfileSha256"]
                        !=peers["comparisonProfileSha256"]
                    || observed_transfer["selectedRecipient"]["role"]!=role
                    || observed_transfer["selectedRecipient"]["path"]!=path
                    || observed_transfer["selectedRecipient"]["sourceSha256"]!=hash
                    || !observed_transfer["references"]
                        .as_array().is_some_and(|refs|refs.iter().any(|ref_row|
                            ref_row["candidateId"]==donor_id
                            && ref_row["sharedFrozenProfileSha256"]
                                ==peers["comparisonProfileSha256"])) {
                    continue;
                }
                let Some(id)=g["jobId"].as_str()else{continue};
                if !observed.insert(id.to_string()){continue;}
                attempts+=1;
                if measured_recipient(g){
                    measured+=1;
                    adopted+=u64::from(recipient_source_adopted(g,path,hash));
                }
            }
            let posterior=(adopted as f64+1.0)/(measured as f64+2.0);
            let unresolved=attempts.saturating_sub(measured);
            // A negative matched-control signal suppresses this method's
            // exploration bonus; it does not falsely establish causality.
            let negative_transfer=measured>=3 && control_trials>=3
                && posterior+0.12<control_p;
            let factor=if negative_transfer{0.0}else{
                (2.0*posterior).clamp(0.25,1.5)
                    *(1.0-0.10*unresolved.min(4) as f64)
                    *(1.0+(posterior-control_p).max(0.0)).min(1.5)
            };
            learned_factors.push((donor_id.to_owned(),factor));
            bridges.push(json!({
                "candidateId":donor["candidateId"],
                "fromCampaign":donor["campaignId"],
                "fromRoles":donor["roles"],
                "donorAcceptedCommit":donor["acceptedCommit"],
                "toRole":role,"toDomain":domain,
                "toSourcePath":path,"toSourceSha256":hash,
                "sharedFrozenProfileSha256":peers["comparisonProfileSha256"],
                "identicalIndexedSourceFiles":donor["identicalSourceFiles"],
                "observedTransferAttempts":attempts,
                "independentlyMeasuredTransferTrials":measured,
                "recipientSourceAdoptions":adopted,
                "empiricalTransferPosterior":posterior,
                "matchedControlTrials":control_trials,
                "matchedControlAdoptions":control_adoptions,
                "matchedControlPosterior":control_p,
                "negativeTransferSignal":negative_transfer,
                "singleDonorAttribution":true,
                "automaticPatchReuse":false,
                "transferProven":false
            }));
        }
        // Select exactly one donor intervention. Other references remain
        // hypotheses, never co-credited after the recipient's evaluation.
        let primary=learned_factors.iter().filter(|(_,factor)|*factor>0.0)
            .max_by(|(id_a,a),(id_b,b)|a.total_cmp(b)
                .then_with(||id_b.cmp(id_a)));
        let active_donor=primary.map(|(id,_)|id.as_str());
        let bonus=primary.map(|(_,factor)|0.12*factor).unwrap_or(0.0);
        let prior=candidate["score"].as_f64().filter(|s|s.is_finite())
            .unwrap_or(0.0);
        candidate["score"]=json!(prior+bonus);
        candidate["portableMethodExplorationBonus"]=json!(bonus);
        candidate["activeTransferDonorCandidateId"]=json!(active_donor);
        candidate["transferHypotheses"]=json!(bridges);
        transferred|=bonus>0.0;
    }
    // Keep negative and neutral trial evidence even when no peer method is
    // activated. The next compiled descendant can change its search rule
    // without mislabeling a rejected transfer as a successful design.
    let learning=alternatives.iter()
        .flat_map(|candidate|candidate["transferHypotheses"].as_array()
            .into_iter().flatten())
        .map(|bridge|json!({
            "donorCandidateId":bridge["candidateId"],
            "recipientRole":bridge["toRole"],
            "recipientSourceSha256":bridge["toSourceSha256"],
            "observedTransferTrials":bridge["independentlyMeasuredTransferTrials"],
            "adoptedTransferTrials":bridge["recipientSourceAdoptions"],
            "matchedControlTrials":bridge["matchedControlTrials"],
            "matchedControlAdoptions":bridge["matchedControlAdoptions"],
            "negativeTransferSignal":bridge["negativeTransferSignal"]
        })).collect::<Vec<_>>();
    policy["peerMethodLearning"]=json!({
        "source":"matched_executed_peer_method_observation_v1",
        "observations":learning.clone(),
        "causalSuperiorityProven":false,
        "isolationRequired":true
    });
    // Repeated independently assessed negative transfers can become a
    // source-bound experiment on HOW the descendant selects future methods.
    // A recent self-edit cooldown prevents repeated meta-only generations.
    let negative_method=learning.iter().filter(|row|
        row["negativeTransferSignal"]==true
            && row["observedTransferTrials"].as_u64().unwrap_or(0)>=3
            && row["matchedControlTrials"].as_u64().unwrap_or(0)>=3).count();
    let recent_meta_attempts=history.iter().rev().take(4).filter(|g|
        g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|path|
                path==TRANSFER_METHOD_SOURCE))).count();
    if negative_method>0 && recent_meta_attempts<2 {
        if let Some(repair)=alternatives.iter().find(|row|
            row["target"]["path"]==TRANSFER_METHOD_SOURCE).cloned() {
            let target=repair["target"].clone();
            policy["target"]=target.clone();
            policy["selectionEvidence"]=repair.clone();
            policy["alternatives"]=json!(alternatives);
            policy["taskOutcomeForecast"]=repair["outcomeForecast"].clone();
            policy["forecast"]["acceptanceProbability"]=
                repair["predictedAdoptionProbability"].clone();
            policy["forecast"]["uncertainty"]=repair["uncertainty"].clone();
            for field in ["role","domain","path","sourceSha256"] {
                policy["controlPolicy"][field]=target[field].clone();
            }
            let next_edge=policy["mutualDevelopmentGraph"]["edges"]
                .as_array().into_iter().flatten()
                .find(|edge|edge["to"]==target["role"])
                .cloned().unwrap_or(Value::Null);
            policy["nextMutualRedesign"]=next_edge;
            policy["mode"]=json!("repair_own_transfer_method");
            policy["metaRepairChosen"]=json!(true);
            policy["metaRepairReason"]=json!("observed_negative_transfer_against_matched_control");
            policy["peerMethodTransfer"]=Value::Null;
            policy["revision"]=json!(format!("{:x}",Sha256::digest(
                serde_json::to_vec(&json!([
                    original_revision,policy["target"],policy["controlPolicy"],
                    policy["peerMethodLearning"],policy["mode"]
                ])).unwrap_or_default())));
            return policy;
        }
    }
    if !transferred{
        if !learning.is_empty(){
            policy["revision"]=json!(format!("{:x}",Sha256::digest(
                serde_json::to_vec(&json!([
                    original_revision,policy["peerMethodLearning"]
                ])).unwrap_or_default())));
        }
        return policy;
    }
    alternatives.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
        .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
        .then_with(||a["role"].as_str().cmp(&b["role"].as_str()))
        .then_with(||a["target"]["path"].as_str().cmp(&b["target"]["path"].as_str())));
    let Some(winner)=alternatives.first().cloned()else{return policy;};
    let target=winner["target"].clone();
    let bridge=winner["activeTransferDonorCandidateId"].is_string();
    policy["target"]=target.clone();
    policy["selectionEvidence"]=winner.clone();
    policy["alternatives"]=json!(alternatives);
    policy["taskOutcomeForecast"]=winner["outcomeForecast"].clone();
    policy["forecast"]["acceptanceProbability"]=
        winner["predictedAdoptionProbability"].clone();
    policy["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    // The selected target must also become the native action, not just a
    // ranking comment. The coordinator still owns independent evaluation.
    for (key,field) in [("role","role"),("domain","domain"),
        ("path","path"),("sourceSha256","sourceSha256")]{
        policy["controlPolicy"][key]=target[field].clone();
    }
    let mutual=policy["mutualDevelopmentGraph"]["edges"].as_array()
        .into_iter().flatten()
        .find(|edge|edge["to"]==target["role"])
        .cloned().unwrap_or(Value::Null);
    policy["nextMutualRedesign"]=mutual;
    policy["peerMethodTransfer"]=json!({
        "source":"compiled_cross_lineage_method_hypothesis_v1",
        "selectedRecipient":target,
        "references":winner["transferHypotheses"],
        "comparisonProfileSha256":peers["comparisonProfileSha256"],
        "activeDonorCandidateId":winner["activeTransferDonorCandidateId"],
        "attributionMode":"one_executed_donor_source_revision_and_suite",
        "active":bridge,
        "sameFrozenComparisonProfile":true,
        "automaticallyApplied":false,
        "transferProven":false
    });
    if bridge{
        policy["mode"]=json!("test_cross_role_portable_method");
    }
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([
            original_revision,policy["target"],policy["alternatives"],
            policy["controlPolicy"],policy["peerMethodTransfer"],
            policy["peerMethodLearning"]
        ])).unwrap_or_default())));
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

