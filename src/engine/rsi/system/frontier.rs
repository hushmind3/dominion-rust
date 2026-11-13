//! Compiled, source-grounded RSI self-model: frontier responsibility.
use super::*;

/// A source-grounded research frontier identifies unmet capabilities, rather
/// than running an optimization operator with a preselected source path.
/// It learns from actual inherited and failed generations and production
/// task failure signals, without turning an untested observation into gain.
pub(crate) fn architecture_frontier(state:&crate::state::AppState,
    model:&Value,history:&[Value],index_id:&str,root:&Path)->Vec<Value>{
    let mut attempted=BTreeMap::<String,u64>::new();
    let mut role_wins=BTreeMap::<String,u64>::new();
    let mut recent_failure=BTreeMap::<String,String>::new();
    for generation in history {
        for file in generation["focusFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str){
            *attempted.entry(file.to_string()).or_default()+=1;
            if generation["accepted"]!=true {
                recent_failure.insert(file.to_string(),
                    generation["evaluation"]["verdict"].as_str()
                        .unwrap_or("source_edit_unproven").to_string());
            }
        }
        if let Some(design)=accepted_model_design(generation) {
            for role in design_roles(design) {
                *role_wins.entry(role).or_default()+=1;
            }
        }
    }
    let usage=crate::engine::rsi_scan::report(state);
    let signals=usage["signals"].as_array().cloned().unwrap_or_default();
    let full_source=source_inventory(root);
    // Research from a previous campaign survives only when its accepted
    // source bytes still occur in the current compiled intelligence.
    let archive=research_archive(state,model,index_id,history);
    for prior in archive["verifiedAdoptedDesigns"].as_array().into_iter().flatten(){
        for role in prior["roles"].as_array().into_iter().flatten().filter_map(Value::as_str){
            *role_wins.entry(role.to_string()).or_default()+=1;
        }
    }
    let mut frontier=Vec::new();
    for component in model["architecture"].as_array().into_iter().flatten() {
        let Some(role)=component["role"].as_str().filter(|role|valid_role(role)) else{continue};
        let domain=component["domain"].as_str()
            .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
            .unwrap_or_else(||role_domain(role));
        // Do not restrict a self-modifying intelligence to the files named
        // by its human-designed ancestor. All current Rust/TS modules
        // participate; accepted novel roles carry their own source links.
        let representatives=component["representativeSources"].as_array()
            .cloned().unwrap_or_default();
        let mut all=representatives.clone();
        let representative_names=representatives.iter().filter_map(|entry|entry["path"]
            .as_str()).map(str::to_string).collect::<BTreeSet<_>>();
        let mut names=representative_names.clone();
        for file in &full_source {
            let Some(path)=file["path"].as_str() else{continue};
            if source_role(path)==role && names.insert(path.to_string()){
                all.push(file.clone());
            }
        }
        let mut sources=all.iter().filter(|file|
            source_entry_valid(state,root,index_id,file,"sha256")).collect::<Vec<_>>();
        sources.sort_by(|a,b| {
            let path_a=a["path"].as_str().unwrap_or("");
            let path_b=b["path"].as_str().unwrap_or("");
            let ca=attempted.get(path_a).copied().unwrap_or(0);
            let cb=attempted.get(path_b).copied().unwrap_or(0);
            let preferred_a=!representative_names.contains(path_a);
            let preferred_b=!representative_names.contains(path_b);
            ca.cmp(&cb)
                .then(preferred_a.cmp(&preferred_b))
                .then(a["bytes"].as_u64().cmp(&b["bytes"].as_u64()))
                .then(a["path"].as_str().cmp(&b["path"].as_str()))
        });
        // Preserve diversity among source-level ways to improve the same
        // intelligence role. One handpicked representative suppresses
        // alternate methods, newly generated modules and metacognition code.
        for (source_rank,source) in sources.into_iter().take(4).enumerate(){
            let Some(path)=source["path"].as_str() else{continue};
            let related=signals.iter().filter(|signal|
                signal["targetFiles"].as_array().is_some_and(|paths|
                    paths.iter().any(|candidate|candidate.as_str()==Some(path))))
                .filter_map(|signal|signal["id"].as_str().map(str::to_string))
                .take(6).collect::<Vec<_>>();
            frontier.push(json!({
                "role":role,"domain":domain,"path":path,
                "sourceSha256":source["sha256"],"sourceBytes":source["bytes"],
                "candidateRankWithinRole":source_rank,
                "methodSource":executable_meta_source(path),
                "attempts":attempted.get(path).copied().unwrap_or(0),
                "verifiedRedesigns":role_wins.get(role).copied().unwrap_or(0),
                "latestFailure":recent_failure.get(path),
                "taskFailureSignalIds":related,
                "reason":if role_wins.contains_key(role){"extend_verified_intelligence"}
                    else if recent_failure.contains_key(path){"invent_alternative_after_failure"}
                    else {"unexplored_intelligence_component"},
                "qualityProven":false
            }));
        }
    }
    frontier.sort_by(|a,b|{
        a["verifiedRedesigns"].as_u64().cmp(&b["verifiedRedesigns"].as_u64())
            .then(a["attempts"].as_u64().cmp(&b["attempts"].as_u64()))
            .then(b["taskFailureSignalIds"].as_array().map(Vec::len)
                .cmp(&a["taskFailureSignalIds"].as_array().map(Vec::len)))
            .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
            .then(a["role"].as_str().cmp(&b["role"].as_str()))
    });
    frontier.truncate(64);
    frontier
}
