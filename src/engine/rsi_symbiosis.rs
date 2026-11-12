//! A compiled descendant's self-rewriteable cross-intelligence curriculum.
//! Observed A->B outcomes are not causal proof; they are falsifiable next
//! experiments. The frozen independent oracle alone promotes source edits.
//!
//! Co-evolve the code that solves tasks with the code that makes subsequent
//! code improvements: capability A -> complementary B -> better A.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;

fn valid_adoption(row:&Value)->bool {
    row["accepted"]==true && row["pairedRevalidated"]==true
        && row["candidateOrigin"]=="model_driven_detached_worktree"
        && row["evaluation"]["metrics"]["eligible"]==true
        && row["systemDesign"]["source"]=="model_authored_system_design"
        && row["systemDesign"]["generatorCommit"]==row["generatorCommit"]
        && row["systemDesign"]["generationJobId"]==row["jobId"]
        && row["acceptedCommit"].as_str().is_some_and(|s|
            s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit()))
}

fn implemented_roles(row:&Value)->Vec<String>{
    if !valid_adoption(row){return Vec::new();}
    let d=&row["systemDesign"];
    let names=if d["component"]=="system"{
        d["components"].as_array().cloned().unwrap_or_default()
    } else{vec![d["component"].clone()]};
    names.iter().filter_map(Value::as_str).filter(|role|{
        let Some(paths)=d["componentFiles"][*role].as_array()
            .filter(|paths|!paths.is_empty()) else{return false;};
        paths.iter().all(|p|p.as_str().is_some_and(|path|
            d["targetFiles"].as_array().is_some_and(|all|
                all.iter().any(|file|file.as_str()==Some(path)))
            && row["acceptedSourceHashes"][path].as_str()
                .is_some_and(|hash|hash.len()==64
                    && hash.bytes().all(|b|b.is_ascii_hexdigit()))))
    }).map(str::to_owned).collect()
}

fn attempted_role(row:&Value)->Option<&str>{
    if row["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"{
        return None;
    }
    row["metaPolicy"]["target"]["role"].as_str()
}

/// Extract directional opportunities only when a genuinely compiled child
/// attempted to improve a different component after its adopted ancestor.
/// Report attempted/adopted transitions as observations, never as causality.
pub(crate) fn transition_graph(history:&[Value])->Value{
    let mut counts=BTreeMap::<(String,String),(u64,u64,u64)>::new();
    for parent in history.iter().filter(|row|valid_adoption(row)){
        let Some(commit)=parent["acceptedCommit"].as_str() else{continue};
        for descendant in history.iter().filter(|row|
            row["generatorCommit"].as_str()==Some(commit)){
            let Some(to)=attempted_role(descendant) else{continue};
            let successful=valid_adoption(descendant)
                && implemented_roles(descendant).iter().any(|role|role==to);
            for from in implemented_roles(parent){
                if from==to{continue;}
                let entry=counts.entry((from,to.to_string())).or_default();
                entry.0+=1;
                if successful{entry.1+=1;}
                if descendant["candidateId"].is_string(){entry.2+=1;}
            }
        }
    }
    let edges=counts.iter().map(|((from,to),(attempts,adopted,edited))|
        json!({"from":from,"to":to,"attempts":attempts,
            "nextGenerationAdoptions":adopted,"codeEditAttempts":edited,
            "observationalOnly":true,
            "posterior":(*adopted as f64+1.0)/(*attempts as f64+2.0)})
    ).collect::<Vec<_>>();
    json!({"source":"compiled_cross_capability_observation_graph_v1",
        "edges":edges,"hasMeasuredCausalInterventions":false})
}

fn score_partner(primary:&Value,other:&Value,graph:&Value)->Option<(f64,Value)>{
    let a=primary["role"].as_str()?;
    let b=other["role"].as_str()?;
    if a==b||primary["path"]==other["path"] {return None;}
    let forward=graph["edges"].as_array().into_iter().flatten()
        .find(|edge|edge["from"]==a&&edge["to"]==b);
    let backward=graph["edges"].as_array().into_iter().flatten()
        .find(|edge|edge["from"]==b&&edge["to"]==a);
    let f=forward.and_then(|row|row["posterior"].as_f64()).unwrap_or(0.5);
    let r=backward.and_then(|row|row["posterior"].as_f64()).unwrap_or(0.5);
    let observed=forward.and_then(|row|row["attempts"].as_u64()).unwrap_or(0)
        +backward.and_then(|row|row["attempts"].as_u64()).unwrap_or(0);
    // Proven gains may increase *exploration* priority, never create a
    // new success label for a pair that was not itself independently tested.
    let shared=if b=="improvement_methods"||a=="improvement_methods"{0.26}else{0.06};
    let unexplored=if other["verifiedRoleImproved"]!=true{0.17}else{0.0};
    let novelty=0.11/(1.0+observed as f64);
    let score=0.35*f+0.35*r+shared+unexplored+novelty;
    Some((score,json!({"source":"compiled_symbiotic_pair_candidate_v1",
        "fromRole":a,"toRole":b,"fromPath":primary["path"],
        "toPath":other["path"],"fromSha256":primary["sourceSha256"],
        "toSha256":other["sourceSha256"],
        "bidirectionalOpportunity":true,"selectionScore":score,
        "forwardObservedPosterior":f,"reverseObservedPosterior":r,
        "recordedTransitionAttempts":observed,
        "causalEffectProven":false})))
}

/// The compiled child's reciprocal search policy links a task subsystem
/// and a separate improver/harness/memory/learning/model subsystem, with an
/// explicit planned return to the original role. Model-written source edits
/// and the next independently adopted descendant determine any real gain.
pub(crate) fn curriculum(
    history:&[Value],frontier:&[Value],meta:&Value,allowed:&[&str]
)->Value{
    let graph=transition_graph(history);
    let primary=&meta["target"];
    let mut candidates=frontier.iter().filter(|item|
        item["domain"].as_str().is_some_and(|domain|allowed.contains(&domain)))
        .filter_map(|other|score_partner(primary,other,&graph))
        .collect::<Vec<_>>();
    candidates.sort_by(|a,b|
        b.0.total_cmp(&a.0).then_with(||a.1["toRole"].as_str()
            .cmp(&b.1["toRole"].as_str())));
    let best=candidates.first().map(|(_,row)|row.clone())
        .unwrap_or(Value::Null);
    let loop_plan=if best.is_object(){
        json!([{
            "phase":"mutate_primary","role":best["fromRole"],
            "source":best["fromPath"],"sha256":best["fromSha256"]
        },{
            "phase":"make_complementary_capability_improve_primary",
            "role":best["toRole"],"source":best["toPath"],
            "sha256":best["toSha256"]
        },{
            "phase":"transfer_improved_method_back_to_primary",
            "role":best["fromRole"],"source":best["fromPath"],
            "sourceRebindRequired":true
        }])
    }else{json!([])};
    let digest=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([primary,best,&graph,&loop_plan]))
            .unwrap_or_default()));
    json!({"source":"compiled_mutual_intelligence_curriculum_v1",
        "revision":digest,
        "primary":primary,"complement":best,
        "recursiveLoop":loop_plan,"transitionEvidence":graph,
        "candidatePairsConsidered":candidates.len(),
        "bothRolesMustChangeSourceToClaimCoupledRedesign":true,
        "nextChildMustRebindActualSourceSha256":true,
        "feedbackTarget":"improve_successor_ability_to_improve",
        "qualityProven":false,"causalEffectProven":false})
}

