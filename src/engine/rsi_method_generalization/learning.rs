//! Original frozen-pair replay, controlled method feedback aggregation and
//! source-revision-specific learning.
use super::*;

pub(crate) fn learn(state:&AppState,history:&[Value],frontier:&[Value],
    algebra:&Value,commit:&str)->Value {
    let mut rows=Vec::new();
    let known_roles=frontier.iter().filter_map(|f|f["role"].as_str())
        .collect::<BTreeSet<_>>();
    for rule in algebra["operators"].as_array().into_iter().flatten() {
        let id=rule["id"].as_str().unwrap_or("");
        let edited_role=rule["methodRole"].as_str().unwrap_or("");
        let mut trials=BTreeSet::new();
        let mut cases=BTreeSet::new();
        let mut verified_method_controls=0u64;
        let mut method_control_quality_wins=0u64;
        let mut method_control_quality_losses=0u64;
        let mut control_receipt_ids=BTreeSet::<String>::new();
        let mut gains=0u64;let mut regressions=0u64;let mut neutral=0u64;
        let mut cross_gains=0u64;let mut cross_regressions=0u64;
        let mut adopted=0u64;
        let mut domains=BTreeSet::<String>::new();
        let mut profiles=BTreeSet::<String>::new();
        let mut commits=BTreeSet::<String>::new();
        let mut receipt_ids=BTreeSet::<String>::new();
        // Compare successive METHOD SOURCE REVISIONS on exactly the same
        // frozen profile, holdout case, domain and baseline result. A new
        // method source SHA alone is not evidence of improved methodology.
        let mut method_versions=Vec::<String>::new();
        let mut matched_cases=BTreeMap::<(String,String,String,bool),
            BTreeMap<String,Vec<i8>>>::new();
        for g in history {
            let Some((candidate,metrics,profile))=independent_use(state,g,rule)
                else{continue};
            let Some(candidate_id)=g["candidateId"].as_str()else{continue};
            if !trials.insert(candidate_id.to_owned()) {continue}
            let method_sha=g["improvementAlgebraPlan"]["selected"]["rule"]["method"]
                ["sourceSha256"].as_str().filter(|sha|hash_ok(sha,64));
            if let Some(sha)=method_sha {
                if !method_versions.iter().any(|old|old==sha) {
                    method_versions.push(sha.to_owned());
                }
            }
            profiles.insert(profile.to_owned());
            if let Some(parent)=g["generatorCommit"].as_str() {
                commits.insert(parent.to_owned());
            }
            if let Some(evidence)=candidate["evaluation"]["evidenceSha256"].as_str() {
                receipt_ids.insert(evidence.to_owned());
            }
            adopted+=u64::from(actual_adoption(g,&candidate,&metrics));
            // The original OFF worker and ON worker must have matched,
            // independent candidate receipts. The descendant learner
            // consumes this intervention, not a stored success label.
            let intervention=crate::engine::rsi_evolve::measured_method_ablation(state,g);
            if actual_adoption(g,&candidate,&metrics)
                && intervention["qualityProven"]==true
                && intervention["methodAssociationObserved"]==true
                && g["methodAblation"]==intervention
                && intervention["methodFamilyId"]==
                    crate::engine::rsi_improvement_algebra::method_family_id(rule) {
                verified_method_controls+=1;
                method_control_quality_wins+=
                    intervention["treatmentOnlyWins"].as_u64().unwrap_or(0)
                    +intervention["treatmentQualityWins"].as_u64().unwrap_or(0);
                method_control_quality_losses+=
                    intervention["controlOnlyWins"].as_u64().unwrap_or(0)
                    +intervention["controlQualityWins"].as_u64().unwrap_or(0);
                if let Some(id)=intervention["controlEvidenceSha256"].as_str(){
                    control_receipt_ids.insert(id.to_owned());
                }
            }
            for m in metrics["measurements"].as_array().into_iter().flatten() {
                if m["holdout"]!=true {continue}
                let (Some(case),Some(domain),Some(effect))=(
                    m["id"].as_str(),m["domain"].as_str(),case_effect(m))
                else{continue};
                // A single case is counted once per independently evaluated
                // candidate+profile+domain, not once per rewritten JSON copy.
                if !cases.insert((candidate_id.to_string(),profile.to_owned(),
                    case.to_owned(),domain.to_owned())) {continue;}
                if let (Some(sha),Some(before))=(method_sha,m["baselinePass"].as_bool()) {
                    matched_cases.entry((profile.to_owned(),case.to_owned(),
                        domain.to_owned(),before)).or_default()
                        .entry(sha.to_owned()).or_default().push(effect);
                }
                let capability=observed_domain(domain);
                if effect>0 {gains+=1;domains.insert(capability.to_owned());}
                else if effect<0 {regressions+=1;}
                else {neutral+=1;}
                if capability!=edited_role && known_roles.contains(capability) {
                    if effect>0 {cross_gains+=1;}
                    if effect<0 {cross_regressions+=1;}
                }
            }
        }
        let current_method=rule["method"]["sourceSha256"].as_str().unwrap_or("");
        let method_path=rule["method"]["path"].as_str().unwrap_or("");
        // After a real A2 rewrite, schedule a downstream B2 episode with
        // the NEW source bytes. Without this transition a successful B1
        // would cause endless A2 rewrites rather than recursive use.
        let method_rewritten=history.iter().rev().take(6).any(|g|
            g["accepted"]==true && g["pairedRevalidated"]==true
            && g["operatorExecution"]["targetPath"]==method_path
            && g["acceptedSourceHashes"][method_path]==current_method
            && g["improvementDirective"]["methodFeedback"]["source"]
                =="replayed_real_method_control_v1"
            && g["improvementDirectiveVerified"]==true
            && g["candidateId"].as_str().and_then(|id|
                state.store.get("rsi_candidate",id))
                .is_some_and(|candidate|candidate["target"]=="improver"
                    && rsi::replayed_pair_metrics(state,&candidate).is_some()));
        let revised_method_used=history.iter().rev().take(6).any(|g|
            g["accepted"]==true
            && g["operatorExecution"]["methodSourceSha256"]==current_method
            && g["operatorExecution"]["targetPath"]!=method_path
            && g["methodAblation"]["methodAssociationObserved"]==true);
        let pending_revised_trial=method_rewritten && !revised_method_used;
        let predecessor=method_versions.iter().rfind(|sha|
            sha.as_str()!=current_method).map(String::as_str);
        let mut matched_revision_cases=0u64;
        let mut revision_lift=0.0f64;
        if let Some(previous)=predecessor {
            for versions in matched_cases.values(){
                if let (Some(new),Some(old))=(
                    versions.get(current_method),versions.get(previous)) {
                    if !new.is_empty() && !old.is_empty(){
                        let new_mean=new.iter().map(|x|*x as f64).sum::<f64>()
                            /new.len() as f64;
                        let old_mean=old.iter().map(|x|*x as f64).sum::<f64>()
                            /old.len() as f64;
                        revision_lift+=new_mean-old_mean;
                        matched_revision_cases+=1;
                    }
                }
            }
            if matched_revision_cases>0 {
                revision_lift=(revision_lift/(matched_revision_cases as f64))
                    .clamp(-2.0,2.0);
            }
        }
        let measured=gains+regressions+neutral;
        let mean=(gains as f64-2.0*regressions as f64)/(measured as f64+8.0);
        let coverage=(domains.len() as f64).min(5.0)/5.0;
        let support=(trials.len() as f64)/(trials.len() as f64+4.0);
        // Shrink strongly with sparse evidence. A positive score is a
        // predictive SEARCH signal, not a declaration of causal improvement.
        let transferable=(mean*(0.7+0.3*coverage)*support).clamp(-1.0,1.0);
        rows.push(json!({
            "ruleId":id,"methodRole":edited_role,
            "methodFamilyId":crate::engine::rsi_improvement_algebra::
                method_family_id(rule),
            "independentReplayedTrials":trials.len(),
            "adoptedDescendantTrials":adopted,
            "frozenHoldoutCases":measured,
            "holdoutGains":gains,"holdoutRegressions":regressions,
            "holdoutNeutral":neutral,
            "crossCapabilityGains":cross_gains,
            "crossCapabilityRegressions":cross_regressions,
            "improvedCapabilityRoles":domains,
            "frozenProfileDigests":profiles.into_iter().take(12).collect::<Vec<_>>(),
            "generatorCommits":commits.into_iter().take(12).collect::<Vec<_>>(),
            "independentReceiptSha256":receipt_ids.into_iter().take(12).collect::<Vec<_>>(),
            "verifiedMethodControlTrials":verified_method_controls,
            "revisedMethodAwaitingDownstreamControl":pending_revised_trial,
            "methodControlQualityWins":method_control_quality_wins,
            "methodControlQualityLosses":method_control_quality_losses,
            "methodControlEvidenceSha256":control_receipt_ids.into_iter().take(12).collect::<Vec<_>>(),
            "transferScore":transferable,
            "currentMethodSourceSha256":current_method,
            "previousMethodSourceSha256":predecessor,
            "matchedFrozenRevisionCases":matched_revision_cases,
            "observedRevisionTransferLift":revision_lift,
            "revisionDifferenceIsCausalProof":false,
            "hasMeasuredTransfer":cross_gains>0 && cross_regressions==0,
            "causalityProven":false,
            "source":"replayed_independent_paired_candidate_receipts"
        }));
    }
    rows.sort_by(|a,b|a["ruleId"].as_str().cmp(&b["ruleId"].as_str()));
    let capability_gaps=capability_pressure(state,history,frontier);
    // An uncovered model/memory/tool failure can drive the NEXT source edit
    // even if nobody has yet authored a successful meta improvement rule.
    let capability_target=capability_gaps.iter()
        .find(|gap|gap["independentHoldoutRegressions"]
            .as_u64().unwrap_or(0)>0 && gap["priority"].as_i64().unwrap_or(0)>0)
        .map(|gap|gap["target"].clone()).unwrap_or(Value::Null);
    let mut result=json!({
        "source":"compiled_replayed_downstream_method_learning_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "operatorLearning":rows,
        "capabilityGaps":capability_gaps,
        "suggestedCapabilityTarget":capability_target,
        "evidenceRequirement":"replayed_frozen_paired_receipt_plus_actual_compiled_source_target",
        "methodSuccessNotInferredFromSourceAdoption":true,
        "causalityProven":false,"qualityProven":false
    });
    result["sha256"]=json!(digest(&result));
    result
}
