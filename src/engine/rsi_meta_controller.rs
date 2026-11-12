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
    // Two distinct facts: an experiment can have a replayable independent
    // measurement even if its patch was rejected, regressed or never adopted.
    // Successful offspring require an actual separate adoption receipt.
    // Source-writing method (native / local / API) is not a success criterion.
    if g["pairedMeasured"]!=true && g["pairedRevalidated"]!=true {return None;}
    if g["evaluation"]["metrics"]["eligible"].as_bool().is_none()
        || g["evaluation"]["metrics"]["verdict"].as_str().is_none() {
        return None;
    }
    let accepted=g["accepted"]==true
        && g["pairedRevalidated"]==true
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && g["acceptedSourceHashes"].as_object()
            .is_some_and(|hashes|!hashes.is_empty() && hashes.values().all(|hash|
                hash.as_str().is_some_and(|sha|sha.len()==64
                    && sha.bytes().all(|b|b.is_ascii_hexdigit()))));
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
/// Evidence about changes that generalize beyond the domain they targeted.
/// Count only frozen, paired, held-out cases attributed to ONE actually changed
/// source role and this exact file. Joint interventions remain confounded.
/// This affects the editable descendant's SEARCH, never the adoption oracle.
fn generalization_evidence(
    history:&[Value],frontier:&[Value],role:&str,path:&str,target_domain:&str
)->Value {
    fn family(domain:&str)->&str {
        match domain {
            "recall"|"memory"=>"memory",
            "context"|"planning"|"plan"|"harness"=>"harness",
            "search"|"improver"|"algorithm"=>"improver",
            "training"|"learning"=>"learning",
            "tool-use"|"tools"=>"tools",
            "model"|"models"=>"models",
            "ui"|"frontend"=>"frontend",
            other=>other
        }
    }
    let mut domains=std::collections::BTreeMap::<String,(u64,u64,u64)>::new();
    for generation in history {
        if independent_outcome(generation).is_none()
            || generation["candidateId"].as_str().is_none()
            || !generation["changedSourcePaths"].as_array().is_some_and(|paths|
                paths.iter().any(|candidate|candidate.as_str()==Some(path))) {
            continue;
        }
        let actual=crate::engine::rsi_interaction::actual_roles(generation,frontier);
        if actual.len()!=1 || actual[0]!=role {continue;}
        for case in generation["evaluation"]["metrics"]["measurements"]
            .as_array().into_iter().flatten() {
            if case["holdout"]!=true {continue;}
            let Some(domain)=case["domain"].as_str() else{continue};
            if family(domain)==family(target_domain) {continue;}
            let (Some(before),Some(after))=
                (case["baselinePass"].as_bool(),case["candidatePass"].as_bool())
                else{continue};
            let mut improved=!before && after;
            let mut regressed=before && !after;
            for metric in case["metrics"].as_array().into_iter().flatten() {
                improved |= metric["improved"]==true;
                regressed |= metric["regressed"]==true;
            }
            // An explicitly measured regression dominates other gains in
            // the same case; it must not be diluted into an apparent win.
            if domains.len()>=16 && !domains.contains_key(family(domain)){continue;}
            let counts=domains.entry(family(domain).to_owned()).or_default();
            if regressed {counts.1+=1;}
            else if improved {counts.0+=1;}
            else {counts.2+=1;}
        }
    }
    let (mut wins,mut losses,mut neutral)=(0_u64,0_u64,0_u64);
    let cases=domains.into_iter().map(|(domain,(gains,regressions,stable))|{
        wins+=gains;losses+=regressions;neutral+=stable;
        json!({"domain":domain,"gains":gains,"regressions":regressions,
            "neutral":stable,"observations":gains+regressions+stable})
    }).collect::<Vec<_>>();
    let n=wins+losses+neutral;
    // Shrink sparse observations toward zero; losing a separate capability
    // carries more cost than a within-domain gain. No unmeasured optimism.
    let adjustment=if n==0 {0.0} else {
        (0.20*wins as f64-0.44*losses as f64)/(n as f64+4.0)
    };
    json!({"source":"isolated_paired_cross_domain_holdout_v1",
        "sourcePath":path,"sourceRole":role,"targetDomain":family(target_domain),
        "observedCases":n,"observedGains":wins,
        "observedRegressions":losses,"observedNeutral":neutral,
        "testedDomains":cases,"selectionAdjustment":adjustment,
        "causalityProven":false,"adoptionAuthority":"independent_supervisor"})
}

