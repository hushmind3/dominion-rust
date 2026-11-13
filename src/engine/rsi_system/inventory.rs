//! Source inventory, digest and editable source-role classification.
//! Standalone filesystem boundary: planning may read immutable facts, not evaluate/adopt candidates.
use super::*;

/// Inventory every compilable agent/frontend source under the selected
/// experimental revision, not merely a hand-picked optimization fixture.
/// An inventory hash changes when ANY generation changes the executable
/// system's source graph; the prompt can remain short without losing lineage.
pub(super) fn source_inventory(root:&Path)->Vec<Value> {
    let Ok(root)=fs::canonicalize(root) else {return Vec::new()};
    let mut pending=vec![root.join("src"),root.join("web").join("src"),root.join("src-tauri").join("src")];
    let mut rows=Vec::new();
    while let Some(directory)=pending.pop() {
        let Ok(entries)=fs::read_dir(&directory) else {continue};
        for entry in entries.flatten() {
            let Ok(file)=fs::canonicalize(entry.path()) else {continue};
            if !file.starts_with(&root){continue;}
            if file.is_dir() {
                pending.push(file);
                continue;
            }
            if !file.is_file(){continue;}
            let Ok(relative)=file.strip_prefix(&root) else {continue};
            let relative=relative.to_string_lossy().replace('\\',"/");
            if !crate::engine::rsi::safe_path(&relative){continue;}
            let allowed=matches!(file.extension().and_then(|e|e.to_str()),
                Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"py"));
            if !allowed {continue;}
            let Ok(bytes)=fs::read(&file) else {continue};
            if bytes.len()>2_000_000 {continue;}
            rows.push(json!({"path":relative,
                "sha256":format!("{:x}",Sha256::digest(&bytes)),
                "bytes":bytes.len()}));
            if rows.len()>=2048 {break;}
        }
        if rows.len()>=2048 {break;}
    }
    rows.sort_by(|a,b|a["path"].as_str().cmp(&b["path"].as_str()));
    rows
}

/// File identity is an invariant checked by the resident supervisor. Role
/// maps, frontier ranking and target choice remain the descendant's policy.
pub(crate) fn source_facts(root:&Path)->Value {
    let files=source_inventory(root);
    json!({"fileCount":files.len(),"sha256":format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()))})
}

/// Bootstrap source ownership from the actual module tree, not from a fixed
/// three-file whitelist. This heuristic itself lives in the descendant's
/// recompilable source and can be redesigned by future generations.
pub(super) fn source_role(path:&str)->&'static str {
    if path.starts_with("web/src/") || path.starts_with("src-tauri/src/") {
        return "frontend";
    }
    // File names such as policy.rs, campaign.rs and worker.rs are generic.
    // Source ownership follows the RSI module boundary, not the basename.
    if path.starts_with("src/engine/rsi/") || path.starts_with("src/engine/rsi_system/") {
        return "improvement_methods";
    }
    if path.starts_with("src/engine/models/") {
        return "models";
    }
    if path=="src/api/providers.rs" || path.starts_with("src/api/providers/") {
        return "models";
    }
    let name=path.rsplit('/').next().unwrap_or(path);
    if name.starts_with("model_") || name.starts_with("training")
        || matches!(name,"models.rs"|"cli_model.rs"|"router.rs") {
        return "models";
    }
    if name.starts_with("rsi_") || matches!(name,"rsi.rs"|"algorithms.rs"|"algorithm_eval.rs"|"policy_patch.rs") {
        return "improvement_methods";
    }
    if path.contains("/chat/") || matches!(name,"chat.rs"|"jobs.rs"|"decompose.rs"|"gateway.rs") {
        return "harness";
    }
    if name.starts_with("memory") || name.starts_with("room_") {
        return "memory";
    }
    if name.starts_with("learning") || name.starts_with("experience")
        || name.starts_with("procedure") || name.starts_with("agent_growth") {
        return "learning";
    }
    if name.starts_with("tools") || name.starts_with("browser") || name.starts_with("mcp") {
        return "tools";
    }
    "algorithms"
}

