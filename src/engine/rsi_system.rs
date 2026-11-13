//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/rsi_worktree.rs","src/engine/chat.rs","src/engine/decompose.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&["src/engine/rsi_open_program.rs","src/engine/rsi_principle_lab.rs","src/engine/rsi_meta_controller.rs","src/engine/rsi_meta_controller/peer_transfer.rs","src/engine/rsi_meta_controller/guidance.rs","src/engine/rsi_meta_controller/observations.rs","src/engine/rsi_recursive_discovery.rs","src/engine/rsi_evolve.rs","src/engine/rsi_mutation.rs","src/engine/rsi_system.rs","src/engine/rsi_successor_productivity.rs","src/engine/rsi_edit_genome.rs","src/engine/rsi_rule_tournament.rs"]),
    ("models",&["src/engine/rsi_model_ecology.rs","src/engine/models.rs","src/engine/cli_model.rs","src/engine/llm.rs","src/engine/router.rs","src/engine/gateway.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

// Unit tests are checked by Cargo, but are not executable intelligence
// methods in a release descendant and must not pollute its editable frontier.
pub(crate) fn runtime_source_file(path:&str)->bool{
    let file=path.rsplit('/').next().unwrap_or(path);
    !file.ends_with("_tests.rs") && file!="tests.rs"
        && !file.ends_with("_fixture.rs")
        && !path.split('/').any(|part|matches!(part,"tests"|"fixtures"))
}

/// Inventory every executable agent/frontend source under the selected
/// experimental revision, not merely a hand-picked optimization fixture.
/// An inventory hash changes when ANY generation changes the executable
/// system's source graph; the prompt can remain short without losing lineage.
fn source_inventory(root:&Path)->Vec<Value> {
    let Ok(root)=fs::canonicalize(root) else{return Vec::new()};
    let mut pending=vec![root.join("scripts"),
        root.join("src-tauri").join("src"),
        root.join("web").join("src"),root.join("src")];
    let mut rows=Vec::new();
    while let Some(dir)=pending.pop(){
        let Ok(entries)=fs::read_dir(&dir) else{continue};
        let mut entries=entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|e|e.file_name());
        let mut children=Vec::new();
        for entry in entries{
            let Ok(path)=fs::canonicalize(entry.path()) else{continue};
            if !path.starts_with(&root){continue;}
            if path.is_dir(){children.push(path);continue;}
            if !path.is_file(){continue;}
            let Some(relative)=path.strip_prefix(&root).ok()
                .and_then(|p|p.to_str()) else{continue};
            if !crate::engine::rsi::safe_path(relative)
                || !runtime_source_file(relative){continue;}
            if !matches!(path.extension().and_then(|e|e.to_str()),
                Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"py")){continue;}
            let Ok(bytes)=fs::read(&path) else{continue};
            if bytes.len()>2_000_000{continue;}
            rows.push(json!({"path":relative,
                "sha256":format!("{:x}",Sha256::digest(&bytes)),
                "bytes":bytes.len()}));
            if rows.len()>=2048{break;}
        }
        for child in children.into_iter().rev(){pending.push(child);}
        if rows.len()>=2048{break;}
    }
    rows.sort_by(|a,b|a["path"].as_str().cmp(&b["path"].as_str()));
    rows
}

/// Cross-component designs are source programs, not prose labels. A bundle
/// has an explicit 1:1 attribution from each changed file to a system role;
/// an unrelated one-file optimization cannot claim six improved capabilities.
/// Adapted from the source-bound coupled redesign in Dominion Sub3/Sub4.
/// Source-file ownership is part of the editable successor policy: the
/// built descendant can extend this mapping when inventing new modules.
fn source_role(path:&str)->&'static str {
    if path.starts_with("web/src/")||path.starts_with("src-tauri/src/"){
        return "frontend";
    }
    let lower=path.to_ascii_lowercase();
    // Submodules owned by the RSI coordinator are policy machinery, not
    // generic "learning" or "algorithms" merely because of their basename.
    if lower.starts_with("src/engine/rsi/"){return "improvement_methods";}
    if lower.starts_with("src/engine/rsi_system/"){
        return "improvement_methods";
    }
    if lower.starts_with("src/engine/rsi_meta_controller/"){
        return "improvement_methods";
    }
    let name=lower.rsplit('/').next().unwrap_or(&lower);
    if name.starts_with("model_")||name.starts_with("train_")
        || name.starts_with("training")||name.starts_with("trainable")
        || matches!(name,"models.rs"|"llm.rs"|"cli_model.rs"|"router.rs"
            |"gateway.rs"|"model_architecture.py"){return "models";}
    // The executable model router is a core inference-intelligence
    // component even though it is also part of the RSI control loop.
    if name=="rsi_model_ecology.rs"{return "models";}
    if name.starts_with("rsi_") || name=="rsi.rs"
        || lower.contains("/self_improv")||lower.contains("/evolution"){
        return "improvement_methods";
    }
    if lower.contains("/chat/")||matches!(name,"jobs.rs"|"decompose.rs"
        |"chat.rs"|"adaptation_loop.rs"|"scheduler.rs"){return "harness";}
    if name.starts_with("memory")||name.starts_with("room_")
        || name.starts_with("recall"){return "memory";}
    if name.starts_with("learning")||name.starts_with("experience")
        || name.starts_with("procedure")||name.starts_with("skill")
        || name.starts_with("agent_growth"){return "learning";}
    if name.starts_with("tools")||name.starts_with("browser")
        || name.starts_with("mcp")||name.starts_with("connector"){return "tools";}
    "algorithms"
}

