//! Independently adopted design lineage, cross-campaign references, and inherited next steps.
use super::{design_roles,valid_role};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

/// A recorded design is inheritable only when the source patch was accepted
/// and every declared design file has a corresponding adopted source digest.
/// This is inheritance evidence, NOT a substitute for the independent paired
/// source-verification receipt used by the supervisor for promotion.
pub(super) fn adopted_design(generation:&Value)->Option<&Value> {
    let design=&generation["systemDesign"];
    if generation["accepted"]!=true
        || generation["evaluation"]["metrics"]["eligible"]!=true
        || generation["acceptedCommit"].as_str().is_none_or(str::is_empty)
        || generation["candidateId"].as_str().is_none_or(str::is_empty)
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=generation["generatorCommit"]
        || design["generationJobId"]!=generation["jobId"] {
        return None;
    }
    let files=design["targetFiles"].as_array().filter(|files|!files.is_empty())?;
    if !files.iter().all(|item|item.as_str().is_some_and(|path|
        crate::engine::rsi::safe_path(path)
        && generation["acceptedSourceHashes"][path].as_str().is_some_and(|hash|
            hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit())))) {
        return None;
    }
    Some(design)
}

pub(super) fn latest_adopted_design(history:&[Value])->Option<&Value> {
    history.iter().rev().find_map(adopted_design)
}

