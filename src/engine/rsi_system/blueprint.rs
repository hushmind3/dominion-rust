//! Descendant-owned source blueprint and provenance.
use super::{COMPONENTS,source_inventory,source_role,architecture_role_domain,
    design_roles,design_role_files};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs,path::Path};

/// Read only source assets under the experiment's pinned checkout. The
/// coordinator cannot author or alter this blueprint without changing its
/// measured generator revision; no holdout content is read here.
pub(crate) fn blueprint(root:&Path, revision:&str, history:&[Value])->Value {
    let canonical_root=fs::canonicalize(root).ok();
    let files=source_inventory(root);
    let mut components=Vec::new();
    for (role,_) in COMPONENTS {
        let inventory=files.iter().filter(|row|
            row["path"].as_str().is_some_and(|path|
                source_role(path)==*role
                    && path!="src/engine/rsi.rs"
                    && path!="src/engine/rsi_worktree.rs"
                    && !path.starts_with("src/engine/rsi_worktree/"))
            .cloned().collect::<Vec<_>>();
        components.push(json!({"role":role,
            "domain":architecture_role_domain(role).unwrap_or("improver"),
            "representativeSources":inventory}));
    }
    // The original seven roles are a seed, not a permanent ceiling.
    // New model-authored intelligence modules become visible in the actual
    // rebuilt descendant only when an accepted source change introduced them.
    for generation in history.iter().filter(|g|g["accepted"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["pairedRevalidated"]==true) {
        let design=&generation["systemDesign"];
        if design["generatorCommit"]!=generation["generatorCommit"]
            || design["generationJobId"]!=generation["jobId"] {continue;}
        for role in design_roles(design) {
            if components.iter().any(|existing|
                existing["role"].as_str()==Some(role.as_str())) {continue;}
            let Some(files)=design_role_files(design,&role) else{continue;};
            let mut sources=Vec::new();
            for entry in files {
                let Some(path)=entry.as_str()
                    .filter(|p|crate::engine::rsi::safe_path(p)) else{continue;};
                let Ok(file)=fs::canonicalize(root.join(path)) else{continue;};
                let Ok(base)=fs::canonicalize(root) else{continue;};
                if !file.starts_with(&base) || !file.is_file() {continue;}
                let Ok(bytes)=fs::read(file) else{continue;};
                let sha=format!("{:x}",Sha256::digest(&bytes));
                // Provenance names the FIRST accepted implementation; the
                // current rebuilt descendant may have further redesigned it.
                // Keep the evolving role alive while its current file exists,
                // and independently fingerprint the current bytes.
                let Some(origin)=generation["acceptedSourceHashes"][path].as_str()
                    .filter(|h|h.len()==64 && h.bytes()
                        .all(|byte|byte.is_ascii_hexdigit())) else{continue;};
                sources.push(json!({"path":path,"sha256":sha,"bytes":bytes.len(),
                    "introducedSourceSha256":origin,
                    "changedSinceIntroduction":sha!=origin}));
            }
            if !sources.is_empty() {
                let domain=design["nextDomain"].as_str()
                    .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
                    .unwrap_or("improver");
                components.push(json!({"role":role,"domain":domain,
                    "representativeSources":sources,
                    "introducedBy":generation["acceptedCommit"]}));
            }
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
        "candidateOrigin":generation["candidateOrigin"]
    })).collect::<Vec<_>>();
    let fingerprint=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()));
    let open_topology=crate::engine::rsi_open_topology::observed(history,root);
    json!({
        "source":"compiled_descendant_system_blueprint_v1",
        "generatorCommit":revision,
        "architecture":components,
        "sourceInventory":files,
        "sourceFileCount":files.len(),
        "sourceTreeSha256":fingerprint,
        "openTopology":open_topology,
        "historyCount":history.len(),
        "inheritedDesign":history.iter().rev().find(|g|g["accepted"]==true && g["evaluation"]["metrics"]["eligible"]==true).map(|g|g["systemDesign"].clone()).unwrap_or(Value::Null),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_and_its_model_worker",
        "immutableBoundary":["experiment_baseline","frozen_paired_evaluator","budget","production_apply_gate"]
    })
}

