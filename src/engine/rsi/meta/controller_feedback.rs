//! Post-evaluation policy learning from independent candidate receipts.
use super::*;

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

