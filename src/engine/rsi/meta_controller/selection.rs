//! selection stage of the compiled, revisable meta-improvement controller.
//! These observations propose experiments; only frozen independent evaluation may adopt them.
use super::*;

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
        let meta_rewrites=relevant.iter().filter(|g|meta_source_changed(g)).count();
        let productive_meta_rewrites=relevant.iter().filter(|g|
            meta_source_changed(g) && next_generation_productivity(history,g)>0)
            .count();
        let failure_posterior=failure_distribution(&relevant);
        let probability=(adopted as f64+1.0)/(attempts as f64+2.0);
        let epistemic=(probability*(1.0-probability)/(attempts as f64+3.0)).sqrt();
        let coverage=if target["verifiedRedesigns"].as_u64().unwrap_or(0)>0{0.0}else{0.22};
        let inherited_bonus=if inherited_domain==target["domain"].as_str(){0.12}else{0.0};
        let size=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(size/12_000.0).min(1.0);
        let penalty=regressions as f64/(attempts.max(1) as f64);
        let stall=fruitless as f64/(attempts.max(1) as f64);
        let compiler_risk=failure_posterior["compiler_failure"].as_f64().unwrap_or(0.2);
        let regression_risk=failure_posterior["behavior_regression"].as_f64().unwrap_or(0.2);
        let edit_risk=failure_posterior["no_verified_code_candidate"].as_f64().unwrap_or(0.2);
        let clade=(productivity as f64/(adopted.max(1) as f64)).min(3.0);
        // UCB-style uncertainty, Bayesian shrinkage, and descendant
        // productivity provide a bounded, auditable choice. A failed edit is
        // evidence about execution feasibility, not proof the design is bad.
        let score=0.55*probability+0.48*epistemic*calibration
            +0.14*((trials+2.0).ln()/(attempts as f64+1.0)).sqrt()
            +coverage+inherited_bonus+0.14*clade
            +0.08*(productive_meta_rewrites as f64).min(3.0)
            -0.12*penalty-0.07*stall-0.035*cost
            -0.09*compiler_risk-0.06*regression_risk-0.06*edit_risk;
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "sourceSha256":target["sourceSha256"],
            "score":score,"predictedAdoptionProbability":probability,
            "uncertainty":epistemic*calibration,
            "failurePosterior":failure_posterior,
            "attempts":attempts,
            "verifiedModelAdoptions":adopted,"regressions":regressions,
            "editsWithoutCandidate":fruitless,
            "productiveDescendantAdoptions":productivity,
            "verifiedMetaSourceRewrites":meta_rewrites,
            "productiveMetaSourceRewrites":productive_meta_rewrites,
            "newComponent":target["verifiedRedesigns"].as_u64().unwrap_or(0)==0
        }),score));
    }
    ranked.sort_by(|(left,a),(right,b)|
        b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||left["role"].as_str().cmp(&right["role"].as_str())));
    let reverse_step=adopted_reciprocal_reversal(history,frontier);
    let mut winner=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    if let Some((ref reverse_target,_))=reverse_step {
        if let Some((row,_))=ranked.iter().find(|(row,_)|
            row["target"]==*reverse_target){
            winner=row.clone();
        }
    }
    let target=winner["target"].clone();
    let reciprocal=reciprocal_evolution_contract(history,&target,frontier);
    let interaction_model=role_influence_model(history);
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
        inherited_domain,history.len(),reciprocal,reverse_step,interaction_model]);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&serialized).unwrap_or_default()));
    json!({
        "source":"compiled_editable_meta_controller_v1",
        "revision":revision,"historyCount":history.len(),
        "mode":mode,"target":target,"selectionEvidence":winner,
        "reciprocalExperiment":reciprocal,
        "interactionDynamics":interaction_model,
        "reciprocalContinuation":reverse_step.as_ref().map(|(target,from)|
            json!({"source":"verified_descendant_reverse_redesign_v1",
                "nextRole":target["role"],"priorRole":from,
                "sourceSha256":target["sourceSha256"],
                "priorChangeWasAdopted":true,"qualityProven":false}))
            .unwrap_or(Value::Null),
        "alternatives":alternatives,
        "referenceDesigns":reference,"counterexamples":counterexamples,
        "forecast":{"acceptanceProbability":winner["predictedAdoptionProbability"],
            "uncertainty":winner["uncertainty"],
            "failurePosterior":winner["failurePosterior"],
            "calibrationFactor":calibration,
            "qualityProven":false},
        "metaMechanismSource":"src/engine/rsi/meta_controller/selection.rs",
        "metaMechanismModules":[
            "src/engine/rsi/meta_controller/provenance.rs",
            "src/engine/rsi/meta_controller/reciprocal.rs",
            "src/engine/rsi/meta_controller/selection.rs",
            "src/engine/rsi/meta_controller/transfer.rs",
            "src/engine/rsi/meta_controller/feedback.rs"
        ],
        "successorCanRewriteMetaMechanism":true,
        "frozenEvaluatorUnchanged":true
    })
}

