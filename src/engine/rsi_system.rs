//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs,collections::{BTreeMap,BTreeSet}};

#[path = "rsi_system/design.rs"]
mod design;
pub(crate) use design::propose;

#[path = "rsi_system/replay.rs"]
mod replay;
pub(crate) use replay::revalidated_history;
#[path = "rsi_system/lineage.rs"]
mod lineage;
pub(crate) use lineage::{inherited_stage,inherited_domain,verified_handoff};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/rsi_worktree.rs","src/engine/chat/runtime.rs","src/engine/decompose.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("scientific_method",&["src/engine/rsi_recursive_scientist.rs",
        "src/engine/rsi_improvement_algebra.rs",
        "src/engine/rsi_method_generalization.rs",
        "src/engine/rsi/lineage/rsi_model_lineage.rs",
        "src/engine/rsi/learning/rsi_negative_transfer.rs"]),
    ("experimental_design",&["src/engine/rsi/evaluation/rsi_factorial.rs"]),
    ("improvement_methods",&["src/engine/rsi_meta_controller.rs","src/engine/rsi_prequential.rs","src/engine/rsi_evolve.rs","src/engine/rsi_mutation.rs","src/engine/rsi_system.rs","src/engine/rsi_symbiosis.rs"]),
    ("models",&["src/engine/models.rs","src/engine/cli_model.rs","src/engine/router.rs","src/engine/gateway.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

/// Inventory every compilable agent/frontend source under the selected
/// experimental revision, not merely a hand-picked optimization fixture.
/// An inventory hash changes when ANY generation changes the executable
/// system's source graph; the prompt can remain short without losing lineage.
fn source_inventory(root:&Path)->Vec<Value> {
    let Ok(root)=fs::canonicalize(root) else {return Vec::new()};
    // Traverse deterministically even when there are more than 2048 sources.
    // An unstable read_dir order changes the pinned tree digest without an edit.
    let mut pending=vec![root.join("src-tauri").join("src"),
        root.join("web").join("src"),root.join("src")];
    let mut rows=Vec::new();
    while let Some(directory)=pending.pop() {
        let Ok(entries)=fs::read_dir(&directory) else {continue};
        let mut entries=entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry|entry.file_name());
        let mut child_directories=Vec::new();
        for entry in entries {
            let Ok(file)=fs::canonicalize(entry.path()) else {continue};
            if !file.starts_with(&root){continue;}
            if file.is_dir() {
                child_directories.push(file);
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
        for directory in child_directories.into_iter().rev() {pending.push(directory);}
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

/// All existing source modules are candidates, including newly invented
/// model, learner, harness and improvement modules. Heuristic ownership is
/// a source-editable offspring policy, never a frozen evaluator taxonomy.
fn source_role(path:&str)->&'static str {
    if path.starts_with("web/src/") || path.starts_with("src-tauri/src/") {
        return "frontend";
    }
    // The executable RSI is no longer a flat list of rsi_*.rs files.
    // Deep planner / executor / evaluator / learner code remains an
    // improvement-method target in every rebuilt descendant's source map.
    if path.starts_with("src/engine/rsi/") {
        return "improvement_methods";
    }
    let name=path.rsplit('/').next().unwrap_or(path);
    if name.starts_with("model_") || name.starts_with("training")
        || matches!(name,"models.rs"|"cli_model.rs"|"router.rs"|"gateway.rs") {
        return "models";
    }
    // Heuristic discovery for unregistered modules; accepted model-authored
    // roles still override this fallback through their own source evidence.
    if name.starts_with("rsi_") || name.contains("meta_")
        || name.contains("_meta") || name.contains("improver")
        || matches!(name,"rsi.rs"|"algorithm_eval.rs"|"policy_patch.rs") {
        return "improvement_methods";
    }
    if path.contains("/chat/") || matches!(name,"chat.rs"|"jobs.rs"|"decompose.rs") {
        return "harness";
    }
    if name.starts_with("memory") || name.starts_with("room_") {return "memory";}
    if name.starts_with("learning") || name.starts_with("experience")
        || name.starts_with("procedure") || name.starts_with("agent_growth") {
        return "learning";
    }
    if name.starts_with("tools") || name.starts_with("browser") || name.starts_with("mcp") {
        return "tools";
    }
    "algorithms"
}

fn valid_role(name:&str)->bool {
    !name.is_empty() && name.len()<=64 && name.bytes().all(|c|
        c.is_ascii_alphanumeric() || c==b'_' || c==b'-')
}

fn role_domain(role:&Value)->&str {
    if let Some(domain)=role["domain"].as_str().filter(|d|
        crate::engine::rsi_evolve::DOMAINS.contains(d)) {return domain;}
    match role["role"].as_str().unwrap_or("") {
        "memory"=>"memory", "learning"=>"learning", "tools"=>"tools",
        "models"=>"models",
        "harness"=>"context", "frontend"=>"ui", _=>"improver"
    }
}

/// Verify a design's complete implementation, not one coincidentally edited
/// file. New/renamed intelligence roles use the same contract as existing ones.
pub(crate) fn design_implemented(design:&Value, paths:&[Value])->bool {
    let Some(targets)=design["targetFiles"].as_array().filter(|v|!v.is_empty()) else{return false;};
    targets.iter().all(|target|target.as_str().is_some_and(|path|
        crate::engine::rsi::safe_path(path) && paths.iter().any(|changed|changed.as_str()==Some(path))))
}

fn design_roles(design:&Value)->Vec<String> {
    design["components"].as_array().map(|roles|roles.iter().filter_map(Value::as_str)
        .filter(|role|valid_role(role)).map(str::to_string).collect())
        .unwrap_or_else(||design["component"].as_str().filter(|role|valid_role(role))
            .map(|role|vec![role.to_string()]).unwrap_or_default())
}

/// Ordinary agent work feeds system evolution as evidence, never as an
/// automatic success label. Retain record IDs so the model can inspect the
/// original execution rather than repeatedly collecting the same failure.
pub(crate) fn usage_evidence(state:&crate::state::AppState)->Value {
    let report=crate::engine::rsi_scan::report(state);
    let signals=report["signals"].as_array().into_iter().flatten()
        .filter(|signal|matches!(signal["kind"].as_str(),Some("execution_failures"|"failed_jobs")))
        .take(6).cloned().collect::<Vec<_>>();
    json!({"source":"recorded_agent_work","signals":signals,"checks":report["checks"],"qualityProven":false})
}

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
    // Multiple executable methods can belong to the same invented role.
    // A frontier of one heuristic source per role would silently strand a
    // method authored in any OTHER source file: expose independently adopted
    // method paths in addition to that role's baseline representative.
    let adopted_method_paths=history.iter().filter(|g|
        g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true)
        .flat_map(|g|g["systemDesign"]["improvementRules"]
            .as_array().into_iter().flatten().filter_map(move |rule|{
                let path=rule["methodPath"].as_str()?;
                let role=rule["methodRole"].as_str()?;
                (g["changedSourcePaths"].as_array().is_some_and(|changed|
                    changed.iter().any(|p|p==path))
                    && g["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|hash|hash.len()==64))
                    .then(||(role.to_owned(),path.to_owned()))
            }))
        .collect::<BTreeSet<_>>();
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
        let Some((path,sha,bytes))=sources.first().copied() else{continue;};
        rows.push(json!({"role":role,"domain":role_domain(component),"path":path,
            "sourceSha256":sha,"sourceBytes":bytes,"sourceAttempts":attempts.get(path).copied().unwrap_or(0),
            "usageSignalIds":related(path),"usageSignalCount":related(path).len(),
            "verifiedRoleChanged":covered.contains(role),"qualityProven":false,
            "reason":if covered.contains(role){"extend_verified_source"}else{"unexplored_component"}}));
        for (method_path,method_sha,method_bytes) in sources.iter().copied() {
            let predictor=method_path=="src/engine/rsi_prequential.rs"
                && role=="improvement_methods";
            if method_path==path || !(predictor || adopted_method_paths.contains(
                &(role.to_owned(),method_path.to_owned()))) {continue;}
            rows.push(json!({"role":role,"domain":role_domain(component),
                "path":method_path,"sourceSha256":method_sha,
                "sourceBytes":method_bytes,
                "sourceAttempts":attempts.get(method_path).copied().unwrap_or(0),
                "usageSignalIds":related(method_path),
                "usageSignalCount":related(method_path).len(),
                "verifiedRoleChanged":true,"qualityProven":false,
                "reason":if predictor{"source_editable_prediction_method"}
                    else{"inherited_model_written_improvement_operator"}}));
        }
    }
    rows.sort_by(|a,b|b["usageSignalCount"].as_u64().cmp(&a["usageSignalCount"].as_u64())
        .then((a["verifiedRoleChanged"]==true).cmp(&(b["verifiedRoleChanged"]==true)))
        .then(a["sourceAttempts"].as_u64().cmp(&b["sourceAttempts"].as_u64()))
        .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
        .then(a["role"].as_str().cmp(&b["role"].as_str())));
    rows
}

/// Immutable fact validation intentionally accepts different descendant
/// rankings/roles. Recomputing architecture_frontier here would freeze the
/// previous generation's intelligence and reject legitimate evolution.
pub(crate) fn validate_plan_evidence(state:&crate::state::AppState, root:&Path,
    commit:&str,index_id:&str,plan:&Value)->Result<(),String> {
    let model=&plan["systemBlueprint"];let facts=source_facts(root);
    if model["generatorCommit"]!=commit || model["sourceTreeSha256"]!=facts["sha256"]
        || model["sourceFileCount"]!=facts["fileCount"] {return Err("후손의 자기모형이 현재 전체 소스와 다릅니다.".into());}
    let roles=model["architecture"].as_array().filter(|roles|!roles.is_empty()&&roles.len()<=64)
        .ok_or("후손의 구성요소 목록이 없습니다.")?;
    let mut names=BTreeSet::new();
    for role in roles {
        let name=role["role"].as_str().filter(|name|valid_role(name)).ok_or("구성요소 이름 오류")?;
        if !names.insert(name){return Err("중복 구성요소 이름".into());}
        for entry in role["representativeSources"].as_array().into_iter().flatten() {
            validate_source_entry(state,root,index_id,entry,"sha256")?;
        }
    }
    let rows=plan["architectureFrontier"].as_array().filter(|rows|rows.len()<=64)
        .ok_or("후손의 구조 탐색 근거가 없습니다.")?;
    for row in rows {
        if !row["role"].as_str().is_some_and(|role|names.contains(role))
            || !row["domain"].as_str().is_some_and(|domain|crate::engine::rsi_evolve::DOMAINS.contains(&domain)) {
            return Err("후손 탐색 근거의 구성요소·분야 오류".into());
        }
        // The successor may change the ranking algorithm, but it cannot
        // attribute a source file to a role that its own pinned architecture
        // did not actually identify.
        let mapped=roles.iter().find(|role|role["role"]==row["role"])
            .and_then(|role|role["representativeSources"].as_array())
            .is_some_and(|files|files.iter().any(|file|
                file["path"]==row["path"] &&
                file["sha256"]==row["sourceSha256"]));
        if !mapped {
            return Err("구조 탐색의 역할과 실제 소스 연결이 다릅니다.".into());
        }
        validate_source_entry(state,root,index_id,row,"sourceSha256")?;
    }
    if !plan["architectureTarget"].is_null() && !rows.iter().any(|row|*row==plan["architectureTarget"]) {
        return Err("선택한 구조 과제의 실제 소스 근거가 없습니다.".into());
    }
    Ok(())
}

fn validate_source_entry(state:&crate::state::AppState,root:&Path,index_id:&str,
    entry:&Value,hash_key:&str)->Result<(),String> {
    let path=entry["path"].as_str().filter(|path|crate::engine::rsi::safe_path(path)).ok_or("허용되지 않은 소스 경로")?;
    let sha=entry[hash_key].as_str().ok_or("소스 해시가 없습니다.")?;
    let base=fs::canonicalize(root).map_err(|e|e.to_string())?;
    let actual=fs::canonicalize(base.join(path)).map_err(|e|e.to_string())?;
    if !actual.starts_with(&base) || !actual.is_file()
        || crate::engine::code_index::indexed_source_hash(state,index_id,path).as_deref()!=Some(sha)
        || format!("{:x}",Sha256::digest(fs::read(actual).map_err(|e|e.to_string())?))!=sha {
        return Err(format!("현재 소스와 다른 탐색 근거: {path}"));
    }
    Ok(())
}

/// Read only source assets under the experiment's pinned checkout. The
/// coordinator cannot author or alter this blueprint without changing its
/// measured generator revision; no holdout content is read here.
pub(crate) fn blueprint(root:&Path, revision:&str, history:&[Value])->Value {
    let canonical_root=fs::canonicalize(root).ok();
    let files=source_inventory(root);
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
        // Dynamic modules feed the exact same source graph and SHA contract.
        // Limit prompt representatives, not the complete source-tree digest.
        let discovered=files.iter().filter(|item|item["path"].as_str()
            .is_some_and(|path|source_role(path)==*role)).count();
        for item in files.iter().filter(|item|item["path"].as_str()
            .is_some_and(|path|source_role(path)==*role)) {
            if inventory.len()>=16 {break;}
            if !inventory.iter().any(|v|v["path"]==item["path"]) {
                inventory.push(item.clone());
            }
        }
        let row=json!({"role":role,"representativeSources":inventory});
        let domain=role_domain(&row).to_string();
        components.push(json!({"role":role,"domain":domain,
            "representativeSources":inventory,"discoveredSourceCount":discovered}));
    }
    let verified_new_roles=crate::engine::rsi_open_topology::inherited_roles(history)
        .into_iter().filter_map(|record|record["role"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for generation in history.iter().filter(|generation|generation["accepted"]==true
        && generation["pairedRevalidated"]==true
        && generation["candidateOrigin"]=="model_driven_detached_worktree"
        && generation["evaluation"]["metrics"]["eligible"]==true) {
        let design=&generation["systemDesign"];
        for role in design_roles(design) {
            // Newly named roles must have an independently adopted
            // executable birth/integration record, not just a design label.
            if components.iter().any(|item|item["role"]==role){continue;}
            if !verified_new_roles.contains(&role) {continue;}
            let paths=design["componentFiles"][&role].as_array().or_else(||
                if design_roles(design).len()==1 {design["targetFiles"].as_array()}else{None});
            let sources=paths.into_iter().flatten().filter_map(|path| {
                let path=path.as_str().filter(|path|crate::engine::rsi::safe_path(path))?;
                let base=fs::canonicalize(root).ok()?;
                let target=fs::canonicalize(base.join(path)).ok()?;
                if !target.starts_with(&base)||!target.is_file(){return None;}
                let bytes=fs::read(target).ok()?;
                let sha=format!("{:x}",Sha256::digest(&bytes));
                if generation["acceptedSourceHashes"][path]!=sha {return None;}
                Some(json!({"path":path,"sha256":sha,"bytes":bytes.len()}))
            }).collect::<Vec<_>>();
            if !sources.is_empty() && components.len()<64 {
                components.push(json!({"role":role,"domain":design["nextDomain"],"representativeSources":sources}));
            }
        }
    }
    // Independently adopted, runtime-integrated NEW intelligence modules
    // join the same architecture frontier as inherited built-in components.
    // This preserves the new subsystem as a future self-redesign TARGET.
    for component in crate::engine::rsi_open_topology::source_components(root,history) {
        if !components.iter().any(|old|old["role"]==component["role"])
            && components.len()<64 {
            components.push(component);
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
        "inheritedDesign":history.last().filter(|g|g["accepted"]==true && g["pairedRevalidated"]==true && g["evaluation"]["metrics"]["eligible"]==true).map(|g|g["systemDesign"].clone()).unwrap_or(Value::Null),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_source_editing_runtime",
        "immutableBoundary":["experiment_baseline","frozen_paired_evaluator","budget","production_apply_gate"]
    })
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
        "[COMPILED DESCENDANT: VERSIONED INTELLIGENCE RUNTIME]\n\
        Running source revision: {}. Assigned task: {}.\n\
        Your own code contains the domain selector, search policy, planning \
        harness, context, memory, learning, source tools and improvement \
        methods. These are evolvable source, not fixed hand-written edit rules.\n\
        Use the CURRENT revision's indexed evidence, previous compiler failures \
        and accepted/rejected candidate receipts to decide what to try next. \
        Inspect current source and its SHA via code_search/workspace_read_range. \
        Use workspace_replace_range, workspace_replace or workspace_write for \
        REAL source edits; a proposal or read-only report is not an improvement.\n\
        If the task is whole-system self-redesign: find a causal limitation of \
        your own algorithm/harness/memory/learning/tools/improver, propose a \
        falsifiable structural change through rsi_system_plan, and implement \
        the actual code, including new modules when justified. For coupled redesigns use \
        component=system, list components, and map every component to its \
        targetFiles via componentFiles. Implement ALL declared target files. A changed \
        improvement procedure must be executed and evaluated in its successor.\n\
        If assigned a concrete downstream coding task: solve THAT task with \
        your current improved intelligence. Do not substitute unrelated agent \
        redesign for the requested patch. The downstream result measures \
        whether the improved generator really solves more problems.\n\
        You can propose source-bound interactionEdges (fromRole/toRole/fromPath/toPath,
        mechanism, observable) that specify HOW improving one intelligence subsystem
        will improve another. You may also write successorStages that transfer
        a new research/learning/editing policy to the real compiled next child,
        with an explicit return to the originally improved subsystem. The
        supervisor validates current source identities, not your conjecture. \
        You may propose an optional actionProgram (ordered allowed tool calls) \
        within rsi_system_plan to redesign your NEXT descendant's real \
        read/search/edit sequence. An accepted source change can transfer \
        this unproven program into the child's executable routing policy. \
        You may propose reusable executable methods with rsi_method_propose \
        and exploratory algorithm selectors with algorithm_propose. Registration \
        is not success. The independent paired evaluator alone decides \
        experimental adoption; never edit the evaluator or claim unevaluated gain.\n\
        Current architecture (representative files; ALL indexed source is open):\n{}\n\
        Measured predecessor generations (not present-tense claims):\n{}\n\
        Last independently inherited design hypothesis: {}\n\
        Report exact changed source and what still requires evaluation.",
        self_model["generatorCommit"],owner_objective,components,history,inherited)
}



/// Behavioral successor instruction derived from *observed* episodes, not
/// model optimism. The rebuilt descendant uses the outcome to choose whether
/// to discover, implement, repair, change approach or transfer an adopted gain.
pub(crate) fn improvement_directive(history:&[Value])->Value {
    let Some(previous)=history.last() else {
        return json!({"mode":"discover","sourceGenerationId":null,
            "reason":"no_recorded_generation"});
    };
    let observed=previous["evaluation"]["metrics"]["eligible"]==true;
    let has_patch=previous["candidateId"].is_string();
    let new_findings=previous["newFindings"].as_u64().unwrap_or(0);
    let prior=previous["improvementDirective"]["mode"].as_str().unwrap_or("");
    let failed_build=previous["evaluation"]["verdict"]=="build_failed"
        || previous["error"].as_str().is_some_and(|s|
            s.contains("build") || s.contains("compiler") || s.contains("compile"));
    let matched_control=previous["accepted"]==true && observed
        && previous["methodAblation"]["source"]
            =="compiled_method_on_off_real_worker_v1"
        && previous["methodAblation"]["methodAssociationObserved"]==true
        && previous["methodAblation"]["qualityProven"]==true
        && previous["methodAblation"]["controlEvidenceSha256"].is_string()
        && previous["methodAblation"]["treatmentEvidenceSha256"]
            ==previous["evaluation"]["evidenceSha256"];
    let mode=if matched_control {
        "evolve_executed_method_from_feedback"
    }else if previous["accepted"]==true && observed {
        "extend_verified_capability"
    }else if failed_build {
        if prior=="repair_counterexample"{"change_approach"}else{"repair_counterexample"}
    }else if has_patch {
        if prior=="revise_candidate"{"change_approach"}else{"revise_candidate"}
    }else if new_findings>0 {
        if prior=="implement_research"{"change_approach"}else{"implement_research"}
    }else{"change_approach"};
    json!({
        "sourceGenerationId":previous["id"],
        "mode":mode,"domain":previous["domain"],
        "previousAccepted":previous["accepted"]==true && observed,
        "previousVerdict":previous["evaluation"]["verdict"],
        "previousCandidateId":previous["candidateId"],
        "methodFeedback":if matched_control{json!({
            "source":"replayed_real_method_control_v1",
            "sourceGenerationId":previous["id"],
            "methodFamilyId":previous["methodAblation"]["methodFamilyId"],
            "originalMethodSourceSha256":previous["methodAblation"]["methodSourceSha256"],
            "controlId":previous["methodAblation"]["controlId"],
            "controlEvaluationId":previous["methodAblation"]["controlEvaluationId"],
            "treatmentEvaluationId":previous["methodAblation"]["treatmentEvaluationId"],
            "controlEvidenceSha256":previous["methodAblation"]["controlEvidenceSha256"],
            "treatmentEvidenceSha256":previous["methodAblation"]["treatmentEvidenceSha256"],
            "treatmentOnlyWins":previous["methodAblation"]["treatmentOnlyWins"],
            "treatmentQualityWins":previous["methodAblation"]["treatmentQualityWins"],
            "requiresNewRevisionAndSuccessorExecution":true
        })}else{Value::Null},
        "previousFocusFiles":previous["focusFiles"].as_array()
            .map(|a|a.iter().take(4).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "previousError":previous["error"].as_str().unwrap_or("")
            .chars().take(500).collect::<String>(),
        "researchArtifactHashes":previous["knowledgeArtifactHashes"].as_array()
            .map(|a|a.iter().take(6).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "newFindings":new_findings
    })
}

/// A model-authored improvement program is translated into the same executable
/// stages consumed by the real chat/runtime.rs tool router. It cannot bypass
/// hash-grounded reads, turn a proposal into an accepted result, or execute
/// unreviewed arbitrary shell code.
pub(crate) fn compile_action_program(sequence:&Value)->Option<Value> {
    let steps=sequence.as_array()?.iter().map(Value::as_str)
        .collect::<Option<Vec<_>>>()?;
    if !(2..=8).contains(&steps.len()) || steps.iter().any(|step|!matches!(*step,
        "code_search"|"workspace_read_range"|"workspace_replace"
        |"workspace_replace_range"|"workspace_write"|"rsi_system_plan"
        |"rsi_method_propose")) {return None;}
    let read=steps.iter().position(|step|*step=="workspace_read_range")?;
    let write=steps.iter().position(|step|matches!(*step,
        "workspace_replace"|"workspace_replace_range"|"workspace_write"))?;
    if read>=write {return None;}
    let mut stages=Vec::new();
    if steps[..read].contains(&"code_search"){stages.push("discover".to_string());}
    stages.push("ground".to_string());
    if steps[read+1..write].contains(&"code_search"){stages.push("contrast".to_string());}
    stages.push("edit".to_string());
    if steps[write+1..].contains(&"workspace_read_range"){
        stages.push("reground".to_string());
    }
    stages.push("verify".to_string());
    let executable=crate::engine::rsi_edit_program::normalize(&json!(stages));
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([steps,executable])).ok()?));
    Some(json!({"kind":"model_authored_executable_meta_program_v1",
        "revision":revision,"sequence":steps,"stages":executable,
        "qualityProven":false,"training":"verified_descendant_design_only"}))
}

/// The last *independently paired, adopted* model design that contains a
/// well-formed new improvement method may seed the next child. The child's
/// use of the method remains an UNMEASURED trial until later evaluation.
pub(crate) fn inherited_program(
    state:&crate::state::AppState,history:&[Value]
)->Option<Value> {
    for generation in history.iter().rev(){
        if generation["accepted"]!=true || generation["pairedRevalidated"]!=true
            || generation["candidateOrigin"]!="model_driven_detached_worktree"
            || generation["evaluation"]["metrics"]["eligible"]!=true {continue;}
        let design=&generation["systemDesign"];
        if design["source"]!="model_authored_system_design"
            || design["generatorCommit"]!=generation["generatorCommit"]
            || design["generationJobId"]!=generation["jobId"] {continue;}
        let Some(program)=compile_action_program(&design["actionProgram"]) else{continue};
        let Some(targets)=design["targetFiles"].as_array().filter(|t|!t.is_empty()) else{continue};
        let Some(id)=generation["candidateId"].as_str() else{continue};
        let Some(candidate)=state.store.get("rsi_candidate",id) else{continue};
        if candidate["status"]!="verified"
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        if !design_implemented(design,&changed)
            || !targets.iter().all(|file|file.as_str().is_some_and(|path|
                generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|hash|hash.len()==64
                        && hash.bytes().all(|b|b.is_ascii_hexdigit())))) {
            continue;
        }
        let mut trial=program;
        trial["inheritedFromCommit"]=generation["acceptedCommit"].clone();
        trial["inheritedFromDesignId"]=design["id"].clone();
        trial["inheritedFromCandidateId"]=candidate["id"].clone();
        trial["trialStatus"]=json!("unmeasured_executable_method_transfer");
        return Some(trial);
    }
    None
}

#[cfg(test)]
#[path = "rsi/tests/system.rs"]
mod tests;

