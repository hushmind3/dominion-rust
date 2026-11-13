//! Canonical recursive proof and campaign lineage verification.
//!
//! Candidate-independent frozen evaluation is owned by rsi.rs. This module
//! consumes its replayed receipts and provides ONE completion verdict for
//! the coordinator, early-stop path, campaign API and verification endpoint.
use super::*;

/// One adopted generation holds proof only when its paired evaluation was
/// replayed from the preserved receipt and matched the exact generator and
/// accepted commits.
fn generation_proof(state:&AppState,generation:&Value)
    ->Option<(String,bool,BTreeSet<String>)> {
    if generation["accepted"] != true {return None;}
    let candidate = generation["candidateId"].as_str()
        .and_then(|id| state.store.get("rsi_candidate", id))?;
    let commit = generation["acceptedCommit"].as_str().filter(|s|sha(s))?;
    generation["generatorCommit"].as_str().filter(|s|sha(s))?;
    let evaluation = &candidate["evaluation"];
    let evidence = evaluation["evidenceSha256"].as_str().is_some_and(|h|
        h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()));
    let verified = candidate["status"] == "verified"
        && evaluation["source"] == "server_invoked_local_paired_git_worktrees"
        && evaluation["metrics"]["eligible"] == true
        && evaluation["metrics"]["regressions"] == 0
        && evaluation["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0) > 0
        && evidence
        && generation["evaluation"]["evidenceSha256"] == evaluation["evidenceSha256"]
        && candidate["experimentAdoption"]["commit"] == generation["acceptedCommit"]
        && candidate["baseCommit"] == generation["generatorCommit"];
    if !verified || !verified_pair_receipt(state,&candidate) {return None;}
    let improved=verified_holdout_domains(&evaluation["metrics"])?;
    let meta = candidate["target"] == "improver"
        && evaluation["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0) > 0
        && improved.contains("improver");
    Some((commit.to_string(),meta,improved))
}

/// Three accepted patches alone do not demonstrate recursive intelligence.
/// Require exact generator succession, independently measured holdout gains,
/// a self-improver change, and subsequent transfer into another capability.
pub fn capability_proven(state: &AppState, history: &[Value]) -> bool {
    let mut chain: Vec<(String, bool, BTreeSet<String>)> = Vec::new();
    for generation in history {
        let Some(generator) = generation["generatorCommit"].as_str().filter(|s|sha(s)) else {
            chain.clear(); continue;
        };
        let Some(proof) = generation_proof(state,generation) else {
            chain.clear(); continue;
        };
        if chain.last().is_some_and(|(prior,_,_)|prior!=generator) {
            chain.clear();
        }
        chain.push(proof);
        if chain.len() >= 3 && chain.windows(3).any(|window|
            window.iter().enumerate().any(|(i,(_,meta,domains))|
                *meta && window[i+1..].iter().any(|(_,_,successor_domains)|
                    successor_domains.iter().any(|domain|
                        domain!="improver" && !domains.contains(domain)))))
        { return true; }
    }
    false
}

/// Gather every inherited segment of a campaign in lineage order.
fn lineage_history(state:&AppState,campaign:&Value)->Vec<Value> {
    let mut segments = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current=campaign.clone();
    for _ in 0..32 {
        let Some(id)=current["id"].as_str() else {break};
        if !seen.insert(id.to_string()) {break;}
        segments.push(current["history"].as_array().cloned().unwrap_or_default());
        if current["lineage"]["reused"]!=true {break;}
        let Some(prior)=current["continuedFrom"].as_str()
            .and_then(|id|state.store.get("rsi_campaign",id)) else {break};
        let selected=current["lineage"]["selectedCommit"].as_str().unwrap_or("");
        let hash=current["sourceFingerprint"].as_str().unwrap_or("");
        if !sha(selected) || prior["headCommit"]!=selected
            || current["sourceDir"]!=prior["sourceDir"]
            || current["sourceBranch"]!=prior["sourceBranch"]
            || current["baseCommit"]!=prior["baseCommit"]
            || prior["sourceFingerprint"]!=hash
            || !crate::engine::rsi_evolve::lineage_fingerprint_valid(hash)
        {break;}
        current=prior;
    }
    segments.reverse();
    segments.into_iter().flatten().collect()
}

/// A continued campaign may inherit proof only from the same source snapshot,
/// work branch and selected descendant. A reused goal label proves nothing.
pub fn campaign_capability_proven(state:&AppState, campaign:&Value)->bool {
    capability_proven(state,&lineage_history(state,campaign))
}

/// The single authoritative recursive-system completion contract.
/// Ordinary source-adoption proficiency remains a separate diagnostic: it can
/// never end a recursive-evolution campaign or be advertised as full RSI.
/// Continued campaigns use lineage_history's source/commit continuity guard.
pub fn recursive_proof_report(state:&AppState,campaign:&Value)->Value {
    let history=lineage_history(state,campaign);
    let system_evolution=campaign["systemEvolution"]==true;
    let system_evidence=if system_evolution {
        crate::engine::rsi_system::handoff_evidence(state,&history)
    }else{Value::Null};
    let closure=system_evidence["causalClosure"].clone();
    let demonstrated=system_evolution
        && system_evidence["demonstrated"]==true
        && closure["demonstrated"]==true;
    let source_proven=capability_proven(state,&history);
    json!({
        "source":"canonical_recursive_proof_gate_v1",
        "demonstrated":demonstrated,
        "capabilityDemonstrated":demonstrated,
        "generationCapabilityProven":source_proven,
        "systemEvolution":system_evolution,
        "historyCount":history.len(),
        "systemEvidence":system_evidence,
        "causalClosure":closure,
        "requires":"independent source adoption, executed inherited methods, revised method generation, downstream holdout gains and the same recompiled causal-closure lineage",
        "modelWeightUpdates":0
    })
}

/// Attach the exact same proof fields to mutable campaign state and API views.
/// Keeping these assignments here prevents a relaxed UI claim from diverging
/// from the coordinator's stop condition and the independent audit endpoint.
pub fn attach_recursive_proof(record:&mut Value, proof:&Value) {
    record["recursiveProof"]=proof.clone();
    record["systemEvidence"]=proof["systemEvidence"].clone();
    record["generationCapabilityProven"]=proof["generationCapabilityProven"].clone();
    record["capabilityDemonstrated"]=proof["demonstrated"].clone();
}

/// Re-run the capability gate over stored evidence and report every generation
/// so the screen can show exactly which row earned proof and why it did not.
pub fn capability_audit(state:&AppState,campaign:&Value)->Value {
    let history=lineage_history(state,campaign);
    let mut rows=Vec::new();
    let mut verified_run=0u64;
    let mut verified_best=0u64;
    for (index,generation) in history.iter().enumerate() {
        let accepted=generation["accepted"]==true;
        let proof=generation_proof(state,generation);
        let candidate=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id));
        let metrics=candidate.as_ref().map(|candidate|candidate["evaluation"]["metrics"].clone());
        let reason=if !accepted {
            "채택되지 않은 세대"
        } else if proof.is_none() {
            "채택됐지만 독립 평가 영수증이 일치하지 않습니다"
        } else {
            ""
        };
        if accepted && proof.is_some() {
            verified_run+=1;
            verified_best=verified_best.max(verified_run);
        } else {
            verified_run=0;
        }
        rows.push(json!({
            "index":index+1,
            "attempt":generation["attempt"],
            "accepted":accepted,
            "domain":generation["domain"],
            "generatorCommit":generation["generatorCommit"],
            "acceptedCommit":generation["acceptedCommit"],
            "target":candidate.as_ref().and_then(|candidate|candidate["target"].as_str()),
            "independentReceipt":proof.is_some(),
            "metaTransfer":proof.as_ref().is_some_and(|(_,meta,_)|*meta),
            "regressions":metrics.as_ref().and_then(|metrics|metrics["regressions"].as_u64()),
            "improvedHoldoutDomains":metrics.as_ref().map(|metrics|metrics["improvedHoldoutDomains"].clone()),
            "metaHoldoutGains":metrics.as_ref().and_then(|metrics|metrics["metaHoldoutGains"].as_u64()),
            "reason":reason,
        }));
    }
    let gate=recursive_proof_report(state,campaign);
    json!({
        "campaignId":campaign["id"],
        "capabilityDemonstrated":gate["demonstrated"],
        "generationCapabilityProven":gate["generationCapabilityProven"],
        "recursiveProof":gate,
        "systemEvidence":gate["systemEvidence"],
        "verifiedStreak":verified_best,
        "required":{"verifiedStreak":3,"holdoutDomains":1,"metaTransfer":true,
            "fullRsi":"canonical_recursive_proof_gate_v1"},
        "generations":rows,
    })
}