/// GEPA-Curator-style experience selection and MGM-style comparative ancestry.
/// Keep multiple adopted designs and distinct actionable failures, not the
/// immediately previous run alone. Fixed-size, content-addressed receipts keep
/// this reusable without replaying an entire paid context on every iteration.
pub(crate) fn lineage_capsule(history:&[Value])->Value {
    let mut successes=Vec::new();
    let mut failures=Vec::new();
    let mut unique_failures=BTreeSet::new();
    for generation in history.iter().rev() {
        if let Some(design)=adopted_design(generation) {
            if successes.len()<8 {
                successes.push(json!({
                    "designId":design["id"],"generationId":generation["id"],
                    "generatorCommit":generation["generatorCommit"],
                    "acceptedCommit":generation["acceptedCommit"],
                    // Keep the actual improvement procedures and their use
                    // across generations in the NEXT compiled worker's finite
                    // context. A name alone never proves the method caused gain.
                    "proposedMethods":generation["methodProposals"].as_array()
                        .map(|items|items.iter().filter(|method|
                            method["sourceGenerationJobId"]==generation["jobId"]
                                && method["synthesis"].is_null())
                            .take(3).map(|method|json!({
                                "id":method["id"],"revision":method["revision"],
                                "name":method["name"],"domain":method["domain"],
                                "sourceJobId":method["sourceGenerationJobId"]
                            })).collect::<Vec<_>>()).unwrap_or_default(),
                    "executedMethods":generation["methodExecutions"].as_array()
                        .map(|items|items.iter().take(3).map(|used|json!({
                            "methodId":used["methodId"],"revision":used["revision"],
                            "executionId":used["executionId"],
                            "verified":used["result"]["verified"],
                            "followed":used["result"]["followed"],
                            "qualityClaim":"independent_pair_receipt_required"
                        })).collect::<Vec<_>>()).unwrap_or_default(),
                    "measuredDomainGains":generation["evaluation"]["metrics"]["improvedHoldoutDomains"],
                    "methodAblation":generation["methodAblation"],
                    "previousFeedbackGeneration":generation["improvementDirective"]["sourceGenerationId"],
                    "components":design["components"].as_array()
                        .map(|items|items.iter().take(12).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "componentFiles":design["componentFiles"],
                    "changedFiles":design["targetFiles"].as_array()
                        .map(|items|items.iter().take(12).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "nextDomain":design["nextDomain"],
                    "hypothesis":design["hypothesis"].as_str().unwrap_or("")
                        .chars().take(360).collect::<String>(),
                    "implementation":design["design"].as_str().unwrap_or("")
                        .chars().take(640).collect::<String>(),
                    "metaHoldoutGains":generation["evaluation"]["metrics"]["metaHoldoutGains"],
                    "source":"recorded_adoption_with_source_digests"
                }));
            }
        } else if generation["accepted"]!=true && failures.len()<6 {
            let source=if generation["verification"]["checksPassed"]==false {
                "reproducible_verifier_failure"
            }else if generation["candidateId"].is_string() && generation["evaluation"]["verdict"].is_string() {
                "independently_measured_candidate_failure"
            }else if generation["error"].as_str().is_some_and(|err|!err.is_empty()) {
                "unverified_execution_failure"
            }else{
                "incomplete_exploration"
            };
            let error=generation["error"].as_str().unwrap_or("")
                .chars().take(420).collect::<String>();
            let key=(generation["generatorCommit"].as_str().unwrap_or("").to_owned(),
                generation["candidateId"].as_str().unwrap_or("").to_owned(),
                error.clone());
            if unique_failures.insert(key) {
                failures.push(json!({
                    "generationId":generation["id"],"generatorCommit":generation["generatorCommit"],
                    "candidateId":generation["candidateId"],
                    "focusFiles":generation["focusFiles"].as_array()
                        .map(|items|items.iter().take(4).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "kind":source,"verdict":generation["evaluation"]["verdict"],
                    "error":error,"newFindings":generation["newFindings"],
                    "researchArtifactHashes":generation["knowledgeArtifactHashes"].as_array()
                        .map(|items|items.iter().take(4).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "qualityProven":false
                }));
            }
        }
        if successes.len()>=8 && failures.len()>=6 {break;}
    }
    let accepted_head=successes.first().map(|item|item["acceptedCommit"].clone())
        .unwrap_or(Value::Null);
    let core=json!({
        "source":"descendant_curated_lineage_v1","historyCount":history.len(),
        "latestAcceptedCommit":accepted_head,
        "acceptedDesigns":successes,"failureCounterexamples":failures,
        "adoptionEvidence":"recorded_only_supervisor_replays_independent_pairs"
    });
    let sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&core).unwrap_or_default()));
    let mut capsule=core;
    capsule["sha256"]=json!(sha);
    capsule
}

/// Comparative evolution without unverified cherry-picks: successful
/// architectures from OTHER campaigns are read-only references. Each must
/// have an independently replayable paired receipt under the same immutable
/// evaluation profile and overlap a module in the current source index.
/// The new compiled descendant must still synthesize and verify its own patch.
pub(crate) fn peer_lineage_references(
    state:&crate::state::AppState,campaign:&str,profile_sha:&str,index_id:&str,
    preferred_role:Option<&str>,
)->Value {
    let mut ranked=Vec::<(i64,Value)>::new();
    let mut patches=BTreeSet::new();
    // Compare the most recent eligible peer successes, not an arbitrary
    // first 256 rows from the store's unspecified iteration order.
    let mut generations=state.store.all("rsi_generation").into_iter()
        .filter(|generation|generation["campaignId"].as_str()!=Some(campaign)
            && generation["evaluationProfileSha256"].as_str()==Some(profile_sha))
        .collect::<Vec<_>>();
    generations.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str())
        .then_with(||b["id"].as_str().cmp(&a["id"].as_str())));
    for generation in generations.into_iter().take(256) {
        if generation["campaignId"].as_str()==Some(campaign)
            || generation["evaluationProfileSha256"].as_str()!=Some(profile_sha) {
            continue;
        }
        let Some(design)=adopted_design(&generation) else {continue};
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
        if candidate["status"]!="verified"
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {
            continue;
        }
        let Some(patch_hash)=candidate["patchSha256"].as_str() else{continue};
        if !patches.insert(patch_hash.to_string()){continue;}
        let mut aligned=Vec::new();
        let mut same_bytes=0usize;
        for path in design["targetFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(24) {
            let current=crate::engine::code_index::indexed_source_hash(state,index_id,path);
            if let Some(hash)=current {
                same_bytes+=usize::from(generation["acceptedSourceHashes"][path].as_str()==Some(hash.as_str()));
                aligned.push(path.to_string());
            }
        }
        if aligned.is_empty() {continue;}
        let components=design_roles(design);
        let role_match=preferred_role.is_some_and(|role|components.iter().any(|name|name.as_str()==role));
        let meta=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0);
        let gains=candidate["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0);
        let score=i64::from(role_match)*1_000+(meta.min(20) as i64)*30
            +(gains.min(30) as i64)*5+(same_bytes.min(6) as i64)*2;
        ranked.push((score,json!({
            "source":"independently_paired_peer_lineage_v1",
            "campaignId":generation["campaignId"],"generationId":generation["id"],
            "candidateId":candidate["id"],"patchSha256":patch_hash,
            "generatorCommit":generation["generatorCommit"],
            "acceptedCommit":generation["acceptedCommit"],
            "evaluationProfileSha256":profile_sha,
            "components":components,"alignedSourceFiles":aligned,
            "identicalSourceFiles":same_bytes,
            "nextDomain":design["nextDomain"],
            "hypothesis":design["hypothesis"].as_str().unwrap_or("")
                .chars().take(420).collect::<String>(),
            "designSummary":design["design"].as_str().unwrap_or("")
                .chars().take(700).collect::<String>(),
            "improvedHoldouts":gains,"metaHoldoutGains":meta,
            "sourceCompatibility":if same_bytes>0 {"some_identical"}else{"requires_reimplementation"},
            "automaticPatchReuse":false,"qualityProvenForThisDescendant":false
        })));
    }
    ranked.sort_by(|a,b|b.0.cmp(&a.0).then_with(||
        a.1["candidateId"].as_str().cmp(&b.1["candidateId"].as_str())));
    let selected=ranked.into_iter().take(4).map(|(_,row)|row).collect::<Vec<_>>();
    json!({"source":"cross_campaign_comparative_archive_v1","profileSha256":profile_sha,
        "references":selected,"automaticPatchReuse":false,
        "policy":"same_frozen_oracle_only_peer_successes_are_references_not_promotions"})
}

