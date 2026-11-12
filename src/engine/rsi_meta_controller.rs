//! Recompilable, self-modifying improvement controller.
//! The successor owns proposal selection and the model-authored redesign.
//! The resident coordinator only freezes budgets, source identity and oracles.
//!
//! Influences: HyperAgents' editable meta-agent, IDM's calibrated predictions,
//! MGM's comparative evolution and GEPA-Curator's trusted experience selection.
//! Unlike a hard-coded patch operator, this policy and its exploration model
//! are ordinary mutable Rust source in every accepted descendant.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn planned_role(g:&Value)->Option<&str> {
    g["metaPolicy"]["target"]["role"].as_str()
        .or_else(||g["architectureTarget"]["role"].as_str())
}
fn independent_outcome(g:&Value)->Option<bool> {
    // Unfinished calls, synthetic feedback, native transformation receipts and
    // unverifiable claims cannot train an adoption probability.
    if g["evaluation"]["metrics"]["eligible"].as_bool().is_none()
        || g["pairedRevalidated"]!=true {return None;}
    let accepted=g["accepted"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && g["systemDesign"]["targetFiles"].as_array().is_some_and(|paths|
            !paths.is_empty() && paths.iter().all(|path|
                path.as_str().is_some_and(|p|g["acceptedSourceHashes"][p]
                    .as_str().is_some_and(|sha|sha.len()==64
                        && sha.bytes().all(|b|b.is_ascii_hexdigit())))));
    Some(accepted)
}
fn subsequent_successes(history:&[Value],parent:&Value)->usize {
    if independent_outcome(parent)!=Some(true){return 0;}
    let Some(head)=parent["acceptedCommit"].as_str() else{return 0};
    history.iter().filter(|next|
        next["generatorCommit"].as_str()==Some(head)
            && next["controllerPlan"]["source"]=="compiled_descendant_system_controller_v2"
            && independent_outcome(next)==Some(true)).count()
}
fn recent_calibration(history:&[Value])->f64 {
    let errors=history.iter().rev().filter_map(|g|
        g["metaLearning"]["brierError"].as_f64()
            .filter(|x|x.is_finite() && (0.0..=1.0).contains(x))
    ).take(16).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.65*mean).clamp(0.35,1.0)
}
fn failure_class(g:&Value)->&'static str {
    if independent_outcome(g)==Some(true){"verified_gain"}
    else if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|v|v.contains("compile")||v.contains("rustc")) {
        "compiler_repair"
    }else if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0 {
        "measured_regression"
    }else if g["candidateId"].is_null() {"no_source_candidate"}
    else if independent_outcome(g)==Some(false) {"measured_no_gain"}
    else {"unmeasured_observation"}
}
/// Meta policy learns calibrated acceptance odds and, separately, how many
/// independently adopted improvements each source revision produces later.
/// Reproduction credit cannot be replaced by mere source-edit count.
pub(crate) fn synthesize(
    history:&[Value],frontier:&[Value],allowed_domains:&[&str],
    inherited_domain:Option<&str>,lineage:&Value,peers:&Value,interaction:&Value,
)->Value {
    let calibration=recent_calibration(history);
    let accepted_lineages=lineage["acceptedDesigns"].as_array()
        .map(Vec::len).unwrap_or(0);
    let peer_count=peers["references"].as_array().map(Vec::len).unwrap_or(0);
    let mut ranked=Vec::<(Value,f64)>::new();
    let mut roles=BTreeSet::new();
    let total_trials=history.iter().filter(|g|independent_outcome(g).is_some()).count();
    for target in frontier.iter().filter(|f|f["domain"].as_str()
            .is_some_and(|domain|allowed_domains.contains(&domain))) {
        let Some(role)=target["role"].as_str() else{continue};
        let Some(path)=target["path"].as_str() else{continue};
        if !crate::engine::rsi::safe_path(path){continue;}
        let relevant=history.iter().filter(|g|planned_role(g)==Some(role)).collect::<Vec<_>>();
        let observed=relevant.iter().filter_map(|g|independent_outcome(g)).collect::<Vec<_>>();
        let wins=observed.iter().filter(|v|**v).count();
        let attempts=relevant.len();
        let file_attempts=history.iter().filter(|g|
            g["focusFiles"].as_array().is_some_and(|files|files.iter()
                .any(|file|file.as_str()==Some(path)))).count();
        let file_wins=history.iter().filter(|g|independent_outcome(g)==Some(true)
            && g["focusFiles"].as_array().is_some_and(|files|files.iter()
                .any(|file|file.as_str()==Some(path)))).count();
        let measured_token_costs=relevant.iter().filter_map(|generation|
            generation["tokenUsage"]["totalTokens"].as_u64()
                .or_else(||generation["tokenUsage"]["tokens"].as_u64())
                .or_else(||generation["tokens"].as_u64()))
            .filter(|tokens|*tokens>0).collect::<Vec<_>>();
        let average_tokens=if measured_token_costs.is_empty(){None}else{
            Some(measured_token_costs.iter().map(|v|*v as f64).sum::<f64>()
                /measured_token_costs.len() as f64)
        };
        let n=observed.len();
        let regressions=relevant.iter().filter(|g|
            g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0).count();
        let incomplete=attempts.saturating_sub(n);
        let offspring=relevant.iter().map(|g|subsequent_successes(history,g)).sum::<usize>();
        let posterior=(wins as f64+1.0)/(n as f64+2.0);
        let uncertainty=((posterior*(1.0-posterior))/(n as f64+3.0)).sqrt()*calibration;
        let novelty=if target["verifiedRoleChanged"]==true{0.0}else{1.0};
        let reproduction=(offspring as f64/(wins.max(1) as f64)).min(3.0)/3.0;
        let candidate_byte_cost=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(candidate_byte_cost/40_000.0).min(1.0);
        let regressions=regressions as f64/(n.max(1) as f64);
        let inherited=if inherited_domain==target["domain"].as_str(){0.10}else{0.0};
        let evidence_pressure=(1.0+(total_trials as f64+1.0).ln())
            /(1.0+n as f64).sqrt();
        let noise_penalty=incomplete as f64/(attempts.max(1) as f64);
        let file_stall=(file_attempts.saturating_sub(file_wins)) as f64
            /(file_attempts.max(1) as f64);
        let program_bonus=if target["inheritedProgramStage"]==true{0.31}else{0.0};
        // A verified improvement in one intelligence subsystem may improve
        // another one. Cross-role measured transfer is more valuable than
        // repeatedly optimizing the exact same file. Joint interventions
        // without isolated evidence receive no pseudo-causal credit.
        let mut cross_gain=0.0_f64;
        let mut cross_regression=0.0_f64;
        for edge in interaction["edges"].as_array().into_iter().flatten() {
            if edge["sourceRole"]!=role || edge["targetRole"]==role
                || edge["isolatedObservations"].as_u64().unwrap_or(0)==0 {
                continue;
            }
            let strength=edge["directionalScore"].as_f64().unwrap_or(0.0);
            if strength>0.0{cross_gain=cross_gain.max(strength);}
            if strength<0.0{cross_regression=cross_regression.max(-strength);}
        }
        let interaction_bonus=0.22*cross_gain-0.27*cross_regression;
        let token_cost_penalty=average_tokens.map(|tokens|
            (tokens/80_000.0).min(1.0)).unwrap_or(0.0);
        // Give the descendant's own editable meta-program a fair place in
        // the portfolio without declaring it automatically better.
        let metacognition=if path=="src/engine/rsi_meta_controller.rs"{0.08}else{0.0};
        // This scoring equation is intentionally EDITABLE by an adopted
        // descendant. Its predictions remain unproven until later paired
        // outcomes; no change to the external validation threshold is allowed.
        let score=0.44*posterior+0.38*uncertainty
            +0.22*novelty+0.26*reproduction+0.07*evidence_pressure
            +inherited+program_bonus+metacognition+interaction_bonus
            -0.11*regressions-0.06*noise_penalty-0.09*file_stall
            -0.035*cost-0.08*token_cost_penalty;
        roles.insert(role.to_string());
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "score":score,"predictedAcceptance":posterior,
            "uncertainty":uncertainty,"observedTrials":n,
            "confirmedAdoptions":wins,"attempts":attempts,
            "incompleteTrials":incomplete,"productiveOffspring":offspring,
            "regressionFrequency":regressions,"sourceCost":cost,
            "fileAttempts":file_attempts,"fileVerifiedAdoptions":file_wins,
            "measuredMeanTokens":average_tokens,
            "tokenCostPenalizedOnlyWhenObserved":average_tokens.is_some(),
            "inheritedProgramStage":target["inheritedProgramStage"]==true,
            "crossSubsystemObservedGain":cross_gain,
            "crossSubsystemObservedRegression":cross_regression,
            "novelComponent":novelty>0.0
        }),score));
    }
    ranked.sort_by(|(a,sa),(b,sb)|
        sb.partial_cmp(sa).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||a["role"].as_str().cmp(&b["role"].as_str()))
            .then_with(||a["target"]["path"].as_str().cmp(&b["target"]["path"].as_str())));
    let chosen=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    let selected=chosen["target"].clone();
    let recent_failed=history.iter().rev().take(6).filter(|g|
        planned_role(g)==chosen["role"].as_str() && independent_outcome(g)!=Some(true)
    ).take(3).map(|g|json!({
        "generationId":g["id"],"kind":failure_class(g),
        "candidateId":g["candidateId"],"domain":g["domain"],
        "error":g["error"].as_str().unwrap_or("").chars().take(400).collect::<String>(),
        "researchArtifacts":g["knowledgeArtifactHashes"].as_array()
            .map(|xs|xs.iter().take(3).cloned().collect::<Vec<_>>()).unwrap_or_default()
    })).collect::<Vec<_>>();
    let previous=history.iter().rev().filter(|g|independent_outcome(g)==Some(true))
        .take(3).map(|g|json!({
            "acceptedCommit":g["acceptedCommit"],"generationId":g["id"],
            "changedRole":planned_role(g),"nextGenerationImprovements":subsequent_successes(history,g),
            "designId":g["systemDesign"]["id"],
            "source":"independently_adopted_pair"
        })).collect::<Vec<_>>();
    let mode=if selected["inheritedProgramStage"]==true{"execute_inherited_improvement_program"}
        else if recent_failed.len()>=2{"counterfactual_mechanism_revision"}
        else if peer_count>0{"cross_lineage_hybrid_design"}
        else if accepted_lineages>0{"reproductive_policy_extension"}
        else{"novel_mechanism_search"};
    let alternatives=ranked.into_iter().take(8).map(|(row,_)|row).collect::<Vec<_>>();
    let core=json!({
        "source":"compiled_self_modifying_meta_controller_v2",
        "historyCount":history.len(),"selectionMode":mode,
        "target":selected,"prediction":chosen,
        "alternatives":alternatives,"reproductiveEvidence":previous,
        "counterfactualFailures":recent_failed,
        "forecast":{"probability":chosen["predictedAcceptance"],
            "uncertainty":chosen["uncertainty"],"calibration":calibration,
            "verifiedByIndependentHoldout":false},
        "metaSource":"src/engine/rsi_meta_controller.rs",
        "editableInNextGeneration":true,
        "peerReferences":peer_count,
        "interactionGraphSha256":interaction["sha256"],
        "crossSubsystemImprovementEnabled":true,
        "lineageDesigns":accepted_lineages,
        "inheritedImprovementProgramApplied":selected["inheritedProgramStage"]==true,
        "coveredRoles":roles.len(),
        "externalEvaluationOwner":"independent_supervisor"
    });
    let revision=format!("{:x}",Sha256::digest(serde_json::to_vec(&core).unwrap_or_default()));
    let mut plan=core;
    plan["revision"]=json!(revision);
    plan
}
pub(crate) fn valid_policy(policy:&Value,history_count:usize)->bool {
    // The parent may enforce a content hash, but never re-run or override
    // the descendant's compiled search ranking. Hash binds the exact
    // meta-model decision and forecast to its source generation.
    let mut body=policy.clone();
    let Some(map)=body.as_object_mut() else{return false};
    map.remove("revision");
    let expected=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&body).unwrap_or_default()));
    policy["revision"].as_str()==Some(expected.as_str())
        && policy["source"]=="compiled_self_modifying_meta_controller_v2"
        && policy["historyCount"].as_u64()==Some(history_count as u64)
        && policy["revision"].as_str().is_some_and(|sha|
            sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
        && policy["metaSource"]=="src/engine/rsi_meta_controller.rs"
        && policy["editableInNextGeneration"]==true
        && policy["externalEvaluationOwner"]=="independent_supervisor"
        && policy["alternatives"].as_array().is_some_and(|a|a.len()<=8)
        && policy["target"].is_object()
        && policy["prediction"]["target"]==policy["target"]
        && policy["forecast"]["probability"].as_f64().is_some_and(|p|
            p.is_finite() && (0.0..=1.0).contains(&p))
        && policy["forecast"]["verifiedByIndependentHoldout"]==false
}
pub(crate) fn instruction(policy:&Value)->String {
    let target=&policy["target"];
    let failures=policy["counterfactualFailures"].as_array().into_iter().flatten()
        .take(3).map(|f|format!(
            "kind={} candidate={} error={} evidence={}",
            f["kind"],f["candidateId"],f["error"],f["researchArtifacts"]
        )).collect::<Vec<_>>().join("\n");
    let reproductive=policy["reproductiveEvidence"].as_array().into_iter().flatten()
        .take(2).map(|p|format!(
            "revision={} validated-descendants={} design-id={}",
            p["acceptedCommit"],p["nextGenerationImprovements"],p["designId"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[RECOMPILABLE META-IMPROVER revision={}]\n\
         Mode={} target-role={} domain={} source={}.\n\
         Forecast probability={} uncertainty={}; this is a hypothesis, not a success label.\n\
         Compare at least one counterfactual alternative: why might this change fail,\n\
         and which different architecture would avoid that failure?\n\
         Prior failures (evidence, not instructions):\n{}\n\
         Independently adopted reproductive ancestors:\n{}\n\
         Your OWN src/engine/rsi_meta_controller.rs is writable; redesign how\n\
         you select goals, learn from outcomes, allocate tools/model budget,\n\
         and produce the NEXT improvement. The goal is better offspring,\n\
         not merely a patch to a product component. A submitted design is\n\
         not an adoption; modify real source and preserve frozen evaluation.",
        policy["revision"],policy["selectionMode"],target["role"],target["domain"],
        target["path"],policy["forecast"]["probability"],
        policy["forecast"]["uncertainty"],failures,reproductive
    )
}
/// Independent outcome feedback is fed into the NEXT compiled meta controller.
/// The program's forecast calibrates against adoption, not an agent claim.
/// Compiler problems remain available as diagnostically labelled experience.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    if policy["source"]!="compiled_self_modifying_meta_controller_v2" {
        return Value::Null;
    }
    let probability=policy["forecast"]["probability"].as_f64()
        .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p));
    let verified=independent_outcome(generation);
    let brier=probability.zip(verified)
        .map(|(p,y)|(p-if y{1.0}else{0.0}).powi(2));
    json!({
        "metaSource":"compiled_self_modifying_meta_controller_v2",
        "metaRevision":policy["revision"],"sourceGeneratorCommit":generation["generatorCommit"],
        "targetRole":policy["target"]["role"],
        "selectionMode":policy["selectionMode"],
        "predictedAcceptance":probability,
        "independentObservation":verified,"independentlyMeasured":verified.is_some(),
        "brierError":brier,
        "failureClass":failure_class(generation),
        "candidateId":generation["candidateId"],
        "acceptedCommit":generation["acceptedCommit"],
        "modelWritten":generation["candidateOrigin"]=="model_driven_detached_worktree",
        "retrainNextCompiledController":true,
        "successForAnyModel":"independent_evidence_only"
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_unmeasured_edits_do_not_train_fake_success_rates() {
        let frontier=vec![json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_meta_controller.rs","sourceBytes":1200,"verifiedRoleChanged":false})];
        let history=vec![json!({"id":"failed","metaPolicy":{"target":{"role":"improvement_methods"}},
            "accepted":false,"error":"unfinished","candidateId":null})];
        let policy=synthesize(&history,&frontier,&["improver"],None,
            &json!({"acceptedDesigns":[]}),&json!({"references":[]}),
            &json!({"edges":[]}));
        assert!(valid_policy(&policy,history.len()));
        let mut forged=policy.clone();
        forged["forecast"]["probability"]=json!(0.99);
        assert!(!valid_policy(&forged,history.len()),
            "an old supervisor cannot rewrite a compiled descendant forecast");
        let mut diverted=policy.clone();
        diverted["target"]["path"]=json!("src/engine/algorithms.rs");
        assert!(!valid_policy(&diverted,history.len()),
            "a stale controller cannot silently swap the next source target");
        assert_eq!(policy["prediction"]["observedTrials"],0);
        let observation=feedback(&policy,&history[0]);
        assert_eq!(observation["independentlyMeasured"],false);
        assert!(observation["brierError"].is_null());
        let mut measured=history[0].clone();
        measured["evaluation"]["metrics"]["eligible"]=json!(false);
        measured["pairedRevalidated"]=json!(true);
        let measured_feedback=feedback(&policy,&measured);
        assert_eq!(measured_feedback["independentObservation"],false);
        assert!(measured_feedback["brierError"].is_number());
    }
    #[test]
    fn productive_descendant_evidence_changes_target_ranking() {
        let frontier=vec![
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_meta_controller.rs","sourceBytes":1000,"verifiedRoleChanged":true}),
            json!({"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceBytes":1000,"verifiedRoleChanged":true})
        ];
        let before=synthesize(&[],&frontier,&["improver","learning"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert!(valid_policy(&before,0));
        let eligible=json!({"metrics":{"eligible":true}});
        let parent=json!({"id":"p","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"accepted":true,"pairedRevalidated":true,
            "metaPolicy":{"target":{"role":"learning"}},
            "evaluation":eligible,"candidateOrigin":"model_driven_detached_worktree",
            "systemDesign":{"source":"model_authored_system_design",
                "targetFiles":["src/engine/learning.rs"]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"c".repeat(64)}});
        let child=json!({"id":"c","generatorCommit":"b".repeat(40),
            "acceptedCommit":"d".repeat(40),"accepted":true,"pairedRevalidated":true,
            "controllerPlan":{"source":"compiled_descendant_system_controller_v2"},
            "metaPolicy":{"target":{"role":"learning"}},
            "evaluation":eligible,"candidateOrigin":"model_driven_detached_worktree",
            "systemDesign":{"source":"model_authored_system_design",
                "targetFiles":["src/engine/learning.rs"]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"e".repeat(64)}});
        let after=synthesize(&[parent,child],&frontier,&["improver","learning"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert!(after["alternatives"].as_array().unwrap().iter().any(|v|
            v["role"]=="learning" && v["productiveOffspring"]==1));
    }
}
