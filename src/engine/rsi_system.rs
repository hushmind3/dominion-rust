//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs,collections::{BTreeMap,BTreeSet}};

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


const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/chat/runtime.rs","src/engine/decompose.rs","src/engine/decompose.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&["src/engine/rsi_meta_controller.rs","src/engine/rsi_evolve.rs","src/engine/rsi_mutation.rs","src/engine/rsi_system.rs"]),
    ("models",&["src/engine/models.rs","src/engine/cli_model.rs","src/engine/llm.rs","src/engine/router.rs","src/engine/gateway.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

/// Complete editable intelligence source inventory. The source tree is not
/// just seven hand-picked files: new descendants may create new modules and
/// rewrite the task agent, meta-agent, tool harness and memory together.
/// Stable sorting + content hashes make this an actual revision self-model.
fn source_inventory(root:&Path)->Vec<Value>{
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
                Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"css")){continue;}
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

fn valid_role(role:&str)->bool{
    !role.is_empty()&&role.len()<=64&&role.bytes().all(|b|
        b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
fn role_domain(role:&str)->&'static str{
    match role {
        "memory"=>"memory","learning"|"models"=>"learning","tools"=>"tools",
        "harness"|"context"=>"context","frontend"|"ui"=>"ui",
        _=>"improver"
    }
}
fn design_roles(design:&Value)->Vec<String>{
    design["components"].as_array().map(|items|items.iter()
        .filter_map(Value::as_str).filter(|role|valid_role(role))
        .map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_else(||design["component"].as_str().filter(|role|valid_role(role))
            .map(|name|vec![name.to_string()]).unwrap_or_default())
}
fn design_implemented(design:&Value, accepted_hashes:&Value)->bool{
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
fn accepted_model_design(generation:&Value)->Option<&Value>{
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
    // New accepted modules and novel architectural roles must remain visible.
    // A fixed seven-role topology would prevent self-redesign of the topology.
    for generation in history {
        let Some(design)=accepted_model_design(generation) else{continue};
        for role in design_roles(design) {
            if components.iter().any(|row|row["role"]==role){continue;}
            let files=design["componentFiles"][&role].as_array().or_else(||
                (design_roles(design).len()==1).then(||design["targetFiles"].as_array()).flatten());
            let entries=files.into_iter().flatten().filter_map(|path|{
                let path=path.as_str().filter(|p|intelligence_editable_path(p))?;
                let base=canonical_root.as_ref()?;
                let real=fs::canonicalize(base.join(path)).ok()?;
                if !real.starts_with(base)||!real.is_file(){return None;}
                let bytes=fs::read(real).ok()?;
                let hash=format!("{:x}",Sha256::digest(&bytes));
                if generation["acceptedSourceHashes"][path]!=hash{return None;}
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



/// A new or previously unlisted source module is still part of the
/// intelligence system. This classification is a descendant-editable search
/// heuristic, not an immutable scoring taxonomy.
fn source_role(path:&str)->&'static str{
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
fn source_entry_valid(state:&crate::state::AppState,root:&Path,index_id:&str,
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
                "methodSource":path.ends_with("rsi_meta_controller.rs")
                    || path.ends_with("rsi_system.rs"),
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

/// Controller output is computed by the newly rebuilt generation, with the
/// entire source tree and measured history available to its own code.
/// Its design, ranking and future execution-method policy may change in the
/// next accepted revision; the supervising process merely authenticates facts.
pub(crate) fn system_agenda(state:&crate::state::AppState,model:&Value,
    history:&[Value],index_id:&str,root:&Path)->Value{
    let mut frontier=architecture_frontier(state,model,history,index_id,root);
    let archive=research_archive(state,model,index_id,history);
    let inherited=history.iter().rev().find_map(accepted_model_design);
    let files=source_inventory(root);
    // A successful previous agent may point to any actual source module,
    // including one it just created. Do not require that source to be the
    // human ancestor's first ranked representative.
    let mut inherited_targets=Vec::<Value>::new();
    if let Some(design)=inherited {
        for path in design["nextTargetFiles"].as_array().into_iter()
            .flatten().take(3).filter_map(Value::as_str){
            let existing=frontier.iter().find(|item|item["path"]==path).cloned();
            let proposed=existing.or_else(||{
                let file=files.iter().find(|row|row["path"]==path)?;
                if !source_entry_valid(state,root,index_id,file,"sha256"){return None;}
                let role=design["componentFiles"].as_object().and_then(|roles|
                    roles.iter().find(|(_,paths)|paths.as_array()
                        .is_some_and(|entries|entries.iter().any(|v|v==path))))
                    .map(|(role,_)|role.as_str()).unwrap_or_else(||source_role(path));
                let role=if valid_role(role){role}else{source_role(path)};
                Some(json!({
                    "role":role,"domain":design["nextDomain"],
                    "path":path,"sourceSha256":file["sha256"],
                    "sourceBytes":file["bytes"],
                    "attempts":0,"verifiedRedesigns":0,
                    "taskFailureSignalIds":[],"latestFailure":null,
                    "reason":"adopted_ancestor_next_system_design",
                    "qualityProven":false
                }))
            });
            if let Some(target)=proposed {
                if !frontier.contains(&target){frontier.push(target.clone());}
                if !inherited_targets.contains(&target){
                    inherited_targets.push(target);
                }
            }
        }
    }
    let staged=inherited_stage(history,&frontier);
    let selected=staged.as_ref().map(|row|row["target"].clone())
        .filter(Value::is_object)
        .or_else(||inherited_targets.first().cloned())
        .or_else(||frontier.first().cloned());
    let focus=if staged.is_some() {
        selected.as_ref().map(|t|vec![t.clone()]).unwrap_or_default()
    }else if !inherited_targets.is_empty() {
        inherited_targets
    }else{
        selected.as_ref().map(|t|vec![t.clone()]).unwrap_or_default()
    };
    json!({
        "source":"compiled_descendant_self_controller_v1",
        "generatorCommit":model["generatorCommit"],
        "sourceTreeSha256":model["sourceTreeSha256"],
        "frontier":frontier,
        "researchArchive":archive,
        "inheritedStage":staged,
        "target":selected,
        "focus":focus,
        "decisionOwner":"current_compiled_descendant",
        "nextHypothesis":inherited.map(|d|d["hypothesis"].clone())
            .unwrap_or(Value::Null),
        "nextImprovementMethod":inherited.and_then(|d|
            d["methodHypothesis"].as_str()).map(|s|json!(s)).unwrap_or(Value::Null)
    })
}

/// Only immutable path+hash evidence is verified by the resident parent.
/// It MUST NOT recompute the last generation's frontier order: that would
/// freeze intelligence selection and reject a genuinely new planner policy.
pub(crate) fn validate_system_agenda(state:&crate::state::AppState,
    model:&Value,agenda:&Value,index_id:&str,root:&Path,
    commit:&str,history_len:usize)->Result<(),String>{
    // The external supervisor cannot require its OWN source inventory
    // algorithm to equal the descendant's: that would freeze the
    // self-modifying architecture analyzer and block legitimate revisions.
    // The measured binary commit and each selected actual source SHA bind
    // the controller to real code without replaying its ranking algorithm.
    let fingerprint=model["sourceTreeSha256"].as_str()
        .is_some_and(|sha|sha.len()==64
            && sha.bytes().all(|byte|byte.is_ascii_hexdigit()));
    if model["generatorCommit"]!=commit
        || model["historyCount"].as_u64()!=Some(history_len as u64)
        || !fingerprint
        || !model["sourceFileCount"].as_u64()
            .is_some_and(|count|count>0 && count<=2048)
        || model["architecture"].as_array().is_none_or(|roles|
            roles.is_empty()||roles.len()>64)
        || agenda["source"]!="compiled_descendant_self_controller_v1"
        || agenda["generatorCommit"]!=commit
        || agenda["sourceTreeSha256"]!=model["sourceTreeSha256"] {
        return Err("후손의 전체 소스 구조와 세대 ID가 일치하지 않습니다.".into());
    }
    let preview=model["sourceInventory"].as_array()
        .filter(|rows|rows.len()<=64)
        .ok_or("후손 자기모형의 소스 표본 형식 오류")?;
    for row in preview {
        if !source_entry_valid(state,root,index_id,row,"sha256") {
            return Err("후손이 보고한 실제 소스 표본의 해시가 다릅니다.".into());
        }
    }
    let rows=agenda["frontier"].as_array()
        .filter(|rows|rows.len()<=64)
        .ok_or("후손 자기재설계 후보가 잘못됐습니다.")?;
    for row in rows {
        if !row["role"].as_str().is_some_and(valid_role)
            || !row["domain"].as_str().is_some_and(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(&d))
            || !source_entry_valid(state,root,index_id,row,"sourceSha256") {
            return Err("후손이 선택한 개선 대상의 실제 소스 증거가 일치하지 않습니다.".into());
        }
    }
    let chosen=&agenda["target"];
    if !chosen.is_null() && !rows.iter().any(|row|row==chosen){
        return Err("후손의 선택 목표가 자체 소스 탐색 결과와 다릅니다.".into());
    }
    let focus=agenda["focus"].as_array()
        .filter(|focus|focus.len()<=3)
        .ok_or("후손의 실제 수정 범위가 너무 큽니다.")?;
    if focus.iter().any(|row|!rows.contains(row)){
        return Err("후손의 집중 수정 대상이 실제 인덱스 증거와 다릅니다.".into());
    }
    Ok(())
}

/// A model may propose a new executable source-edit procedure. It inherits
/// only after the associated source redesign is independently adopted.
/// This is a new *trial* for the next generation, not proof that the proposed
/// program already improved performance.
pub(crate) fn inherited_program(history:&[Value])->Option<Value>{
    // A newer independently adopted source revision supersedes an older
    // learned method, even if it chose to remove the old method. Reviving an
    // earlier program after a new source adoption is stale policy inheritance.
    let last=history.iter().rev().find(|row|row["accepted"]==true)?;
    let design=accepted_model_design(last)?;
    let sequence=design["actionProgram"].as_array()
        .filter(|seq|seq.len()>=2&&seq.len()<=8)?;
    let allowed=|tool:&str|matches!(tool,
        "code_search"|"workspace_read_range"|"workspace_replace"
        |"workspace_replace_range"|"workspace_replace_function"
        |"workspace_write"|"rsi_system_plan"|"rsi_method_propose");
    if !sequence.iter().all(|tool|tool.as_str().is_some_and(allowed)){return None;}
    let read=sequence.iter().position(|tool|tool=="workspace_read_range")?;
    let write=sequence.iter().position(|tool|matches!(tool.as_str(),
        Some("workspace_replace"|"workspace_replace_range"
            |"workspace_replace_function"|"workspace_write")))?;
    if read>=write{return None;}
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&sequence).ok()?));
    Some(json!({
        "kind":"rsi.induced_action_program","revision":revision,
        "sequence":sequence,"onFailure":[],"recoveryPaths":[],
        "status":"trial_inherited_from_verified_descendant_design",
        "origin":"model_authored_self_modification",
        "evidence":{
            "adoptedCommit":last["acceptedCommit"],"designId":design["id"],
            "generatorCommit":last["generatorCommit"],
            "pairedRevalidated":true,
            "methodTrialUnverified":true
        }
    }))
}


/// An independently accepted model may teach its next compiled descendant
/// a branching, failure-aware METHOD rather than a fixed editing tool list.
/// The compiled child itself chooses between staged redesign hypotheses.
pub(crate) fn inherited_stage(history:&[Value],frontier:&[Value])->Option<Value>{
    let last_adopted=history.iter().rposition(|g|g["accepted"]==true)?;
    let ancestor=&history[last_adopted];
    let design=accepted_model_design(ancestor)?;
    let stages=design["successorStages"].as_array()?;
    let later=&history[last_adopted+1..];
    stages.iter().enumerate().filter_map(|(index,stage)|{
        let path=stage["path"].as_str()?;
        let role=stage["role"].as_str()?;
        let domain=stage["domain"].as_str()?;
        let target=frontier.iter().find(|row|
            row["path"]==path && row["role"]==role && row["domain"]==domain)?;
        let failures=later.iter().filter(|g|g["accepted"]!=true
            && g["focusFiles"].as_array().is_some_and(|files|
                files.iter().any(|file|file.as_str()==Some(path)))).count() as i64;
        let wins=later.iter().filter(|g|g["accepted"]==true
            && g["focusFiles"].as_array().is_some_and(|files|
                files.iter().any(|file|file.as_str()==Some(path)))).count() as i64;
        let policy=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        let max_retries=stage["maxRetries"].as_i64().unwrap_or(2).clamp(0,8);
        if policy=="repair_failed" && failures>max_retries{return None;}
        let prior=stage["priority"].as_i64().unwrap_or(0).clamp(-20,20);
        let gain=match policy {
            "avoid_failed"=>-3*failures,
            "repair_failed"=>if failures==0{0}else{2*failures},
            "explore_novel"=>if wins>0{-4*wins}else{3},
            "repeat_verified"=>4*wins,
            _=>return None
        };
        let score=prior+gain-2*failures.min(3);
        Some((score,index,json!({
            "source":"adopted_descendant_method_stage_v1",
            "sourceDesignId":design["id"],
            "predecessorCommit":ancestor["acceptedCommit"],
            "stageIndex":index,
            "target":target,
            "role":role,"domain":domain,
            "path":path,"sourceSha256":target["sourceSha256"],
            "objective":stage["objective"],
            "learningPolicy":policy,
            "executionPolicy":stage["executionPolicy"],
            "failedAttemptsSinceAdoption":failures,
            "adoptedAttemptsSinceAdoption":wins,
            "qualityProven":false
        })))
    }).max_by(|a,b|a.0.cmp(&b.0).then_with(||b.1.cmp(&a.1)))
        .map(|(_,_,plan)|plan)
}

/// The next compiled intelligence worker receives its own source-grounded
/// research agenda and actual inherited experimental editing method. The
/// model remains free to reject the hypothesis and implement a better one.
pub(crate) fn agenda_instruction(agenda:&Value)->String{
    let target=&agenda["target"];
    let focus=agenda["focus"].as_array().into_iter().flatten()
        .filter_map(|row|row["path"].as_str())
        .collect::<Vec<_>>().join(", ");
    let frontier=agenda["frontier"].as_array().into_iter().flatten()
        .take(7).map(|row|format!(
            "{}: path={} priorAttempts={} verifiedRedesigns={} lastFailure={} reason={}",
            row["role"],row["path"],row["attempts"],
            row["verifiedRedesigns"],row["latestFailure"],row["reason"]
        )).collect::<Vec<_>>().join("\n");
    let stage=&agenda["inheritedStage"];
    let stage_instruction=if stage.is_object(){
        format!(
            "INHERITED EXECUTABLE METHOD STAGE: role={} domain={} file={} sha={} objective={} learning={} execution={} failures={} sourceDesign={}",
            stage["role"],stage["domain"],stage["path"],
            stage["sourceSha256"],stage["objective"],
            stage["learningPolicy"],stage["executionPolicy"],
            stage["failedAttemptsSinceAdoption"],stage["sourceDesignId"])
    }else{String::new()};
    let archive=agenda["researchArchive"]["verifiedAdoptedDesigns"]
        .as_array().into_iter().flatten().take(4).map(|row|
            format!("ADOPTED roles={} sources={} hypothesis={} method={}",
                row["roles"],row["targetFiles"],row["hypothesis"],row["methodHypothesis"]))
        .chain(agenda["researchArchive"]["unverifiedFailureLessons"]
            .as_array().into_iter().flatten().take(3).map(|row|
                format!("FAILED (unproven) domain={} hypothesis={} observed={}",
                    row["domain"],row["hypothesis"],row["observedFailure"])))
        .collect::<Vec<_>>().join("\n");
    format!(
        "[SELF-OWNED TASK+META RESEARCH CONTROLLER]\n\
        Your compiled agent chose component={} domain={} because {}.\n\
        Actual target source: {}, sourceSha256={}.\n\
        Grounded focus: {}. Source-tree revision SHA-256: {}.\n\
        Your model must inspect these true source bytes, evaluate the prior \
        limitations, and invent a causal change to the *same intelligence \
        system* which will design the next descendant. Use rsi_system_plan \
        with component=system, components, componentFiles and all targetFiles \
        for a coupled redesign; new source modules are permitted. Use the \
        optional actionProgram to teach the next accepted agent a DIFFERENT \
        executable method. The first action must still be actual source \
        observation and the method must write implementation, not prose.\n\
        Previous independently adopted hypothesis: {}.\n\
        If inheritedStage is present, implement its model-authored, failure-adapted         objective using the indicated executionPolicy rather than blindly         repeating the previous attempt. This whole method can itself be         edited inside the next compiled meta-controller.\n\
        Self-selected source research frontier:\n{}\n        Persisted previous research (only adopted source gains are verified):\n{}\n{}",
        target["role"],target["domain"],target["reason"],
        target["path"],target["sourceSha256"],focus,
        agenda["sourceTreeSha256"],agenda["nextHypothesis"],frontier,archive,stage_instruction
    )
}

/// A system-generation *model* chooses the actual design and edits. The
/// descendant's source-grounded map is context, not a fixed list of allowed
/// mutations. The code index and source-edit tools still cover the repository.
pub(crate) fn instruction(self_model:&Value,owner_objective:&str)->String {
    let components=self_model["architecture"].as_array()
        .into_iter().flatten()
        .filter_map(|row|{
            let name=row["role"].as_str()?;
            let paths=row["representativeSources"].as_array()?
                .iter().filter_map(|item|item["path"].as_str()).take(3)
                .collect::<Vec<_>>().join(", ");
            Some(format!("{name}: {paths}"))
        }).collect::<Vec<_>>().join("\n");
    let inherited=self_model["inheritedDesign"].as_object().map(|_|format!("Previously accepted design: nextDomain={} hypothesis={}",self_model["inheritedDesign"]["nextDomain"],self_model["inheritedDesign"]["hypothesis"])).unwrap_or_default();
    let history=self_model["recentMeasuredGenerations"].as_array()
        .into_iter().flatten().take(4).map(|row|format!(
            "domain={} adopted={} verdict={} holdoutGain={} metaGain={}",
            row["domain"],row["accepted"],row["verdict"],
            row["improvedHoldouts"],row["metaHoldoutGains"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[COMPILED DESCENDANT: SYSTEM SELF-REDESIGN]\n\
        You are running from revision {}. The user goal is: {}.\n\
        You control design, source analysis, algorithm choice, method learning and \
        actual code edits for your next descendant. Do not treat a prewritten \
        source mutation or a single benchmark speedup as system evolution.\n\
        Inspect real source and identify a causal limitation in the current \
        agent's algorithm, harness, memory, learning, tools or improvement \
        method. Propose a genuinely new executable design, inspect source \
        hashes with code_search/workspace_read_range and WRITE the required \
        cross-module Rust changes with workspace_replace_range/function/write. \
        Before editing submit a structured, causal system design with rsi_system_plan (component, hypothesis, design, targetFiles, nextDomain), then actually WRITE code. A design alone earns no credit. Where beneficial, propose a reusable executable method using \
        rsi_method_propose; mere registration is not evidence of execution. \
        Make your next-generation planning and implementation behavior better, \
        not only an external product function. The independently frozen paired \
        evaluator alone decides adoption; do not edit the evaluator. \
        The accepted source will be compiled as your successor, which must \
        design and execute the subsequent change.\n\
        Source architecture (representatives; the full repository is searchable):\n{}\n\
        Measured predecessor outcomes (not claims about the new generation):\n{}\n\
        Prior accepted design intent: {}\nReport source changes and receipts, not a claimed AGI or RSI completion.",
        self_model["generatorCommit"],owner_objective,components,history,inherited)
}


/// A model-authored design is an unproven experiment. It cannot modify the
/// frozen evaluator or steer successors unless source changes are adopted.
/// A descendant-authored meta-design is persisted as an experiment contract
/// with exact source observations. The agent, not its supervisor, invents
/// components, techniques, and executable exploration programs.
pub(crate) fn propose(
    state:&crate::state::AppState,session_id:&str,body:&Value
)->Result<Value,String>{
    let mut session=state.store.get("session",session_id)
        .ok_or("시스템 설계 세션 없음")?;
    let campaign_id=session["rsiCampaignId"].as_str()
        .ok_or("자기개선 캠페인 ID 없음")?.to_string();
    let child_id=session["rsiGenerationJobId"].as_str()
        .ok_or("후손 작업 ID 없음")?.to_string();
    let parent=state.store.get("job",&campaign_id)
        .ok_or("캠페인 작업 없음")?;
    let child=state.store.get("job",&child_id)
        .ok_or("후손 작업 없음")?;
    if session["rsiRun"]!=true || parent["kind"]!="rsi_evolve"
        || parent["status"]!="running" || parent["cancelRequested"]==true
        || child["kind"]!="rsi_generation" || child["status"]!="running"
        || child["input"]["rsiCampaignId"]!=campaign_id
        || child["input"]["rsiSystemEvolution"]!=true
        || child["cancelRequested"]==true {
        return Err("진행 중인 자기재설계 후손만 설계할 수 있습니다.".into());
    }
    let component=body["component"].as_str()
        .filter(|name|valid_role(name))
        .ok_or("시스템 구성요소 이름 오류")?;
    let mut roles=body["components"].as_array()
        .map(|items|items.iter().filter_map(Value::as_str)
            .map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_else(||vec![component.to_string()]);
    if component=="system" && roles.len()<2 {
        return Err("전체 시스템 결합 설계는 2개 이상의 지능 구성요소를 지정해야 합니다.".into());
    }
    if roles.is_empty() || roles.len()>12 || roles.iter().any(|role|!valid_role(role)){
        return Err("지능 구성요소 목록 형식 오류".into());
    }
    let mut unique_roles=BTreeSet::new();
    if roles.iter().any(|role|!unique_roles.insert(role.clone())){
        return Err("중복된 설계 구성요소".into());
    }
    if component!="system" && roles.len()>1 && !roles.iter().any(|r|r==component){
        return Err("복합 설계에서 주 구성요소가 누락됐습니다.".into());
    }
    if component!="system" && roles.len()==1 && roles[0]!=component {
        return Err("주 구성요소와 설계 범위가 일치하지 않습니다.".into());
    }
    let domain=body["nextDomain"].as_str()
        .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
        .ok_or("후손의 다음 탐색 분야 오류")?;
    let hypothesis=body["hypothesis"].as_str()
        .filter(|v|v.trim().len()>=20 && v.len()<=2000)
        .ok_or("인과적인 개선 가설 20~2000자를 입력하세요.")?;
    let design=body["design"].as_str()
        .filter(|v|v.trim().len()>=30 && v.len()<=6000)
        .ok_or("실제 구현할 구조 또는 학습 방법 30~6000자가 필요합니다.")?;
    let targets=body["targetFiles"].as_array()
        .filter(|files|!files.is_empty()&&files.len()<=12)
        .ok_or("변경할 소스 파일은 1~12개여야 합니다.")?;
    let root=fs::canonicalize(&state.config.workspace).map_err(|e|e.to_string())?;
    let mut target_set=BTreeSet::new();
    let mut observed=serde_json::Map::new();
    for file in targets {
        let path=file.as_str().ok_or("소스 경로는 문자열이어야 합니다.")?;
        if !intelligence_editable_path(path)
            || !target_set.insert(path.to_string())
            || !(path.starts_with("src/")||path.starts_with("web/src/")
                || path.starts_with("src-tauri/src/")) {
            return Err("허용되지 않은 구성요소 파일 또는 중복 경로".into());
        }
        let target=root.join(path);
        if target.exists() {
            let real=fs::canonicalize(&target).map_err(|e|e.to_string())?;
            if !real.starts_with(&root)||!real.is_file(){
                return Err("작업공간 외부 소스를 변경할 수 없습니다.".into());
            }
            let bytes=fs::read(real).map_err(|e|e.to_string())?;
            observed.insert(path.to_string(),json!({
                "sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len()
            }));
        } else {
            // New code modules are essential for real architectural novelty.
            // Only an existing canonical parent under the detached checkout
            // can authorize a new source. A proposal does NOT create it.
            let parent=target.parent().ok_or("새 소스의 상위 경로 없음")?;
            let canonical=fs::canonicalize(parent)
                .map_err(|_|"새 소스는 기존 소스 디렉터리 아래에 만드세요.".to_string())?;
            if !canonical.starts_with(&root)||!canonical.is_dir(){
                return Err("새 소스의 상위 폴더가 실험 루트 밖입니다.".into());
            }
            observed.insert(path.to_string(),json!({"plannedNewSource":true}));
        }
    }
    let mut component_files=body["componentFiles"].clone();
    if roles.len()==1 && !component_files.is_object(){
        let mut map=serde_json::Map::new();
        map.insert(roles[0].clone(),json!(targets));
        component_files=Value::Object(map);
    }
    for role in &roles {
        let files=component_files[role].as_array()
            .filter(|rows|!rows.is_empty())
            .ok_or("각 구성요소에 실제 변경 대상 파일을 배정하세요.")?;
        if files.iter().any(|file|file.as_str()
            .is_none_or(|path|!target_set.contains(path))) {
            return Err("구성요소 파일이 전체 수정 대상에 포함되지 않습니다.".into());
        }
    }
    if let Some(program)=body["actionProgram"].as_array(){
        if program.len()<2 || program.len()>8
            || !program.iter().all(|step|step.as_str().is_some_and(|tool|
                matches!(tool,"code_search"|"workspace_read_range"|"workspace_replace"
                    |"workspace_replace_range"|"workspace_replace_function"
                    |"workspace_write"|"rsi_system_plan"|"rsi_method_propose"))) {
            return Err("새 개선 프로그램에는 허용된 도구 2~8개가 필요합니다.".into());
        }
        let read=program.iter().position(|step|step=="workspace_read_range");
        let write=program.iter().position(|step|matches!(step.as_str(),Some(
            "workspace_replace"|"workspace_replace_range"
            |"workspace_replace_function"|"workspace_write")));
        if read.zip(write).is_none_or(|(r,w)|r>=w) {
            return Err("개선 프로그램은 실제 소스 관찰 후 소스 편집을 수행해야 합니다.".into());
        }
    }
    let next_files=body["nextTargetFiles"].as_array()
        .cloned().unwrap_or_default();
    if next_files.len()>3 || next_files.iter().any(|file|file.as_str()
        .is_none_or(|path|!intelligence_editable_path(path))) {
        return Err("다음 세대의 목표 파일은 허용된 소스 3개 이하여야 합니다.".into());
    }
    let stages=body["successorStages"].as_array().cloned().unwrap_or_default();
    if stages.len()>10 {
        return Err("후손의 새로운 개선 프로그램은 최대 10단계입니다.".into());
    }
    let mut stage_ids=BTreeSet::new();
    for stage in &stages{
        let role=stage["role"].as_str().filter(|r|valid_role(r))
            .ok_or("후손 단계의 지능 구성요소가 유효하지 않습니다.")?;
        let domain=stage["domain"].as_str().filter(|d|
            crate::engine::rsi_evolve::DOMAINS.contains(d))
            .ok_or("후손 단계의 탐색 분야가 유효하지 않습니다.")?;
        let file=stage["path"].as_str().filter(|p|intelligence_editable_path(p))
            .ok_or("후손 단계의 소스 경로가 유효하지 않습니다.")?;
        if !stage_ids.insert((role.to_string(),domain.to_string(),file.to_string())){
            return Err("중복된 개선 단계".into());
        }
        let path=root.join(file);
        if !path.is_file() && !target_set.contains(file){
            return Err("후손의 개선 단계는 실재하거나 이번 세대가 생성할 소스를 가리켜야 합니다.".into());
        }
        let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        if !matches!(learning,"avoid_failed"|"repair_failed"|"explore_novel"|"repeat_verified")
            || !matches!(stage["executionPolicy"].as_str().unwrap_or("grounded_edit"),
                "grounded_edit"|"meta_rewrite_first"|"cross_component_revision"
                |"repair_then_edit") {
            return Err("후손 개선 단계의 학습·실행 정책 오류".into());
        }
        if !stage["objective"].as_str().is_some_and(|v|v.len()>=15 && v.len()<=1200)
            || !stage["priority"].as_i64().is_none_or(|v|(-20..=20).contains(&v))
            || !stage["maxRetries"].as_i64().is_none_or(|v|(0..=8).contains(&v)) {
            return Err("후손 단계의 가설·우선순위 또는 재시도 한도 오류".into());
        }
    }
    let method_hypothesis=body["methodHypothesis"].as_str().unwrap_or("");
    if method_hypothesis.len()>1200 {
        return Err("개선 방법의 인과 설명이 너무 깁니다.".into());
    }
    let generator=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 소스 버전이 없습니다.")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        generator,component,&roles,domain,hypothesis,design,targets,
        &component_files,&body["actionProgram"],&stages,&next_files,method_hypothesis
    ]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),
        "source":"model_authored_system_design","status":"proposed",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":generator,
        "component":component,"components":roles,"componentFiles":component_files,
        "nextDomain":domain,"nextTargetFiles":next_files,
        "hypothesis":hypothesis,"design":design,"targetFiles":targets,
        "methodHypothesis":method_hypothesis,
        "actionProgram":body["actionProgram"],
        "successorStages":stages,
        "observedSources":observed,
        "qualityProven":false,"adopted":false
    });
    session["rsiSystemDesign"]=proposal.clone();
    state.store.put("session",&session).ok_or("개선 설계 기록 저장 실패")?;
    state.store.audit("rsi.system_design_proposed",json!({
        "campaignId":campaign_id,"generationJobId":child_id,
        "designId":proposal["id"],"roles":proposal["components"],
        "targetFiles":proposal["targetFiles"],"qualityProven":false
    }));
    Ok(json!({"proposal":proposal,
        "next":"이제 실제 Rust 소스를 수정하세요. 변경된 모든 targetFiles가 채택되기 전에는 설계나 개선 프로그램을 다음 세대에 계승하지 않습니다."}))
}

/// The model's accepted goal remains an input to its own next binary.
/// An unadopted written design cannot dictate the descendant's behavior.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    let design=history.iter().rev().find_map(accepted_model_design)?;
    let next=design["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&next)
        .then(||next.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_self_model_tracks_real_source_revision_and_covers_all_capabilities() {
        let root=std::env::temp_dir().join(format!("rsi-system-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/algorithms.rs"),"pub fn policy() {}").unwrap();
        fs::write(root.join("src/engine/rsi_evolve.rs"),"pub fn evolve() {}").unwrap();
        let history=vec![json!({"generatorCommit":"first","accepted":true,
            "evaluation":{"verdict":"improved","metrics":{"improvedHoldouts":1}}})];
        let first=blueprint(&root,"commit-one",&history);
        assert_eq!(first["architecture"].as_array().unwrap().len(),7);
        assert_eq!(first["generatorCommit"],"commit-one");
        assert_eq!(first["historyCount"],1);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace_range"));
        let baseline=first["architecture"][0]["representativeSources"][0]["sha256"].clone();
        fs::write(root.join("src/engine/algorithms.rs"),"pub fn changed_policy() {}").unwrap();
        let second=blueprint(&root,"commit-two",&history);
        assert_ne!(baseline,second["architecture"][0]["representativeSources"][0]["sha256"]);
        assert_ne!(first["generatorCommit"],second["generatorCommit"]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn model_can_propose_design_only_in_its_own_active_system_generation() {
        let root=std::env::temp_dir().join(format!("rsi-system-proposal-{}",uuid::Uuid::new_v4()));
        let workspace=root.join("work");
        fs::create_dir_all(workspace.join("src/engine")).unwrap();
        fs::write(workspace.join("src/engine/algorithms.rs"),"pub fn base() {}").unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:workspace.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve",
            "status":"running","input":{"rsiNativeApproved":true}})).unwrap();
        state.store.put("job",&json!({"id":"descendant","kind":"rsi_generation",
            "status":"running","input":{"rsiCampaignId":"campaign",
                "rsiSystemEvolution":true,"rsiGeneratorCommit":"revision-a"}})).unwrap();
        state.store.put("session",&json!({"id":"session","rsiRun":true,
            "rsiCampaignId":"campaign","rsiGenerationJobId":"descendant"})).unwrap();
        let request=json!({"component":"algorithms","nextDomain":"learning",
            "hypothesis":"The compiled planner has no causal source revision feedback.",
            "design":"Introduce an inherited source-specific planning policy with evidence checks.",
            "targetFiles":["src/engine/algorithms.rs"]});
        let proposed=propose(&state,"session",&request).unwrap();
        assert_eq!(proposed["proposal"]["status"],"proposed");
        assert_eq!(proposed["proposal"]["qualityProven"],false);
        assert_eq!(proposed["proposal"]["generatorCommit"],"revision-a");
        assert_eq!(state.store.get("session","session").unwrap()["rsiSystemDesign"]["nextDomain"],"learning");
        let mut bad=request.clone();
        bad["targetFiles"]=json!(["src/engine/../escape.rs"]);
        assert!(propose(&state,"session",&bad).is_err());
        let mut disabled=state.store.get("job","descendant").unwrap();
        disabled["input"]["rsiSystemEvolution"]=json!(false);
        state.store.put("job",&disabled).unwrap();
        assert!(propose(&state,"session",&request).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_adopted_model_design_can_control_a_new_compiled_descendant() {
        let base=json!({"generatorCommit":"parent","jobId":"worker",
            "acceptedCommit":"cccccccccccccccccccccccccccccccccccccccc",
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedSourceHashes":{"src/engine/learning.rs":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"parent","generationJobId":"worker",
                "targetFiles":["src/engine/learning.rs"],
                "nextDomain":"learning"}});
        assert_eq!(inherited_domain(&[base.clone()]),Some("learning".into()));
        for (key,value) in [("accepted",json!(false)),
                            ("pairedRevalidated",json!(false)),
                            ("candidateOrigin",json!("native_inductive_source_mutation"))] {
            let mut invalid=base.clone();
            invalid[key]=value;
            assert_eq!(inherited_domain(&[invalid]),None);
        }
        let mut irrelevant=base.clone();
        irrelevant["systemDesign"]["targetFiles"]=json!(["src/engine/browser.rs"]);
        assert_eq!(inherited_domain(&[irrelevant]),None);
        let mut wrong=base;
        wrong["systemDesign"]["generatorCommit"]=json!("stale");
        assert_eq!(inherited_domain(&[wrong]),None);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_outside_experiment_cannot_enter_descendant_model() {
        let root=std::env::temp_dir().join(format!("rsi-system-escape-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts",root.join("src/engine/algorithms.rs")).unwrap();
        let map=blueprint(&root,"pinned",&[]);
        assert_eq!(map["architecture"][0]["representativeSources"].as_array().unwrap().len(),0);
        fs::remove_dir_all(root).unwrap();
    }
}
