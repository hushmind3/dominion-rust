//! Compiled, source-grounded RSI self-model: inventory responsibility.
use super::*;

/// Independent baseline, metric and adoption logic is not intelligence
/// subject to self-modification; all other agent system source is editable.
pub(crate) fn intelligence_editable_path(path:&str)->bool{
    let extension=Path::new(path).extension().and_then(|ext|ext.to_str());
    crate::engine::rsi::safe_path(path)
        && matches!(extension,Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"css"|"py"))
        && !crate::engine::rsi::hidden_rsi_oracle_path(path)
        && !matches!(path,
            "src/engine/rsi.rs"|"src/engine/rsi_worktree.rs"
            |"src/engine/rsi_metrics.rs"|"src/engine/rsi_profile.rs")
}


pub(super) const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/chat/runtime.rs","src/engine/decompose.rs","src/engine/chat/mod.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/rsi_whole_agent.rs","src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&["src/engine/rsi_edit_genome.rs","src/engine/rsi_edit_program.rs","src/engine/rsi_rule_induction.rs","src/engine/rsi_rule_tournament.rs","src/engine/rsi_decision_rules.rs","src/engine/rsi_rule_rewrite.rs","src/engine/rsi_archive.rs","src/engine/rsi_principle_learner.rs","src/engine/rsi_recursive_laboratory.rs","src/engine/rsi_causal_ecology.rs","src/engine/rsi_meta_controller.rs","src/engine/rsi/meta_controller/provenance.rs","src/engine/rsi/meta_controller/reciprocal.rs","src/engine/rsi/meta_controller/selection.rs","src/engine/rsi/meta_controller/transfer.rs","src/engine/rsi/meta_controller/feedback.rs","src/engine/rsi_evolve.rs","src/engine/rsi_mutation.rs","src/engine/rsi_system.rs"]),
    ("models",&["src/engine/rsi_model_learning.rs","src/engine/router.rs","src/engine/models.rs","src/engine/cli_model.rs","src/engine/llm.rs","src/engine/gateway.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

/// Complete editable intelligence source inventory. The source tree is not
/// just seven hand-picked files: new descendants may create new modules and
/// rewrite the task agent, meta-agent, tool harness and memory together.
/// Stable sorting + content hashes make this an actual revision self-model.
pub(super) fn source_inventory(root:&Path)->Vec<Value>{
    let Ok(root)=fs::canonicalize(root) else{return Vec::new()};
    let mut pending=vec![root.join("src"),root.join("web").join("src"),
        root.join("src-tauri").join("src")];
    let mut visited=BTreeSet::<std::path::PathBuf>::new();
    let mut files=Vec::new();
    while let Some(directory)=pending.pop(){
        let Ok(directory)=fs::canonicalize(directory) else{continue};
        if !directory.starts_with(&root)||!visited.insert(directory.clone()){continue;}
        let Ok(entries)=fs::read_dir(&directory) else{continue};
        // Readdir ordering is unspecified. The same source commit must
        // generate the same whole-system identity even after a restart.
        let mut entries=entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry|entry.file_name());
        for entry in entries {
            let Ok(path)=fs::canonicalize(entry.path()) else{continue};
            if !path.starts_with(&root){continue;}
            if path.is_dir(){pending.push(path);continue;}
            if !path.is_file(){continue;}
            let Ok(relative)=path.strip_prefix(&root) else{continue};
            let relative=relative.to_string_lossy().replace('\\',"/");
            if !intelligence_editable_path(&relative){continue;}
            if !matches!(path.extension().and_then(|ext|ext.to_str()),
                Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"css"|"py")){continue;}
            let Ok(bytes)=fs::read(&path) else{continue};
            if bytes.len()>2_000_000 {continue;}
            files.push(json!({"path":relative,"sha256":format!("{:x}",Sha256::digest(&bytes)),
                "bytes":bytes.len()}));
            if files.len()>=2048 {break;}
        }
        if files.len()>=2048 {break;}
    }
    files.sort_by(|a,b|a["path"].as_str().cmp(&b["path"].as_str()));
    files
}

/// Parent-visible invariant: only the source bytes and commit identity are
/// stable. The descendant's role graph, exploration order and search program
/// are evolvable algorithms, never recomputed as a parent policy veto.
pub(crate) fn source_facts(root:&Path)->Value{
    let files=source_inventory(root);
    json!({"fileCount":files.len(),"sha256":format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()))})
}

pub(super) fn valid_role(role:&str)->bool{
    !role.is_empty()&&role.len()<=64&&role.bytes().all(|b|
        b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
pub(super) fn role_domain(role:&str)->&'static str{
    match role {
        "memory"=>"memory","learning"|"models"=>"learning","tools"=>"tools",
        "harness"|"context"=>"context","frontend"|"ui"=>"ui",
        _=>"improver"
    }
}
pub(super) fn design_roles(design:&Value)->Vec<String>{
    design["components"].as_array().map(|items|items.iter()
        .filter_map(Value::as_str).filter(|role|valid_role(role))
        .map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_else(||design["component"].as_str().filter(|role|valid_role(role))
            .map(|name|vec![name.to_string()]).unwrap_or_default())
}
pub(super) fn design_implemented(design:&Value, accepted_hashes:&Value)->bool{
    let Some(targets)=design["targetFiles"].as_array()
        .filter(|targets|!targets.is_empty()) else{return false};
    targets.iter().all(|target|target.as_str().is_some_and(|path|
        intelligence_editable_path(path) &&
        accepted_hashes[path].as_str().is_some_and(|sha|
            sha.len()==64&&sha.bytes().all(|c|c.is_ascii_hexdigit()))))
}

/// A plan is not inherited because the model said it was good. Every
/// declared target must actually be part of an accepted, hash-recorded
/// model-authored source change. Unverified candidates cannot train policy.
pub(super) fn accepted_model_design(generation:&Value)->Option<&Value>{
    let design=&generation["systemDesign"];
    if generation["accepted"]!=true || generation["pairedRevalidated"]!=true
        || generation["evaluation"]["metrics"]["eligible"]!=true
        || generation["candidateOrigin"]!="model_driven_detached_worktree"
        || !generation["acceptedCommit"].as_str().is_some_and(|s|s.len()==40)
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=generation["generatorCommit"]
        || design["generationJobId"]!=generation["jobId"]
        || !design_implemented(design,&generation["acceptedSourceHashes"]) {
        return None;
    }
    Some(design)
}