pub(crate) fn instruction(program:&Value)->String{
    let c=&program["complement"];
    if !c.is_object(){return String::new();}
    format!(
        "[CROSS-SUBSYSTEM RECURSIVE INTELLIGENCE EXPERIMENT]\n\
        Your compiled architecture planner selected a new *reciprocal* development\
        hypothesis: {} ({}, sha={}) improves {} ({}, sha={}), and the latter\
        must develop a BETTER method to improve the former in the NEXT\
        compiled generation. Required causal loop: {}.\n\
        Code must implement actual changes in BOTH source modules (or explain\
        why they must be sequenced), not merely append a declaration.\
        Use rsi_system_plan(component=system, components=[...], componentFiles,\
        interactionEdges, successorStages), then grounded source editing tools.\
        Make the upstream intelligence improvement increase downstream\
        generator productivity or reliability. Record observable behavior,\
        counterfactual prediction and failure mechanism. This is a proposed\
        experiment, not an accepted gain.\n",
        c["fromRole"],c["fromPath"],c["fromSha256"],
        c["toRole"],c["toPath"],c["toSha256"],
        program["recursiveLoop"]
    )
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn builds_real_reciprocal_program_without_claiming_causality(){
        let a=json!({"role":"algorithms","domain":"improver",
            "path":"src/engine/algorithms.rs","sourceSha256":"a".repeat(64),
            "verifiedRoleImproved":false});
        let m=json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_meta_controller.rs","sourceSha256":"b".repeat(64),
            "verifiedRoleImproved":false});
        let policy=json!({"target":a});
        let plan=curriculum(&[],&[a,m],&policy,&["improver"]);
        assert_eq!(plan["recursiveLoop"].as_array().unwrap().len(),3);
        assert_eq!(plan["complement"]["toRole"],"improvement_methods");
        assert_eq!(plan["causalEffectProven"],false);
    }
    #[test]
    fn excludes_unrelated_owner_domains(){
        let a=json!({"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceSha256":"a".repeat(64)});
        let b=json!({"role":"harness","domain":"context",
            "path":"src/engine/jobs.rs","sourceSha256":"b".repeat(64)});
        let plan=curriculum(&[],&[a.clone(),b],&json!({"target":a}),&["memory"]);
        assert!(plan["complement"].is_null());
    }
}
