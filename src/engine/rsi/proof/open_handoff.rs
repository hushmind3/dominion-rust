//! Protected evidence replay and lineage attestation, separate from editable
//! improvement targeting and learned program synthesis.
use super::*;

pub(crate) fn handoff(history:&[Value])->Value {
    let mut steps=Vec::new();let mut rewrites=Vec::new();
    for descendant in history.iter().filter(|g|
        rsi_mutual_program::adopted_source(g) && executed(g)) {
        let plan=&descendant["openImprovementProgram"];
        let Some(path)=plan["target"]["path"].as_str() else{continue};
        if !descendant["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().any(|p|p.as_str()==Some(path)))
            || descendant["acceptedSourceHashes"][path].as_str().is_none_or(|h|
                h.len()!=64 || !h.bytes().all(|b|b.is_ascii_hexdigit())) {continue;}
        steps.push(json!({
            "generationId":descendant["id"],"originGenerationId":plan["originGenerationId"],
            "nodeId":plan["nodeId"],"programSha256":plan["programSha256"],
            "acceptedCommit":descendant["acceptedCommit"],"changedRole":plan["target"]["role"],
            "adoptedSource":true
        }));
        if plan["originatedFromNewAdoptedDesign"]==true {
            let Some(author)=history.iter().find(|g|
                g["id"]==plan["originGenerationId"]
                && rsi_mutual_program::adopted_source(g)
                && g["acceptedCommit"]==descendant["generatorCommit"]
                && g["systemDesign"]["openProgram"]==plan["program"])
                else{continue};
            let ancestor=&author["openImprovementProgram"]["program"];
            let rewritten_source=author["changedSourcePaths"].as_array()
                .is_some_and(|changed|
                    changed.iter().filter_map(Value::as_str).any(|p|
                        matches!(p,"src/engine/rsi_program_graph.rs"
                            |"src/engine/rsi_meta_controller.rs"
                            |"src/engine/rsi/meta/controller_selection.rs"
                            |"src/engine/rsi/meta/controller_policy.rs"
                            |"src/engine/rsi/meta/evolution.rs"
                            |"src/engine/rsi/meta/composition.rs")
                        && author["acceptedSourceHashes"][p].as_str()
                            .is_some_and(|h|h.len()==64
                                && h.bytes().all(|b|b.is_ascii_hexdigit()))));
            if valid_authored(ancestor)
                && ancestor["revision"]!=plan["programSha256"]
                && rewritten_source {
                rewrites.push(json!({
                    "authorGenerationId":author["id"],
                    "executingGenerationId":descendant["id"],
                    "previousProgramSha256":ancestor["revision"],
                    "newProgramSha256":plan["programSha256"],
                    "newProgramExecutedAndSourceAdopted":true
                }));
            }
        }
    }
    let mut out=json!({
        "source":"observed_open_improvement_program_lineage_v1",
        "independentlyAdoptedProgramSteps":steps.len(),"steps":steps,
        "executedProgramRewrites":rewrites.len(),"rewrites":rewrites,
        "meaning":"executed_source_program_handoffs_not_unbounded_recursive_gain"
    });
    out["sha256"]=json!(sha(&out));out
}

