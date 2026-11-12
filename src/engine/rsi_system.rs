//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs,collections::{BTreeMap,BTreeSet}};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/rsi_worktree.rs","src/engine/chat/runtime.rs","src/engine/decompose.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&["src/engine/rsi_evolve.rs","src/engine/rsi_mutation.rs","src/engine/rsi_system.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

/// Inventory every compilable agent/frontend source under the selected
/// experimental revision, not merely a hand-picked optimization fixture.
/// An inventory hash changes when ANY generation changes the executable
/// system's source graph; the prompt can remain short without losing lineage.
fn source_inventory(root:&Path)->Vec<Value> {
    let Ok(root)=fs::canonicalize(root) else {return Vec::new()};
    let mut pending=vec![root.join("src"),root.join("web").join("src")];
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
                Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"));
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

fn valid_role(name:&str)->bool {
    !name.is_empty() && name.len()<=64 && name.bytes().all(|c|
        c.is_ascii_alphanumeric() || c==b'_' || c==b'-')
}

fn role_domain(role:&Value)->&str {
    if let Some(domain)=role["domain"].as_str().filter(|d|
        crate::engine::rsi_evolve::DOMAINS.contains(d)) {return domain;}
    match role["role"].as_str().unwrap_or("") {
        "memory"=>"memory", "learning"=>"learning", "tools"=>"tools",
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
        let row=json!({"role":role,"representativeSources":inventory});
        let domain=role_domain(&row).to_string();
        components.push(json!({"role":role,"domain":domain,"representativeSources":inventory}));
    }
    for generation in history.iter().filter(|generation|generation["accepted"]==true
        && generation["pairedRevalidated"]==true
        && generation["candidateOrigin"]=="model_driven_detached_worktree"
        && generation["evaluation"]["metrics"]["eligible"]==true) {
        let design=&generation["systemDesign"];
        for role in design_roles(design) {
            if components.iter().any(|item|item["role"]==role){continue;}
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
    let files=source_inventory(root);
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
    let mode=if previous["accepted"]==true && observed {
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
        "previousFocusFiles":previous["focusFiles"].as_array()
            .map(|a|a.iter().take(4).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "previousError":previous["error"].as_str().unwrap_or("")
            .chars().take(500).collect::<String>(),
        "researchArtifactHashes":previous["knowledgeArtifactHashes"].as_array()
            .map(|a|a.iter().take(6).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "newFindings":new_findings
    })
}

/// A model-authored design is an unproven experiment. It cannot modify the
/// frozen evaluator or steer successors unless source changes are adopted.
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
        || child["cancelRequested"]==true{
        return Err("진행 중인 전체 시스템 자기개선 후손만 설계할 수 있습니다.".into());
    }
    // A succeeding intelligence may invent a new architecture component.
    // Requiring membership in the ORIGINAL seven subsystem names would make
    // every valid redesign fail before its source module could be created.
    let component=body["component"].as_str()
        .filter(|name|!name.is_empty() && name.len()<=64
            && name.bytes().all(|c|c.is_ascii_alphanumeric()
                || c==b'_' || c==b'-'))
        .ok_or("시스템 구성요소 이름은 영문·숫자·밑줄·하이픈 1~64자여야 합니다.")?;
    let components=if component=="system" {
        let roles=body["components"].as_array().filter(|roles|(2..=32).contains(&roles.len()))
            .ok_or("전체 시스템 설계에는 구성요소 2~32개가 필요합니다.")?;
        let mut seen=BTreeSet::new();
        for role in roles {
            let name=role.as_str().filter(|name|valid_role(name)&&*name!="system").ok_or("구성요소 이름 오류")?;
            if !seen.insert(name){return Err("중복 구성요소 이름".into());}
        }
        roles.clone()
    }else{vec![json!(component)]};
    let next=body["nextDomain"].as_str()
        .filter(|name|crate::engine::rsi_evolve::DOMAINS.contains(name))
        .ok_or("다음 개선 분야 오류")?;
    let hypothesis=body["hypothesis"].as_str()
        .filter(|v|v.trim().len()>=20 && v.len()<=2000)
        .ok_or("검증 가능한 가설 20~2000자가 필요합니다.")?;
    let design=body["design"].as_str()
        .filter(|v|v.trim().len()>=30 && v.len()<=6000)
        .ok_or("실행할 설계 30~6000자가 필요합니다.")?;
    let paths=body["targetFiles"].as_array()
        .filter(|rows|!rows.is_empty()&&rows.len()<=24 && (component!="system" || rows.len()>=2))
        .ok_or("변경할 실제 소스 1~24개를 선택하고, 다중 설계는 2개 이상 지정하세요.")?;
    let root=fs::canonicalize(&state.config.workspace).map_err(|e|e.to_string())?;
    let mut unique=std::collections::BTreeSet::new();
    for path in paths {
        let path=path.as_str().ok_or("소스 경로 문자열이 필요합니다.")?;
        if !crate::engine::rsi::safe_path(path)||!unique.insert(path.to_string()){
            return Err("중복 또는 허용되지 않은 소스 경로입니다.".into());
        }
        let proposed=root.join(path);
        if proposed.exists() {
            let file=fs::canonicalize(&proposed)
                .map_err(|_|"설계 대상 경로를 확인할 수 없습니다.".to_string())?;
            if !file.starts_with(&root)||!file.is_file(){
                return Err("외부 파일은 설계 범위가 아닙니다.".into());
            }
        } else {
            // A new intelligence architecture may create additional modules.
            // Resolve the existing parent to exclude symlink escapes.
            let parent=proposed.parent()
                .and_then(|dir|fs::canonicalize(dir).ok())
                .ok_or("새 파일을 만들 실제 소스 폴더가 없습니다.")?;
            if !parent.starts_with(&root)
                || !matches!(proposed.extension().and_then(|ext|ext.to_str()),
                    Some("rs"|"ts"|"tsx"|"js"|"jsx")) {
                return Err("새 모듈은 현재 저장소의 소스 폴더 아래에 작성해야 합니다.".into());
            }
        }
    }
    let mut assignments=serde_json::Map::new();
    let mut assigned=BTreeSet::new();
    for role in &components {
        let name=role.as_str().ok_or("구성요소 이름 오류")?;
        let files=if component=="system" {
            body["componentFiles"][name].as_array().filter(|files|!files.is_empty())
                .ok_or("다중 설계에는 각 구성요소의 componentFiles 소스 연결이 필요합니다.")?.clone()
        }else{paths.clone()};
        let mut unique=BTreeSet::new();
        for file in &files {
            let path=file.as_str().ok_or("구성요소 소스 경로 오류")?;
            if !paths.iter().any(|target|target.as_str()==Some(path)) || !unique.insert(path) {
                return Err("구성요소 소스는 targetFiles에 중복 없이 포함돼야 합니다.".into());
            }
            assigned.insert(path.to_string());
        }
        assignments.insert(name.to_string(),json!(files));
    }
    if paths.iter().filter_map(Value::as_str).any(|path|!assigned.contains(path)) {
        return Err("모든 targetFiles를 구성요소에 연결하세요.".into());
    }
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,components,assignments,next,hypothesis,design,paths]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,"components":components,"componentFiles":assignments,
        "nextDomain":next,"hypothesis":hypothesis,
        "design":design,"targetFiles":paths,
        "qualityProven":false,"adopted":false
    });
    session["rsiSystemDesign"]=proposal.clone();
    state.store.put("session",&session).ok_or("설계 저장 실패")?;
    state.store.audit("rsi.system_design_proposed",json!({
        "campaignId":campaign_id,"generationJobId":child_id,
        "designId":proposal["id"],"qualityProven":false
    }));
    Ok(json!({"proposal":proposal,
        "next":"실제 코드를 변경해야 합니다. 독립 A/B 채택 이전에는 개선 증거가 아닙니다."}))
}

