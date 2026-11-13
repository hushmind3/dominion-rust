//! Recompiled reflexive intelligence curriculum: improve the improver,
//! use its *new executable binary* to improve a different subsystem, then
//! let that descendant improve the improver again.  The independent oracle
//! (outside the editable descendant) remains the only adoption authority.
//!
//! This program plans edits and records witnessed lineage transitions.
//! A proposed stage is never a proof of actual learning or capability gain.
use crate::engine::{rsi,rsi_mutual_program};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

// Model-authored experiment design and supervisor proof have different owners.
#[path = "rsi/meta/reflexive_design.rs"]
mod design;
use design::*;
pub(crate) use design::{propose,instruction};
#[path = "rsi/proof/reflexive_evidence.rs"]
mod evidence;
pub(crate) use evidence::{valid,completed,completed_method_cycles};

#[cfg(test)]
mod tests {
    use super::*;
    fn frontier()->Vec<Value> {
        vec![
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_meta_controller.rs",
                "sourceSha256":"a".repeat(64),"sourceAttempts":0}),
            json!({"role":"memory","domain":"memory","path":"src/engine/memory.rs",
                "sourceSha256":"b".repeat(64),"sourceAttempts":0}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_factorial.rs",
                "sourceSha256":"f".repeat(64),"sourceAttempts":0}),
        ]
    }
    fn accepted(id:&str,commit:&str,generator:&str,role:&str,path:&str,
        plan:Value)->Value{
        let mut hashes=serde_json::Map::new();
        hashes.insert(path.to_string(),json!("c".repeat(64)));
        let mut role_files=serde_json::Map::new();
        role_files.insert(role.to_string(),json!([path]));
        let plan_sha=plan["sha256"].clone();
        let phase=plan["phase"].clone();
        json!({"id":id,"jobId":format!("job-{id}"),"candidateId":format!("cand-{id}"),
            "generatorCommit":generator,"acceptedCommit":commit,
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "changedSourcePaths":[path],
            "acceptedSourceHashes":Value::Object(hashes),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":generator,"metaRevision":"d".repeat(64),
                "reflexiveSha256":plan_sha,"reflexivePhase":phase},
            "metaPolicy":{"revision":"d".repeat(64),
                "selectionMode":"recompiled_reflexive_method_evolution",
                "reflexiveProgramSha256":plan["sha256"]},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":generator,"generationJobId":format!("job-{id}"),
                "components":[role],"componentFiles":Value::Object(role_files),
                "targetFiles":[path]},
            "reflexiveProgram":plan})
    }
    #[test]
    fn source_adopted_meta_to_capability_to_meta_is_a_compiled_feedback_loop(){
        let f=frontier();
        let p0=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p0["phase"],"seed");
        assert!(valid(&p0,&[],&f,&"0".repeat(40)));
        let first=accepted("first",&"1".repeat(40),&"0".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        let p1=propose(&[first.clone()],&f,&"1".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p1["phase"],"transfer");
        assert!(valid(&p1,&[first.clone()],&f,&"1".repeat(40)));
        let middle=accepted("middle",&"2".repeat(40),&"1".repeat(40),
            "memory","src/engine/memory.rs",p1);
        let p2=propose(&[first.clone(),middle.clone()],&f,&"2".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p2["phase"],"return");
        assert!(valid(&p2,&[first.clone(),middle.clone()],&f,&"2".repeat(40)));
        let last=accepted("last",&"3".repeat(40),&"2".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p2);
        assert_eq!(completed(&[first,middle,last])["completedLoops"],1);
    }
    #[test]
    fn missing_real_meta_change_cannot_advance_source_cycle(){
        let f=frontier();
        let p0=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        let mut fake=accepted("fake",&"1".repeat(40),&"0".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        fake["changedSourcePaths"]=json!(["src/engine/tools.rs"]);
        let p=propose(&[fake],&f,&"1".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p["phase"],"seed");
        assert_eq!(p["inheritedFromVerifiedParent"],false);
    }
    #[test]
    fn fabricated_executed_meta_sha_is_not_a_real_transfer(){
        let f=frontier();
        let p0=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        let mut first=accepted("first",&"1".repeat(40),&"0".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        first["compiledPlanExecuted"]["reflexiveSha256"]=json!("e".repeat(64));
        let next=propose(&[first],&f,&"1".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(next["phase"],"seed");
    }
    #[test]
    fn repeated_failed_reflexive_edits_release_the_source_target(){
        let f=frontier();
        let initial=propose(&[],&f,&"0".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        let failed=|id:&str|json!({
            "id":id,"jobId":id,"generatorCommit":"0".repeat(40),
            "candidateId":id,"accepted":false,"reflexiveProgram":initial.clone()
        });
        let plan=propose(&[failed("failure-one"),failed("failure-two")],&f,
            &"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        assert!(plan.is_null());
    }
    #[test]
    fn adopted_experiment_rewrite_is_actually_executed_by_next_compiled_generation(){
        let mut f=frontier();
        f.push(json!({"role":"tools","domain":"tools",
            "path":"src/engine/tools.rs","sourceSha256":"c".repeat(64),
            "sourceAttempts":0}));
        let allowed=&["improver","memory","tools"];
        let zero="0".repeat(40);
        let one="1".repeat(40);
        let two="2".repeat(40);
        let three="3".repeat(40);
        let four="4".repeat(40);
        let five="5".repeat(40);
        let p0=propose(&[],&f,&zero,allowed,&Value::Null,&Value::Null);
        let a=accepted("meta-seed",&one,&zero,
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        let p1=propose(&[a.clone()],&f,&one,allowed,&Value::Null,&Value::Null);
        let b=accepted("capability",&two,&one,"memory","src/engine/memory.rs",p1);
        let p2=propose(&[a.clone(),b.clone()],&f,&two,allowed,
            &Value::Null,&Value::Null);
        let c=accepted("meta-return",&three,&two,
            "improvement_methods","src/engine/rsi_meta_controller.rs",p2);
        let ancestors=vec![a.clone(),b.clone(),c.clone()];
        assert_eq!(completed(&ancestors)["completedLoops"],1);
        let p3=propose(&ancestors,&f,&three,allowed,&Value::Null,&Value::Null);
        assert_eq!(p3["phase"],"redesign_experiment");
        assert!(valid(&p3,&ancestors,&f,&three));
        let d=accepted("experiment-redesign",&four,&three,
            "improvement_methods","src/engine/rsi_factorial.rs",p3);
        // A real frozen-case joint intervention provides the newly compiled
        // factorial method with an observed missing isolated arm.
        let joint=json!({"id":"joint-record","candidateId":"joint-candidate",
            "generatorCommit":four,"evaluationProfileSha256":"e".repeat(64),
            "pairedMeasured":true,"accepted":false,
            "changedSourcePaths":["src/engine/memory.rs","src/engine/tools.rs"],
            "evaluation":{"metrics":{"verdict":"measured_non_adoption",
                "measurements":[{"id":"holdout-1","holdout":true,
                    "baselinePass":false,"candidatePass":true}]}}});
        let mut history=vec![a,b,c,d,joint];
        let graph=json!({"sha256":"f".repeat(64),"edges":[]});
        let experiment=crate::engine::rsi_factorial::design(
            &history,&f,&graph,allowed,&four);
        assert_eq!(experiment["selected"]["phase"],"isolate_left");
        assert_eq!(experiment["selected"]["target"]["role"],"memory");
        let p4=propose(&history,&f,&four,allowed,&Value::Null,&experiment);
        assert_eq!(p4["phase"],"execute_revised_experiment");
        assert_eq!(p4["experimentPlanSha256"],experiment["sha256"]);
        assert!(valid(&p4,&history,&f,&four));
        let mut e=accepted("experiment-executed",&five,&four,
            "memory","src/engine/memory.rs",p4);
        e["factorialExperiment"]=experiment.clone();
        e["compiledPlanExecuted"]["factorialSha256"]=experiment["sha256"].clone();
        e["metaPolicy"]["revisedExperimentPlanSha256"]=experiment["sha256"].clone();
        history.push(e.clone());
        assert_eq!(completed_method_cycles(&history)["completedMethodCycles"],1);
        let last=history.len()-1;
        history[last]["compiledPlanExecuted"]["factorialSha256"]=json!("0".repeat(64));
        assert_eq!(completed_method_cycles(&history)["completedMethodCycles"],0,
            "a source patch with an unexecuted revision cannot close the method loop");
    }
    #[test]
    fn source_rebound_or_false_parent_commit_is_rejected(){
        let f=frontier();
        let p=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        let mut stale=p.clone();
        stale["target"]["sourceSha256"]=json!("wrong");
        let mut unsigned=stale.clone();
        unsigned.as_object_mut().unwrap().remove("sha256");
        stale["sha256"]=json!(digest(&unsigned));
        assert!(!valid(&stale,&[],&f,&"0".repeat(40)));
    }
}
