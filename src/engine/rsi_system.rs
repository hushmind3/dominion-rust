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
    ("improvement_methods",&["src/engine/rsi_evolve.rs","src/engine/rsi_meta_controller.rs","src/engine/rsi_system.rs"]),
    ("models",&["src/engine/models.rs","src/engine/cli_model.rs","src/engine/router.rs","src/engine/gateway.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

/// Inventory every compilable agent/frontend source under the selected
/// experimental revision, not merely a hand-picked optimization fixture.
/// An inventory hash changes when ANY generation changes the executable
/// system's source graph; the prompt can remain short without losing lineage.
fn source_inventory(root:&Path)->Vec<Value> {
    let Ok(root)=fs::canonicalize(root) else {return Vec::new()};
    let mut pending=vec![root.join("src"),root.join("web").join("src"),root.join("src-tauri").join("src")];
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
                Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"py"));
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

/// Bootstrap source ownership from the actual module tree, not from a fixed
/// three-file whitelist. This heuristic itself lives in the descendant's
/// recompilable source and can be redesigned by future generations.
fn source_role(path:&str)->&'static str {
    if path.starts_with("web/src/") || path.starts_with("src-tauri/src/") {
        return "frontend";
    }
    let name=path.rsplit('/').next().unwrap_or(path);
    if name.starts_with("model_") || name.starts_with("training")
        || matches!(name,"models.rs"|"cli_model.rs"|"router.rs") {
        return "models";
    }
    if name.starts_with("rsi_") || matches!(name,"rsi.rs"|"algorithms.rs"|"algorithm_eval.rs"|"policy_patch.rs") {
        return "improvement_methods";
    }
    if path.contains("/chat/") || matches!(name,"chat.rs"|"jobs.rs"|"decompose.rs"|"gateway.rs") {
        return "harness";
    }
    if name.starts_with("memory") || name.starts_with("room_") {
        return "memory";
    }
    if name.starts_with("learning") || name.starts_with("experience")
        || name.starts_with("procedure") || name.starts_with("agent_growth") {
        return "learning";
    }
    if name.starts_with("tools") || name.starts_with("browser") || name.starts_with("mcp") {
        return "tools";
    }
    "algorithms"
}

/// A recorded design is inheritable only when the source patch was accepted
/// and every declared design file has a corresponding adopted source digest.
/// This is inheritance evidence, NOT a substitute for the independent paired
/// source-verification receipt used by the supervisor for promotion.
fn adopted_design(generation:&Value)->Option<&Value> {
    let design=&generation["systemDesign"];
    if generation["accepted"]!=true
        || generation["evaluation"]["metrics"]["eligible"]!=true
        || generation["acceptedCommit"].as_str().is_none_or(str::is_empty)
        || generation["candidateId"].as_str().is_none_or(str::is_empty)
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=generation["generatorCommit"]
        || design["generationJobId"]!=generation["jobId"] {
        return None;
    }
    let files=design["targetFiles"].as_array().filter(|files|!files.is_empty())?;
    if !files.iter().all(|item|item.as_str().is_some_and(|path|
        crate::engine::rsi::safe_path(path)
        && generation["acceptedSourceHashes"][path].as_str().is_some_and(|hash|
            hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit())))) {
        return None;
    }
    Some(design)
}

fn latest_adopted_design(history:&[Value])->Option<&Value> {
    history.iter().rev().find_map(adopted_design)
}

