//! Lineage-bound evidence that an accepted change to a compiled improvement
//! method entered the NEXT descendant and participated in its actual planning.
//! This is a source/behavior observation, never a randomized quality estimate.
use serde_json::{json,Value};

/// Methods on the real child execution path. Merely creating a Rust file, or
/// adding an uncalled helper, is insufficient evidence of inherited control.
pub(super) const EXECUTABLE_METHOD_SOURCES:&[&str]=&[
    "src/engine/rsi_meta_controller.rs",
    "src/engine/rsi_meta_controller/peer_transfer.rs",
    "src/engine/rsi_meta_controller/observations.rs",
    "src/engine/rsi/planner/compiled.rs",
    "src/engine/rsi/planner/actions.rs",
    "src/engine/rsi/planner/edit_policy.rs",
    "src/engine/rsi/learning/selection.rs",
    "src/engine/rsi_recursive_discovery.rs",
    "src/engine/rsi_system/planning.rs",
    "src/engine/rsi_system/method_lineage.rs",
];

fn sha256(value:&str)->bool {
    value.len()==64 && value.bytes().all(|byte|byte.is_ascii_hexdigit())
}

fn inventory_hash<'a>(blueprint:&'a Value,path:&str)->Option<&'a str>{
    let source=blueprint["sourceInventory"].as_array()?.iter()
        .find(|row|row["path"]==path)?;
    source["sha256"].as_str().filter(|hash|sha256(hash))
}

/// Input history must FIRST pass revalidated_method_history; caller cannot
/// pass untrusted mutable adopted labels from a stored generation record.
/// The current source ledger comes from the running compiled child's verified
/// blueprint and is matched byte-for-byte with its adopted parent's revision.
pub(crate) fn inherited_method_receipt(
    checked_history:&[Value],blueprint:&Value,policy:&Value,
    generator_commit:&str,actual_focus:&Value,
)->Value {
    if generator_commit.len()!=40
        || !generator_commit.bytes().all(|b|b.is_ascii_hexdigit())
        || blueprint["generatorCommit"]!=generator_commit
        || policy["source"]!="compiled_editable_meta_controller_v1" {
        return Value::Null;
    }
    let Some(parent)=checked_history.iter().rev().find(|row|
        row["accepted"]==true && row["pairedRevalidated"]==true
            && row["candidateOrigin"]=="model_driven_detached_worktree"
            && row["acceptedCommit"]==generator_commit
            && row["systemDesign"]["source"]=="model_authored_system_design"
            && row["systemDesign"]["generatorCommit"]==row["generatorCommit"]
            && row["systemDesign"]["generationJobId"]==row["jobId"])
        else{return Value::Null;};
    let mut changes=Vec::new();
    for &path in EXECUTABLE_METHOD_SOURCES {
        if !parent["systemDesign"]["targetFiles"].as_array()
            .is_some_and(|targets|targets.iter().any(|entry|entry==path)){
            continue;
        }
        let Some(before)=inventory_hash(&parent["systemBlueprint"],path)else{continue;};
        let Some(after)=parent["acceptedSourceHashes"][path].as_str()
            .filter(|hash|sha256(hash)&&*hash!=before)else{continue;};
        if inventory_hash(blueprint,path)!=Some(after){continue;}
        changes.push(json!({
            "path":path,"parentSourceSha256":before,
            "acceptedSourceSha256":after,"currentCompiledSourceSha256":after
        }));
    }
    if changes.is_empty(){return Value::Null;}
    let old_target=&parent["metaPolicy"]["target"];
    let current_target=&policy["target"];
    let actual_focus_applied=current_target["path"].as_str().is_some_and(|path|
        actual_focus.as_array().is_some_and(|files|
            files.iter().any(|item|item==path)));
    json!({
        "source":"source_bound_compiled_meta_method_inheritance_v1",
        "parentGenerationId":parent["id"],
        "parentAcceptedCommit":parent["acceptedCommit"],
        "parentCandidateId":parent["candidateId"],
        "generatorCommit":generator_commit,
        "sourceChanges":changes,
        "rebuildConfirmed":true,
        "parentPolicyRevision":parent["metaPolicy"]["revision"],
        "compiledChildPolicyRevision":policy["revision"],
        "parentTargetRole":old_target["role"],
        "parentTargetPath":old_target["path"],
        "childTargetRole":current_target["role"],
        "childTargetPath":current_target["path"],
        "childMode":policy["mode"],
        "observedPlanDifference":old_target!=current_target
            || parent["metaPolicy"]["mode"]!=policy["mode"],
        "chosenTargetActuallyFocused":actual_focus_applied,
        "causalMethodSuperiorityProven":false,
        "independentQualityGainProven":false
    })
}