/// Reuse the model's target only when its *source change*, not its
/// explanation, was independently evaluated and adopted.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    // A failed generation must not erase an adopted design. Yet continuing
    // to pin its target after repeated unsuccessful attempts creates an
    // infinite repair loop. Keep the design in the lineage capsule while
    // releasing the planner to explore after two actual failed continuations.
    let (accepted_at,design)=history.iter().enumerate().rev()
        .find_map(|(index,record)|adopted_design(record).map(|design|(index,design)))?;
    let domain=design["nextDomain"].as_str()?;
    if !crate::engine::rsi_evolve::DOMAINS.contains(&domain) {return None;}
    let exhausted=history[accepted_at+1..].iter()
        .filter(|attempt|attempt["accepted"]!=true && attempt["domain"]==domain)
        .filter(|attempt|attempt["candidateId"].is_string()
            || attempt["error"].as_str().is_some_and(|s|!s.is_empty())
            || attempt["newFindings"].as_u64().unwrap_or(0)>0)
        .take(2).count();
    (exhausted<2).then(||domain.to_string())
}

/// Rebind an independently adopted model-authored improvement PROGRAM to
/// current source bytes. This is executable *planning policy*, not a prompt
/// about the last task. The new binary may rewrite this selector, use a new
/// search formula or replace the whole program in its next accepted revision.
pub(crate) fn inherited_stage(history:&[Value],frontier:&[Value])->Option<Value> {
    let (accepted_index,parent)=history.iter().enumerate().rev()
        .find(|(_,g)|adopted_design(g).is_some())?;
    let design=adopted_design(parent)?;
    let stages=design["successorStages"].as_array()?;
    let mut scored=Vec::<(i64,usize,Value)>::new();
    for (index,stage) in stages.iter().enumerate() {
        let (Some(role),Some(domain),Some(path))=(
            stage["role"].as_str(),stage["domain"].as_str(),stage["path"].as_str()
        )else{continue};
        if !valid_role(role)||!crate::engine::rsi::safe_path(path)
            || !crate::engine::rsi_evolve::DOMAINS.contains(&domain){continue;}
        let Some(source)=frontier.iter().find(|entry|
            entry["role"]==role && entry["domain"]==domain && entry["path"]==path)
        else{continue};
        let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        let tools=stage["toolPolicy"].as_str().unwrap_or("allow_new_modules");
        let execution=stage["executionPolicy"].as_str().unwrap_or("explore_then_edit");
        if !matches!(learning,"avoid_failed"|"retry_failed")
            || !matches!(tools,"patch_only"|"allow_new_modules")
            || !matches!(execution,"grounded_edit"|"explore_then_edit"){continue;}
        let failed=history[accepted_index+1..].iter()
            .filter(|g|g["accepted"]!=true
                && g["focusFiles"].as_array().is_some_and(|f|
                    f.iter().any(|p|p.as_str()==Some(path))))
            .count() as i64;
        let reproduced=history[accepted_index+1..].iter()
            .filter(|g|g["accepted"]==true
                && g["evaluation"]["metrics"]["eligible"]==true
                && g["focusFiles"].as_array().is_some_and(|f|
                    f.iter().any(|p|p.as_str()==Some(path))))
            .count() as i64;
        // Failure contrast is policy, not a hardcoded target. Altering this
        // weight in the next source revision alters its own improvement plan.
        let score=(if learning=="retry_failed"{2*failed}else{-3*failed})
            +2*reproduced;
        scored.push((score,index,json!({
            "source":"inherited_model_authored_improvement_program_v1",
            "role":role,"domain":domain,"path":path,
            "sourceSha256":source["sourceSha256"],
            "objective":stage["objective"],"stageIndex":index,
            "sourceDesignId":design["id"],"parentAcceptedCommit":parent["acceptedCommit"],
            "learningPolicy":learning,"toolPolicy":tools,
            "executionPolicy":execution,"failuresSinceAdoption":failed,
            "reproductiveAdoptions":reproduced,
            "qualityProven":false
        })));
    }
    scored.sort_by(|a,b|b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().next().map(|(_,_,v)|v)
}
