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

/// Complete editable intelligence source inventory. The source tree is not
/// just seven hand-picked files: new descendants may create new modules and
/// rewrite the task agent, meta-agent, tool harness and memory together.
/// Stable sorting + content hashes make this an actual revision self-model.
fn source_inventory(root:&Path)->Vec<Value>{
    let Ok(root)=fs::canonicalize(root) else{return Vec::new()};
    let mut pending=vec![root.join("src"),root.join("web").join("src")];
    let mut visited=BTreeSet::<std::path::PathBuf>::new();
    let mut files=Vec::new();
    while let Some(directory)=pending.pop(){
        let Ok(directory)=fs::canonicalize(directory) else{continue};
        if !directory.starts_with(&root)||!visited.insert(directory.clone()){continue;}
        let Ok(entries)=fs::read_dir(&directory) else{continue};
        for entry in entries.flatten(){
            let Ok(path)=fs::canonicalize(entry.path()) else{continue};
            if !path.starts_with(&root){continue;}
            if path.is_dir(){pending.push(path);continue;}
            if !path.is_file(){continue;}
            let Ok(relative)=path.strip_prefix(&root) else{continue};
            let relative=relative.to_string_lossy().replace('\\',"/");
            if !crate::engine::rsi::safe_path(&relative)
                || crate::engine::rsi::hidden_rsi_oracle_path(&relative){continue;}
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
        "memory"=>"memory","learning"=>"learning","tools"=>"tools",
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
        crate::engine::rsi::safe_path(path) &&
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
            if !crate::engine::rsi::safe_path(relative){continue;}
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
                let path=path.as_str().filter(|p|crate::engine::rsi::safe_path(p))?;
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


/// The source graph is factual evidence. Ranking is an evolvable algorithm
/// in the currently built descendant; the external parent validates only
/// that chosen files and their hashes are real.
fn source_entry_valid(state:&crate::state::AppState,root:&Path,index_id:&str,
    entry:&Value,hash_key:&str)->bool{
    let Some(path)=entry["path"].as_str() else{return false};
    let Some(sha)=entry[hash_key].as_str() else{return false};
    if !crate::engine::rsi::safe_path(path)
        || crate::engine::rsi::hidden_rsi_oracle_path(path)
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
    let mut frontier=Vec::new();
    for component in model["architecture"].as_array().into_iter().flatten() {
        let Some(role)=component["role"].as_str().filter(|role|valid_role(role)) else{continue};
        let domain=component["domain"].as_str()
            .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
            .unwrap_or_else(||role_domain(role));
        let mut sources=component["representativeSources"].as_array()
            .into_iter().flatten()
            .filter(|file|source_entry_valid(state,root,index_id,file,"sha256"))
            .collect::<Vec<_>>();
        sources.sort_by(|a,b| {
            let ca=attempted.get(a["path"].as_str().unwrap_or("")).copied().unwrap_or(0);
            let cb=attempted.get(b["path"].as_str().unwrap_or("")).copied().unwrap_or(0);
            ca.cmp(&cb).then(a["bytes"].as_u64().cmp(&b["bytes"].as_u64()))
                .then(a["path"].as_str().cmp(&b["path"].as_str()))
        });
        let Some(source)=sources.first() else{continue};
        let Some(path)=source["path"].as_str() else{continue};
        let related=signals.iter().filter(|signal|
            signal["targetFiles"].as_array().is_some_and(|paths|
                paths.iter().any(|candidate|candidate.as_str()==Some(path))))
            .filter_map(|signal|signal["id"].as_str().map(str::to_string))
            .take(6).collect::<Vec<_>>();
        frontier.push(json!({
            "role":role,"domain":domain,"path":path,
            "sourceSha256":source["sha256"],"sourceBytes":source["bytes"],
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
    frontier.sort_by(|a,b|{
        a["verifiedRedesigns"].as_u64().cmp(&b["verifiedRedesigns"].as_u64())
            .then(a["attempts"].as_u64().cmp(&b["attempts"].as_u64()))
            .then(b["taskFailureSignalIds"].as_array().map(Vec::len)
                .cmp(&a["taskFailureSignalIds"].as_array().map(Vec::len)))
            .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
            .then(a["role"].as_str().cmp(&b["role"].as_str()))
    });
    frontier
}

/// Controller output is computed by the newly rebuilt generation, with the
/// entire source tree and measured history available to its own code.
/// Its design, ranking and future execution-method policy may change in the
/// next accepted revision; the supervising process merely authenticates facts.
pub(crate) fn system_agenda(state:&crate::state::AppState,model:&Value,
    history:&[Value],index_id:&str,root:&Path)->Value{
    let frontier=architecture_frontier(state,model,history,index_id,root);
    let inherited=history.iter().rev().find_map(accepted_model_design);
    let inherited_targets=inherited.and_then(|design|
        design["nextTargetFiles"].as_array()).into_iter().flatten()
        .take(3).filter_map(Value::as_str)
        .filter_map(|path|{
            frontier.iter().find(|item|item["path"]==path).cloned()
        }).collect::<Vec<_>>();
    let selected=inherited_targets.first().cloned().or_else(||frontier.first().cloned());
    let focus=if !inherited_targets.is_empty() {
        inherited_targets
    } else {
        selected.as_ref().map(|t|vec![t.clone()]).unwrap_or_default()
    };
    json!({
        "source":"compiled_descendant_self_controller_v1",
        "generatorCommit":model["generatorCommit"],
        "sourceTreeSha256":model["sourceTreeSha256"],
        "frontier":frontier,
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
    if model["generatorCommit"]!=commit
        || model["historyCount"].as_u64()!=Some(history_len as u64)
        || model["sourceTreeSha256"]!=source_facts(root)["sha256"]
        || model["sourceFileCount"]!=source_facts(root)["fileCount"]
        || model["architecture"].as_array().is_none_or(|roles|
            roles.is_empty()||roles.len()>64)
        || agenda["source"]!="compiled_descendant_self_controller_v1"
        || agenda["generatorCommit"]!=commit
        || agenda["sourceTreeSha256"]!=model["sourceTreeSha256"] {
        return Err("후손의 전체 소스 구조와 세대 ID가 일치하지 않습니다.".into());
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
    let last=history.iter().rev().find(|row|accepted_model_design(row).is_some())?;
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
    let component=body["component"].as_str()
        .filter(|name|COMPONENTS.iter().any(|row|row.0==*name))
        .ok_or("시스템 구성요소 지정 오류")?;
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
        .filter(|rows|!rows.is_empty()&&rows.len()<=8)
        .ok_or("변경할 실제 소스 1~8개를 선택하세요.")?;
    let root=fs::canonicalize(&state.config.workspace).map_err(|e|e.to_string())?;
    let mut unique=std::collections::BTreeSet::new();
    for path in paths {
        let path=path.as_str().ok_or("소스 경로 문자열이 필요합니다.")?;
        if !crate::engine::rsi::safe_path(path)||!unique.insert(path.to_string()){
            return Err("중복 또는 허용되지 않은 소스 경로입니다.".into());
        }
        let file=fs::canonicalize(root.join(path))
            .map_err(|_|"설계 대상이 실제 소스에 없습니다.".to_string())?;
        if !file.starts_with(&root)||!file.is_file(){
            return Err("외부 파일은 설계 범위가 아닙니다.".into());
        }
    }
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,next,hypothesis,design,paths]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,
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