fn valid_role(role:&str)->bool {
    !role.is_empty() && role.len()<=64
        && role.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_'||c==b'-')
        && role!="system"
}

fn design_roles(design:&Value)->Vec<String> {
    design["components"].as_array().map(|roles|
        roles.iter().filter_map(Value::as_str)
            .filter(|role|valid_role(role)).map(str::to_string).collect()
    ).unwrap_or_else(||design["component"].as_str()
        .filter(|role|valid_role(role))
        .map(|role|vec![role.to_string()])
        .unwrap_or_else(||{
            // Legacy, independently adopted single-file proposals had no
            // component field. Recover only an UNAMBIGUOUS role from the
            // original source map; never guess for a shared or new module.
            let Some(targets)=design["targetFiles"].as_array()
                .filter(|files|files.len()==1) else{return Vec::new();};
            let Some(path)=targets[0].as_str() else{return Vec::new();};
            let matched=COMPONENTS.iter()
                .filter(|(_,files)|files.contains(&path))
                .map(|(role,_)|role.to_string()).collect::<Vec<_>>();
            if matched.len()==1{matched}else{Vec::new()}
        }))
}

fn design_role_files<'a>(design:&'a Value,role:&str)->Option<&'a Vec<Value>> {
    design["componentFiles"][role].as_array().or_else(||
        (design_roles(design).len()==1).then(||design["targetFiles"].as_array()).flatten())
}

/// A bundle is implemented only if every declared file is in the independently
/// measured source diff, with an unchanged design-to-role mapping. No
/// automatic "model says succeeded" or inferred coverage is accepted.
pub(crate) fn design_implemented(design:&Value,changed:&[Value])->bool {
    let Some(targets)=design["targetFiles"].as_array().filter(|files|!files.is_empty())
        else{return false;};
    let roles=design_roles(design);
    if roles.is_empty(){return false;}
    let mut mapped=std::collections::BTreeSet::<String>::new();
    for role in &roles {
        let Some(files)=design_role_files(design,role).filter(|files|!files.is_empty())
            else{return false;};
        for file in files {
            let Some(path)=file.as_str().filter(|p|crate::engine::rsi::safe_path(p))
                else{return false;};
            if !targets.iter().any(|entry|entry.as_str()==Some(path))
                || !changed.iter().any(|entry|entry.as_str()==Some(path))
                || !mapped.insert(path.to_string()) {return false;}
        }
    }
    targets.iter().all(|entry|entry.as_str()
        .is_some_and(|path|mapped.contains(path)))
}

