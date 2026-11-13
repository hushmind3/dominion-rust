//! Open-ended, source-grounded intelligence topology authored by adopted
//! descendants. Unlike a fixed role enum, future agents may invent new
//! subsystems AND make those subsystems targets of recursive self-improvement.
//! The evaluator alone decides whether actual source changes can be inherited.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,fs,path::Path};

pub(crate) fn valid_role(role:&str)->bool {
    (3..=48).contains(&role.len())
        && role!="system" && role!="unknown"
        && role.bytes().next().is_some_and(|b|b.is_ascii_lowercase())
        && role.bytes().all(|b|b.is_ascii_lowercase()
            || b.is_ascii_digit() || b==b'_')
}

fn hash_valid(hash:&str)->bool {
    hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())
}

fn source_claims(g:&Value,role:&str)->bool {
    let Some(paths)=g["systemDesign"]["componentFiles"][role].as_array()
        .filter(|v|!v.is_empty()&&v.len()<=12) else{return false};
    paths.iter().all(|path|path.as_str().is_some_and(|p|
        crate::engine::rsi::safe_path(p)
            && g["systemDesign"]["targetFiles"].as_array().is_some_and(|all|
                all.iter().any(|row|row==p))
            && g["acceptedSourceHashes"][p].as_str().is_some_and(hash_valid)))
}

/// A new subsystem becomes a durable intelligence component only when
/// source-level edits were independently accepted by the frozen paired oracle
/// and the executed model design explicitly attributes the actual files.
pub(crate) fn inherited_roles(history:&[Value])->Vec<Value> {
    let mut roles=BTreeMap::<String,Value>::new();
    for g in history {
        if g["accepted"]!=true || g["pairedRevalidated"]!=true
            || g["evaluation"]["metrics"]["eligible"]!=true
            || g["candidateOrigin"]!="model_driven_detached_worktree"
            || g["systemDesign"]["source"]!="model_authored_system_design"
            || g["systemDesign"]["generatorCommit"]!=g["generatorCommit"]
            || g["systemDesign"]["generationJobId"]!=g["jobId"]
            || g["acceptedCommit"].as_str().is_none_or(|s|
                s.len()!=40||!s.bytes().all(|b|b.is_ascii_hexdigit())) {
            continue;
        }
        for proposal in g["systemDesign"]["newComponents"].as_array()
            .into_iter().flatten() {
            let Some(role)=proposal["role"].as_str().filter(|r|valid_role(r))
                else{continue};
            let Some(domain)=proposal["domain"].as_str()
                .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
                else{continue};
            if !source_claims(g,role){continue;}
            let Some(files)=proposal["sourcePaths"].as_array()
                .filter(|files|!files.is_empty()&&files.len()<=4)
                else{continue};
            if !files.iter().all(|file|file.as_str().is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && g["systemDesign"]["componentFiles"][role]
                    .as_array().is_some_and(|rows|rows.iter().any(|r|r==path))
                && g["acceptedSourceHashes"][path].as_str().is_some_and(hash_valid))) {
                continue;
            }
            // An invented module that is never wired into executable agent
            // source is a dead file, not an intelligence capability.
            let Some(wires)=proposal["integrationPaths"].as_array()
                .filter(|w|!w.is_empty()&&w.len()<=4) else{continue};
            if !wires.iter().all(|wire|wire.as_str().is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && !files.iter().any(|f|f==path)
                && g["systemDesign"]["targetFiles"].as_array()
                    .is_some_and(|all|all.iter().any(|p|p==path))
                && g["acceptedSourceHashes"][path].as_str().is_some_and(hash_valid))) {
                continue;
            }
            roles.insert(role.to_string(),json!({
                "role":role,"domain":domain,"purpose":proposal["purpose"],
                "sourcePaths":files,
                "integrationPaths":wires,
                "bornAtCommit":g["acceptedCommit"],
                "bornByModelGeneration":g["jobId"],
                "source":"independently_adopted_new_intelligence_component_v1",
                "qualityProvenForNewRevision":false
            }));
        }
    }
    roles.into_values().take(20).collect()
}

/// The newly compiled agent discovers current descendants of every admitted
/// source module; no historical hash is misrepresented as the present bytes.
pub(crate) fn source_components(root:&Path,history:&[Value])->Vec<Value> {
    let Ok(root)=fs::canonicalize(root) else{return Vec::new()};
    inherited_roles(history).into_iter().filter_map(|role|{
        let mut sources=Vec::new();
        for entry in role["sourcePaths"].as_array().into_iter().flatten().take(4) {
            let path=entry.as_str()?;
            let file=fs::canonicalize(root.join(path)).ok()?;
            if !file.starts_with(&root)||!file.is_file(){return None;}
            let bytes=fs::read(file).ok()?;
            if bytes.len()>2_000_000 {return None;}
            sources.push(json!({"path":path,
                "sha256":format!("{:x}",Sha256::digest(&bytes)),
                "bytes":bytes.len()}));
        }
        if sources.is_empty(){return None;}
        Some(json!({"role":role["role"],"domain":role["domain"],
            "purpose":role["purpose"],"bornAtCommit":role["bornAtCommit"],
            "roleSource":role["source"],"representativeSources":sources}))
    }).collect()
}

