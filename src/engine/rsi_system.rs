//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/rsi_worktree.rs","src/engine/chat.rs","src/engine/decompose.rs"]),
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
            let Some(relative)=file.strip_prefix(&root).ok()
                .and_then(|p|p.to_str()) else {continue};
            if !crate::engine::rsi::safe_path(relative){continue;}
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

/// Cross-component designs are source programs, not prose labels. A bundle
/// has an explicit 1:1 attribution from each changed file to a system role;
/// an unrelated one-file optimization cannot claim six improved capabilities.
/// Adapted from the source-bound coupled redesign in Dominion Sub3/Sub4.
fn valid_role(role:&str)->bool {
    !role.is_empty() && role.len()<=64
        && role.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_'||c==b'-')
        && role!="system"
}

fn design_roles(design:&Value)->Vec<String> {
    design["components"].as_array().map(|roles|
        roles.iter().filter_map(Value::as_str)
            .filter(|role|valid_role(role)).map(str::to_string).collect()
    ).unwrap_or_else(||design["component"].as_str()
        .filter(|role|valid_role(role))
        .map(|role|vec![role.to_string()])
        .unwrap_or_else(||{
            // Legacy, independently adopted single-file proposals had no
            // component field. Recover only an UNAMBIGUOUS role from the
            // original source map; never guess for a shared or new module.
            let Some(targets)=design["targetFiles"].as_array()
                .filter(|files|files.len()==1) else{return Vec::new();};
            let Some(path)=targets[0].as_str() else{return Vec::new();};
            let matched=COMPONENTS.iter()
                .filter(|(_,files)|files.contains(&path))
                .map(|(role,_)|role.to_string()).collect::<Vec<_>>();
            if matched.len()==1{matched}else{Vec::new()}
        }))
}

fn design_role_files<'a>(design:&'a Value,role:&str)->Option<&'a Vec<Value>> {
    design["componentFiles"][role].as_array().or_else(||
        (design_roles(design).len()==1).then(||design["targetFiles"].as_array()).flatten())
}

/// A bundle is implemented only if every declared file is in the independently
/// measured source diff, with an unchanged design-to-role mapping. No
/// automatic "model says succeeded" or inferred coverage is accepted.
pub(crate) fn design_implemented(design:&Value,changed:&[Value])->bool {
    let Some(targets)=design["targetFiles"].as_array().filter(|files|!files.is_empty())
        else{return false;};
    let roles=design_roles(design);
    if roles.is_empty(){return false;}
    let mut mapped=std::collections::BTreeSet::<String>::new();
    for role in &roles {
        let Some(files)=design_role_files(design,role).filter(|files|!files.is_empty())
            else{return false;};
        for file in files {
            let Some(path)=file.as_str().filter(|p|crate::engine::rsi::safe_path(p))
                else{return false;};
            if !targets.iter().any(|entry|entry.as_str()==Some(path))
                || !changed.iter().any(|entry|entry.as_str()==Some(path))
                || !mapped.insert(path.to_string()) {return false;}
        }
    }
    targets.iter().all(|entry|entry.as_str()
        .is_some_and(|path|mapped.contains(path)))
}