/// Reuse the model's target only when its *source change*, not its
/// explanation, was independently evaluated and adopted.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    let previous=history.last()?;
    let design=&previous["systemDesign"];
    if previous["accepted"]!=true || previous["pairedRevalidated"]!=true
        || previous["evaluation"]["metrics"]["eligible"]!=true
        || previous["candidateOrigin"]!="model_driven_detached_worktree"
        || !previous["acceptedCommit"].is_string()
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=previous["generatorCommit"]
        || design["generationJobId"]!=previous["jobId"]
    {return None;}
    // A good score for an unrelated patch cannot establish that the
    // model's proposed system design was actually implemented.
    let paths=design["targetFiles"].as_array()?;
    if paths.is_empty() || !paths.iter().all(|entry|entry.as_str().is_some_and(|path|
        previous["acceptedSourceHashes"][path].as_str().is_some_and(|hash|
            hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())))){
        return None;
    }
    let domain=design["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&domain)
        .then(||domain.to_string())
}

/// This is a strictly narrower claim than general autonomous intelligence:
/// the *model* produced a cross-component design, the accepted patch touched
/// that design, and the rebuilt child executed its own subsequent plan.
/// The caller separately replays independent paired receipts and the exact
/// source lineage; JSON hints alone cannot establish capability.
pub(crate) fn verified_handoff(history:&[Value])->bool {
    let mut chain:Vec<&Value>=Vec::new();
    for generation in history {
        let design=&generation["systemDesign"];
        let edits=generation["acceptedSourceHashes"].as_object();
        let target=design["targetFiles"].as_array();
        let valid=generation["accepted"]==true
            && generation["systemEvolution"]==true
            && generation["pairedRevalidated"]==true
            && generation["candidateOrigin"]=="model_driven_detached_worktree"
            && generation["evaluation"]["metrics"]["eligible"]==true
            && generation["evaluation"]["metrics"]["regressions"]==0
            && generation["evaluation"]["metrics"]["improvedHoldouts"]
                .as_u64().unwrap_or(0)>0
            && generation["generatorBinarySha256"].as_str()
                .is_some_and(|hash|hash.len()==64)
            && generation["systemBlueprint"]["source"]=="compiled_descendant_system_blueprint_v1"
            && generation["systemBlueprint"]["generatorCommit"]==generation["generatorCommit"]
            && generation["descendantPlan"]["generatorCommit"]==generation["generatorCommit"]
            && generation["descendantPlan"]["systemBlueprint"]==
                generation["systemBlueprint"]
            && design["source"]=="model_authored_system_design"
            && design["generatorCommit"]==generation["generatorCommit"]
            && design["generationJobId"]==generation["jobId"]
            && design["component"].as_str().is_some_and(valid_role)
            && target.is_some_and(|files|!files.is_empty() && files.iter()
                .all(|entry|entry.as_str().is_some_and(|path|edits.is_some_and(|all|
                    all.get(path).and_then(Value::as_str).is_some_and(|hash|
                        hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()))))));
        if !valid {
            chain.clear();
            continue;
        }
        if chain.last().is_some_and(|last|
            last["acceptedCommit"]!=generation["generatorCommit"]) {
            chain.clear();
        }
        chain.push(generation);
        if chain.len()<3 {continue;}
        let distinct=chain.iter().filter_map(|g|
            g["generatorBinarySha256"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if distinct.len()!=chain.len(){continue;}
        let roles=chain.iter().flat_map(|g|design_roles(&g["systemDesign"]))
            .collect::<std::collections::BTreeSet<_>>();
        if roles.len()<3 || !roles.contains("improvement_methods"){continue;}
        let meta_transferred=chain.iter().enumerate().any(|(i,g)|
            g["domain"]=="improver"
            && g["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0)>0
            && chain[i+1..].iter().any(|successor|
                successor["domain"]!="improver"));
        if meta_transferred {return true;}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn systemic_handoff_requires_rebuilt_model_authored_design_and_real_source_receipts() {
        let make=|i:usize,component:&str,domain:&str| {
            let parent=format!("{:040x}",i+1);
            let next=format!("{:040x}",i+2);
            let path=format!("src/engine/{}.rs",component);
            json!({
                "accepted":true,"systemEvolution":true,"pairedRevalidated":true,
                "candidateOrigin":"model_driven_detached_worktree",
                "generatorCommit":parent,"acceptedCommit":next,
                "generatorBinarySha256":format!("{:064x}",i+1),
                "jobId":format!("job-{i}"),"domain":domain,
                "evaluation":{"metrics":{"eligible":true,"regressions":0,
                    "improvedHoldouts":1,"metaHoldoutGains":if domain=="improver"{1}else{0}}},
                "systemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                    "generatorCommit":parent,"architecture":[]},
                "descendantPlan":{"generatorCommit":parent,
                    "systemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                        "generatorCommit":parent,"architecture":[]}},
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":parent,"generationJobId":format!("job-{i}"),
                    "component":component,"targetFiles":[path]},
                "acceptedSourceHashes":{(path):"a".repeat(64)}
            })
        };
        let proof=vec![make(0,"algorithms","algorithm"),
            make(1,"improvement_methods","improver"),
            make(2,"memory","memory")];
        assert!(verified_handoff(&proof));
        let mut forged=proof.clone();
        forged[1]["systemDesign"]["generatorCommit"]=json!("stale");
        assert!(!verified_handoff(&forged));
        let mut copied=proof.clone();
        copied[2]["generatorBinarySha256"]=copied[1]["generatorBinarySha256"].clone();
        assert!(!verified_handoff(&copied));
        let mut no_edits=proof.clone();
        no_edits[1]["acceptedSourceHashes"]=json!({});
        assert!(!verified_handoff(&no_edits));
        let mut no_descendant=proof.clone();
        no_descendant[2]["descendantPlan"]=Value::Null;
        assert!(!verified_handoff(&no_descendant));
        let mut native=proof;
        native[1]["candidateOrigin"]=json!("native_inductive_source_mutation");
        assert!(!verified_handoff(&native));
    }

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
        // New executable modules must be designable, not only edits to a
        // predeclared static set of optimizer operators.
        let mut novel=request.clone();
        novel["targetFiles"]=json!(["src/engine/novel_meta_planner.rs"]);
        assert!(propose(&state,"session",&novel).is_ok());
        // A symlinked parent cannot be used to write outside the experiment.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc",workspace.join("src/engine/escape")).unwrap();
            novel["targetFiles"]=json!(["src/engine/escape/outside.rs"]);
            assert!(propose(&state,"session",&novel).is_err());
        }
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
            "acceptedCommit":"child","candidateOrigin":"model_driven_detached_worktree",
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
