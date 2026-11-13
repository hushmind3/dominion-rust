//! Evidence-weighted responsibility assignment for recursive intelligence redesign.
//!
//! A jointly adopted patch is evidence for the WHOLE changed system, not an
//! independent win for every role named by its author. The compiled successor
//! uses the source paths in the independently revalidated generation receipt
//! to distribute exploration credit without promoting guesses to causal proof.
//! This program is itself part of the editable descendant source.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn sha256(s:&str)->bool {
    s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn commit_id(s:&str)->bool {
    s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn verified(g:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
        && g["acceptedCommit"].as_str().is_some_and(commit_id)
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
}
fn declared_roles(g:&Value)->Vec<String> {
    let design=&g["systemDesign"];
    let names=if design["component"]=="system" {
        design["components"].as_array().cloned().unwrap_or_default()
    } else {
        vec![design["component"].clone()]
    };
    names.iter().filter_map(Value::as_str)
        .filter(|r|crate::engine::rsi_open_topology::valid_role(r))
        .map(str::to_string).collect::<BTreeSet<_>>().into_iter().collect()
}
fn grounded_roles(g:&Value)->Vec<String> {
    if !verified(g) { return Vec::new(); }
    let Some(changed)=g["changedSourcePaths"].as_array()
        .filter(|rows|!rows.is_empty()&&rows.len()<=24) else {return Vec::new()};
    let mut changed_paths=BTreeSet::<&str>::new();
    for path in changed {
        let Some(path)=path.as_str().filter(|p|crate::engine::rsi::safe_path(p))
            else{return Vec::new()};
        if !changed_paths.insert(path)
            || !g["acceptedSourceHashes"][path].as_str().is_some_and(sha256) {
            return Vec::new();
        }
    }
    let design=&g["systemDesign"];
    let Some(targets)=design["targetFiles"].as_array()
        .filter(|rows|!rows.is_empty() && rows.len()<=24)
        else{return Vec::new()};
    if !targets.iter().all(|row|row.as_str().is_some_and(|path|
        changed_paths.contains(path) && g["acceptedSourceHashes"][path]
            .as_str().is_some_and(sha256))) {
        return Vec::new();
    }
    // A model cannot gain role credit by naming a module it did not edit.
    // Every credited role must own at least one independently adopted path.
    declared_roles(g).into_iter().filter(|role|{
        let files=design["componentFiles"][role.as_str()].as_array()
            .filter(|paths|!paths.is_empty())
            .or_else(||(design["component"]==role.as_str()
                && design["componentFiles"].is_null())
                .then(||design["targetFiles"].as_array()).flatten());
        let Some(files)=files else{return false};
        files.iter().all(|path|path.as_str().is_some_and(|p|
            changed_paths.contains(p)
                && targets.iter().any(|target|target.as_str()==Some(p))
                && g["acceptedSourceHashes"][p]
                    .as_str().is_some_and(sha256)))
    }).collect()
}

/// Assign a limited unit of evidence to each validated adoption. If a
/// generation edits N grounded intelligence roles, no one role inherits N
/// independent wins. This is ACCOUNTING for search, not causal attribution.
/// A subsequent matched isolation provides a stronger, still observational,
/// basis for which roles merit exploration.
pub(crate) fn credit(history:&[Value],role:&str)->Value {
    let mut mass=0.0f64;
    let mut verified_roles=0usize;
    let mut isolated=0usize;
    let mut mixed=0usize;
    let mut matched_isolations=0usize;
    let mut receipts=Vec::<Value>::new();
    let mut seen=BTreeSet::<String>::new();
    for g in history {
        let roles=grounded_roles(g);
        if !roles.iter().any(|r|r==role){continue;}
        let Some(candidate)=g["candidateId"].as_str() else{continue};
        if !seen.insert(candidate.to_string()){continue;}
        verified_roles+=1;
        mass+=1.0/(roles.len() as f64);
        let single=roles.len()==1;
        if single {isolated+=1;}else{mixed+=1;}
        let matched=single && g["factorialMeasured"]==true
            && g["factorialCompliance"]["eligibleForFactorial"]==true
            && g["comparisonProfileSha256"].as_str().is_some_and(sha256)
            && g["factorialExperiment"]["generatorCommit"]==g["generatorCommit"]
            && g["factorialExperiment"]["comparisonProfileSha256"]
                ==g["comparisonProfileSha256"]
            && matches!(g["factorialCompliance"]["phase"].as_str(),
                Some("isolate_a"|"isolate_b"));
        if matched{matched_isolations+=1;}
        if receipts.len()<12 {
            receipts.push(json!({"candidateId":candidate,
                "acceptedCommit":g["acceptedCommit"],"groundedRoles":roles,
                "fractionalCredit":1.0/(roles.len() as f64),
                "isolated":single,"matchedFactorial":matched}));
        }
    }
    let mixed_fraction=if verified_roles==0 {0.0}
        else{mixed as f64/verified_roles as f64};
    // Do not use this debt for acceptance, only for the NEXT experiment
    // selection. Even an isolated source edit is not proof of causality.
    let debt=mixed>0 && matched_isolations==0;
    let mut output=json!({
        "source":"revalidated_source_role_credit_v1",
        "role":role,"verifiedRoleAdoptions":verified_roles,
        "attributedAdoptionMass":mass,
        "singleRoleAdoptions":isolated,"mixedRoleAdoptions":mixed,
        "matchedFactorialIsolations":matched_isolations,
        "ambiguousShare":mixed_fraction,
        "requiresIsolation":debt,
        "receipts":receipts,
        "causalEffectProven":false,
        "mutationMethodEditable":true
    });
    output["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&output).unwrap_or_default())));
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn accepted()->Value{
        let a="a".repeat(64);
        json!({"accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "generatorCommit":"g".repeat(40),"jobId":"job-1",
            "acceptedCommit":"e".repeat(40),"candidateId":"candidate-1",
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"g".repeat(40),"generationJobId":"job-1",
                "component":"system","components":["memory","learning"],
                "targetFiles":["src/engine/memory.rs","src/engine/learning.rs"],
                "componentFiles":{"memory":["src/engine/memory.rs"],
                    "learning":["src/engine/learning.rs"]}},
            "changedSourcePaths":["src/engine/memory.rs","src/engine/learning.rs"],
            "acceptedSourceHashes":{"src/engine/memory.rs":a,
                "src/engine/learning.rs":"b".repeat(64)} })
    }
    #[test]
    fn joint_adoption_does_not_award_two_independent_wins(){
        let g=accepted();
        let m=credit(&[g.clone()],"memory");
        let l=credit(&[g],"learning");
        assert_eq!(m["attributedAdoptionMass"],json!(0.5));
        assert_eq!(l["attributedAdoptionMass"],json!(0.5));
        assert_eq!(m["mixedRoleAdoptions"],1);
        assert_eq!(m["requiresIsolation"],true);
        assert_eq!(m["causalEffectProven"],false);
    }
    #[test]
    fn declared_but_unmodified_role_receives_no_credit(){
        let mut g=accepted();
        g["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        g["systemDesign"]["targetFiles"]=json!(["src/engine/memory.rs"]);
        assert_eq!(credit(&[g],"learning")["verifiedRoleAdoptions"],0);
    }
    #[test]
    fn single_matched_isolation_is_different_from_joint_evidence(){
        let mut g=accepted();
        g["systemDesign"]["components"]=json!(["memory"]);
        g["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        g["systemDesign"]["targetFiles"]=json!(["src/engine/memory.rs"]);
        g["comparisonProfileSha256"]=json!("c".repeat(64));
        g["factorialExperiment"]=json!({"generatorCommit":"g".repeat(40),
            "comparisonProfileSha256":"c".repeat(64)});
        g["factorialMeasured"]=json!(true);
        g["factorialCompliance"]=json!({"eligibleForFactorial":true,
            "phase":"isolate_a"});
        let c=credit(&[g],"memory");
        assert_eq!(c["attributedAdoptionMass"],json!(1.0));
        assert_eq!(c["matchedFactorialIsolations"],1);
        assert_eq!(c["requiresIsolation"],false);
    }
    #[test]
    fn unverifiable_adoption_is_not_training_evidence(){
        let mut g=accepted();
        g["pairedRevalidated"]=json!(false);
        assert_eq!(credit(&[g],"memory")["verifiedRoleAdoptions"],0);
    }
}