/// An accepted design can invent a future two-step recursive agenda even
/// without a hard-coded successorStages array. Both phases are executable
/// assignments selected by later compiled descendants, source-rebound and
/// revalidated on every generation. The child may supersede this plan by
/// writing an improved plan in its own adopted source revision.
pub(crate) fn derived_cycle_stages(design:&Value)->Vec<Value>{
    let mut agenda=Vec::new();
    for edge in design["interactionEdges"].as_array().into_iter().flatten() {
        let Some(a)=edge["fromRole"].as_str().filter(|r|valid_role(r))
            else{continue};
        let Some(b)=edge["toRole"].as_str().filter(|r|valid_role(r)&&*r!=a)
            else{continue};
        let Some(a_path)=edge["fromPath"].as_str()
            .filter(|p|crate::engine::rsi::safe_path(p)) else{continue};
        let Some(b_path)=edge["toPath"].as_str()
            .filter(|p|crate::engine::rsi::safe_path(p)) else{continue};
        let Some(a_domain)=domain_for_role(design,a) else{continue};
        let Some(b_domain)=domain_for_role(design,b) else{continue};
        let mechanism=edge["mechanism"].as_str().unwrap_or("");
        let observable=edge["observable"].as_str().unwrap_or("");
        for (role,domain,path,goal) in [
            (b,b_domain,b_path,format!("Evolve {b} in {b_path} using the adopted {a}->{b} mechanism: {mechanism}. Observe {observable}. Improve the ability to develop {a}.")),
            (a,a_domain,a_path,format!("Use newly developed {b} methods to redesign {a} in {a_path}. Compare successor outcomes against the earlier source-grounded intervention. Observe {observable}."))
        ] {
            agenda.push(json!({"role":role,"domain":domain,"path":path,
                "objective":goal,"learningPolicy":"avoid_failed",
                "toolPolicy":"allow_new_modules",
                "executionPolicy":"explore_then_edit",
                "source":"compiled_from_adopted_mutual_intelligence_design",
                "qualityProven":false}));
            if agenda.len()>=8{return agenda;}
        }
    }
    agenda
}

fn domain_for_role<'a>(design:&'a Value,role:&str)->Option<&'a str>{
    if let Some(novel)=design["newComponents"].as_array()
        .into_iter().flatten().find(|item|item["role"]==role) {
        return novel["domain"].as_str();
    }
    match role{
        "algorithms"|"improvement_methods"=>Some("improver"),
        "harness"=>Some("context"),"memory"=>Some("memory"),
        "learning"|"model_architecture"=>Some("learning"),
        "tools"=>Some("tools"),"frontend"=>Some("ui"),_=>None
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn dynamic_role_names_are_unbounded_by_original_eight(){
        assert!(valid_role("world_model_planner_v2"));
        assert!(valid_role("causal_hypothesis_engine"));
        assert!(!valid_role("../config"));
        assert!(!valid_role("System"));
    }
    #[test]
    fn causal_edges_turn_into_executable_descendant_phases(){
        let d=json!({"interactionEdges":[{
            "fromRole":"memory","toRole":"improvement_methods",
            "fromPath":"src/engine/memory.rs",
            "toPath":"src/engine/rsi/meta/symbiosis.rs",
            "mechanism":"Memory-aware search improves downstream source selection",
            "observable":"improved next-generation holdouts"}]});
        let cycle=derived_cycle_stages(&d);
        assert_eq!(cycle.len(),2);
        assert_eq!(cycle[0]["role"],"improvement_methods");
        assert_eq!(cycle[1]["role"],"memory");
    }
    #[test]
    fn accepted_source_plus_runtime_integration_inherits_a_new_subsystem(){
        let new_path="src/engine/causal_research.rs";
        let runtime="src/engine/mod.rs";
        let generation=json!({
            "accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"jobId":"child-1",
            "acceptedSourceHashes":{"src/engine/causal_research.rs":"c".repeat(64),
                "src/engine/mod.rs":"d".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),
                "generationJobId":"child-1",
                "targetFiles":[new_path,runtime],
                "componentFiles":{"causal_research":[new_path]},
                "newComponents":[{"role":"causal_research","domain":"learning",
                    "purpose":"Test source-grounded theories and improve the agent research algorithm",
                    "sourcePaths":[new_path],"integrationPaths":[runtime]}]}
        });
        let inherited=inherited_roles(&[generation]);
        assert_eq!(inherited.len(),1);
        assert_eq!(inherited[0]["role"],"causal_research");
        assert_eq!(inherited[0]["integrationPaths"][0],runtime);
    }
    #[test]
    fn nonintegrated_module_never_becomes_intelligence(){
        let file="src/engine/unused.rs";
        let g=json!({"accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "generatorCommit":"a".repeat(40),"acceptedCommit":"b".repeat(40),
            "jobId":"worker",
            "acceptedSourceHashes":{"src/engine/unused.rs":"c".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),
                "generationJobId":"worker",
                "targetFiles":[file],
                "componentFiles":{"new_unused":[file]},
                "newComponents":[{"role":"new_unused","domain":"learning",
                    "purpose":"An isolated source file does not constitute an integrated new intelligence",
                    "sourcePaths":[file],"integrationPaths":[]}]}
        });
        assert!(inherited_roles(&[g]).is_empty());
    }
    #[test]
    fn proposed_capability_is_not_inherited_without_paired_acceptance(){
        let g=json!({"accepted":true,"pairedRevalidated":false,
            "systemDesign":{"newComponents":[{"role":"causal_world_model",
                "domain":"learning","sourcePaths":["src/engine/world.rs"]}]}});
        assert!(inherited_roles(&[g]).is_empty());
    }
}