/// The predecessor may leave executable *planning intent* for its compiled
/// successor. The successor is free to reject a stale stage and replan from
/// its own real source graph. This is a task directive, not a proven gain.
pub(crate) fn inherited_stage(
    history:&[Value],frontier:&[Value]
)->Option<Value>{
    // Failed experiments do not erase an adopted model-authored program:
    // the binary still runs the LAST accepted revision. A new accepted
    // revision supersedes the programme, whether or not it authored one.
    let prior=history.iter().rev().find(|g|g["accepted"]==true)?;
    let design=&prior["systemDesign"];
    if prior["accepted"]!=true
        || prior["evaluation"]["metrics"]["eligible"]!=true
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=prior["generatorCommit"]
        || design["generationJobId"]!=prior["jobId"]
        || !design_implemented(design,prior["acceptedSourceHashes"]
            .as_object()?.keys().map(|key|json!(key)).collect::<Vec<_>>().as_slice())
    {return None;}
    let stages=design["successorStages"].as_array()?;
    let previous=history.iter().skip_while(|generation|
        generation["id"]!=prior["id"]).skip(1).collect::<Vec<_>>();
    // A compiled successor schedules the PREDECESSOR'S stages from live
    // outcomes. Repeating a failed source target incurs a cost; a different
    // model-proposed stage may win next, without the resident coordinator
    // inventing the next strategy or claiming a quality gain.
    stages.iter().enumerate().filter_map(|(stage_index,stage)|{
        let role=stage["role"].as_str()?;
        let domain=stage["domain"].as_str()?;
        let path=stage["path"].as_str()?;
        if !crate::engine::rsi_evolve::DOMAINS.contains(&domain){return None;}
        let actual=frontier.iter().find(|row|
            row["path"]==path && row["role"]==role && row["domain"]==domain)?;
        let failures=previous.iter().filter(|g|
            g["accepted"]!=true && g["focusFiles"].as_array()
                .is_some_and(|files|files.iter().any(|f|f.as_str()==Some(path))))
            .count();
        // This is an inherited *control policy*: recorded failures change
        // the next task, rather than merely appearing in the model prompt.
        let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        let tools=stage["toolPolicy"].as_str().unwrap_or("allow_new_modules");
        let execution=stage["executionPolicy"].as_str().unwrap_or("explore_then_edit");
        if !matches!(learning,"avoid_failed"|"retry_failed")
            || !matches!(tools,"patch_only"|"allow_new_modules")
            || !matches!(execution,"grounded_edit"|"explore_then_edit") {
            return None;
        }
        let rank=if learning=="retry_failed" {-(failures as i64)}
            else {failures as i64};
        Some((stage_index,rank,json!({"role":role,"domain":domain,"path":path,
            "sourceSha256":actual["sourceSha256"],
            "objective":stage["objective"],
            "sourceDesignId":design["id"],
            "predecessorCommit":prior["acceptedCommit"],
            "programmeOriginDesignId":design["id"],
            "programmeOriginCommit":prior["acceptedCommit"],
            "programmeStageIndex":stage_index,
            "failedAttemptsSinceAdoption":failures,
            "learningPolicy":learning,
            "toolPolicy":tools,
            "executionPolicy":execution,
            "qualityProven":false})))
    }).min_by_key(|(order,rank,_)|(*order,*rank))
        .map(|(_,_,stage)|stage)
}

/// DGM/AlphaEvolve-style architecture frontier, evaluated by the descendant
/// executable against its OWN current source graph. The resident coordinator
/// receives a proposed next capability, not authority to invent that proposal.
/// Scores schedule exploratory edits only: no item is an adopted gain.
fn architecture_role_domain(role:&str)->Option<&'static str>{
    match role {
        "algorithms"|"improvement_methods"=>Some("improver"),
        "harness"=>Some("context"),
        "memory"=>Some("memory"),
        "learning"=>Some("learning"),
        "models"=>Some("learning"),
        "tools"=>Some("tools"),
        "frontend"=>Some("ui"),
        _=>None,
    }
}

