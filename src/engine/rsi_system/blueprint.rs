//! The recompilable descendant's source-grounded system blueprint.
//! No model inference, experiment adoption or frozen-oracle mutation lives here.
use super::*;

/// Read only source assets under the experiment's pinned checkout. The
/// coordinator cannot author or alter this blueprint without changing its
/// measured generator revision; no holdout content is read here.
pub(crate) fn blueprint(root:&Path, revision:&str, history:&[Value])->Value {
    let canonical_root=fs::canonicalize(root).ok();
    let all_files=source_inventory(root);
    let mut components=Vec::new();
    for (role,paths) in COMPONENTS {
        let mut inventory=Vec::new();
        for relative in *paths {
            if !crate::engine::rsi::safe_path(relative) {continue;}
            let Some(root)=canonical_root.as_ref() else {continue;};
            let Ok(file)=fs::canonicalize(root.join(relative)) else {continue;};
            if !file.starts_with(root)||!file.is_file(){continue;}
            let Ok(contents)=fs::read(&file) else {continue;};
            inventory.push(json!({"path":relative,"sha256":format!("{:x}",Sha256::digest(&contents)),
                "bytes":contents.len()}));
        }
        // Candidate selection is source-graph aware: newly created files may
        // enter any subsystem without first being added to this static table.
        // Bound the transmitted representatives, not the source fingerprint.
        let total_sources=all_files.iter().filter(|entry|
            entry["path"].as_str().is_some_and(|path|source_role(path)==*role)).count();
        for entry in all_files.iter().filter(|entry|
            entry["path"].as_str().is_some_and(|path|source_role(path)==*role)) {
            if inventory.len()>=24 {break;}
            if !inventory.iter().any(|item|item["path"]==entry["path"]) {
                inventory.push(entry.clone());
            }
        }
        let row=json!({"role":role,"representativeSources":inventory});
        let domain=role_domain(&row).to_string();
        components.push(json!({"role":role,"domain":domain,"representativeSources":inventory,
            "discoveredSourceCount":total_sources}));
    }
    for generation in history.iter().filter(|generation|
        crate::engine::rsi_mutual_program::adopted_source(generation)) {
        let design=&generation["systemDesign"];
        for role in design_roles(design) {
            if components.iter().any(|item|item["role"]==role){continue;}
            // Explicitly invented capabilities have a stronger contract:
            // both the module and its distinct runtime integration must be
            // independently adopted; the old fallback cannot bypass it.
            if design["newComponents"].as_array().is_some_and(|items|
                items.iter().any(|item|item["role"]==role)) {continue;}
            let paths=design["componentFiles"][&role].as_array().or_else(||
                if design_roles(design).len()==1 {design["targetFiles"].as_array()}else{None});
            let sources=paths.into_iter().flatten().filter_map(|path| {
                let path=path.as_str().filter(|path|crate::engine::rsi::safe_path(path))?;
                let bytes=fs::read(root.join(path)).ok()?;let sha=format!("{:x}",Sha256::digest(&bytes));
                if generation["acceptedSourceHashes"][path]!=sha {return None;}
                Some(json!({"path":path,"sha256":sha,"bytes":bytes.len()}))
            }).collect::<Vec<_>>();
            if !sources.is_empty() && components.len()<64 {
                components.push(json!({"role":role,"domain":design["nextDomain"],"representativeSources":sources}));
            }
        }
    }
    for born in crate::engine::rsi_component_birth::admitted(root,history) {
        if !components.iter().any(|item|item["role"]==born["role"])
            && components.len()<64 {
            components.push(born);
        }
    }
    let evidence=history.iter().rev().take(8).map(|generation|json!({
        "generatorCommit":generation["generatorCommit"],
        "acceptedCommit":generation["acceptedCommit"],
        "domain":generation["domain"],
        "candidateTarget":generation["candidateTarget"],
        "accepted":generation["accepted"]==true,
        "verdict":generation["evaluation"]["verdict"],
        "improvedHoldouts":generation["evaluation"]["metrics"]["improvedHoldouts"],
        "metaHoldoutGains":generation["evaluation"]["metrics"]["metaHoldoutGains"],
        "regressions":generation["evaluation"]["metrics"]["regressions"],
        "methodAblation":generation["methodAblation"],
        "candidateOrigin":generation["candidateOrigin"]
    })).collect::<Vec<_>>();
    let files=all_files;
    let fingerprint=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()));
    let total=files.len();
    // Hash ALL indexed source, but transport only a bounded sample: a
    // 2,048-module system would overflow the planner's stdout capture and
    // needlessly resend tens of thousands of SHA strings to the model.
    let preview=files.into_iter().take(64).collect::<Vec<_>>();
    json!({
        "source":"compiled_descendant_system_blueprint_v1",
        "generatorCommit":revision,
        "architecture":components,
        "sourceInventory":preview,
        "sourceFileCount":total,
        "sourceInventoryTruncated":total>64,
        "sourceTreeSha256":fingerprint,
        "historyCount":history.len(),
        "inheritedDesign":latest_adopted_design(history).cloned().unwrap_or(Value::Null),
        "inheritedDesignLineage":lineage_capsule(history)["acceptedDesigns"].clone(),
        "lineageSha256":lineage_capsule(history)["sha256"].clone(),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_source_editing_runtime",
        "immutableBoundary":["experiment_baseline","frozen_paired_evaluator","budget","production_apply_gate"]
    })
}

