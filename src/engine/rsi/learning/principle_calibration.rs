//! Calibration of meta-selection prediction against independently measured task cases.
//! Pure history-in / evidence-out; no AppState, evaluator mutation or source promotion.
use super::*;

/// Calibrate the *selection algorithm itself*. Only forecasts that were
/// chosen BEFORE independent case evaluation enter this error signal.
/// Model-authored confidence or source-adoption alone earns no credit.
pub(super) fn predictive_calibration(history:&[Value])->Value {
    let mut results=Vec::<(f64,Value)>::new();
    let mut seen=BTreeSet::new();
    for g in history {
        if !independent_trial(g){continue;}
        let plan=&g["openImprovementProgram"];
        let Some(predicted)=plan["principleSelectionEvidence"]["caseQualityPosterior"]
            .as_f64().filter(|p|p.is_finite()&&(0.0..=1.0).contains(p))
            else{continue};
        if !sha_ok(&plan["principleLearningSha256"])
            || g["principleLearning"]["sha256"]!=plan["principleLearningSha256"] {
            continue;
        }
        let Some(candidate)=g["candidateId"].as_str() else{continue};
        for (domain,id,delta) in cases(g) {
            if !seen.insert((candidate.to_owned(),domain.clone(),id.clone())){continue;}
            let reward=if delta>0{1.0}else{0.0};
            let error=(predicted-reward).powi(2);
            results.push((error,json!({
                "generationId":g["id"],"candidateId":candidate,"domain":domain,
                "caseId":id,"forecast":predicted,"caseImproved":delta>0,
                "brierError":error,"source":"precommitted_compiled_principle_estimate"
            })));
        }
    }
    let trials=results.len();
    let mean=if trials>0 {
        results.iter().map(|x|x.0).sum::<f64>()/trials as f64
    }else{0.0};
    results.sort_by(|a,b|b.0.total_cmp(&a.0));
    json!({
        "source":"independent_case_level_meta_predictor_calibration_v1",
        "measuredCases":trials,"meanBrierError":mean,
        "highErrorCases":results.into_iter().take(6).map(|v|v.1).collect::<Vec<_>>(),
        "needsLearnerSourceRevision":trials>=3 && mean>0.28,
        "recommendsEditableSource":"src/engine/rsi_principle_learning.rs",
        "fixedOracleUnchanged":true,
        "qualityImprovedByRevision":false
    })
}
