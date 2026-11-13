//! Editable source-grounded architecture frontier ranking.
//! Runs in the compiled descendant; frozen validators must not duplicate this policy.
use super::*;

/// Source-backed gaps and prior experiment costs are scheduling evidence.
/// The supervisor checks source identity, never replays this ranking policy.
pub(crate) fn architecture_frontier(state:&crate::state::AppState, model:&Value,
    history:&[Value], index_id:&str, root:&Path)->Vec<Value> {
    let mut covered=BTreeSet::new();let mut attempts=BTreeMap::<String,u64>::new();
    for generation in history {
        for path in generation["focusFiles"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            *attempts.entry(path.to_string()).or_default()+=1;
        }
        let Some(candidate)=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id)) else{continue;};
        if generation["accepted"]!=true || candidate["status"]!="verified"
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
        let design=&generation["systemDesign"];
        let paths=candidate["paths"].as_array().cloned().unwrap_or_default();
        if design["generatorCommit"]!=generation["generatorCommit"]
            || design["generationJobId"]!=generation["jobId"]
            || !design_implemented(design,&paths) {continue;}
        for role in design_roles(design) {
            let assigned=design["componentFiles"][&role].as_array()
                .or_else(||if design_roles(design).len()==1 {design["targetFiles"].as_array()}else{None});
            if assigned.is_some_and(|files|!files.is_empty() && files.iter().all(|file|
                file.as_str().is_some_and(|path|generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|sha|crate::engine::code_index::indexed_source_hash(state,index_id,path).as_deref()==Some(sha))))) {
                covered.insert(role);
            }
        }
    }
    let usage=usage_evidence(state);
    let related=|path:&str|usage["signals"].as_array().into_iter().flatten().filter(|signal|
        signal["targetFiles"].as_array().is_some_and(|paths|paths.iter().any(|entry|entry==path)))
        .map(|signal|signal["id"].clone()).collect::<Vec<_>>();
    let mut rows=Vec::new();
    for component in model["architecture"].as_array().into_iter().flatten() {
        let Some(role)=component["role"].as_str().filter(|role|valid_role(role)) else{continue;};
        let mut sources=component["representativeSources"].as_array().into_iter().flatten()
            .filter_map(|entry| {
                let path=entry["path"].as_str()?;let sha=entry["sha256"].as_str()?;
                if !crate::engine::rsi::safe_path(path)
                    || crate::engine::code_index::indexed_source_hash(state,index_id,path).as_deref()!=Some(sha) {return None;}
                let bytes=fs::read(root.join(path)).ok()?;
                if format!("{:x}",Sha256::digest(&bytes))!=sha {return None;}
                Some((path,sha,bytes.len() as u64))
            }).collect::<Vec<_>>();
        sources.sort_by_key(|(path,_,bytes)|(std::cmp::Reverse(related(path).len()),attempts.get(*path).copied().unwrap_or(0),*bytes,*path));
        // A single representative hides the newly generated meta-controller
        // whenever another file happens to sort first. Publish a bounded
        // portfolio of distinct, SHA-grounded source candidates per role.
        // The compiled meta-controller (itself mutable) scores this portfolio.
        for (path,sha,bytes) in sources.into_iter().take(6) {
            rows.push(json!({"role":role,"domain":role_domain(component),"path":path,
                "sourceSha256":sha,"sourceBytes":bytes,
                "sourceAttempts":attempts.get(path).copied().unwrap_or(0),
                "usageSignalIds":related(path),"usageSignalCount":related(path).len(),
                "verifiedRoleChanged":covered.contains(role),"qualityProven":false,
                "reason":if covered.contains(role){"extend_verified_source"}else{"unexplored_component"}}));
        }
    }
    rows.sort_by(|a,b|b["usageSignalCount"].as_u64().cmp(&a["usageSignalCount"].as_u64())
        .then((a["verifiedRoleChanged"]==true).cmp(&(b["verifiedRoleChanged"]==true)))
        .then(a["sourceAttempts"].as_u64().cmp(&b["sourceAttempts"].as_u64()))
        .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
        .then(a["role"].as_str().cmp(&b["role"].as_str()))
        .then(a["path"].as_str().cmp(&b["path"].as_str())));
    rows.truncate(64);
    rows
}