/// GEPA-Curator-style experience selection and MGM-style comparative ancestry.
/// Keep multiple adopted designs and distinct actionable failures, not the
/// immediately previous run alone. Fixed-size, content-addressed receipts keep
/// this reusable without replaying an entire paid context on every iteration.
pub(crate) fn lineage_capsule(history:&[Value])->Value {
    let mut successes=Vec::new();
    let mut failures=Vec::new();
    let mut unique_failures=BTreeSet::new();
    for generation in history.iter().rev() {
        if let Some(design)=adopted_design(generation) {
            if successes.len()<8 {
                successes.push(json!({
                    "designId":design["id"],"generationId":generation["id"],
                    "generatorCommit":generation["generatorCommit"],
                    "acceptedCommit":generation["acceptedCommit"],
                    "components":design["components"].as_array()
                        .map(|items|items.iter().take(12).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "componentFiles":design["componentFiles"],
                    "changedFiles":design["targetFiles"].as_array()
                        .map(|items|items.iter().take(12).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "nextDomain":design["nextDomain"],
                    "hypothesis":design["hypothesis"].as_str().unwrap_or("")
                        .chars().take(360).collect::<String>(),
                    "implementation":design["design"].as_str().unwrap_or("")
                        .chars().take(640).collect::<String>(),
                    "metaHoldoutGains":generation["evaluation"]["metrics"]["metaHoldoutGains"],
                    "source":"recorded_adoption_with_source_digests"
                }));
            }
        } else if generation["accepted"]!=true && failures.len()<6 {
            let source=if generation["verification"]["checksPassed"]==false {
                "reproducible_verifier_failure"
            }else if generation["candidateId"].is_string() && generation["evaluation"]["verdict"].is_string() {
                "independently_measured_candidate_failure"
            }else if generation["error"].as_str().is_some_and(|err|!err.is_empty()) {
                "unverified_execution_failure"
            }else{
                "incomplete_exploration"
            };
            let error=generation["error"].as_str().unwrap_or("")
                .chars().take(420).collect::<String>();
            let key=(generation["generatorCommit"].as_str().unwrap_or("").to_owned(),
                generation["candidateId"].as_str().unwrap_or("").to_owned(),
                error.clone());
            if unique_failures.insert(key) {
                failures.push(json!({
                    "generationId":generation["id"],"generatorCommit":generation["generatorCommit"],
                    "candidateId":generation["candidateId"],
                    "focusFiles":generation["focusFiles"].as_array()
                        .map(|items|items.iter().take(4).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "kind":source,"verdict":generation["evaluation"]["verdict"],
                    "error":error,"newFindings":generation["newFindings"],
                    "researchArtifactHashes":generation["knowledgeArtifactHashes"].as_array()
                        .map(|items|items.iter().take(4).cloned().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    "qualityProven":false
                }));
            }
        }
        if successes.len()>=8 && failures.len()>=6 {break;}
    }
    let accepted_head=successes.first().map(|item|item["acceptedCommit"].clone())
        .unwrap_or(Value::Null);
    let core=json!({
        "source":"descendant_curated_lineage_v1","historyCount":history.len(),
        "latestAcceptedCommit":accepted_head,
        "acceptedDesigns":successes,"failureCounterexamples":failures,
        "adoptionEvidence":"recorded_only_supervisor_replays_independent_pairs"
    });
    let sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&core).unwrap_or_default()));
    let mut capsule=core;
    capsule["sha256"]=json!(sha);
    capsule
}

/// Comparative evolution without unverified cherry-picks: successful
/// architectures from OTHER campaigns are read-only references. Each must
/// have an independently replayable paired receipt under the same immutable
/// evaluation profile and overlap a module in the current source index.
/// The new compiled descendant must still synthesize and verify its own patch.
pub(crate) fn peer_lineage_references(
    state:&crate::state::AppState,campaign:&str,profile_sha:&str,index_id:&str,
    preferred_role:Option<&str>,
)->Value {
    let mut ranked=Vec::<(i64,Value)>::new();
    let mut patches=BTreeSet::new();
    for generation in state.store.all("rsi_generation").into_iter().take(256) {
        if generation["campaignId"].as_str()==Some(campaign)
            || generation["evaluationProfileSha256"].as_str()!=Some(profile_sha) {
            continue;
        }
        let Some(design)=adopted_design(&generation) else {continue};
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
        if candidate["status"]!="verified"
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || candidate["baseCommit"]!=generation["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {
            continue;
        }
        let Some(patch_hash)=candidate["patchSha256"].as_str() else{continue};
        if !patches.insert(patch_hash.to_string()){continue;}
        let mut aligned=Vec::new();
        let mut same_bytes=0usize;
        for path in design["targetFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(24) {
            let current=crate::engine::code_index::indexed_source_hash(state,index_id,path);
            if let Some(hash)=current {
                same_bytes+=usize::from(generation["acceptedSourceHashes"][path].as_str()==Some(hash.as_str()));
                aligned.push(path.to_string());
            }
        }
        if aligned.is_empty() {continue;}
        let components=design_roles(design);
        let role_match=preferred_role.is_some_and(|role|components.iter().any(|name|name.as_str()==role));
        let meta=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0);
        let gains=candidate["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0);
        let score=i64::from(role_match)*1_000+(meta.min(20) as i64)*30
            +(gains.min(30) as i64)*5+(same_bytes.min(6) as i64)*2;
        ranked.push((score,json!({
            "source":"independently_paired_peer_lineage_v1",
            "campaignId":generation["campaignId"],"generationId":generation["id"],
            "candidateId":candidate["id"],"patchSha256":patch_hash,
            "generatorCommit":generation["generatorCommit"],
            "acceptedCommit":generation["acceptedCommit"],
            "evaluationProfileSha256":profile_sha,
            "components":components,"alignedSourceFiles":aligned,
            "identicalSourceFiles":same_bytes,
            "nextDomain":design["nextDomain"],
            "hypothesis":design["hypothesis"].as_str().unwrap_or("")
                .chars().take(420).collect::<String>(),
            "designSummary":design["design"].as_str().unwrap_or("")
                .chars().take(700).collect::<String>(),
            "improvedHoldouts":gains,"metaHoldoutGains":meta,
            "sourceCompatibility":if same_bytes>0 {"some_identical"}else{"requires_reimplementation"},
            "automaticPatchReuse":false,"qualityProvenForThisDescendant":false
        })));
    }
    ranked.sort_by(|a,b|b.0.cmp(&a.0).then_with(||
        a.1["candidateId"].as_str().cmp(&b.1["candidateId"].as_str())));
    let selected=ranked.into_iter().take(4).map(|(_,row)|row).collect::<Vec<_>>();
    json!({"source":"cross_campaign_comparative_archive_v1","profileSha256":profile_sha,
        "references":selected,"automaticPatchReuse":false,
        "policy":"same_frozen_oracle_only_peer_successes_are_references_not_promotions"})
}

/// The old supervisor verifies archived comparison receipts, never the peer
/// agent's proposal text or source patch as an instruction.
pub(crate) fn validate_peer_references(
    state:&crate::state::AppState,reference_set:&Value,profile_sha:&str,
)->Result<(),String> {
    if reference_set["source"]!="cross_campaign_comparative_archive_v1"
        || reference_set["profileSha256"]!=profile_sha
        || reference_set["automaticPatchReuse"]!=false {
        return Err("교차 계보 출처·평가 프로필 오류".into());
    }
    let references=reference_set["references"].as_array()
        .filter(|rows|rows.len()<=4).ok_or("교차 계보 참조 수 오류")?;
    let mut seen=BTreeSet::new();
    for row in references {
        let id=row["candidateId"].as_str().ok_or("교차 계보 후보 ID 없음")?;
        if !seen.insert(id) || row["source"]!="independently_paired_peer_lineage_v1"
            || row["evaluationProfileSha256"]!=profile_sha
            || row["automaticPatchReuse"]!=false {
            return Err("중복·비검증 교차 계보 참조".into());
        }
        let generation_id=row["generationId"].as_str()
            .ok_or("교차 계보 세대 ID 없음")?;
        let original=state.store.get("rsi_generation",generation_id)
            .ok_or("교차 계보 세대 소실")?;
        if original["campaignId"]!=row["campaignId"]
            || original["candidateId"]!=id
            || original["evaluationProfileSha256"]!=profile_sha
            || adopted_design(&original).is_none() {
            return Err("교차 계보 출처·후손 설계의 원본 기록 오류".into());
        }
        let candidate=state.store.get("rsi_candidate",id).ok_or("교차 계보 후보 소실")?;
        if candidate["status"]!="verified"
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || candidate["patchSha256"]!=row["patchSha256"]
            || candidate["baseCommit"]!=row["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=row["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {
            return Err("교차 계보 후보의 독립 채택 증거 오류".into());
        }
    }
    Ok(())
}

fn valid_role(name:&str)->bool {
    !name.is_empty() && name.len()<=64 && name.bytes().all(|c|
        c.is_ascii_alphanumeric() || c==b'_' || c==b'-')
}

fn role_domain(role:&Value)->&str {
    if let Some(domain)=role["domain"].as_str().filter(|d|
        crate::engine::rsi_evolve::DOMAINS.contains(d)) {return domain;}
    match role["role"].as_str().unwrap_or("") {
        "memory"=>"memory", "learning"=>"learning", "models"=>"learning", "tools"=>"tools",
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
            if inventory.len()>=16 {break;}
            if !inventory.iter().any(|item|item["path"]==entry["path"]) {
                inventory.push(entry.clone());
            }
        }
        let row=json!({"role":role,"representativeSources":inventory});
        let domain=role_domain(&row).to_string();
        components.push(json!({"role":role,"domain":domain,"representativeSources":inventory,
            "discoveredSourceCount":total_sources}));
    }
    for generation in history.iter().filter(|generation|generation["accepted"]==true
        && generation["evaluation"]["metrics"]["eligible"]==true) {
        let design=&generation["systemDesign"];
        for role in design_roles(design) {
            if components.iter().any(|item|item["role"]==role){continue;}
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
                    Some("rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"py")) {
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
    // The next source revision can inherit a model-written multi-step
    // improvement procedure, including its learning and tool-use policies.
    // It is activated only after this generation's REAL source patch passes
    // independent validation and the new compiled binary rebinds its hashes.
    let mut successor_stages=Vec::new();
    let mut stage_names=BTreeSet::new();
    if let Some(rows)=body["successorStages"].as_array() {
        if rows.len()>8 {return Err("후속 개선 프로그램은 최대 8단계입니다.".into());}
        for row in rows {
            let role=row["role"].as_str().filter(|r|valid_role(r))
                .ok_or("후속 프로그램 역할 이름 오류")?;
            let domain=row["domain"].as_str().filter(|d|
                crate::engine::rsi_evolve::DOMAINS.contains(d))
                .ok_or("후속 프로그램 분야 오류")?;
            let path=row["path"].as_str().filter(|p|crate::engine::rsi::safe_path(p))
                .ok_or("후속 프로그램 소스 경로 오류")?;
            let objective=row["objective"].as_str().filter(|o|
                (25..=1200).contains(&o.chars().count()))
                .ok_or("후속 프로그램 실행 목표 길이 오류")?;
            if !stage_names.insert((role.to_string(),path.to_string())) {
                return Err("중복된 후속 개선 단계".into());
            }
            let source=root.join(path);
            if source.exists() {
                let canonical=fs::canonicalize(&source).map_err(|e|e.to_string())?;
                if !canonical.starts_with(&root)||!canonical.is_file() {
                    return Err("후속 개선 프로그램의 파일이 저장소 밖입니다.".into());
                }
            }else if !paths.iter().any(|p|p.as_str()==Some(path)) {
                return Err("새 모듈의 후속 개선은 이번 설계 targetFiles에 포함해야 합니다.".into());
            }
            let learning=row["learningPolicy"].as_str().unwrap_or("avoid_failed");
            let tools=row["toolPolicy"].as_str().unwrap_or("allow_new_modules");
            let execution=row["executionPolicy"].as_str().unwrap_or("explore_then_edit");
            if !matches!(learning,"avoid_failed"|"retry_failed")
                || !matches!(tools,"patch_only"|"allow_new_modules")
                || !matches!(execution,"grounded_edit"|"explore_then_edit") {
                return Err("후속 프로그램의 학습·도구·실행 정책 오류".into());
            }
            successor_stages.push(json!({"role":role,"domain":domain,
                "path":path,"objective":objective,"learningPolicy":learning,
                "toolPolicy":tools,"executionPolicy":execution}));
        }
    }
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,components,assignments,next,hypothesis,design,paths,successor_stages]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,"components":components,"componentFiles":assignments,
        "nextDomain":next,"hypothesis":hypothesis,
        "design":design,"targetFiles":paths,"successorStages":successor_stages,
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
    // A failed generation must not erase an adopted design. Yet continuing
    // to pin its target after repeated unsuccessful attempts creates an
    // infinite repair loop. Keep the design in the lineage capsule while
    // releasing the planner to explore after two actual failed continuations.
    let (accepted_at,design)=history.iter().enumerate().rev()
        .find_map(|(index,record)|adopted_design(record).map(|design|(index,design)))?;
    let domain=design["nextDomain"].as_str()?;
    if !crate::engine::rsi_evolve::DOMAINS.contains(&domain) {return None;}
    let exhausted=history[accepted_at+1..].iter()
        .filter(|attempt|attempt["accepted"]!=true && attempt["domain"]==domain)
        .filter(|attempt|attempt["candidateId"].is_string()
            || attempt["error"].as_str().is_some_and(|s|!s.is_empty())
            || attempt["newFindings"].as_u64().unwrap_or(0)>0)
        .take(2).count();
    (exhausted<2).then(||domain.to_string())
}

/// Rebind an independently adopted model-authored improvement PROGRAM to
/// current source bytes. This is executable *planning policy*, not a prompt
/// about the last task. The new binary may rewrite this selector, use a new
/// search formula or replace the whole program in its next accepted revision.
pub(crate) fn inherited_stage(history:&[Value],frontier:&[Value])->Option<Value> {
    let (accepted_index,parent)=history.iter().enumerate().rev()
        .find(|(_,g)|adopted_design(g).is_some())?;
    let design=adopted_design(parent)?;
    let stages=design["successorStages"].as_array()?;
    let mut scored=Vec::<(i64,usize,Value)>::new();
    for (index,stage) in stages.iter().enumerate() {
        let (Some(role),Some(domain),Some(path))=(
            stage["role"].as_str(),stage["domain"].as_str(),stage["path"].as_str()
        )else{continue};
        if !valid_role(role)||!crate::engine::rsi::safe_path(path)
            || !crate::engine::rsi_evolve::DOMAINS.contains(&domain){continue;}
        let Some(source)=frontier.iter().find(|entry|
            entry["role"]==role && entry["domain"]==domain && entry["path"]==path)
        else{continue};
        let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        let tools=stage["toolPolicy"].as_str().unwrap_or("allow_new_modules");
        let execution=stage["executionPolicy"].as_str().unwrap_or("explore_then_edit");
        if !matches!(learning,"avoid_failed"|"retry_failed")
            || !matches!(tools,"patch_only"|"allow_new_modules")
            || !matches!(execution,"grounded_edit"|"explore_then_edit"){continue;}
        let failed=history[accepted_index+1..].iter()
            .filter(|g|g["accepted"]!=true
                && g["focusFiles"].as_array().is_some_and(|f|
                    f.iter().any(|p|p.as_str()==Some(path))))
            .count() as i64;
        let reproduced=history[accepted_index+1..].iter()
            .filter(|g|g["accepted"]==true
                && g["evaluation"]["metrics"]["eligible"]==true
                && g["focusFiles"].as_array().is_some_and(|f|
                    f.iter().any(|p|p.as_str()==Some(path))))
            .count() as i64;
        // Failure contrast is policy, not a hardcoded target. Altering this
        // weight in the next source revision alters its own improvement plan.
        let score=if learning=="retry_failed"{2*failed}
            else{-3*failed}+2*reproduced;
        scored.push((score,index,json!({
            "source":"inherited_model_authored_improvement_program_v1",
            "role":role,"domain":domain,"path":path,
            "sourceSha256":source["sourceSha256"],
            "objective":stage["objective"],"stageIndex":index,
            "sourceDesignId":design["id"],"parentAcceptedCommit":parent["acceptedCommit"],
            "learningPolicy":learning,"toolPolicy":tools,
            "executionPolicy":execution,"failuresSinceAdoption":failed,
            "reproductiveAdoptions":reproduced,
            "qualityProven":false
        })));
    }
    scored.sort_by(|a,b|b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().next().map(|(_,_,v)|v)
}

/// Whole-system handoff evidence is separate from a source patch's score.
/// Coverage means source-backed redesigned roles, not measured intelligence
/// in every role. The actual downstream A/B remains the performance evidence.
pub(crate) fn handoff_evidence(state:&crate::state::AppState, history:&[Value])->Value {
    let profile=history.iter().find_map(|generation|state.store.get("job",generation["campaignId"].as_str()?)
        .and_then(|job|crate::engine::rsi_profile::from_input(&job["input"]).ok()))
        .unwrap_or_else(crate::engine::rsi_profile::default_profile);
    let required=profile["requiredComponents"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>();
    let mut chain=0usize;let mut handoffs=0usize;let mut prior_commit=String::new();
    let mut binaries=BTreeSet::new();let mut covered=BTreeSet::new();
    let mut methods=BTreeMap::<String,String>::new();let mut method_transfer=false;
    let mut meta_gain=false;let mut verified=0usize;
    for generation in history {
        if generation["accepted"]!=true {continue;}
        let Some(candidate)=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id)) else{continue;};
        let design=&generation["systemDesign"];
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        let generator=generation["generatorCommit"].as_str().unwrap_or("");
        let accepted=generation["acceptedCommit"].as_str().unwrap_or("");
        let binary=generation["generatorBinarySha256"].as_str().unwrap_or("");
        let controller=&generation["controllerPlan"];
        let valid=generation["systemEvolution"]==true
            && (generation["evaluationProfileSha256"].is_null() || generation["evaluationProfileSha256"]==profile["sha256"])
            && candidate["status"]=="verified" && candidate["origin"]=="model_driven_detached_worktree"
            && candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
            && candidate["baseCommit"]==generator && candidate["experimentAdoption"]["commit"]==accepted
            && crate::engine::rsi::verified_pair_receipt(state,&candidate)
            && generation["systemBlueprintVerified"]==true
            && generation["systemBlueprint"]["generatorCommit"]==generator
            && controller["generatorCommit"]==generator
            && controller["source"]=="compiled_descendant_system_controller_v2"
            && controller["systemBlueprint"]==generation["systemBlueprint"]
            && design["source"]=="model_authored_system_design"
            && design["generatorCommit"]==generator && design["generationJobId"]==generation["jobId"]
            && design_implemented(design,&changed)
            && design["targetFiles"].as_array().is_some_and(|paths|paths.iter().all(|path|
                path.as_str().is_some_and(|path|generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|sha|sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
            && generation["tokenUsage"]["calls"].as_u64().unwrap_or(0)>0
            && generator.len()==40 && accepted.len()==40 && binary.len()==64;
        if !valid {continue;}
        verified+=1;
        if chain>0 && (generator!=prior_commit || binaries.contains(binary)) {
            chain=0;handoffs=0;covered.clear();binaries.clear();methods.clear();
            method_transfer=false;meta_gain=false;
        }
        if chain>0 {handoffs+=1;}
        for execution in generation["methodExecutions"].as_array().into_iter().flatten() {
            if execution["result"]["verified"]!=true || execution["result"]["followed"]!=true {continue;}
            if let (Some(id),Some(revision))=(execution["methodId"].as_str(),execution["revision"].as_str()) {
                if methods.get(id).is_some_and(|expected|expected==revision)
                    && state.store.all("experience").iter().any(|record|
                        record["executableMethods"].as_array().into_iter().flatten().any(|method|
                            method["id"]==id && method["revision"]==revision
                            && method["applications"].as_array().into_iter().flatten().any(|app|
                                app["jobId"]==generation["jobId"] && app["methodWroteAcceptedCandidate"]==true
                                && app["verificationVerdict"]=="passed")))
                    && state.store.get("job",generation["jobId"].as_str().unwrap_or(""))
                        .and_then(|job|job["sessionId"].as_str().map(str::to_string))
                        .is_some_and(|session|crate::engine::rsi_evolve::method_wrote_accepted_candidate(
                            state,generation,execution,&session)) {method_transfer=true;}
            }
        }
        for proposal in generation["methodProposals"].as_array().into_iter().flatten() {
            if proposal["sourceGenerationJobId"]==generation["jobId"] && proposal["synthesis"].is_null()
                && proposal["verification"]["type"]=="procedure"
                && proposal["verification"]["nodes"].as_array().is_some_and(|nodes|!nodes.is_empty()) {
                if let (Some(id),Some(revision))=(proposal["id"].as_str(),proposal["revision"].as_str()) {
                    methods.insert(id.to_string(),revision.to_string());
                }
            }
        }
        for role in design_roles(design){covered.insert(role);}
        meta_gain|=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0;
        chain+=1;prior_commit=accepted.into();binaries.insert(binary.to_string());
    }
    let missing=required.iter().filter(|role|!covered.contains(**role)).copied().collect::<Vec<_>>();
    json!({"evaluationProfileId":profile["id"],"evaluationProfileSha256":profile["sha256"],"verifiedSystemGenerations":verified,"verifiedLineageLength":chain,"rebuiltHandoffs":handoffs,
        "sourceChangedComponents":covered,"requiredComponents":required,"missingComponents":missing,
        "metaImprovementObserved":meta_gain,"inventedMethodTransferred":method_transfer,
        "demonstrated":chain>=3 && handoffs>=2 && missing.is_empty() && method_transfer && meta_gain,
        "coverageMeaning":"independently adopted source roles; performance is measured separately by downstream tasks"})
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
        assert_eq!(first["architecture"].as_array().unwrap().len(),8);
        assert_eq!(first["generatorCommit"],"commit-one");
        assert_eq!(first["historyCount"],1);
        assert_eq!(first["sourceInventory"].as_array().unwrap().len(),2);
        assert_eq!(first["sourceTreeSha256"].as_str().unwrap().len(),64);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace_range"));
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
    fn large_agent_source_map_has_full_digest_but_bounded_transfer() {
        let root=std::env::temp_dir().join(format!("rsi-system-many-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        for n in 0..90 {
            fs::write(root.join(format!("src/engine/method_{n:03}.rs")),
                format!("pub fn method_{n:03}() {{}}")).unwrap();
        }
        let full=blueprint(&root,"old",&[]);
        assert_eq!(full["sourceFileCount"],90);
        assert_eq!(full["sourceInventory"].as_array().unwrap().len(),64);
        assert_eq!(full["sourceInventoryTruncated"],true);
        fs::write(root.join("src/engine/method_089.rs"),"pub fn changed() {}").unwrap();
        let changed=blueprint(&root,"new",&[]);
        assert_eq!(full["sourceInventory"],changed["sourceInventory"],
            "the changed file falls outside the bounded model preview");
        assert_ne!(full["sourceTreeSha256"],changed["sourceTreeSha256"],
            "the complete source fingerprint must include every file");
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
        let mut new_module=request.clone();
        new_module["targetFiles"]=json!(["src/engine/new_agent_method.rs"]);
        assert!(propose(&state,"session",&new_module).is_ok(),
            "a successor may design a new source module");
        let mut fresh_component=new_module.clone();
        fresh_component["component"]=json!("new_reasoning_controller");
        assert!(propose(&state,"session",&fresh_component).is_ok(),
            "the intelligence may invent architectural roles beyond original fixed components");
        fresh_component["component"]=json!("../outside");
        assert!(propose(&state,"session",&fresh_component).is_err());

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
    fn compiled_successor_distinguishes_whole_system_design_from_downstream_tasks() {
        let blueprint=json!({"generatorCommit":"source-revision",
            "architecture":[{"role":"harness","representativeSources":[]}],
            "recentMeasuredGenerations":[]});
        let task="Improve a specific Rust parser and preserve existing tests";
        let instructions=instruction(&blueprint,task);
        assert!(instructions.contains(task));
        assert!(instructions.contains("solve THAT task"));
        assert!(instructions.contains("rsi_system_plan"));
        assert!(instructions.contains("workspace_replace_range"));
        assert!(!instructions.contains("workspace_replace_function"),
            "do not advertise a tool missing from the active harness");
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
            "acceptedCommit":"child","candidateId":"verified-candidate",
            "candidateOrigin":"model_driven_detached_worktree",
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
        let mut after_failure=vec![base.clone(),json!({
            "id":"failed-next","accepted":false,"domain":"learning",
            "error":"first attempt did not compile","newFindings":1
        })];
        assert_eq!(inherited_domain(&after_failure),Some("learning".into()),
            "one failure must not erase an adopted design");
        assert_eq!(lineage_capsule(&after_failure)["acceptedDesigns"].as_array().unwrap().len(),1);
        assert_eq!(lineage_capsule(&after_failure)["failureCounterexamples"].as_array().unwrap().len(),1);
        after_failure.push(json!({"id":"failed-again","accepted":false,
            "domain":"learning","error":"different approach failed"}));
        assert_eq!(inherited_domain(&after_failure),None,
            "after repeated failure a descendant may explore another domain");
        assert_eq!(lineage_capsule(&after_failure)["acceptedDesigns"].as_array().unwrap().len(),1,
            "target exploration must not discard adopted design history");
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

#[cfg(test)]
mod integration_contract_tests {
    use super::*;
    use crate::{config::Config,db::Store,state::AppState,vault::Vault};
    fn state()->(AppState,std::path::PathBuf) {
        let root=std::env::temp_dir().join(format!("rsi-integrated-contract-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        (state,root)
    }
    #[test]
    fn source_portfolio_exposes_new_meta_mechanisms_to_descendants() {
        let (state,root)=state();
        fs::write(root.join("src/engine/rsi_evolve.rs"),"pub fn parent() {}\n").unwrap();
        fs::write(root.join("src/engine/rsi_meta_controller.rs"),
            "pub fn meta_improve() {}\n").unwrap();
        fs::write(root.join("src/engine/rsi_system.rs"),"pub fn plan() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let blueprint=blueprint(&root,"source-commit",&[]);
        let frontier=architecture_frontier(&state,&blueprint,&[],
            index["id"].as_str().unwrap(),&root);
        let improvers=frontier.iter().filter(|row|
            row["role"]=="improvement_methods").collect::<Vec<_>>();
        assert!(improvers.len()>=3,
            "meta-self-modification needs multiple candidate source files");
        assert!(improvers.iter().any(|row|
            row["path"]=="src/engine/rsi_meta_controller.rs"));
        let target=improvers[0].clone();
        let plan=json!({"systemBlueprint":blueprint,
            "architectureFrontier":frontier,"architectureTarget":target});
        assert!(validate_plan_evidence(&state,&root,"source-commit",
            index["id"].as_str().unwrap(),&plan).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn supervisor_accepts_new_frontier_order_and_role_but_rejects_stale_source() {
        let (state,root)=state();
        fs::write(root.join("src/engine/memory.rs"),"pub fn recall() {}\n").unwrap();
        fs::write(root.join("src/engine/tools.rs"),"pub fn act() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut model=blueprint(&root,"revision",&[]);
        let frontier=architecture_frontier(&state,&model,&[],index["id"].as_str().unwrap(),&root);
        assert_eq!(frontier.len(),2);
        let mut changed_order=frontier.clone();changed_order.reverse();
        let plan=json!({"systemBlueprint":model,"architectureFrontier":changed_order,
            "architectureTarget":changed_order.last()});
        assert!(validate_plan_evidence(&state,&root,"revision",index["id"].as_str().unwrap(),&plan).is_ok(),
            "the old parent must not require its own ranking or first target");
        let mut new_role=plan.clone();
        model["architecture"].as_array_mut().unwrap().push(json!({"role":"novel_executor","domain":"tools",
            "representativeSources":[{"path":"src/engine/tools.rs","sha256":frontier.iter().find(|r|r["role"]=="tools").unwrap()["sourceSha256"]}]}));
        new_role["systemBlueprint"]=model;
        assert!(validate_plan_evidence(&state,&root,"revision",index["id"].as_str().unwrap(),&new_role).is_ok());
        fs::write(root.join("src/engine/tools.rs"),"pub fn act_differently() {}\n").unwrap();
        assert!(validate_plan_evidence(&state,&root,"revision",index["id"].as_str().unwrap(),&new_role).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn real_agent_failures_become_descendant_scheduling_evidence() {
        let (state,root)=state();fs::create_dir_all(root.join("src/engine/chat")).unwrap();
        fs::write(root.join("src/engine/chat/runtime.rs"),"pub fn run() {}\n").unwrap();
        fs::write(root.join("src/engine/decompose.rs"),"pub fn short() {}\n").unwrap();
        state.store.put("execution",&json!({"id":"ordinary-task","taskOutcome":"failed","failureClass":"tool","error":"file operation failed"}));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let rows=architecture_frontier(&state,&blueprint(&root,"r",&[]),&[],index["id"].as_str().unwrap(),&root);
        assert_eq!(rows[0]["path"],"src/engine/chat/runtime.rs");
        assert!(rows[0]["usageSignalCount"].as_u64().unwrap()>0);
        assert_eq!(usage_evidence(&state)["signals"][0]["evidence"][0]["executionId"],"ordinary-task");
        assert_eq!(rows[0]["qualityProven"],false);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unverified_success_claim_does_not_close_an_architecture_gap() {
        let (state,root)=state();fs::write(root.join("src/engine/memory.rs"),"pub fn recall() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();let model=blueprint(&root,"r",&[]);
        let fake=json!({"accepted":true,"pairedRevalidated":true,"candidateId":"nonexistent",
            "focusFiles":["src/engine/memory.rs"],"systemDesign":{"component":"memory"}});
        let rows=architecture_frontier(&state,&model,&[fake],index["id"].as_str().unwrap(),&root);
        assert_eq!(rows[0]["verifiedRoleChanged"],false);assert_eq!(rows[0]["sourceAttempts"],1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn multi_component_design_requires_complete_source_mapping_and_complete_patch() {
        let (state,root)=state();
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"running"}));
        state.store.put("job",&json!({"id":"child","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"campaign","rsiSystemEvolution":true,"rsiGeneratorCommit":"a".repeat(40)}}));
        state.store.put("session",&json!({"id":"session","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"child"}));
        let mut body=json!({"component":"system","components":["memory","novel_planner"],"nextDomain":"memory",
            "hypothesis":"A source-backed coupled redesign should retain original facts and reduce retries",
            "design":"Introduce a new planner module and bind its memory access to an independently checked context protocol",
            "targetFiles":["src/engine/memory.rs","src/engine/new_planner.rs"]});
        assert!(propose(&state,"session",&body).is_err());
        body["componentFiles"]=json!({"memory":["src/engine/memory.rs"],"novel_planner":["src/engine/new_planner.rs"]});
        let proposal=propose(&state,"session",&body).unwrap()["proposal"].clone();
        assert_eq!(proposal["components"].as_array().unwrap().len(),2);
        assert!(!design_implemented(&proposal,&[json!("src/engine/memory.rs")]));
        assert!(design_implemented(&proposal,body["targetFiles"].as_array().unwrap()));
        let mut history=json!({"accepted":true,"evaluation":{"metrics":{"eligible":true}},"generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"jobId":"child","systemDesign":proposal,
            "acceptedSourceHashes":{"src/engine/memory.rs":"c".repeat(64)}});
        assert!(inherited_domain(&[history.clone()]).is_none());
        history["acceptedSourceHashes"]["src/engine/new_planner.rs"]=json!("d".repeat(64));
        assert_eq!(inherited_domain(&[history]),Some("memory".into()));
        fs::remove_dir_all(root).unwrap();
    }
    // These are simulated recorded-experiment receipts, not claims of new live
    // RSI gains. Exercise the real independent result replayer and reject
    // forged flags, incomplete changes and unwitnessed method transfer.
    fn verified_generation(state:&AppState,n:usize,roles:[&str;2])->Value {
        let generator=format!("{}",n+1).repeat(40);let accepted=format!("{}",n+2).repeat(40);
        let paths=[format!("src/engine/{}_part.rs",roles[0]),format!("src/engine/{}_part.rs",roles[1])];
        let patch=paths.iter().map(|path|format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-fn before() {{}}\n+fn after() {{}}\n")).collect::<String>();
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i<4{"improver"}else{"memory"},
            "holdout":i%2==0,"filter":format!("fixture::case_{i}"),"metrics":[{"key":"quality","direction":"higher"}]})).collect::<Vec<_>>();
        let c=crate::engine::rsi::propose(state,&json!({"target":"harness","baseCommit":generator,"patch":patch,
            "hypothesis":"An independently measured fixture generation with causal source and design binding",
            "cases":cases,"oracleVersion":"contract-fixture","generatorModel":"fixture"})).unwrap();
        let report=json!({"baseCommit":generator,"patchSha256":c["patchSha256"],"oracleVersion":"contract-fixture",
            "runner":"fixture","reportSha256":"f".repeat(64),"baselineBuildOk":true,"candidateBuildOk":true,
            "cases":cases.iter().map(|case|json!({"id":case["id"],"baselinePass":true,"candidatePass":true,
                "metrics":{"quality":{"baseline":0.5,"candidate":0.8}}})).collect::<Vec<_>>()});
        let id=c["id"].as_str().unwrap();let mut c=crate::engine::rsi::evaluate(state,id,&report).unwrap();
        c["origin"]=json!("model_driven_detached_worktree");c["experimentAdoption"]=json!({"commit":accepted});
        c["evaluation"]["source"]=json!("server_invoked_local_paired_git_worktrees");
        c["evaluation"]["evidenceSha256"]=json!("f".repeat(64));state.store.put("rsi_candidate",&c);
        state.store.put("rsi_pair",&json!({"id":format!("rsi-pair-{id}"),"candidateId":id,"runner":"local_gitrepo_not_os_sandbox",
            "report":report,"evidenceSha256":"f".repeat(64),"metrics":c["evaluation"]["metrics"],"verdict":c["evaluation"]["verdict"]}));
        let mut hashes=serde_json::Map::new();for path in &paths{hashes.insert(path.clone(),json!("e".repeat(64)));}
        let mut mapping=serde_json::Map::new();for (role,path) in roles.iter().zip(paths.iter()){mapping.insert(role.to_string(),json!([path]));}
        let blueprint=json!({"generatorCommit":generator,"sourceTreeSha256":"a".repeat(64)});
        json!({"id":format!("generation-{n}"),"jobId":format!("job-{n}"),"accepted":true,"systemEvolution":true,
            "generatorCommit":generator,"acceptedCommit":accepted,"generatorBinarySha256":format!("{}",n+1).repeat(64),
            "candidateId":id,"systemBlueprintVerified":true,"systemBlueprint":blueprint,"tokenUsage":{"calls":1},
            "controllerPlan":{"source":"compiled_descendant_system_controller_v2","generatorCommit":generator,"systemBlueprint":blueprint},
            "acceptedSourceHashes":hashes,"systemDesign":{"source":"model_authored_system_design","component":"system","components":roles,
                "componentFiles":mapping,"generatorCommit":generator,"generationJobId":format!("job-{n}"),"targetFiles":paths}})
    }
    #[test]
    fn complete_handoff_requires_real_pair_replay_complete_design_and_inherited_method_execution() {
        let (state,root)=state();
        let mut history=vec![verified_generation(&state,0,["algorithms","harness"]),
            verified_generation(&state,1,["memory","learning"]),verified_generation(&state,2,["tools","improvement_methods"])];
        let first=handoff_evidence(&state,&history);
        assert_eq!(first["verifiedLineageLength"],3);assert_eq!(first["demonstrated"],false);
        assert_eq!(first["missingComponents"],json!([]));assert_eq!(first["metaImprovementObserved"],true);
        history[0]["methodProposals"]=json!([{"id":"new-method","revision":"a".repeat(64),"sourceGenerationJobId":"job-0",
            "verification":{"type":"procedure","nodes":[{"tool":"code_search"}]}}]);
        history[1]["methodExecutions"]=json!([{"methodId":"new-method","revision":"a".repeat(64),"executionId":"exec-1","operationIds":["proof-write"],"result":{"verified":true,"followed":true}}]);
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false,"an assertion of execution is not causal proof");
        state.store.put("experience",&json!({"id":"method-use","executableMethods":[{"id":"new-method","revision":"a".repeat(64),
            "applications":[{"jobId":"job-1","methodWroteAcceptedCandidate":true,"verificationVerdict":"passed"}]}]}));
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false,"a stored application flag is not a tool receipt");
        state.store.put("job",&json!({"id":"job-1","sessionId":"session-1"}));
        let mut operation=json!({"id":"proof-write","runId":"exec-1","sessionId":"session-1",
            "origin":"procedure","procedureStrategyId":"new-method","procedureRevision":"a".repeat(64),
            "status":"completed","tool":"workspace_replace_range","arguments":{"path":"src/engine/memory_part.rs"},
            "result":{"written":true},"toolReceipt":{"verified":true,"observedVersion":format!("sha256:{}","e".repeat(64))}});
        state.store.put("execution_operation",&operation);
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],true);
        operation["toolReceipt"]["observedVersion"]=json!(format!("sha256:{}","d".repeat(64)));
        state.store.put("execution_operation",&operation);
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false,"superseded method output cannot claim the adopted code");
        operation["toolReceipt"]["observedVersion"]=json!(format!("sha256:{}","e".repeat(64)));
        state.store.put("execution_operation",&operation);
        history[1]["acceptedSourceHashes"]["src/engine/learning_part.rs"]=Value::Null;
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false);
        fs::remove_dir_all(root).unwrap();
    }
}