/// Separate prequential target: a source patch can be accepted without
/// improving a held-out TASK. Measure those events independently of adoption.
/// The compiled source and frozen pair are evaluated before they can teach the
/// next generation; no model-reported outcome is accepted as training data.
fn heldout_quality(g:&Value)->Vec<(bool,bool)> {
    if independent_outcome(g).is_none()
        || g["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || g["candidateId"].as_str().is_none()
        || g["generatorCommit"].as_str().is_none_or(|sha|sha.len()!=40)
        || g["evaluationProfileSha256"].as_str().is_none_or(|sha|sha.len()!=64)
        || g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["compiledPlanExecuted"]["generatorCommit"]!=g["generatorCommit"] {
        return Vec::new();
    }
    let mut seen=BTreeSet::new();
    let mut observations=Vec::new();
    for case in g["evaluation"]["metrics"]["measurements"].as_array().into_iter().flatten() {
        if case["holdout"]!=true {continue;}
        let (Some(id),Some(domain))=(case["id"].as_str(),case["domain"].as_str())
            else{continue};
        if !seen.insert((id.to_string(),domain.to_string())) {continue;}
        let (Some(before),Some(after))=
            (case["baselinePass"].as_bool(),case["candidatePass"].as_bool())
            else{continue};
        let mut improved=!before&&after;
        let mut regressed=before&&!after;
        for metric in case["metrics"].as_array().into_iter().flatten() {
            improved|=metric["improved"]==true;
            regressed|=metric["regressed"]==true;
        }
        observations.push((improved&&!regressed,regressed));
    }
    observations
}
fn task_quality_prior(history:&[Value],role:&str)->Value {
    let mut candidates=BTreeSet::new();
    let (mut n,mut gain,mut loss)=(0_u64,0_u64,0_u64);
    for generation in history.iter().filter(|g|planned_role(g)==Some(role)) {
        let Some(candidate)=generation["candidateId"].as_str() else{continue};
        let key=(generation["evaluationProfileSha256"].as_str().unwrap_or("").to_owned(),
            candidate.to_string());
        if !candidates.insert(key){continue;}
        for (better,worse) in heldout_quality(generation) {
            n+=1;gain+=u64::from(better);loss+=u64::from(worse);
        }
    }
    json!({"source":"independently_paired_heldout_task_quality_prior_v1",
        "observedCases":n,"observedGains":gain,"observedRegressions":loss,
        "probability":(gain as f64+1.0)/(n as f64+2.0),
        "notAcceptanceProbability":true})
}
/// Compute prequential forecast error from the OLD policy sealed before
/// editing each candidate. A high error means "rewrite this predictor" is
/// an exploration target, not evidence that the revision is better.
fn quality_forecast_calibration(history:&[Value])->Value {
    let mut seen=BTreeSet::new();
    let mut errors=Vec::<f64>::new();
    let mut regressions=0_u64;
    for generation in history.iter().rev() {
        let Some(id)=generation["candidateId"].as_str() else{continue};
        let key=(generation["evaluationProfileSha256"].as_str().unwrap_or("").to_owned(),
            id.to_owned());
        if !seen.insert(key) {continue;}
        let forecast=&generation["metaPolicy"]["forecast"];
        let Some(p)=forecast["taskCaseGainProbability"].as_f64()
            .filter(|v|v.is_finite()&&(0.0..=1.0).contains(v)) else{continue};
        if generation["metaPolicy"]["revision"].as_str()
            .is_none_or(|sha|sha.len()!=64)
            || generation["compiledPlanExecuted"]["metaRevision"]!=generation["metaPolicy"]["revision"] {
            continue;
        }
        for (better,worse) in heldout_quality(generation) {
            errors.push((p-f64::from(u8::from(better))).powi(2));
            regressions+=u64::from(worse);
        }
        if errors.len()>=64 {break;}
    }
    let n=errors.len();
    let mean=if n>0 {errors.iter().sum::<f64>()/n as f64}else{0.0};
    // Four independent case outcomes in the last observed window trigger
    // hypothesis generation, never self-acceptance or oracle edits.
    json!({"source":"prequential_sealed_task_gain_calibration_v1",
        "observations":n,"meanBrierError":mean,
        "observedRegressions":regressions,
        "needsPredictorSourceRevision":n>=4 && mean>0.35,
        "editableSource":"src/engine/rsi_meta_controller.rs",
        "qualityImproved":false,"independentMetaABProven":false})
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
    let transfer=crate::engine::rsi_mutual_program::transfer_graph(history);
    let learned_principles=crate::engine::rsi_principle_learning::study(history);
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
        let generalization=generalization_evidence(
            history,frontier,role,path,
            target["domain"].as_str().unwrap_or(""));
        let generalization_score=generalization["selectionAdjustment"]
            .as_f64().unwrap_or(0.0);
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
        // This is a different timescale from within-generation A/B gains:
        // reward a source role only when a genuinely rebuilt successor
        // improves ANOTHER role with an independently adopted patch.
        let links=transfer["edges"].as_array();
        let outgoing=links.into_iter().flatten().filter(|row|
            row["fromRole"]==role).collect::<Vec<_>>();
        let productive=outgoing.iter().map(|row|
            row["nextGenerationAdoptions"].as_u64().unwrap_or(0)).sum::<u64>();
        let measured=outgoing.iter().map(|row|
            row["independentlyMeasured"].as_u64().unwrap_or(0)).sum::<u64>();
        let reciprocal=outgoing.iter().map(|row|{
            let Some(other)=row["toRole"].as_str() else{return 0};
            let backward=links.into_iter().flatten().find(|reverse|
                reverse["fromRole"]==other && reverse["toRole"]==role)
                .and_then(|reverse|reverse["nextGenerationAdoptions"].as_u64())
                .unwrap_or(0);
            backward.min(row["nextGenerationAdoptions"].as_u64().unwrap_or(0))
        }).sum::<u64>();
        let generational_bonus=if measured>0 {
            0.22*(productive as f64/(measured as f64+2.0))
                +0.20*(reciprocal.min(3) as f64/3.0)
        }else{0.0};
        let token_cost_penalty=average_tokens.map(|tokens|
            (tokens/80_000.0).min(1.0)).unwrap_or(0.0);
        // Prefer improvements that actually help TASKS and later compiled
        // offspring, not just source edit acceptance. Only observed case
        // measurements enter these numbers. Unmeasured roles retain a prior
        // rather than receiving fabricated learned gains.
        let (quality_sum,quality_cases,productive_future,future_cases)=
            learned_principles["principles"].as_array().into_iter().flatten()
                .filter(|item|item["changedRoles"].as_array().is_some_and(|roles|
                    roles.iter().any(|r|r==role)))
                .fold((0.0_f64,0_u64,0.0_f64,0_u64),|acc,item|{
                    let n=item["measuredCases"].as_u64().unwrap_or(0);
                    let next=item["nextGenerationCases"].as_u64().unwrap_or(0);
                    (acc.0+item["posteriorCaseGain"].as_f64().unwrap_or(0.5)*n as f64,
                     acc.1+n,
                     acc.2+item["posteriorDescendantGain"].as_f64().unwrap_or(0.5)*next as f64,
                     acc.3+next)
                });
        let quality_signal=if quality_cases>0 {
            (quality_sum/quality_cases as f64-0.5)*0.35
        }else{0.0};
        let future_signal=if future_cases>0 {
            (productive_future/future_cases as f64-0.5)*0.30
        }else{0.0};
        // Expose model and learning runtime as active, hypothesis-bearing
        // source targets. They can be improved via different verified tasks;
        // this is no claim that local LLM parameters were already trained.
        let underexplored_intelligence=if matches!(role,"models"|"learning")
            && quality_cases==0 {0.05}else{0.0};
        let principle_effect=quality_signal+future_signal+underexplored_intelligence;
        // Give the descendant's own editable meta-program a fair place in
        // the portfolio without declaring it automatically better.
        let metacognition=if path=="src/engine/rsi_meta_controller.rs"{0.08}else{0.0};
        // This scoring equation is intentionally EDITABLE by an adopted
        // descendant. Its predictions remain unproven until later paired
        // outcomes; no change to the external validation threshold is allowed.
        let score=0.44*posterior+0.38*uncertainty
            +0.22*novelty+0.26*reproduction+0.07*evidence_pressure
            +inherited+program_bonus+metacognition+interaction_bonus
            +generational_bonus+principle_effect+generalization_score
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
            "offspringCrossRoleAdoptions":productive,
            "offspringReciprocalImprovementObservations":reciprocal,
            "crossGenerationReward":generational_bonus,
            "measuredTaskQualityCases":quality_cases,
            "measuredFutureOffspringCases":future_cases,
            "measuredPrincipleLearningBonus":principle_effect,
            "crossDomainEvidence":generalization,
            "crossDomainGeneralizationAdjustment":generalization_score,
            "novelComponent":novelty>0.0
        }),score));
    }
    ranked.sort_by(|(a,sa),(b,sb)|
        sb.partial_cmp(sa).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||a["role"].as_str().cmp(&b["role"].as_str()))
            .then_with(||a["target"]["path"].as_str().cmp(&b["target"]["path"].as_str())));
    let chosen=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    let selected=chosen["target"].clone();
    let task_prior=task_quality_prior(&history,
        selected["role"].as_str().unwrap_or(""));
    let quality_calibration=quality_forecast_calibration(history);
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
        else if chosen["crossDomainEvidence"]["observedRegressions"].as_u64().unwrap_or(0)>=2
            {"cross_domain_generalization_repair"}
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
            "taskCaseGainProbability":task_prior["probability"],
            "taskEvidenceCases":task_prior["observedCases"],
            "taskPredictionSource":task_prior["source"],
            "uncertainty":chosen["uncertainty"],"calibration":calibration,
            "verifiedByIndependentHoldout":false},
        "metaSource":"src/engine/rsi_meta_controller.rs",
        "editableInNextGeneration":true,
        "peerReferences":peer_count,
        "generalizationEvidenceSource":"isolated_paired_cross_domain_holdout_v1",
        "taskQualityPrior":task_prior,
        "qualityPredictorCalibration":quality_calibration,
        "interactionGraphSha256":interaction["sha256"],
        "principleLearningSha256":learned_principles["sha256"],
        "intergenerationalGraphSha256":transfer["sha256"],
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
/// Slow-timescale feedback is allowed to change WHAT the next compiled
/// system improves, including revisiting a prior capability after a new
/// complementary one is independently adopted. The algorithm lives here,
/// in the same source that the descendant can rewrite.
pub(crate) fn steer_reciprocal(mut policy:Value,program:&Value)->Value {
    if !matches!(program["phase"].as_str(),Some("complement"|"return")){
        return policy;
    }
    let Some(target)=policy["alternatives"].as_array().into_iter().flatten()
        .find(|candidate|candidate["target"]["path"]==program["target"]["path"]
            && candidate["target"]["role"]==program["target"]["role"]
            && candidate["target"]["sourceSha256"]==program["target"]["sourceSha256"])
        .cloned() else{return policy};
    policy["target"]=target["target"].clone();
    policy["prediction"]=target.clone();
    policy["forecast"]["probability"]=target["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=target["uncertainty"].clone();
    policy["selectionMode"]=json!("reciprocal_descendant_capability_transfer");
    policy["mutualProgramRevision"]=program["revision"].clone();
    policy["mutualProgramPhase"]=program["phase"].clone();
    let mut unsigned=policy.clone();
    if let Some(map)=unsigned.as_object_mut(){map.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// The descendant can prioritize a missing frozen A-only/B-only/AB
/// intervention rather than replaying a confounded success story. This
/// switches the next *source-edit target*, not the independent evaluation.
pub(crate) fn steer_factorial(mut policy:Value,experiment:&Value)->Value {
    let variant=&experiment["selected"];
    if !matches!(variant["phase"].as_str(),
        Some("isolate_left"|"isolate_right"|"test_joint_interaction")) {
        return policy;
    }
    let target=&variant["target"];
    let Some(path)=target["path"].as_str() else{return policy};
    if !crate::engine::rsi::safe_path(path){return policy;}
    let matching=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned();
    let mut prediction=matching.unwrap_or_else(||json!({
        "target":target,"predictedAcceptance":0.5,
        "uncertainty":0.5,"observedTrials":0,
        "source":"unmeasured_factorial_experiment_prior"
    }));
    prediction["target"]=target.clone();
    policy["target"]=target.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("matched_factorial_experiment_design");
    policy["factorialExperimentSha256"]=experiment["sha256"].clone();
    policy["factorialPhase"]=variant["phase"].clone();
    // A signed descendant decision is passed unchanged to the resident
    // supervisor; the supervisor does not recompute its target ranking.
    let mut unsigned=policy.clone();
    if let Some(body)=unsigned.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// Recursion is a program over real compiled descendants, not a fixed
/// number of high scores. The current successor can rewrite this selector
/// and the separate reflexive curriculum as ordinary Rust source.
pub(crate) fn steer_reflexive(mut policy:Value,program:&Value)->Value {
    let Some(phase)=program["phase"].as_str() else{return policy};
    if !matches!(phase,"seed"|"transfer"|"return"|
        "redesign_experiment"|"execute_revised_experiment"){return policy;}
    let selected=&program["target"];
    let Some(path)=selected["path"].as_str() else{return policy};
    if !crate::engine::rsi::safe_path(path) {return policy;}
    let prediction=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*selected).cloned()
        .unwrap_or_else(||json!({
            "target":selected,"predictedAcceptance":0.5,
            "uncertainty":0.5,"observedTrials":0,
            "source":"inherited_reflexive_curriculum_prior"
        }));
    policy["target"]=selected.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("recompiled_reflexive_method_evolution");
    policy["reflexivePhase"]=program["phase"].clone();
    policy["reflexiveProgramSha256"]=program["sha256"].clone();
    policy["reflexiveRootGenerationId"]=program["rootGenerationId"].clone();
    if phase=="execute_revised_experiment" {
        policy["revisedExperimentPlanSha256"]=program["experimentPlanSha256"].clone();
    }
    let mut unsigned=policy.clone();
    if let Some(obj)=unsigned.as_object_mut(){obj.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// The accepted model-authored program graph is not a fixed stage list.
/// The current recompiled meta-controller executes its selected node as a
/// real source-editing assignment. Its policy and graph-selection logic are
/// ordinary Rust and can themselves be rewritten by a later descendant.
pub(crate) fn steer_open_program(mut policy:Value,graph:&Value)->Value {
    if graph["source"]!="compiled_descendant_open_improvement_program_v1" {
        return policy;
    }
    let target=&graph["target"];
    let Some(path)=target["path"].as_str() else{return policy};
    if !crate::engine::rsi::safe_path(path){return policy;}
    let prediction=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,"uncertainty":0.5,
            "observedTrials":0,"source":"model_authored_graph_exploration_prior"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("authored_open_improvement_program");
    policy["openImprovementRevision"]=graph["revision"].clone();
    policy["openImprovementProgramSha256"]=graph["programSha256"].clone();
    policy["openImprovementNodeId"]=graph["nodeId"].clone();
    let mut unsigned=policy.clone();
    if let Some(body)=unsigned.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// Learner-on-learner feedback. Repeated independent forecasting errors
/// trigger a source-edit assignment for the LEARNING ALGORITHM itself,
/// rather than adding another predetermined recursive stage.
pub(crate) fn steer_learner_repair(mut policy:Value,ledger:&Value,
    frontier:&[Value],allowed:&[&str])->Value {
    if ledger["predictiveCalibration"]["needsLearnerSourceRevision"]!=true {
        return policy;
    }
    let target=frontier.iter().find(|row|
        row["role"]=="improvement_methods"
        && row["path"]=="src/engine/rsi_principle_learning.rs"
        && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))
        && row["sourceSha256"].as_str().is_some_and(|s|s.len()==64));
    let Some(target)=target else{return policy};
    let prediction=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,
            "uncertainty":0.5,"observedTrials":0,
            "source":"independent_meta_calibration_repair_hypothesis"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("verified_calibration_driven_learner_rewrite");
    policy["principleLearningSha256"]=ledger["sha256"].clone();
    policy["learnerDiagnostic"]=ledger["predictiveCalibration"].clone();
    let mut copy=policy.clone();
    if let Some(body)=copy.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&copy).unwrap_or_default())));
    policy
}
/// Rewrite the descendant's actual forecast selection algorithm when its
/// precommitted task-quality predictions repeatedly miss independent outcomes.
/// This is an editable source assignment; the external evaluator stays frozen.
pub(crate) fn steer_quality_predictor_repair(mut policy:Value,history:&[Value],
    frontier:&[Value],allowed:&[&str])->Value {
    let diagnostic=quality_forecast_calibration(history);
    if diagnostic["needsPredictorSourceRevision"]!=true {return policy;}
    // Repeated predictor rewrite without measured descendants wastes paid
    // tokens. Give the previous policy revision two episodes to be evaluated.
    if history.iter().rev().take(2).any(|g|
        g["metaPolicy"]["selectionMode"]=="prequential_task_quality_predictor_repair") {
        return policy;
    }
    let Some(target)=frontier.iter().find(|row|
        row["role"]=="improvement_methods"
            && row["path"]=="src/engine/rsi_meta_controller.rs"
            && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))
            && row["sourceSha256"].as_str().is_some_and(|sha|
                sha.len()==64 && sha.bytes().all(|c|c.is_ascii_hexdigit())))
        else{return policy};
    let selected=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,
            "uncertainty":0.5,"observedTrials":0,
            "source":"frozen_task_forecast_calibration_hypothesis"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=selected.clone();
    policy["forecast"]["probability"]=selected["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=selected["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("prequential_task_quality_predictor_repair");
    policy["qualityCalibration"]=diagnostic;
    policy["predictorSource"]=json!("src/engine/rsi_meta_controller.rs");
    let mut unsigned=policy.clone();
    if let Some(body)=unsigned.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
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
        && policy["forecast"]["taskCaseGainProbability"].as_f64()
            .is_some_and(|p|p.is_finite() && (0.0..=1.0).contains(&p))
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
         Cross-domain holdout observations (isolated edits only): {}.\n\
         Precommitted task-quality forecast={} (distinct from adoption).\n\
         Predictor calibration={} (source revision is an experiment, not proof).\n\
         Your OWN src/engine/rsi_meta_controller.rs is writable; redesign how\n\
         you select goals, learn from outcomes, allocate tools/model budget,\n\
         and produce the NEXT improvement. The goal is better offspring,\n\
         not merely a patch to a product component. A submitted design is\n\
         not an adoption; modify real source and preserve frozen evaluation.",
        policy["revision"],policy["selectionMode"],target["role"],target["domain"],
        target["path"],policy["forecast"]["probability"],
        policy["forecast"]["uncertainty"],failures,reproductive,
        policy["prediction"]["crossDomainEvidence"],
        policy["forecast"]["taskCaseGainProbability"],
        policy["qualityPredictorCalibration"]
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
    let task_cases=heldout_quality(generation);
    let task_prob=policy["forecast"]["taskCaseGainProbability"].as_f64()
        .filter(|p|p.is_finite() && (0.0..=1.0).contains(p));
    let task_error=task_prob.filter(|_|!task_cases.is_empty()).map(|p|
        task_cases.iter().map(|(gain,_)|
            (p-f64::from(u8::from(*gain))).powi(2)).sum::<f64>()
            /task_cases.len() as f64);
    json!({
        "taskQualityObservations":task_cases.len(),
        "taskQualityImprovedCases":task_cases.iter().filter(|(gain,_)|*gain).count(),
        "taskQualityRegressedCases":task_cases.iter().filter(|(_,loss)|*loss).count(),
        "taskQualityBrierError":task_error,
        "precommittedTaskQualityProbability":task_prob,
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
        "crossDomainEvidenceAtSelection":policy["prediction"]["crossDomainEvidence"],
        "modelWritten":generation["candidateOrigin"]=="model_driven_detached_worktree",
        "retrainNextCompiledController":true,
        "successForAnyModel":"independent_evidence_only"
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_holdout_loss_changes_the_descendant_search_prior() {
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":1000}),
            json!({"role":"tools","domain":"tools",
                "path":"src/engine/tools.rs","sourceBytes":1000})
        ];
        let baseline=synthesize(&[],&frontier,&["memory","tools"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        let rejected=json!({
            "id":"evaluated","candidateId":"candidate-a",
            "generatorCommit":"a".repeat(40),
            "pairedMeasured":true,"pairedRevalidated":false,"accepted":false,
            "metaPolicy":{"target":{"role":"memory"}},
            "changedSourcePaths":["src/engine/memory.rs"],
            "evaluation":{"metrics":{"eligible":false,"verdict":"regressed",
                "regressions":1,"measurements":[
                    {"domain":"tools","holdout":true,"baselinePass":true,
                        "candidatePass":false,"metrics":[]},
                    {"domain":"recall","holdout":true,"baselinePass":true,
                        "candidatePass":false,"metrics":[]},
                    {"domain":"tools","holdout":false,"baselinePass":true,
                        "candidatePass":false,"metrics":[]}
                ]}}
        });
        let after=synthesize(&[rejected],&frontier,&["memory","tools"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        let memory=after["alternatives"].as_array().unwrap().iter()
            .find(|row|row["target"]["role"]=="memory").unwrap();
        let evidence=&memory["crossDomainEvidence"];
        assert_eq!(evidence["observedCases"],1);
        assert_eq!(evidence["observedRegressions"],1);
        assert!(memory["crossDomainGeneralizationAdjustment"]
            .as_f64().unwrap()<0.0);
        assert!(valid_policy(&after,1));
        assert!(valid_policy(&baseline,0));
    }
    #[test]
    fn joint_and_unpaired_source_changes_cannot_claim_generalization() {
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":10}),
            json!({"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceBytes":10})
        ];
        let observed=json!({
            "candidateId":"joint","generatorCommit":"b".repeat(40),
            "pairedMeasured":true,
            "changedSourcePaths":["src/engine/memory.rs","src/engine/learning.rs"],
            "evaluation":{"metrics":{"eligible":false,"verdict":"regressed",
                "measurements":[{"domain":"tools","holdout":true,
                    "baselinePass":true,"candidatePass":false}]}}
        });
        let confounded=generalization_evidence(
            &[observed.clone()],&frontier,"memory","src/engine/memory.rs","memory");
        assert_eq!(confounded["observedCases"],0);
        let mut unpaired=observed;
        unpaired["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        unpaired["pairedMeasured"]=json!(false);
        let untrusted=generalization_evidence(
            &[unpaired],&frontier,"memory","src/engine/memory.rs","memory");
        assert_eq!(untrusted["observedCases"],0);
    }
    #[test]
    fn independent_task_forecast_error_targets_its_own_real_source() {
        let mut history=Vec::new();
        for i in 0..4 {
            let revision="a".repeat(64);
            history.push(json!({
                "id":format!("prequential-{i}"),
                "candidateId":format!("forecast-{i}"),
                "generatorCommit":"b".repeat(40),
                "evaluationProfileSha256":"c".repeat(64),
                "pairedMeasured":true,"accepted":false,
                "changedSourcePaths":["src/engine/memory.rs"],
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                    "metrics":{"eligible":false,"verdict":"measured",
                        "measurements":[{"id":format!("heldout-{i}"),
                            "domain":"memory","holdout":true,
                            "baselinePass":false,"candidatePass":false}]}},
                "metaPolicy":{"revision":revision,
                    "target":{"role":"memory"},
                    "forecast":{"taskCaseGainProbability":0.95}},
                "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                    "generatorCommit":"b".repeat(40),"metaRevision":revision}
            }));
        }
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":50,
                "sourceSha256":"e".repeat(64)}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_meta_controller.rs",
                "sourceSha256":"f".repeat(64),"sourceBytes":100})
        ];
        let base=synthesize(&history,&frontier,&["memory","improver"],
            None,&json!({}),&json!({}),&json!({"edges":[]}));
        assert!(valid_policy(&base,history.len()));
        let repaired=steer_quality_predictor_repair(
            base,&history,&frontier,&["memory","improver"]);
        assert_eq!(repaired["selectionMode"],"prequential_task_quality_predictor_repair");
        assert_eq!(repaired["target"]["path"],"src/engine/rsi_meta_controller.rs");
        assert_eq!(repaired["qualityCalibration"]["observations"],4);
        assert_eq!(repaired["qualityCalibration"]["independentMetaABProven"],false);
        assert!(valid_policy(&repaired,history.len()));
        history[0]["pairedMeasured"]=json!(false);
        let insufficient=quality_forecast_calibration(&history);
        assert_eq!(insufficient["needsPredictorSourceRevision"],false);
    }
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
        measured["evaluation"]["metrics"]["verdict"]=json!("regressed");
        measured["pairedMeasured"]=json!(true);
        measured["pairedRevalidated"]=json!(false);
        let measured_feedback=feedback(&policy,&measured);
        assert_eq!(measured_feedback["independentObservation"],false);
        assert!(measured_feedback["brierError"].is_number());
    }
    #[test]
    fn independently_replayed_rejection_lowers_next_success_forecast() {
        let frontier=vec![json!({"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceBytes":900})];
        let base=synthesize(&[],&frontier,&["memory"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        let failed=json!({"metaPolicy":{"target":{"role":"memory"}},
            "pairedMeasured":true,"pairedRevalidated":false,
            "accepted":false,"candidateId":"replayed-rejected-source",
            "evaluation":{"metrics":{"eligible":false,"verdict":"regressed",
                "regressions":1}}});
        let updated=synthesize(&[failed.clone()],&frontier,&["memory"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert_eq!(updated["prediction"]["observedTrials"],1);
        assert!(updated["forecast"]["probability"].as_f64().unwrap()
            <base["forecast"]["probability"].as_f64().unwrap());
        let feedback=feedback(&updated,&failed);
        assert_eq!(feedback["independentObservation"],false);
        assert!(feedback["brierError"].as_f64().is_some_and(|e|e>0.0));
    }
    #[test]
    fn demonstrated_bad_meta_calibration_reassigns_the_real_learning_source(){
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":100}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_principle_learning.rs",
                "sourceSha256":"a".repeat(64),"sourceBytes":1500})
        ];
        let base=synthesize(&[],&frontier,&["memory","improver"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert!(valid_policy(&base,0));
        let ledger=json!({"sha256":"b".repeat(64),
            "predictiveCalibration":{"needsLearnerSourceRevision":true,
                "measuredCases":4,"meanBrierError":0.81}});
        let revised=steer_learner_repair(base.clone(),&ledger,&frontier,
            &["memory","improver"]);
        assert_eq!(revised["selectionMode"],
            "verified_calibration_driven_learner_rewrite");
        assert_eq!(revised["target"]["path"],
            "src/engine/rsi_principle_learning.rs");
        assert_eq!(revised["principleLearningSha256"],ledger["sha256"]);
        assert!(valid_policy(&revised,0));
        let unchanged=steer_learner_repair(base.clone(),
            &json!({"predictiveCalibration":{"needsLearnerSourceRevision":false}}),
            &frontier,&["memory","improver"]);
        assert_eq!(unchanged,base);
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
        let eligible=json!({"metrics":{"eligible":true,"verdict":"improved"}});
        let parent=json!({"id":"p","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"accepted":true,
            "pairedMeasured":true,"pairedRevalidated":true,
            "metaPolicy":{"target":{"role":"learning"}},
            "evaluation":eligible,"candidateOrigin":"model_driven_detached_worktree",
            "systemDesign":{"source":"model_authored_system_design",
                "targetFiles":["src/engine/learning.rs"]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"c".repeat(64)}});
        let child=json!({"id":"c","generatorCommit":"b".repeat(40),
            "acceptedCommit":"d".repeat(40),"accepted":true,
            "pairedMeasured":true,"pairedRevalidated":true,
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