/// The predecessor may leave executable *planning intent* for its compiled
/// successor. The successor is free to reject a stale stage and replan from
/// its own real source graph. This is a task directive, not a proven gain.
pub(crate) fn inherited_stage(
    history:&[Value],frontier:&[Value]
)->Option<Value>{
    // Failed experiments do not erase an adopted model-authored program:
    // the binary still runs the LAST accepted revision. A new accepted
    // revision supersedes the programme, whether or not it authored one.
    let prior=history.iter().rev().find(|g|g["accepted"]==true)?;
    let design=&prior["systemDesign"];
    if prior["accepted"]!=true
        || prior["evaluation"]["metrics"]["eligible"]!=true
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=prior["generatorCommit"]
        || design["generationJobId"]!=prior["jobId"]
        || !design_implemented(design,prior["acceptedSourceHashes"]
            .as_object()?.keys().map(|key|json!(key)).collect::<Vec<_>>().as_slice())
    {return None;}
    let stages=design["successorStages"].as_array()?;
    let previous=history.iter().skip_while(|generation|
        generation["id"]!=prior["id"]).skip(1).collect::<Vec<_>>();
    // A compiled successor schedules the PREDECESSOR'S stages from live
    // outcomes. Repeating a failed source target incurs a cost; a different
    // model-proposed stage may win next, without the resident coordinator
    // inventing the next strategy or claiming a quality gain.
    stages.iter().filter_map(|stage|{
        let role=stage["role"].as_str()?;
        let domain=stage["domain"].as_str()?;
        let path=stage["path"].as_str()?;
        if !crate::engine::rsi_evolve::DOMAINS.contains(&domain){return None;}
        let actual=frontier.iter().find(|row|
            row["path"]==path && row["role"]==role && row["domain"]==domain)?;
        let failures=previous.iter().filter(|g|
            g["accepted"]!=true && g["focusFiles"].as_array()
                .is_some_and(|files|files.iter().any(|f|f.as_str()==Some(path))))
            .count();
        // This is an inherited *control policy*: recorded failures change
        // the next task, rather than merely appearing in the model prompt.
        let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        let tools=stage["toolPolicy"].as_str().unwrap_or("allow_new_modules");
        let execution=stage["executionPolicy"].as_str().unwrap_or("explore_then_edit");
        if !matches!(learning,"avoid_failed"|"retry_failed")
            || !matches!(tools,"patch_only"|"allow_new_modules")
            || !matches!(execution,"grounded_edit"|"explore_then_edit") {
            return None;
        }
        let rank=if learning=="retry_failed" {-(failures as i64)}
            else {failures as i64};
        Some((rank,json!({"role":role,"domain":domain,"path":path,
            "sourceSha256":actual["sourceSha256"],
            "objective":stage["objective"],
            "sourceDesignId":design["id"],
            "predecessorCommit":prior["acceptedCommit"],
            "failedAttemptsSinceAdoption":failures,
            "learningPolicy":learning,
            "toolPolicy":tools,
            "executionPolicy":execution,
            "qualityProven":false})))
    }).min_by_key(|(rank,_)|*rank).map(|(_,stage)|stage)
}

/// DGM/AlphaEvolve-style architecture frontier, evaluated by the descendant
/// executable against its OWN current source graph. The resident coordinator
/// receives a proposed next capability, not authority to invent that proposal.
/// Scores schedule exploratory edits only: no item is an adopted gain.
fn architecture_role_domain(role:&str)->Option<&'static str>{
    match role {
        "algorithms"|"improvement_methods"=>Some("improver"),
        "harness"=>Some("context"),
        "memory"=>Some("memory"),
        "learning"=>Some("learning"),
        "tools"=>Some("tools"),
        "frontend"=>Some("ui"),
        _=>None,
    }
}

