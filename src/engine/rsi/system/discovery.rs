//! Compiled, source-grounded RSI self-model: discovery responsibility.
use super::*;

/// A new or previously unlisted source module is still part of the
/// intelligence system. This classification is a descendant-editable search
/// heuristic, not an immutable scoring taxonomy.
/// Only the runtime-compiled meta-controller can be a source for revising
/// the improvement method. Test-only modules cannot establish that change.
pub(super) fn executable_meta_source(path:&str)->bool{
    matches!(path,
        "src/engine/rsi_meta_controller.rs"|"src/engine/rsi_system.rs"
        |"src/engine/rsi/meta_controller/provenance.rs"
        |"src/engine/rsi/meta_controller/reciprocal.rs"
        |"src/engine/rsi/meta_controller/selection.rs"
        |"src/engine/rsi/meta_controller/transfer.rs"
        |"src/engine/rsi/meta_controller/feedback.rs"
        |"src/engine/rsi/system/inventory.rs"
        |"src/engine/rsi/system/blueprint.rs"
        |"src/engine/rsi/system/discovery.rs"
        |"src/engine/rsi/system/frontier.rs"
        |"src/engine/rsi/system/agenda.rs"
        |"src/engine/rsi/system/design.rs")
}

pub(super) fn source_role(path:&str)->&'static str{
    let lower=path.to_ascii_lowercase();
    if lower.starts_with("web/src/") || lower.starts_with("src-tauri/src/"){
        return "frontend";
    }
    let name=lower.rsplit('/').next().unwrap_or(&lower);
    if name.starts_with("model_") || name.starts_with("training")
        || name.starts_with("trainable") || matches!(name,
            "models.rs"|"cli_model.rs"|"router.rs"|"llm.rs"|"gateway.rs"
            |"codex_oauth.rs"|"model_architecture.py"){
        return "models";
    }
    if lower.starts_with("src/engine/rsi_")
        || lower.starts_with("src/engine/rsi/")
        || lower.contains("/self_improv") || lower.contains("/evolution"){
        return "improvement_methods";
    }
    if lower.contains("/memory") || lower.contains("/room_state")
        || lower.contains("/room_knowledge") || lower.contains("/recall"){
        return "memory";
    }
    if lower.contains("/learn") || lower.contains("/experience")
        || lower.contains("/procedure") || lower.contains("/strategy"){
        return "learning";
    }
    if lower.contains("/tools") || lower.contains("/browser")
        || lower.contains("/mcp") || lower.contains("/connector"){
        return "tools";
    }
    if lower.contains("/algorithm") || lower.contains("/reasoning")
        || lower.contains("/search") || lower.contains("/planner"){
        return "algorithms";
    }
    "harness"
}

/// The source graph is factual evidence. Ranking is an evolvable algorithm
/// in the currently built descendant; the external parent validates only
/// that chosen files and their hashes are real.
pub(super) fn source_entry_valid(state:&crate::state::AppState,root:&Path,index_id:&str,
    entry:&Value,hash_key:&str)->bool{
    let Some(path)=entry["path"].as_str() else{return false};
    let Some(sha)=entry[hash_key].as_str() else{return false};
    if !intelligence_editable_path(path)
        || sha.len()!=64 || !sha.bytes().all(|b|b.is_ascii_hexdigit()) {
        return false;
    }
    let Ok(base)=fs::canonicalize(root) else{return false};
    let Ok(file)=fs::canonicalize(base.join(path)) else{return false};
    if !file.starts_with(base)||!file.is_file(){return false;}
    if crate::engine::code_index::indexed_source_hash(state,index_id,path)
        .as_deref()!=Some(sha){return false;}
    fs::read(file).is_ok_and(|bytes|format!("{:x}",Sha256::digest(bytes))==sha)
}


/// Persistent research state spans campaigns without assuming that a previous
/// design's source still exists. Only model-authored, independently adopted
/// source revisions matching the CURRENT indexed bytes enter verified memory.
/// Rejected explorations remain explicitly unverified failure hypotheses.
pub(crate) fn research_archive(state:&crate::state::AppState,
    model:&Value,index_id:&str,history:&[Value])->Value{
    let ids=history.iter().filter_map(|row|row["id"].as_str())
        .collect::<BTreeSet<_>>();
    let mut prior=state.store.all("rsi_generation");
    prior.sort_by(|a,b|b["completedAt"].as_str().cmp(&a["completedAt"].as_str())
        .then_with(||b["id"].as_str().cmp(&a["id"].as_str())));
    let mut successes=Vec::new();
    let mut failures=Vec::new();
    for generation in &prior {
        if generation["id"].as_str().is_some_and(|id|ids.contains(id)){continue;}
        if let Some(design)=accepted_model_design(generation){
            let compatible=design["targetFiles"].as_array().is_some_and(|targets|
                targets.iter().all(|target|target.as_str().is_some_and(|path|{
                    let Some(accepted)=generation["acceptedSourceHashes"][path].as_str() else{
                        return false;
                    };
                    crate::engine::code_index::indexed_source_hash(state,index_id,path)
                        .as_deref()==Some(accepted)
                })));
            if compatible && successes.len()<8 {
                successes.push(json!({
                    "source":"current_source_matched_adopted_design",
                    "designId":design["id"],
                    "generatorCommit":generation["generatorCommit"],
                    "acceptedCommit":generation["acceptedCommit"],
                    "roles":design_roles(design),
                    "targetFiles":design["targetFiles"],
                    "hypothesis":design["hypothesis"],
                    "methodHypothesis":design["methodHypothesis"],
                    "nextDomain":design["nextDomain"],
                    "qualityProven":true
                }));
            }
        }else if failures.len()<6 && generation["accepted"]!=true
            && generation["systemBlueprint"]["sourceTreeSha256"]==model["sourceTreeSha256"]
            && generation["systemDesign"]["hypothesis"].is_string(){
            failures.push(json!({
                "source":"source_version_matched_failed_experiment",
                "designId":generation["systemDesign"]["id"],
                "domain":generation["domain"],
                "hypothesis":generation["systemDesign"]["hypothesis"],
                "observedFailure":generation["evaluation"]["verdict"],
                "error":generation["error"],
                "qualityProven":false
            }));
        }
        if successes.len()>=8 && failures.len()>=6 {break;}
    }
    json!({
        "source":"persistent_source_verified_research_archive",
        "indexedRevision":model["sourceTreeSha256"],
        "verifiedAdoptedDesigns":successes,
        "unverifiedFailureLessons":failures,
        "qualityBoundary":"source_matched_adoption_only"
    })
}

