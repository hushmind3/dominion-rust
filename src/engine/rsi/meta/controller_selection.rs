//! Evolvable target selector trained on frozen independent outcomes.
use super::*;

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