/// A source path may participate in several architectural roles. The
/// resulting frontier makes unmodified subsystems visible to the compiled
/// successor while preserving their original hashes and real index location.
pub(crate) fn architecture_frontier(
    state:&crate::state::AppState,
    blueprint:&Value,history:&[Value],index_id:&str,root:&Path,
)->Vec<Value>{
    use std::collections::{BTreeMap,BTreeSet};
    let mut verified_roles=BTreeSet::<String>::new();
    let mut source_attempts=BTreeMap::<String,u64>::new();
    for generation in history {
        for path in generation["focusFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str){
            *source_attempts.entry(path.to_string()).or_default()+=1;
        }
        if generation["accepted"]!=true
            || generation["pairedRevalidated"]!=true {continue;}
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else {continue;};
        if candidate["status"]!="verified"
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {continue;}
        // Only credit the explicit, COMPLETELY implemented source-to-role
        // contract. A helper file shared by two seed roles cannot claim
        // that both intelligence subsystems were actually redesigned.
        let design=&generation["systemDesign"];
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        if design["generatorCommit"]!=generation["generatorCommit"]
            || design["generationJobId"]!=generation["jobId"]
            || !design_implemented(design,&changed) {continue;}
        for role in design_roles(design) {
            let Some(files)=design_role_files(design,&role) else{continue;};
            if files.iter().all(|file|file.as_str().is_some_and(|path|{
                let Some(sha)=generation["acceptedSourceHashes"][path].as_str()
                    else{return false;};
                crate::engine::code_index::indexed_source_hash(state,index_id,path)
                    .as_deref()==Some(sha)
            })){
                verified_roles.insert(role);
            }
        }
    }
    let mut frontier=Vec::<Value>::new();
    // The independent whole-system completion gate requires SIX core
    // intelligence roles. Frontend is a seventh, optional application role;
    // do not spend the scarce first generations there while a core is missing.
    let complete_core=["algorithms","harness","memory","learning","tools",
        "improvement_methods"].iter().all(|role|verified_roles.contains(*role));
    for component in blueprint["architecture"].as_array().into_iter().flatten() {
        let Some(role)=component["role"].as_str() else {continue;};
        if role=="frontend" && !complete_core {continue;}
        let Some(domain)=architecture_role_domain(role).or_else(||
            component["domain"].as_str().filter(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(d))) else {continue;};
        let touched=verified_roles.contains(role);
        let mut available=component["representativeSources"].as_array()
            .into_iter().flatten()
            .filter_map(|item|{
                let path=item["path"].as_str()?;
                if !crate::engine::rsi::safe_path(path) {return None;}
                let hash=item["sha256"].as_str()?;
                if crate::engine::code_index::indexed_source_hash(state,index_id,path)
                    .as_deref()!=Some(hash) {return None;}
                let bytes=item["bytes"].as_u64()?;
                // The code-index DB is an observation from before the child
                // may have changed source. Re-read the ACTUAL checked-out
                // file so an old index cannot authorise stale descendant edits.
                let real=fs::read(root.join(path)).ok()?;
                if real.len() as u64!=bytes
                    || format!("{:x}",Sha256::digest(&real))!=hash {return None;}
                Some((path,hash,bytes))
            }).collect::<Vec<_>>();
        // Prioritize new systems, cheap-to-inspect source and actual failures.
        // A role's observed experiments are a scheduling cost, never a gain.
        available.sort_by(|a,b|{
            let cost=|p:&str,bytes:u64| source_attempts.get(p).copied().unwrap_or(0)*100
                + bytes/4096;
            cost(a.0,a.2).cmp(&cost(b.0,b.2))
                .then(a.0.cmp(b.0))
        });
        // Preserve alternate source targets. The original frontier exposed
        // only ONE representative per role, silently discarding a model's
        // perfectly valid inherited next-stage path whenever another file
        // happened to be smaller. Multiple options give the child an actual
        // architectural search space without pre-authoring an optimization.
        for (path,hash,bytes) in available.into_iter().take(8) {
            let prior=source_attempts.get(path).copied().unwrap_or(0);
            frontier.push(json!({
                "role":role,"domain":domain,"path":path,
                "sourceSha256":hash,"sourceBytes":bytes,
                "verifiedRoleImproved":touched,
                "sourceAttempts":prior,
                "reason":if touched{"extend_prior_verified_component"}
                    else {"unmodified_intelligence_component"},
                "qualityProven":false
            }));
            if frontier.len()>=128 {break;}
        }
    }
    // Bounded, stable portfolio: avoid serializing thousands of model tool
    // inputs when future generations invent many additional intelligence roles.
    frontier.sort_by(|a,b|{
        (a["verifiedRoleImproved"]==true).cmp(&(b["verifiedRoleImproved"]==true))
            .then(a["sourceAttempts"].as_u64().cmp(&b["sourceAttempts"].as_u64()))
            .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
            .then(a["role"].as_str().cmp(&b["role"].as_str()))
    });
    frontier.truncate(128);
    frontier
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
    let files=source_inventory(root);
    let fingerprint=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&files).unwrap_or_default()));
    json!({
        "source":"compiled_descendant_system_blueprint_v1",
        "generatorCommit":revision,
        "architecture":components,
        "sourceInventory":files,
        "sourceTreeSha256":fingerprint,
        "historyCount":history.len(),
        "inheritedDesign":history.iter().rev().find(|g|g["accepted"]==true && g["evaluation"]["metrics"]["eligible"]==true).map(|g|g["systemDesign"].clone()).unwrap_or(Value::Null),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_and_its_model_worker",
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
        "[COMPILED DESCENDANT: SYSTEM SELF-REDESIGN]\n\
        You are running from revision {}. The user goal is: {}.\n\
        You control design, source analysis, algorithm choice, method learning and \
        actual code edits for your next descendant. Do not treat a prewritten \
        source mutation or a single benchmark speedup as system evolution.\n\
        Inspect real source and identify a causal limitation in the current \
        agent's algorithm, harness, memory, learning, tools or improvement \
        method. Propose a genuinely new executable design, inspect source \
        hashes with code_search/workspace_read_range and WRITE the required \
        cross-module Rust changes with workspace_replace_function (complete Rust function), workspace_replace (line mode or exact oldText), workspace_write. \
        Before editing call rsi_system_plan(component, nextDomain, hypothesis, design, targetFiles). For a coupled redesign of algorithm, harness, memory, learning, tools, and improvement methods, use component=system and explicitly assign each file to one role with components and componentFiles. Then WRITE ALL the named source files: partially implemented designs receive no credit. New roles and source modules are allowed. Optionally author successorStages [{role,domain,path,objective,learningPolicy,toolPolicy,executionPolicy}]: the accepted descendant will apply observed-failure task ranking, source-edit focus and restricted tool access from these fields; only after your source is independently adopted will the NEW compiled descendant read its own code and attempt those next tasks. A design and a saved stage alone earn zero. At least one adopted generation must INVENT an executable method with rsi_method_propose and a later compiled descendant must EXECUTE it with verified postconditions. \
        Improve the intelligence control loop, not only a benchmark number. \
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
    let component=body["component"].as_str()
        .filter(|name|*name=="system"||valid_role(name))
        .ok_or("시스템 구성요소 이름은 1~64자 영문·숫자·밑줄·하이픈이어야 합니다.")?;
    // Sub4's coupled design + Sub3's attributed source-role contract:
    // one model may redesign several interdependent intelligence components.
    // New role names are legitimate, but have no credited capability before
    // a real independently accepted patch introduces their implementation.
    let roles=if component=="system" {
        let rows=body["components"].as_array()
            .filter(|rows|(2..=16).contains(&rows.len()))
            .ok_or("결합 구조 설계에는 구성요소 2~16개가 필요합니다.")?;
        let mut names=std::collections::BTreeSet::new();
        for row in rows {
            let role=row.as_str().filter(|role|valid_role(role))
                .ok_or("설계 구성요소 이름 오류")?;
            if !names.insert(role){return Err("중복된 설계 구성요소".into());}
        }
        rows.clone()
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
        .filter(|rows|!rows.is_empty()&&rows.len()<=24
            && (component!="system"||rows.len()>=2))
        .ok_or("실제 변경할 소스 1~24개, 결합 설계는 2개 이상 지정하세요.")?;
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
    // Every proposed component must have concrete implementation files.
    // One changed source cannot be credited to several intelligence roles.
    let mut assignments=serde_json::Map::new();
    let mut assigned=std::collections::BTreeSet::<String>::new();
    for role in &roles {
        let name=role.as_str().ok_or("설계 구성요소 문자열 오류")?;
        let files=if component=="system" {
            body["componentFiles"][name].as_array()
                .filter(|files|!files.is_empty())
                .ok_or("결합 설계에는 각 구성요소에 대응하는 componentFiles가 필요합니다.")?
                .clone()
        }else{paths.clone()};
        for file in &files {
            let path=file.as_str().ok_or("소스 파일 이름 오류")?;
            if !paths.iter().any(|target|target.as_str()==Some(path))
                || !assigned.insert(path.to_string()) {
                return Err("구성요소의 소스 파일은 중복 없이 targetFiles에 연결해야 합니다.".into());
            }
        }
        assignments.insert(name.to_string(),json!(files));
    }
    if paths.iter().filter_map(Value::as_str).any(|path|!assigned.contains(path)) {
        return Err("모든 targetFiles를 실제 구성요소에 할당해야 합니다.".into());
    }
    // Successor program is a model-authored decision schedule, not a claim
    // that the tasks are done. The compiled child will rebind each stage to
    // the actual post-adoption source and may reject it if the code changed.
    let mut stages=Vec::new();
    if let Some(rows)=body["successorStages"].as_array() {
        if rows.len()>8{return Err("후손 설계 단계는 최대 8개입니다.".into());}
        for row in rows {
            let role=row["role"].as_str().filter(|role|valid_role(role))
                .ok_or("후손 단계 구성요소 오류")?;
            let domain=row["domain"].as_str().filter(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(d))
                .ok_or("후손 단계 분야 오류")?;
            let path=row["path"].as_str()
                .filter(|p|crate::engine::rsi::safe_path(p))
                .ok_or("후손 단계 소스 경로 오류")?;
            let objective=row["objective"].as_str()
                .filter(|s|(25..=1200).contains(&s.chars().count()))
                .ok_or("후손 단계 실행 목표 25~1200자 필요")?;
            let candidate=root.join(path);
            if !paths.iter().any(|file|file.as_str()==Some(path)) {
                let real=fs::canonicalize(candidate).map_err(|_|"후손 단계 파일 없음".to_string())?;
                if !real.starts_with(&root)||!real.is_file(){
                    return Err("후손 단계는 현재 시스템 소스에 속해야 합니다.".into());
                }
            }
            let learning=row["learningPolicy"].as_str().unwrap_or("avoid_failed");
            let tools=row["toolPolicy"].as_str().unwrap_or("allow_new_modules");
            let execution=row["executionPolicy"].as_str().unwrap_or("explore_then_edit");
            if !matches!(learning,"avoid_failed"|"retry_failed")
                || !matches!(tools,"patch_only"|"allow_new_modules")
                || !matches!(execution,"grounded_edit"|"explore_then_edit") {
                return Err("후손 정책은 허용된 학습·도구·실행 모드만 지정할 수 있습니다.".into());
            }
            stages.push(json!({"role":role,"domain":domain,"path":path,
                "objective":objective,"learningPolicy":learning,
                "toolPolicy":tools,"executionPolicy":execution,
                "qualityProven":false}));
        }
    }
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,roles,assignments,next,hypothesis,design,paths,stages
    ]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,
        "components":roles,"componentFiles":assignments,
        "successorStages":stages,
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
    // The verified successor domain survives inconclusive later attempts.
    let previous=history.iter().rev().find(|g|g["accepted"]==true)?;
    let design=&previous["systemDesign"];
    if previous["accepted"]!=true
        || previous["evaluation"]["metrics"]["eligible"]!=true
        || !previous["acceptedCommit"].is_string()
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=previous["generatorCommit"]
        || design["generationJobId"]!=previous["jobId"]
    {return None;}
    // A good score for an unrelated patch cannot establish that the
    // model's proposed system design was actually implemented.
    let paths=design["targetFiles"].as_array()?;
    if !design_implemented(design,
        &previous["acceptedSourceHashes"].as_object()?.keys()
            .map(|key|json!(key)).collect::<Vec<_>>())
        || !paths.iter().filter_map(Value::as_str).all(|path|
            previous["acceptedSourceHashes"][path].as_str()
                .is_some_and(|hash|hash.len()==64
                    && hash.bytes().all(|byte|byte.is_ascii_hexdigit()))) {
        return None;
    }
    let domain=design["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&domain)
        .then(||domain.to_string())
}

/// Recompute whole-system recursion from model-produced source changes,
/// actual compiled planner ownership, independent paired receipts and a
/// cumulative set of redesigned intelligence components. Speeding up one
/// memorized benchmark three times is never whole-system evolution.
pub(crate) fn system_capability_proven(state:&crate::state::AppState, history:&Value)->bool {
    let Some(rows)=history.as_array() else {return false;};
    let mut chain=0usize;
    let mut prior_commit=String::new();
    let mut prior_binary=String::new();
    let mut covered=std::collections::BTreeSet::new();
    let mut independently_planned=0usize;
    let mut proposed_model_methods=std::collections::BTreeSet::new();
    let mut witnessed_inherited_method=false;
    for generation in rows {
        if generation["accepted"]!=true {chain=0;prior_commit.clear();covered.clear();
            proposed_model_methods.clear();witnessed_inherited_method=false;
            independently_planned=0;continue;}
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else {
            chain=0;prior_commit.clear();covered.clear();
            proposed_model_methods.clear();witnessed_inherited_method=false;
            independently_planned=0;continue;
        };
        let Some(generator)=generation["generatorCommit"].as_str() else {return false;};
        let Some(accepted)=generation["acceptedCommit"].as_str() else {return false;};
        let Some(binary)=generation["generatorBinarySha256"].as_str() else {return false;};
        if chain>0 && (generator!=prior_commit || binary==prior_binary) {
            chain=0;covered.clear();independently_planned=0;
            proposed_model_methods.clear();witnessed_inherited_method=false;
        }
        let model=&generation["systemBlueprint"];
        let design=&generation["systemDesign"];
        let plan=&generation["descendantPlan"];
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        let independently_verified=
            generation["pairedRevalidated"]==true
            && crate::engine::rsi::verified_pair_receipt(state,&candidate)
            && candidate["status"]=="verified"
            && candidate["origin"]=="model_driven_detached_worktree"
            && candidate["experimentAdoption"]["commit"]==accepted
            && candidate["baseCommit"]==generator
            && generation["candidateOrigin"]==candidate["origin"]
            && generation["systemEvolution"]==true
            && generation["systemBlueprintVerified"]==true
            && model["generatorCommit"]==generator
            && model["architecture"].as_array().is_some_and(|a|
                (7..=64).contains(&a.len()))
            && model["sourceTreeSha256"].as_str().is_some_and(|h|
                h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
            && plan["source"]=="compiled_descendant_system_controller_v3"
            && plan["generatorCommit"]==generator
            && plan["systemBlueprint"]==*model
            && design["source"]=="model_authored_system_design"
            && design["generatorCommit"]==generator
            && design["generationJobId"]==generation["jobId"]
            && design_implemented(design,&changed)
            && design["targetFiles"].as_array().is_some_and(|targets|
                targets.iter().all(|target|target.as_str().is_some_and(|path|
                    generation["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|hash|hash.len()==64
                            && hash.bytes().all(|b|b.is_ascii_hexdigit())))))
            && generation["evaluation"]["metrics"]["eligible"]==true
            && generation["evaluation"]["metrics"]["regressions"]==0;
        if !independently_verified {
            chain=0;prior_commit.clear();covered.clear();independently_planned=0;
            continue;
        }
        // A new executable improvement method must be authored by an
        // independently ADOPTED model generation, then ACTUALLY run by a
        // later compiled descendant with witnessed verified postconditions.
        // Merely registering a procedure or replaying an engine's fixed
        // repair recipe cannot earn the claimed method-learning transfer.
        for execution in generation["methodExecutions"].as_array()
            .into_iter().flatten() {
            let observed=execution["result"]["verified"]==true
                && execution["result"]["followed"]==true
                && execution["result"]["status"]=="followed"
                && execution["operationIds"].as_array()
                    .is_some_and(|actions|!actions.is_empty());
            if observed {
                if let (Some(id),Some(revision))=(
                    execution["methodId"].as_str(),execution["revision"].as_str()) {
                    if proposed_model_methods.contains(&(id.to_string(),revision.to_string()))
                        // The existing method execution scorer binds a
                        // procedure's actual source-write operation SHA to
                        // the independently adopted candidate. Read that
                        // witnessed causal receipt, not a model's report.
                        && state.store.all("experience").iter().any(|record|
                            record["executableMethods"].as_array().is_some_and(|methods|
                                methods.iter().any(|method|
                                    method["id"]==id && method["revision"]==revision
                                    && method["applications"].as_array().is_some_and(|apps|
                                        apps.iter().any(|episode|
                                            episode["jobId"]==generation["jobId"]
                                            && episode["methodWroteAcceptedCandidate"]==true
                                            && episode["verificationVerdict"]=="passed"))))) {
                        witnessed_inherited_method=true;
                    }
                }
            }
        }
        for proposal in generation["methodProposals"].as_array()
            .into_iter().flatten() {
            if proposal["sourceGenerationJobId"]==generation["jobId"]
                && proposal["synthesis"].is_null()
                && proposal["verification"]["type"]=="procedure"
                && proposal["verification"]["nodes"].as_array()
                    .is_some_and(|nodes|!nodes.is_empty())
                && proposal["revision"].as_str().is_some_and(|hash|
                    hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit())) {
                if let (Some(id),Some(revision))=(
                    proposal["id"].as_str(),proposal["revision"].as_str()) {
                    proposed_model_methods.insert((id.to_string(),revision.to_string()));
                }
            }
        }
        // Source coverage follows the MODEL'S validated, completely
        // implemented design programme, not a hard-coded file-name lookup.
        // A changed file only counts for its explicitly assigned role;
        // invented roles remain evolvable without faking the six core ones.
        for role in design_roles(design) {
            if design_role_files(design,&role).is_some_and(|files|
                files.iter().all(|file|file.as_str().is_some_and(|path|
                    changed.iter().any(|entry|entry==path)
                    && generation["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|sha|sha.len()==64
                            && sha.bytes().all(|b|b.is_ascii_hexdigit()))))) {
                covered.insert(role);
            }
        }
        if chain>0 {independently_planned+=1;}
        chain+=1;
        prior_commit=accepted.to_string();
        prior_binary=binary.to_string();
        // Scope is the intelligence-bearing software stack, not the UI.
        // Six physically edited roles must all be represented by adopted code.
        if chain>=3 && independently_planned>=2
            && witnessed_inherited_method
            && ["algorithms","harness","memory","learning","tools",
                "improvement_methods"].iter().all(|role|covered.contains(*role)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coupled_design_counts_only_all_real_changed_files_and_distinct_roles() {
        let design=json!({"component":"system","components":["memory","learning"],
            "targetFiles":["src/engine/memory.rs","src/engine/learning.rs"],
            "componentFiles":{
                "memory":["src/engine/memory.rs"],
                "learning":["src/engine/learning.rs"]
            }});
        assert!(!design_implemented(&design,&[json!("src/engine/memory.rs")]),
            "a partially written multi-role redesign must be rejected");
        assert!(design_implemented(&design,&[
            json!("src/engine/memory.rs"),json!("src/engine/learning.rs")]));
        let mut duplicate=design.clone();
        duplicate["componentFiles"]["learning"]=json!(["src/engine/memory.rs"]);
        assert!(!design_implemented(&duplicate,&[
            json!("src/engine/memory.rs"),json!("src/engine/learning.rs")]),
            "a shared source path must never count twice as two redesigned roles");
    }

    #[test]
    fn only_an_adopted_model_can_program_a_rebuilt_descendants_next_task() {
        let rows=vec![json!({"role":"learning","domain":"learning",
            "path":"src/engine/learning.rs",
            "sourceSha256":"a".repeat(64)})];
        let model=json!({"accepted":true,"generatorCommit":"parent",
            "acceptedCommit":"child","jobId":"episode",
            "evaluation":{"metrics":{"eligible":true}},
            "acceptedSourceHashes":{
                "src/engine/memory.rs":"b".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "id":"design1","component":"memory",
                "generatorCommit":"parent","generationJobId":"episode",
                "targetFiles":["src/engine/memory.rs"],
                "successorStages":[{"role":"learning","domain":"learning",
                    "path":"src/engine/learning.rs",
                    "objective":"Build a distinct learned policy informed by real runtime failures"}]}});
        let picked=inherited_stage(&[model.clone()],&rows).unwrap();
        assert_eq!(picked["path"],"src/engine/learning.rs");
        assert_eq!(picked["qualityProven"],false);
        let mut unadopted=model.clone();
        unadopted["accepted"]=json!(false);
        assert!(inherited_stage(&[unadopted],&rows).is_none());
        let mut mismatched=model;
        mismatched["acceptedSourceHashes"]=json!({});
        assert!(inherited_stage(&[mismatched],&rows).is_none());
        let stale=vec![json!({"role":"learning","domain":"learning",
            "path":"src/engine/unknown.rs"})];
        assert!(inherited_stage(&[json!({"accepted":false})],&stale).is_none());
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
        assert_eq!(first["sourceInventory"].as_array().unwrap().len(),2);
        assert_eq!(first["sourceTreeSha256"].as_str().unwrap().len(),64);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace"));
        let baseline=first["architecture"][0]["representativeSources"][0]["sha256"].clone();
        fs::write(root.join("src/engine/algorithms.rs"),"pub fn changed_policy() {}").unwrap();
        let second=blueprint(&root,"commit-two",&history);
        assert_ne!(baseline,second["architecture"][0]["representativeSources"][0]["sha256"]);
        assert_ne!(first["generatorCommit"],second["generatorCommit"]);
        assert_ne!(first["sourceTreeSha256"],second["sourceTreeSha256"],
            "the full system inventory must follow real source changes");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn architecture_frontier_rejects_unverified_adoption_and_uses_real_index() {
        let root=std::env::temp_dir().join(format!("rsi-frontier-{}",
            uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/algorithms.rs"),
            "pub fn selector() -> usize { 1 }").unwrap();
        fs::write(root.join("src/engine/memory.rs"),
            "pub fn memory() -> usize { 2 }").unwrap();
        fs::write(root.join("src/engine/tools.rs"),
            "pub fn tool() -> usize { 3 }").unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let model=blueprint(&root,"a".repeat(40).as_str(),&[]);
        let baseline=architecture_frontier(&state,&model,&[],
            index["id"].as_str().unwrap(),&root);
        assert!(baseline.iter().any(|row|row["role"]=="memory"));
        assert!(baseline.iter().all(|row|row["qualityProven"]==false));
        let forged=json!({"accepted":true,"pairedRevalidated":true,
            "candidateId":"fabricated","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),
            "acceptedSourceHashes":{"src/engine/memory.rs":
                model["architecture"][2]["representativeSources"][0]["sha256"]}});
        let explored=architecture_frontier(&state,&model,&[forged],index["id"].as_str().unwrap(),&root);
        assert_eq!(explored,baseline,
            "a claimed adoption cannot remove an unmodified system component");
        fs::write(root.join("src/engine/memory.rs"),
            "pub fn memory() -> usize { 4 }").unwrap();
        let stale=architecture_frontier(&state,&model,&[],index["id"].as_str().unwrap(),&root);
        assert!(stale.iter().all(|row|row["role"]!="memory"),
            "a changed source without a fresh index must not inherit its old SHA");
        let _=fs::remove_dir_all(root);
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
        let mut new_module=request.clone();
        new_module["targetFiles"]=json!(["src/engine/new_agent_method.rs"]);
        assert!(propose(&state,"session",&new_module).is_ok(),
            "a successor may design a new source module");
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
    fn measured_outcomes_change_successors_without_rewarding_research_only_generations() {
        assert_eq!(improvement_directive(&[])["mode"],"discover");
        let research=json!({"id":"g1","accepted":false,"newFindings":3,
            "candidateId":null,"domain":"harness"});
        assert_eq!(improvement_directive(&[research.clone()])["mode"],"implement_research");
        let mut attempted=research.clone();
        attempted["improvementDirective"]=json!({"mode":"implement_research"});
        assert_eq!(improvement_directive(&[attempted])["mode"],"change_approach");
        let accepted=json!({"id":"g2","accepted":true,"candidateId":"real-candidate",
            "evaluation":{"metrics":{"eligible":true}}});
        assert_eq!(improvement_directive(&[research,accepted])["mode"],"extend_verified_capability");
        let unverified=json!({"id":"fake","accepted":true,
            "evaluation":{"metrics":{"eligible":false}},"candidateId":null});
        assert_ne!(improvement_directive(&[unverified])["mode"],"extend_verified_capability");
    }

    #[test]
    fn only_adopted_model_design_can_control_a_new_compiled_descendant() {
        let base=json!({"generatorCommit":"parent","jobId":"worker",
            "acceptedCommit":"child","candidateOrigin":"model_driven_detached_worktree",
            "acceptedSourceHashes":{"src/engine/learning.rs":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "accepted":true,
            "evaluation":{"metrics":{"eligible":true}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"parent","generationJobId":"worker",
                "targetFiles":["src/engine/learning.rs"],
                "nextDomain":"learning"}});
        assert_eq!(inherited_domain(&[base.clone()]),Some("learning".into()));
        for (key,value) in [("accepted",json!(false))] {
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
