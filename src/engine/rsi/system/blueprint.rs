//! Compiled, source-grounded RSI self-model: blueprint responsibility.
use super::*;

/// Read only original, real source assets under the experiment checkout.
pub(crate) fn blueprint(root:&Path,revision:&str,history:&[Value])->Value{
    let canonical_root=fs::canonicalize(root).ok();
    let mut components=Vec::new();
    for (role,paths) in COMPONENTS {
        let mut inventory=Vec::new();
        for relative in *paths {
            if !intelligence_editable_path(relative){continue;}
            let Some(root)=canonical_root.as_ref() else{continue};
            let Ok(file)=fs::canonicalize(root.join(relative)) else{continue};
            if !file.starts_with(root)||!file.is_file(){continue;}
            let Ok(bytes)=fs::read(&file) else{continue};
            inventory.push(json!({"path":relative,
                "sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len()}));
        }
        components.push(json!({"role":role,"domain":role_domain(role),
            "representativeSources":inventory}));
    }
    // Descendant-invented intelligence components must have real, adopted
    // source code AND an adopted runtime connection. Role birth is not a
    // label: a source module that is never called by the agent is not a
    // reusable intelligence subsystem.
    for generation in history {
        let Some(design)=accepted_model_design(generation) else{continue};
        for invented in design["newComponents"].as_array().into_iter().flatten() {
            let Some(role)=invented["role"].as_str().filter(|r|valid_role(r))
                else{continue};
            if components.iter().any(|item|item["role"]==role) || components.len()>=64 {
                continue;
            }
            let Some(domain)=invented["domain"].as_str().filter(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(d)) else{continue};
            let Some(source_paths)=invented["sourcePaths"].as_array()
                .filter(|p|!p.is_empty() && p.len()<=4) else{continue};
            let Some(wires)=invented["integrationPaths"].as_array()
                .filter(|p|!p.is_empty() && p.len()<=4) else{continue};
            let rust_module=source_paths.iter().any(|file|file.as_str()
                .is_some_and(|path|path.starts_with("src/engine/")
                    && path.ends_with(".rs")));
            if rust_module && (!wires.iter().any(|w|w=="src/engine/mod.rs")
                || !wires.iter().any(|w|w!="src/engine/mod.rs")) {
                // A declaration in mod.rs does not make a novel algorithm run.
                continue;
            }
            let target_files=design["targetFiles"].as_array();
            let valid_source=|path:&str|{
                target_files.is_some_and(|rows|rows.iter().any(|p|p==path))
                    && generation["acceptedSourceHashes"][path].as_str().is_some_and(|sha|
                        sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
                    && intelligence_editable_path(path)
            };
            let explicit_mapping=design["componentFiles"][role].as_array();
            if source_paths.iter().any(|file|file.as_str().is_none_or(|path|
                !valid_source(path) || !explicit_mapping.is_some_and(|rows|
                    rows.iter().any(|item|item==path))))
                || wires.iter().any(|wire|wire.as_str().is_none_or(|path|
                    !valid_source(path) || source_paths.iter().any(|p|p==path))) {
                continue;
            }
            // Rebind to the current descendant's ACTUAL bytes. Later
            // accepted rewrites must not erase a previously born subsystem
            // just because its original introduction hash is now obsolete.
            let entries=source_paths.iter().filter_map(|item|{
                let path=item.as_str()?;
                let root=canonical_root.as_ref()?;
                let file=fs::canonicalize(root.join(path)).ok()?;
                if !file.starts_with(root)||!file.is_file(){return None;}
                let bytes=fs::read(file).ok()?;
                Some(json!({"path":path,
                    "sha256":format!("{:x}",Sha256::digest(&bytes)),
                    "bytes":bytes.len()}))
            }).collect::<Vec<_>>();
            if entries.len()!=source_paths.len(){continue;}
            // A changed integration file is not sufficient. The currently
            // compiled descendant must still name/import the new module.
            // This is source linkage evidence, not an inferred benchmark win.
            let stems=source_paths.iter().filter_map(Value::as_str)
                .filter_map(|p|Path::new(p).file_stem().and_then(|s|s.to_str()))
                .collect::<Vec<_>>();
            let wired=wires.iter().all(|wire|{
                let Some(path)=wire.as_str() else{return false};
                let Some(root)=canonical_root.as_ref() else{return false};
                let Ok(file)=fs::canonicalize(root.join(path)) else{return false};
                if !file.starts_with(root)||!file.is_file(){return false;}
                let Ok(contents)=fs::read(file) else{return false};
                let text=String::from_utf8_lossy(&contents);
                stems.iter().any(|stem|text.contains(*stem))
            });
            if !wired {continue;}
            components.push(json!({"role":role,"domain":domain,
                "representativeSources":entries,
                "birthCommit":generation["acceptedCommit"],
                "integrationPaths":wires,
                "purpose":invented["purpose"],
                "origin":"accepted_compiled_descendant_wired_intelligence_v1",
                "roleQualityProven":false}));
        }
    }
    // New accepted modules and novel architectural roles must remain visible.
    // A fixed seven-role topology would prevent self-redesign of the topology.
    for generation in history {
        let Some(design)=accepted_model_design(generation) else{continue};
        for role in design_roles(design) {
            if components.iter().any(|row|row["role"]==role){continue;}
            // Invented roles have stricter birth criteria; they must not
            // sneak back through the legacy generic-role path if their
            // runtime wiring was rejected.
            if design["newComponents"].as_array().is_some_and(|rows|
                rows.iter().any(|entry|entry["role"]==role)){continue;}
            let files=design["componentFiles"][&role].as_array().or_else(||
                (design_roles(design).len()==1).then(||design["targetFiles"].as_array()).flatten());
            let entries=files.into_iter().flatten().filter_map(|path|{
                let path=path.as_str().filter(|p|intelligence_editable_path(p))?;
                let base=canonical_root.as_ref()?;
                let real=fs::canonicalize(base.join(path)).ok()?;
                if !real.starts_with(base)||!real.is_file(){return None;}
                let bytes=fs::read(real).ok()?;
                let hash=format!("{:x}",Sha256::digest(&bytes));
                // Original role mapping must have been part of the independently
                // adopted source patch. Current hashes belong to this NEW
                // descendant's worktree, not to its historical creator.
                if !design["targetFiles"].as_array().is_some_and(|rows|
                    rows.iter().any(|item|item==path))
                    || !generation["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|sha|sha.len()==64
                            && sha.bytes().all(|b|b.is_ascii_hexdigit())) {
                    return None;
                }
                Some(json!({"path":path,"sha256":hash,"bytes":bytes.len()}))
            }).collect::<Vec<_>>();
            if !entries.is_empty() && components.len()<64 {
                components.push(json!({"role":role,"domain":design["nextDomain"],
                    "representativeSources":entries}));
            }
        }
    }
    let evidence=history.iter().rev().take(8).map(|generation|json!({
        "generatorCommit":generation["generatorCommit"],
        "acceptedCommit":generation["acceptedCommit"],
        "domain":generation["domain"],"candidateTarget":generation["candidateTarget"],
        "accepted":generation["accepted"]==true,
        "verdict":generation["evaluation"]["verdict"],
        "improvedHoldouts":generation["evaluation"]["metrics"]["improvedHoldouts"],
        "metaHoldoutGains":generation["evaluation"]["metrics"]["metaHoldoutGains"],
        "regressions":generation["evaluation"]["metrics"]["regressions"],
        "candidateOrigin":generation["candidateOrigin"],
        "designId":generation["systemDesign"]["id"],
        "attemptedFiles":generation["focusFiles"],
        "modelError":generation["error"]
    })).collect::<Vec<_>>();
    let files=source_inventory(root);
    let tree=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()));
    let count=files.len();
    json!({
        "source":"compiled_descendant_system_blueprint_v2",
        "generatorCommit":revision,
        "architecture":components,
        "sourceInventory":files.into_iter().take(64).collect::<Vec<_>>(),
        "sourceFileCount":count,"sourceTreeSha256":tree,
        "historyCount":history.len(),
        "inheritedDesign":history.iter().rev().find_map(accepted_model_design)
            .cloned().unwrap_or(Value::Null),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_task_and_meta_controller",
        "immutableBoundary":["experiment_baseline","frozen_paired_evaluator","budget",
            "production_apply_gate"]
    })
}