/// Outcome of a rebuilt method's NEXT source-changing generation. Even
/// independently adopted results do not prove the new method caused them.
pub(crate) fn observed_method_outcome(generation:&Value)->Value {
    let inherited=&generation["metaMethodInheritance"];
    if inherited["source"]!="source_bound_compiled_meta_method_inheritance_v1"
        || inherited["rebuildConfirmed"]!=true
        || inherited["generatorCommit"]!=generation["generatorCommit"]
        || inherited["chosenTargetActuallyFocused"]!=true {
        return Value::Null;
    }
    let assessed=generation["evaluation"]["verdict"].is_string()
        && generation["evaluation"]["metrics"]["eligible"].is_boolean();
    let adopted=assessed && generation["accepted"]==true
        && generation["pairedRevalidated"]==true
        && generation["candidateOrigin"]=="model_driven_detached_worktree";
    let descendant_meta_edit=adopted
        && generation["systemDesign"]["targetFiles"].as_array().is_some_and(|paths|
            paths.iter().any(|path|path.as_str().is_some_and(|source|
                EXECUTABLE_METHOD_SOURCES.contains(&source)
                && inventory_hash(&generation["systemBlueprint"],source)
                    .is_some_and(|prior|
                        generation["acceptedSourceHashes"][source].as_str()
                            .is_some_and(|after|sha256(after)&&after!=prior)))));
    json!({
        "source":"observed_compiled_meta_method_successor_outcome_v1",
        "inheritedParentCommit":inherited["parentAcceptedCommit"],
        "inheritedSources":inherited["sourceChanges"],
        "currentGeneratorCommit":generation["generatorCommit"],
        "generationId":generation["id"],
        "compiledPolicyRevision":inherited["compiledChildPolicyRevision"],
        "observedPlanDifference":inherited["observedPlanDifference"],
        "actuallyFocused":true,
        "independentlyAssessed":assessed,
        "independentCandidateAdopted":adopted,
        "childRewroteImprovementMethod":descendant_meta_edit,
        "causalMethodSuperiorityProven":false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_source_method_reaches_actual_next_compiled_selector() {
        let commit="b".repeat(40);
        let path="src/engine/rsi_meta_controller/peer_transfer.rs";
        let parent=json!({
            "id":"generation-1","candidateId":"candidate-1",
            "generatorCommit":"a".repeat(40),"acceptedCommit":commit,
            "accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "jobId":"worker-1",
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),"generationJobId":"worker-1",
                "targetFiles":[path]},
            "systemBlueprint":{"sourceInventory":[{
                "path":path,"sha256":"1".repeat(64)}]},
            "acceptedSourceHashes":{
                "src/engine/rsi_meta_controller/peer_transfer.rs":"2".repeat(64)
            },
            "metaPolicy":{"revision":"before","mode":"open_ended_mechanism_discovery",
                "target":{"role":"memory","path":"src/engine/memory.rs"}}
        });
        let current=json!({"generatorCommit":commit,"sourceInventory":[
            {"path":path,"sha256":"2".repeat(64)}]});
        let policy=json!({"source":"compiled_editable_meta_controller_v1",
            "revision":"after","mode":"repair_own_transfer_method",
            "target":{"role":"improvement_methods","path":path}});
        let receipt=inherited_method_receipt(&[parent.clone()],&current,&policy,
            &commit,&json!([path]));
        assert_eq!(receipt["rebuildConfirmed"],true);
        assert_eq!(receipt["observedPlanDifference"],true);
        assert_eq!(receipt["chosenTargetActuallyFocused"],true);
        let generation=json!({"generatorCommit":commit,"metaMethodInheritance":receipt,
            "id":"generation-2","accepted":false,
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}}});
        let outcome=observed_method_outcome(&generation);
        assert_eq!(outcome["independentlyAssessed"],true);
        assert_eq!(outcome["independentCandidateAdopted"],false);
        assert_eq!(outcome["childRewroteImprovementMethod"],false);
        assert_eq!(outcome["causalMethodSuperiorityProven"],false);
        let mut mismatch=current.clone();
        mismatch["sourceInventory"][0]["sha256"]=json!("3".repeat(64));
        assert!(inherited_method_receipt(&[parent],&mismatch,&policy,
            &commit,&json!([path])).is_null());
    }
}