/// A source path may participate in several architectural roles. The
/// resulting frontier makes unmodified subsystems visible to the compiled
/// successor while preserving their original hashes and real index location.
pub(crate) fn architecture_frontier(
    state:&crate::state::AppState,
    blueprint:&Value,history:&[Value],index_id:&str,root:&Path,
)->Vec<Value>{
    use std::collections::{BTreeMap,BTreeSet};
    let mut verified_roles=BTreeSet::<String>::new();
    let mut source_attempts=BTreeMap::<String,u64>::new();
    for generation in history {
        for path in generation["focusFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str){
            *source_attempts.entry(path.to_string()).or_default()+=1;
        }
        if generation["accepted"]!=true
            || generation["pairedRevalidated"]!=true {continue;}
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else {continue;};
        if candidate["status"]!="verified"
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
        // Only credit the explicit, COMPLETELY implemented source-to-role
        // contract. A helper file shared by two seed roles cannot claim
        // that both intelligence subsystems were actually redesigned.
        let design=&generation["systemDesign"];
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        if design["generatorCommit"]!=generation["generatorCommit"]
            || design["generationJobId"]!=generation["jobId"]
            || !design_implemented(design,&changed) {continue;}
        for role in design_roles(design) {
            let Some(files)=design_role_files(design,&role) else{continue;};
            if files.iter().all(|file|file.as_str().is_some_and(|path|{
                let Some(sha)=generation["acceptedSourceHashes"][path].as_str()
                    else{return false;};
                crate::engine::code_index::indexed_source_hash(state,index_id,path)
                    .as_deref()==Some(sha)
            })){
                verified_roles.insert(role);
            }
        }
    }
    let mut frontier=Vec::<Value>::new();
    // Keep the frontier open even when no component has been adopted yet.
    // The editable meta-controller decides priorities; this source scanner
    // must not forbid an intelligence role or a newly invented module.
    for component in blueprint["architecture"].as_array().into_iter().flatten() {
        let Some(role)=component["role"].as_str() else {continue;};
        let Some(domain)=architecture_role_domain(role).or_else(||
            component["domain"].as_str().filter(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(d))) else {continue;};
        let touched=verified_roles.contains(role);
        let mut available=component["representativeSources"].as_array()
            .into_iter().flatten()
            .filter_map(|item|{
                let path=item["path"].as_str()?;
                if !crate::engine::rsi::safe_path(path) {return None;}
                let hash=item["sha256"].as_str()?;
                // The parent's index is a search acceleration, not an
                // immutable allowlist of source languages. Byte-for-byte
                // source verification below remains the adoption boundary.
                if crate::engine::code_index::indexed_source_hash(state,index_id,path)
                    .as_deref().is_some_and(|indexed|indexed!=hash){return None;}
                let bytes=item["bytes"].as_u64()?;
                // The code-index DB is an observation from before the child
                // may have changed source. Re-read the ACTUAL checked-out
                // file so an old index cannot authorise stale descendant edits.
                let real=fs::read(root.join(path)).ok()?;
                if real.len() as u64!=bytes
                    || format!("{:x}",Sha256::digest(&real))!=hash {return None;}
                Some((path,hash,bytes))
            }).collect::<Vec<_>>();
        // Prioritize new systems, cheap-to-inspect source and actual failures.
        // A role's observed experiments are a scheduling cost, never a gain.
        available.sort_by(|a,b|{
            let cost=|p:&str,bytes:u64| source_attempts.get(p).copied().unwrap_or(0)*100
                + bytes/4096;
            cost(a.0,a.2).cmp(&cost(b.0,b.2))
                .then(a.0.cmp(b.0))
        });
        // Keep the CURRENT self-evolving scientific-method source in the
        // actual frontier. Otherwise cost-ranked file caps may hide the very
        // algorithm that an invented intelligence must later re-improve.
        // This changes SOURCE DISCOVERY only, not the self-chosen target rank.
        if role=="improvement_methods" {
            // A size-ranked frontier must not conceal the executable
            // evaluator-error learning law or the program-discovery method.
            // Both are ordinary editable source targets, never privileged
            // for independent adoption or scoring.
            // A source-level method that can be inherited must also remain
            // discoverable after the per-role limit. Share one source list
            // with the actual lineage verifier, never maintain stale copies.
            for anchor in method_lineage::EXECUTABLE_METHOD_SOURCES
                .iter().rev().copied() {
                if let Some(pos)=available.iter().position(|(path,_,_)|
                    *path==anchor) {
                    let method=available.remove(pos);
                    available.insert(0,method);
                }
            }
        }
        // Preserve alternate source targets. The original frontier exposed
        // only ONE representative per role, silently discarding a model's
        // perfectly valid inherited next-stage path whenever another file
        // happened to be smaller. Multiple options give the child an actual
        // architectural search space without pre-authoring an optimization.
        for (path,hash,bytes) in available.into_iter().take(12) {
            let prior=source_attempts.get(path).copied().unwrap_or(0);
            frontier.push(json!({
                "role":role,"domain":domain,"path":path,
                "sourceSha256":hash,"sourceBytes":bytes,
                "verifiedRoleImproved":touched,
                "sourceAttempts":prior,
                "reason":if touched{"extend_prior_verified_component"}
                    else {"unmodified_intelligence_component"},
                "qualityProven":false
            }));
            if frontier.len()>=128 {break;}
        }
    }
    // Bounded, stable portfolio: avoid serializing thousands of model tool
    // inputs when future generations invent many additional intelligence roles.
    frontier.sort_by(|a,b|{
        (a["verifiedRoleImproved"]==true).cmp(&(b["verifiedRoleImproved"]==true))
            .then(a["sourceAttempts"].as_u64().cmp(&b["sourceAttempts"].as_u64()))
            .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
            .then(a["role"].as_str().cmp(&b["role"].as_str()))
    });
    frontier.truncate(128);
    frontier
}


// Narrow source boundaries preserve public call paths while allowing the
// descendant to mutate planning, validation and proof logic independently.
mod evidence;
mod blueprint;
mod history;
mod guidance;
mod planning;
mod proof;
mod method_lineage;
// Revalidate the observed B outcome and propose the EXACT executable method
// source as the next C editing target in the compiled descendant.
mod return_feedback;
pub(crate) use return_feedback::{next_method_return,next_capability_reapplication};
pub(crate) use method_lineage::{
    inherited_method_receipt,observed_method_outcome,executable_method_source
};
pub(crate) use evidence::validate_plan_evidence;
pub(crate) use blueprint::blueprint;
pub(crate) use history::revalidated_method_history;
pub(crate) use guidance::instruction;
pub(crate) use planning::{improvement_directive,propose,inherited_domain};
pub(crate) use proof::system_capability_proven;
#[cfg(test)]
mod tests;
