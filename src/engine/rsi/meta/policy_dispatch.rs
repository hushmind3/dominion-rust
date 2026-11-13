//! Pure adapter between an adopted typed meta protocol and the existing
//! editable meta-controller. No access to AppState, evaluator or tool executor.
//! This adapter changes the *actual* compiled target and policy fingerprint.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

pub(crate) fn steer(mut policy:Value,protocol:&Value)->Value {
    if protocol["source"]!="compiled_descendant_recursive_evolution_protocol_v1"
        || !protocol["target"].is_object(){return policy;}
    let target=&protocol["target"];
    let predicted=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,"uncertainty":0.5,
            "observedTrials":0,"source":"unmeasured_evolvable_operator_prior"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=predicted.clone();
    policy["forecast"]["probability"]=predicted["predictedAcceptance"].clone();
    policy["forecast"]["uncertainty"]=predicted["uncertainty"].clone();
    policy["forecast"]["taskCaseGainProbability"]=json!(0.5);
    policy["forecast"]["taskEvidenceCases"]=json!(0);
    policy["forecast"]["taskPredictionSource"]=json!("unmeasured_evolvable_operator_prior");
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["taskQualityPrior"]=json!({
        "source":"unmeasured_evolvable_operator_prior",
        "observedCases":0,"observedGains":0,"observedRegressions":0,
        "probability":0.5,"notAcceptanceProbability":true
    });
    policy["selectionMode"]=json!("authored_recursive_evolution_protocol");
    policy["evolutionProtocolRevision"]=protocol["revision"].clone();
    policy["evolutionProtocolOperatorId"]=protocol["operatorId"].clone();
    policy["evolutionProtocolExperiment"]=protocol["operator"]["experiment"].clone();
    let mut unsigned=policy.clone();
    if let Some(m)=unsigned.as_object_mut(){m.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
