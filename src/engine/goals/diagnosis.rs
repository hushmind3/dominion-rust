//! Trace-attributed repair hints for real autonomous-goal failures.
//! Classification is advisory: the original tool/verification receipts,
//! not an inferred root cause, determine whether work can be completed.
use serde_json::{json,Value};
use std::collections::BTreeSet;

fn repair_hint(layer:&str)->&'static str {
    match layer{
        "coding_test"=> "Run the repository's actual build/test command after the final source edit. Inspect the exact exit code and repair failing tests.",
        "tool_permission"=> "Inspect the recorded denied operation and the owner's active tool approval configuration before changing the tool or its arguments.",
        "tool_execution"=> "Inspect the failing tool's original arguments and result. Re-read the current source/filesystem before a modified retry.",
        "acceptance"=> "Re-evaluate the exact unmet deterministic predicate against current files and evidence; do not substitute a model assertion.",
        "stagnation"=> "The prior strategy generated no accepted new file bytes or cleared checks. Change the source-edit hypothesis or narrow the next target.",
        _=> "Inspect actual tool receipts before proposing the next bounded edit.",
    }
}

/// Only actual Rust gate failures, tool errors and material-progress signals
/// may generate a diagnosis. Reviewer text is not input to this classifier.
pub(super) fn classify(
    gate_gaps:&[String],tool_errors:&[String],stalled:usize,
    material_progress:bool
)->Value{
    let mut seen=BTreeSet::new();
    let mut observations=Vec::new();
    let mut add=|layer:&str,reason:String|{
        if observations.len()>=5 || !seen.insert(layer.to_string()){return;}
        observations.push(json!({
            "layer":layer,
            "observed":reason.chars().take(420).collect::<String>(),
            "nextProbe":repair_hint(layer),
            "rootCauseProven":false
        }));
    };
    for gap in gate_gaps{
        if gap.contains("실제 빌드")||gap.contains("테스트 실행 증거"){
            add("coding_test",gap.clone());
        }else{
            add("acceptance",gap.clone());
        }
    }
    for error in tool_errors.iter().take(8){
        if error.contains("승인")||error.to_lowercase().contains("permission")
            ||error.contains("권한"){
            add("tool_permission",error.clone());
        }else{
            add("tool_execution",error.clone());
        }
    }
    if !material_progress && stalled>0{
        add("stagnation",format!("{} consecutive turns without measured material progress",stalled));
    }
    json!({
        "source":"actual_goal_tool_and_verification_receipts_v1",
        "observations":observations,
        "modelReviewIsNotEvidence":true,
        "rootCauseProven":false,
        "toolErrorCount":tool_errors.len(),
        "deterministicGateGapCount":gate_gaps.len(),
        "stalledIterations":stalled
    })
}

#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn diagnostics_only_use_verifier_and_tool_receipts(){
    let d=classify(&[
       "마지막 소스 수정 이후 실제 빌드·테스트 실행 증거가 없습니다".into(),
       "file not changed".into(),
    ],&["terminal_exec exit code 1".into()],2,false);
    let classes=d["observations"].as_array().unwrap().iter()
       .filter_map(|row|row["layer"].as_str()).collect::<Vec<_>>();
    assert_eq!(classes,vec!["coding_test","acceptance","tool_execution","stagnation"]);
    assert_eq!(d["rootCauseProven"],false);
    assert_eq!(d["toolErrorCount"],1);
 }
 #[test]
 fn labels_owner_approval_and_no_other_unobserved_causes(){
    let d=classify(&[],&["승인 보류".into(),"permission denied".into()],0,true);
    assert_eq!(d["observations"].as_array().unwrap().len(),1);
    assert_eq!(d["observations"][0]["layer"],"tool_permission");
 }
 #[test]
 fn bounded_noisy_tool_diagnostics_do_not_expand_context(){
    let errors=vec!["fail".repeat(1000);30];
    let d=classify(&[],&errors,10,false);
    assert!(d["observations"].as_array().unwrap().len()<=5);
    assert!(d["observations"][0]["observed"].as_str().unwrap().chars().count()<=420);
 }
}
