//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/rsi/evaluation/worktree.rs","src/engine/chat.rs","src/engine/decompose.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&["src/engine/rsi_meta_controller.rs","src/engine/rsi_evolve.rs","src/engine/rsi/execution/mutation.rs","src/engine/rsi_system.rs","src/engine/rsi/meta/symbiosis.rs","src/engine/rsi/evaluation/intervention_lab.rs","src/engine/rsi_recursive_rewrite.rs","src/engine/rsi_principle_learning.rs","src/engine/rsi/meta/model_ecology.rs","src/engine/rsi_prequential.rs"]),
    ("model_architecture",&["src/engine/models.rs","src/engine/llm.rs","src/engine/cli_model.rs","src/engine/learning.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];


/**
 * Only independently revalidated, actually adopted source edits can become
 * an inherited design. Failed research-only descendants must not erase the
 * last successful design, and unverified prose must never become policy.
 */
fn valid_adopted_system_design(generation:&Value)->bool {
    let design=&generation["systemDesign"];
    generation["accepted"]==true
        && generation["pairedRevalidated"]==true
        && generation["evaluation"]["metrics"]["eligible"]==true
        && generation["candidateOrigin"]=="model_driven_detached_worktree"
        && generation["acceptedCommit"].as_str().is_some_and(|commit|!commit.is_empty())
        && design["source"]=="model_authored_system_design"
        && design["generatorCommit"]==generation["generatorCommit"]
        && design["generationJobId"]==generation["jobId"]
        && design["targetFiles"].as_array().is_some_and(|paths|
            !paths.is_empty() && paths.iter().all(|item|
                item.as_str().is_some_and(|path|
                    generation["acceptedSourceHashes"][path].as_str()
                        .is_some_and(|hash|hash.len()==64
                            && hash.bytes().all(|b|b.is_ascii_hexdigit())))))
}

fn last_verified_system_design(history:&[Value])->Option<&Value>{
    history.iter().rev().find(|row|valid_adopted_system_design(row))
}

/**
 * Whole-source fingerprint: component examples are never a hard-coded
 * mutation allowlist. Record source revisions beyond the representative list
 * while keeping the serialized model bounded for descendant workers.
 */
fn source_tree_manifest(root: &Path) -> Value {
    let Ok(root) = fs::canonicalize(root) else {
        return json!({"fileCount":0,"sourceTreeSha256":Value::Null,"paths":[]});
    };
    let mut directories=vec![
        root.join("src"), root.join("web/src"), root.join("scripts"),
        root.join("tests"), root.join(".github/workflows"), root.join("assets")
    ];
    let mut paths=std::collections::BTreeSet::<String>::new();
    for entry in ["Cargo.toml","build.rs","web/package.json"] {
        if root.join(entry).is_file() {paths.insert(entry.to_string());}
    }
    while let Some(dir)=directories.pop() {
        if paths.len()>=2048 {break;}
        let Ok(entries)=fs::read_dir(&dir) else {continue;};
        for entry in entries.flatten() {
            let Ok(kind)=entry.file_type() else {continue;};
            if kind.is_symlink() {continue;}
            let path=entry.path();
            if kind.is_dir() {directories.push(path);continue;}
            if !kind.is_file() {continue;}
            let supported=path.extension().and_then(|ext|ext.to_str())
                .is_some_and(|ext|matches!(ext,
                    "rs"|"py"|"ts"|"tsx"|"js"|"mjs"|"json"|"toml"|"yml"|"yaml"));
            if !supported {continue;}
            if let Ok(relative)=path.strip_prefix(&root) {
                if let Some(name)=relative.to_str() {
                    paths.insert(name.to_string());
                }
            }
        }
    }
    let mut fingerprint=Sha256::new();
    let mut included=Vec::new();
    for relative in paths.iter().take(2048) {
        let Ok(file)=fs::canonicalize(root.join(relative)) else {continue;};
        if !file.starts_with(&root) || !file.is_file() {continue;}
        let Ok(bytes)=fs::read(file) else {continue;};
        fingerprint.update(relative.as_bytes());
        fingerprint.update([0]);
        fingerprint.update(Sha256::digest(&bytes));
        included.push(relative.clone());
    }
    json!({"fileCount":included.len(),
        "sourceTreeSha256":format!("{:x}",fingerprint.finalize()),
        "paths":included.iter().take(96).collect::<Vec<_>>(),
        "additionalPaths":included.len().saturating_sub(96),
        "complete":paths.len()<=2048})
}


fn architecture_domain(role:&str)->Option<&'static str> {
    match role {
        "algorithms"|"improvement_methods"=>Some("improver"),
        "harness"=>Some("context"), "memory"=>Some("memory"),
        "learning"|"model_architecture"=>Some("learning"),"tools"=>Some("tools"),
        "frontend"=>Some("ui"),_=>None
    }
}

/// An executable descendant inspects the exact code it is about to modify
/// and the independently revalidated adopted generations. Components without
/// a prior demonstrated source change enter the search frontier first. This
/// chooses *experiments*, never grants improvement credit by itself.
pub(crate) fn architecture_frontier(
    state:&crate::state::AppState,
    blueprint:&Value,history:&[Value],index_id:&str,root:&Path
)->Vec<Value> {
    use std::collections::{BTreeMap,BTreeSet};
    let history=revalidated_history(state,history);
    let mut verified_roles=BTreeSet::<String>::new();
    let mut attempts=BTreeMap::<String,u64>::new();
    for generation in &history {
        for path in generation["focusFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str) {
            *attempts.entry(path.to_string()).or_default()+=1;
        }
        if generation["accepted"]!=true || generation["pairedRevalidated"]!=true
            || generation["candidateOrigin"]!="model_driven_detached_worktree"
            || generation["systemDesign"]["source"]!="model_authored_system_design"
            || generation["systemDesign"]["generatorCommit"]!=generation["generatorCommit"]
            || generation["systemDesign"]["generationJobId"]!=generation["jobId"] {
            continue;
        }
        let Some(candidate)=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
        if candidate["status"]!="verified"
            || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
            || candidate["experimentAdoption"]["generatorCommit"]!=generation["generatorCommit"] {
            continue;
        }
        for path in candidate["paths"].as_array().into_iter().flatten()
            .filter_map(Value::as_str) {
            let Some(hash)=generation["acceptedSourceHashes"][path].as_str()
                .filter(|hash|hash.len()==64
                    && hash.bytes().all(|b|b.is_ascii_hexdigit())) else{continue};
            for component in blueprint["architecture"].as_array().into_iter().flatten(){
                let Some(role)=component["role"].as_str() else{continue};
                let mapped=generation["systemDesign"]["componentFiles"][role]
                    .as_array().is_some_and(|files|
                        files.iter().any(|file|file.as_str()==Some(path)));
                let legacy_single=generation["systemDesign"]["component"]==role
                    && generation["systemDesign"]["componentFiles"].is_null();
                if (mapped || legacy_single)
                    && component["representativeSources"].as_array().into_iter().flatten()
                        .any(|item|item["path"]==path) {
                    // The SHA is an independently adopted source receipt.
                    // Subsequent changes to this module do not erase
                    // independently verified historical coverage.
                    let _=hash;
                    verified_roles.insert(role.to_string());
                }
            }
        }
    }
    let core=["algorithms","harness","memory","learning","tools","improvement_methods"];
    let complete_core=core.iter().all(|role|verified_roles.contains(*role));
    let mut frontier=Vec::new();
    let canonical=match fs::canonicalize(root){Ok(value)=>value,Err(_)=>return frontier};
    for component in blueprint["architecture"].as_array().into_iter().flatten(){
        let Some(role)=component["role"].as_str() else{continue};
        if role=="frontend" && !complete_core {continue;}
        let Some(domain)=component["domain"].as_str()
            .filter(|domain|crate::engine::rsi_evolve::DOMAINS.contains(domain))
            .or_else(||architecture_domain(role)) else{continue};
        let improved=verified_roles.contains(role);
        let mut sources=component["representativeSources"].as_array().into_iter().flatten()
            .filter_map(|item|{
                let path=item["path"].as_str()?;
                if !crate::engine::rsi::safe_path(path){return None;}
                let hash=item["sha256"].as_str()?;
                let bytes=item["bytes"].as_u64()?;
                if crate::engine::code_index::indexed_source_hash(state,index_id,path)
                    .as_deref()!=Some(hash){return None;}
                let file=fs::canonicalize(canonical.join(path)).ok()?;
                if !file.starts_with(&canonical)||!file.is_file(){return None;}
                let actual=fs::read(&file).ok()?;
                if actual.len() as u64!=bytes
                    || format!("{:x}",Sha256::digest(&actual))!=hash{return None;}
                Some((path.to_string(),hash.to_string(),bytes))
            }).collect::<Vec<_>>();
        sources.sort_by(|a,b|
            attempts.get(&a.0).copied().unwrap_or(0)
                .cmp(&attempts.get(&b.0).copied().unwrap_or(0))
            .then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
        let Some((path,hash,bytes))=sources.into_iter().next() else{continue};
        let count=attempts.get(&path).copied().unwrap_or(0);
        frontier.push(json!({"role":role,"domain":domain,"path":path,
            "sourceSha256":hash,"sourceBytes":bytes,"sourceAttempts":count,
            "verifiedRoleImproved":improved,
            "reason":if improved{"extend_accepted_ability"}
                else{"untouched_core_intelligence"},
            "qualityProven":false
        }));
    }
    frontier.sort_by(|a,b|
        (a["verifiedRoleImproved"]==true).cmp(&(b["verifiedRoleImproved"]==true))
            .then(a["sourceAttempts"].as_u64().cmp(&b["sourceAttempts"].as_u64()))
            .then(a["sourceBytes"].as_u64().cmp(&b["sourceBytes"].as_u64()))
            .then(a["role"].as_str().cmp(&b["role"].as_str())));
    frontier
}


/// Discover live source by module semantics, including newly created code.
/// Static paths above remain orientation examples, never the whole search
/// space. The executable meta-improver can now choose real functions outside
/// yesterday's hand-picked file set.
fn discovered_architecture_sources(root:&Path)->Vec<(String,Vec<&'static str>,Value)>{
    let Ok(root)=fs::canonicalize(root) else{return Vec::new()};
    let mut pending=vec![root.join("src"),root.join("web/src"),root.join("scripts")];
    let mut files=std::collections::BTreeMap::<String,(Vec<&'static str>,Value)>::new();
    while let Some(directory)=pending.pop(){
        if files.len()>=2048 {break;}
        let Ok(entries)=fs::read_dir(&directory) else{continue};
        for entry in entries.flatten(){
            let Ok(kind)=entry.file_type() else{continue};
            if kind.is_symlink(){continue;}
            let path=entry.path();
            if kind.is_dir(){pending.push(path);continue;}
            if !kind.is_file(){continue;}
            let Some(relative)=path.strip_prefix(&root).ok()
                .and_then(|value|value.to_str()) else{continue};
            if !crate::engine::rsi::safe_path(relative)
                || relative.ends_with("_tests.rs")
                || relative.ends_with("/rsi_oracle_fixture.rs")
                || relative=="src/bin/dominion-benchmark.rs"
                {continue;}
            let supported=path.extension().and_then(|ext|ext.to_str())
                .is_some_and(|ext|matches!(ext,"rs"|"py"|"ts"|"tsx"|"js"|"jsx"|"mjs"));
            if !supported{continue;}
            let name=relative.to_ascii_lowercase();
            let mut roles=Vec::new();
            if name.starts_with("web/"){roles.push("frontend");}
            else if name.starts_with("src/") || name.starts_with("scripts/"){
                if name.contains("/rsi")||name.contains("algorithm_eval")
                    ||name.contains("policy_patch")||name.contains("improve")
                    ||name.contains("agent_growth"){
                    roles.push("improvement_methods");
                }
                if name.contains("algorith")||name.contains("reason")
                    ||name.contains("decompose")||name.contains("goal_control")
                    ||name.contains("router")||name.contains("policy"){
                    roles.push("algorithms");
                }
                if name.contains("memory")||name.contains("knowledge")
                    ||name.contains("room_state")||name.contains("compact")
                    ||name.contains("recall"){
                    roles.push("memory");
                }
                if name.contains("model")||name.contains("network")
                    ||name.contains("train")||name.contains("inference")
                    ||name.contains("weight")||name.contains("tokeniz")
                    ||name.contains("embedding")||name.contains("transformer"){
                    roles.push("model_architecture");
                }
                if name.contains("learn")||name.contains("experience")
                    ||name.contains("distill")||name.contains("adaptation")
                    ||name.contains("procedure")||name.contains("reflect"){
                    roles.push("learning");
                }
                if name.contains("tool")||name.contains("/mcp")
                    ||name.contains("browser")||name.contains("gateway")
                    ||name.contains("bridge")||name.contains("cli_model"){
                    roles.push("tools");
                }
                if name.contains("harness")||name.contains("jobs")
                    ||name.contains("scheduler")||name.contains("runtime")
                    ||name.contains("worktree")||name.contains("run_state")
                    ||name.contains("chat")||name.contains("lease")
                    ||name.contains("verification"){
                    roles.push("harness");
                }
                // All executable source not otherwise classified still
                // belongs to the agent harness; it cannot disappear from
                // architectural redesign simply because its filename is new.
                if roles.is_empty(){roles.push("harness");}
            }
            if roles.is_empty(){continue;}
            let Ok(file)=fs::canonicalize(&path) else{continue};
            if !file.starts_with(&root) || !file.is_file(){continue;}
            let Ok(contents)=fs::read(file) else{continue};
            if contents.len()>2_000_000{continue;}
            let row=json!({"path":relative,
                "sha256":format!("{:x}",Sha256::digest(&contents)),
                "bytes":contents.len()});
            files.insert(relative.to_string(),(roles,row));
        }
    }
    files.into_iter().map(|(name,(roles,row))|(name,roles,row)).collect()
}

/// Read only source assets under the experiment's pinned checkout. The
/// coordinator cannot author or alter this blueprint without changing its
/// measured generator revision; no holdout content is read here.
pub(crate) fn blueprint(root:&Path, revision:&str, history:&[Value])->Value {
    let canonical_root=fs::canonicalize(root).ok();
    let discovered=discovered_architecture_sources(root);
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
        let mut known=inventory.iter().filter_map(|item|item["path"].as_str())
            .map(str::to_string).collect::<std::collections::BTreeSet<_>>();
        for (relative,roles,row) in &discovered {
            if inventory.len()>=12{break;}
            if roles.contains(role) && known.insert(relative.clone()){
                inventory.push(row.clone());
            }
        }
        components.push(json!({"role":role,"representativeSources":inventory}));
    }
    components.extend(crate::engine::rsi_open_topology::source_components(root,history));
    let inherited=last_verified_system_design(history);
    let evidence=history.iter().rev().take(8).map(|generation|json!({
        "generatorCommit":generation["generatorCommit"],
        "acceptedCommit":generation["acceptedCommit"],
        "domain":generation["domain"],
        "candidateTarget":generation["candidateTarget"],
        "accepted":generation["accepted"]==true,
        "sourceWritingModel":generation["modelEcologyExecuted"],
        "verdict":generation["evaluation"]["verdict"],
        "improvedHoldouts":generation["evaluation"]["metrics"]["improvedHoldouts"],
        "metaHoldoutGains":generation["evaluation"]["metrics"]["metaHoldoutGains"],
        "regressions":generation["evaluation"]["metrics"]["regressions"],
        "candidateOrigin":generation["candidateOrigin"]
    })).collect::<Vec<_>>();
    json!({
        "source":"compiled_descendant_system_blueprint_v1",
        "generatorCommit":revision,
        "architecture":components,
        "sourceTree":source_tree_manifest(root),
        "historyCount":history.len(),
        "inheritedDesign":inherited.map(|row|row["systemDesign"].clone()).unwrap_or(Value::Null),
        "inheritedDesignEvidence":inherited.map(|row|json!({
            "acceptedCommit":row["acceptedCommit"],
            "generatorCommit":row["generatorCommit"],
            "generationJobId":row["jobId"],
            "pairedRevalidated":true
        })).unwrap_or(Value::Null),
        "recentMeasuredGenerations":evidence,
        "scope":"whole_system",
        "designOwnership":"compiled_descendant_and_its_model_worker",
        "immutableBoundary":["experiment_baseline","frozen_paired_evaluator","budget","production_apply_gate"]
    })
}


/// The frozen boundary validates source ownership, indexed hashes and exact
/// donor lineage. It must not rerun the child's editable source-ranker or
/// metacognitive selection algorithm: doing so would prevent genuine
/// self-modification of the improver itself.
pub(crate) fn validate_plan_evidence(
    state:&crate::state::AppState,root:&Path,plan:&Value,index_id:&str
)->Result<(),String>{
    let root=fs::canonicalize(root).map_err(|e|e.to_string())?;
    if plan["indexId"]!=index_id
        || plan["sourceTreeSha256"]!=plan["systemBlueprint"]["sourceTree"]["sourceTreeSha256"]
        || plan["systemBlueprint"]["sourceTree"]["sourceTreeSha256"].as_str()
            .is_none_or(|sha|sha.len()!=64
                || !sha.bytes().all(|b|b.is_ascii_hexdigit())) {
        return Err("후손 자기모형의 실제 소스 지문 또는 색인이 일치하지 않습니다.".into());
    }
    let frontier=plan["architectureFrontier"].as_array()
        .filter(|rows|rows.len()<=32)
        .ok_or("후손 전체 시스템 구조 근거가 없거나 비정상적으로 큽니다.")?;
    let ranked=plan["rankedFiles"].as_array()
        .filter(|rows|rows.len()<=32)
        .ok_or("후손 코드 대상 목록이 범위를 벗어났습니다.")?;
    // All source facts are verified at the current detached revision.
    // Ranked scores, role assignment and source-search strategy are mutable
    // intelligence and deliberately NOT reconstructed by the supervisor.
    for row in frontier.iter().chain(ranked.iter()) {
        let path=row["path"].as_str()
            .filter(|path|crate::engine::rsi::safe_path(path))
            .ok_or("후손이 허용되지 않은 소스 경로를 선택했습니다.")?;
        let hash=row["sourceSha256"].as_str()
            .or_else(||row["sourceHash"].as_str())
            .filter(|sha|sha.len()==64
                && sha.bytes().all(|b|b.is_ascii_hexdigit()))
            .ok_or("선택된 소스의 원본 해시가 없습니다.")?;
        if crate::engine::code_index::indexed_source_hash(state,index_id,path)
            .as_deref()!=Some(hash){
            return Err(format!("후손 소스 색인이 실제 revision과 다릅니다: {path}"));
        }
        let file=fs::canonicalize(root.join(path))
            .map_err(|e|format!("후손 소스 경로 없음: {path}: {e}"))?;
        if !file.starts_with(&root)||!file.is_file(){
            return Err("소스 범위를 벗어난 자기수정 대상입니다.".into());
        }
        let bytes=fs::read(&file).map_err(|e|e.to_string())?;
        if format!("{:x}",Sha256::digest(&bytes))!=hash{
            return Err(format!("실제 후손 코드가 기록된 해시와 다릅니다: {path}"));
        }
    }
    let target=&plan["architectureTarget"];
    let meta=&plan["metaPolicy"];
    if meta["target"]!=*target || meta["revision"].as_str()
        .is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){
        return Err("후손 메타 판단의 설계 대상 또는 실행 revision이 불일치합니다.".into());
    }
    if target.is_object(){
        if !frontier.iter().any(|row|row==target){
            return Err("후손이 전체 시스템 소스에 없는 대상을 선택했습니다.".into());
        }
    }else if !target.is_null(){
        return Err("후손 선택의 데이터 형식이 올바르지 않습니다.".into());
    }
    let coupling=&plan["symbioticCurriculum"];
    if coupling["source"]!="compiled_mutual_intelligence_curriculum_v1"
        || coupling["primary"]!=*target
        || coupling["revision"].as_str()
            .is_none_or(|sha|sha.len()!=64||!sha.bytes().all(|c|c.is_ascii_hexdigit())) {
        return Err("후손의 상호 지능 개선 계획의 출처가 다릅니다.".into());
    }
    let peer=&coupling["complement"];
    if peer.is_object(){
        if peer["fromRole"]!=target["role"]
            || peer["fromPath"]!=target["path"]
            || peer["fromSha256"]!=target["sourceSha256"]
            || !frontier.iter().any(|source|
                source["role"]==peer["toRole"] && source["path"]==peer["toPath"]
                && source["sourceSha256"]==peer["toSha256"])
            || coupling["recursiveLoop"].as_array().is_none_or(|stages|
                stages.len()!=3){
            return Err("후손 상호 개선 대상의 소스 계보가 불일치합니다.".into());
        }
    }else if !peer.is_null(){
        return Err("후손 상호 개선 계획의 자료형이 올바르지 않습니다.".into());
    }
    let factorial=&plan["factorialExperiment"];
    if factorial.is_object() && (
        factorial["selected"]["target"]!=*target
        || meta["factorialExperimentSha256"]!=factorial["sha256"]
        || meta["mode"]!="independently_matched_factorial_experiment_design"){
        return Err("요인별 실험이 실제 후손 메타 선택과 일치하지 않습니다.".into());
    }
    let negative=&plan["negativeTransfer"];
    if negative.is_object(){
        let selected=&negative["selected"];
        let source=&selected["source"];
        let victim=&selected["victim"];
        if source!=target
            || meta["negativeTransfer"]["active"]!=true
            || meta["negativeTransfer"]["selected"]!=*selected
            || meta["negativeTransfer"]["evidenceSha256"]!=negative["sha256"]
            || meta["mode"]!="replayed_cross_capability_negative_transfer_repair"
            || ranked.len()!=2
            || ranked[0]["path"]!=source["path"]
            || ranked[0]["sourceHash"]!=source["sourceSha256"]
            || ranked[1]["path"]!=victim["path"]
            || ranked[1]["sourceHash"]!=victim["sourceSha256"] {
            return Err("역전이 검증 계획이 실제 두 소스 편집 목표와 다릅니다.".into());
        }
    }else if !negative.is_null() || meta["negativeTransfer"]["active"]==true {
        return Err("존재하지 않는 역전이 편집 근거가 지정됐습니다.".into());
    }
    let stage=&plan["successorStage"];
    if stage.is_object() && (
        stage["source"]!="rebound_verified_model_successor_program_v1"
        || stage["path"]!=target["path"]
        || stage["sourceSha256"]!=target["sourceSha256"]){
        return Err("모델이 작성한 후손 실행 단계가 실제 소스와 다릅니다.".into());
    }
    Ok(())
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
    let tree=&self_model["sourceTree"];
    let components=format!("{components}\nWhole-source snapshot: files={}, sha256={}, remaining indexed paths={}. Search other source files by code_index, not only the examples.",
        tree["fileCount"],tree["sourceTreeSha256"],tree["additionalPaths"]);
    let inherited=self_model["inheritedDesign"].as_object().map(|_|format!(
        "Previously independently adopted system design: nextDomain={} hypothesis={} implementation={} acceptedCommit={}",
        self_model["inheritedDesign"]["nextDomain"],self_model["inheritedDesign"]["hypothesis"],
        self_model["inheritedDesign"]["design"].as_str().unwrap_or("").chars().take(1200).collect::<String>(),
        self_model["inheritedDesignEvidence"]["acceptedCommit"])).unwrap_or_default();
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
        Before editing submit a structured, causal system design with rsi_system_plan (component, hypothesis, design, targetFiles, nextDomain), then actually WRITE code. For a coupled redesign of several subsystems set component=system and specify 2-24 actual or invented roles in components, with up to 24 source-grounded paths; you can change algorithm, harness, memory, learning, tools and improvement methods together. A design alone earns no credit. Where beneficial, propose a reusable executable method using \
        rsi_method_propose; mere registration is not evidence of execution. \
        You may INVENT intelligence roles via newComponents with role, \
        domain, purpose, sourcePaths and integrationPaths that wire your new \
        source into the executable agent. Dead modules are not new intelligence. \
        They become new editable descendant \
        components only after independently adopted source-level improvements. \
        For a coupled redesign list interactionEdges defining fromRole, toRole, \
        fromPath, toPath, mechanism and observable NEXT-GENERATION effect. \
        For a coupled redesign list componentFiles mapping EVERY declared \
        intelligence role to its actual targetFiles. Claims about unedited \
        roles will not be credited as inherited architecture changes. \
        You can submit successorStages in rsi_system_plan: concrete future roles, \
        source files, objectives and learning/tool/execution policies. After \
        independent adoption your new descendant will execute that program, \
        not merely display it in a report. \
        You may rewrite src/engine/rsi/evaluation/intervention_lab.rs and propose \
        experimentalPolicy (mode, explorationWeight, interactionWeight,maxPairs). \
        Only a new independently adopted experiment-planner source revision \
        may pass its model-authored policy to the next compiled child. \
        Every invented role should be linked back to an executable meta \
        improvement method: edit a real search/rewrite source and explain \
        how that method will improve this role in the NEXT binary. The \
        recursive policy can then demand an adopted meta edit, a later \
        adopted role edit, and repeat the loop without a fixed stop generation. \
        The meta-improver in src/engine/rsi_meta_controller.rs is ALSO YOUR EDITABLE CODE, \
        not a fixed external planner. You can redesign how experiments are generated, \
        compared, forecast, selected and inherited across generations. \
        Make your next-generation planning and implementation behavior better, \
        not only an external product function. The independently frozen paired \
        evaluator alone decides adoption; do not edit the evaluator. \
        When a genuine NEW improvement mechanism is implemented, submit \
        improvementPrinciples with id, mechanism, falsifier, prediction, \
        methodPaths, targetRoles, transferDomains and composedFrom. The \
        accepted descendant will evaluate subsequent cross-domain effects \
        and can synthesize NEW mechanisms from prior verified outcomes. \
        Do not credit a good hypothesis as an actual learned improvement. \
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
    let mut known=child["input"]["rsiSystemBlueprint"]["architecture"]
        .as_array().into_iter().flatten()
        .filter_map(|row|row["role"].as_str()).map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    // Built-in roles remain valid even for older generation receipts that
    // predate the compiled source-blueprint protocol.
    known.extend(COMPONENTS.iter().map(|(role,_)|(*role).to_string()));
    let mut named=known.clone();
    let mut new_components=Vec::<Value>::new();
    if let Some(rows)=body["newComponents"].as_array(){
        if rows.len()>4 || body["component"]!="system" {
            return Err("새 지능 구조는 전체 시스템 설계에서 최대 4개씩 작성합니다.".into());
        }
        for row in rows {
            let role=row["role"].as_str()
                .filter(|role|crate::engine::rsi_open_topology::valid_role(role))
                .ok_or("새 지능 구성요소의 이름이 올바르지 않습니다.")?;
            if !named.insert(role.to_string()){
                return Err("지능 구성요소 이름이 중복됩니다.".into());
            }
            let domain=row["domain"].as_str()
                .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
                .ok_or("새 지능 구성요소의 실행 분야가 없습니다.")?;
            let purpose=row["purpose"].as_str()
                .filter(|t|(25..=1200).contains(&t.chars().count()))
                .ok_or("새 지능 모듈의 개선 기능과 목적이 필요합니다.")?;
            let sources=row["sourcePaths"].as_array()
                .filter(|v|!v.is_empty()&&v.len()<=4)
                .ok_or("새 지능 구성요소에는 실제 소스 경로가 필요합니다.")?;
            if !sources.iter().all(|item|item.as_str().is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && body["targetFiles"].as_array().is_some_and(|files|
                    files.iter().any(|f|f==path)))){
                return Err("새 구성요소의 소스는 반드시 실제 변경 대상이어야 합니다.".into());
            }
            let integrations=row["integrationPaths"].as_array()
                .filter(|v|!v.is_empty()&&v.len()<=4)
                .ok_or("새 지능 구성요소를 실행하는 연결 소스 integrationPaths가 필요합니다.")?;
            if !integrations.iter().all(|item|item.as_str().is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && !sources.iter().any(|source|source==path)
                && body["targetFiles"].as_array().is_some_and(|files|
                    files.iter().any(|f|f==path)))){
                return Err("새 지능 모듈과 실행 하네스의 실제 변경 파일이 모두 필요합니다.".into());
            }
            new_components.push(json!({"role":role,"domain":domain,
                "purpose":purpose,"sourcePaths":sources,
                "integrationPaths":integrations,"qualityProven":false}));
        }
    }
    let component=body["component"].as_str()
        .filter(|name| *name=="system" || known.contains(*name))
        .ok_or("시스템 구성요소 지정 오류")?;
    let components=if component=="system" {
        let list=body["components"].as_array()
            .filter(|rows| (2..=24).contains(&rows.len()))
            .ok_or("전체 시스템 설계에는 구체적인 구성요소 2~24개가 필요합니다.")?;
        let mut seen=std::collections::BTreeSet::new();
        let mut chosen=Vec::new();
        for row in list {
            let name=row.as_str()
                .filter(|value|named.contains(*value))
                .ok_or("알 수 없는 시스템 구성요소입니다.")?;
            if !seen.insert(name) {
                return Err("중복된 시스템 구성요소입니다.".into());
            }
            chosen.push(name);
        }
        chosen
    }else{
        if body["components"].as_array().is_some_and(|rows|
            rows.len()!=1 || rows[0]!=component) {
            return Err("단일 구성요소 설계에는 다른 구성요소를 지정할 수 없습니다.".into());
        }
        vec![component]
    };
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
        .filter(|rows|!rows.is_empty() && rows.len()<=24 &&
            (component!="system" || rows.len()>=2))
        .ok_or("실제 소스 1~24개가 필요하며 시스템 재설계에는 2개 이상이 필요합니다.")?;
    let root=fs::canonicalize(&state.config.workspace).map_err(|e|e.to_string())?;
    let mut unique=std::collections::BTreeSet::new();
    for path in paths {
        let path=path.as_str().ok_or("소스 경로 문자열이 필요합니다.")?;
        if !crate::engine::rsi::safe_path(path)||!unique.insert(path.to_string()){
            return Err("중복 또는 허용되지 않은 소스 경로입니다.".into());
        }
        let candidate=root.join(path);
        if fs::symlink_metadata(&candidate).is_ok() {
            let file=fs::canonicalize(&candidate)
                .map_err(|_|"설계 대상 소스를 정규화할 수 없습니다.".to_string())?;
            if !file.starts_with(&root)||!file.is_file(){
                return Err("외부 파일은 설계 범위가 아닙니다.".into());
            }
        } else {
            // An architectural redesign can introduce a new module; requiring
            // every planned path to exist made genuine new architectures
            // impossible. Only a source file in an existing workspace
            // directory may be proposed, never an out-of-root symlink target.
            let parent=candidate.parent().ok_or("신규 소스의 상위 폴더가 없습니다.")?;
            let parent=fs::canonicalize(parent)
                .map_err(|_|"신규 소스의 상위 폴더가 없습니다.".to_string())?;
            let source_extension=candidate.extension().and_then(|ext|ext.to_str())
                .is_some_and(|ext|matches!(ext,
                    "rs"|"py"|"ts"|"tsx"|"js"|"mjs"|"json"|"toml"|"yml"|"yaml"));
            if !parent.starts_with(&root) || !parent.is_dir() || !source_extension {
                return Err("신규 설계 대상은 작업공간 내부의 실제 소스 경로여야 합니다.".into());
            }
        }
    }
    // A multi-capability redesign is a source-to-role program, not a label.
    // Each declared intelligence component must own at least one actually
    // changed source path. Shared executable modules can contribute to more
    // than one subsystem, but every claim must have grounded code evidence.
    let mut component_files=serde_json::Map::new();
    if component=="system" {
        let declared=body["componentFiles"].as_object()
            .ok_or("복합 설계에는 역할별 수정 파일 componentFiles가 필요합니다.")?;
        if declared.len()!=components.len(){
            return Err("역할별 수정 파일 구성이 선언한 지능 구성요소와 다릅니다.".into());
        }
        for role in &components {
            let files=declared.get(*role).and_then(Value::as_array)
                .filter(|files|!files.is_empty() && files.len()<=24)
                .ok_or("복합 설계의 모든 구성요소에 실제 소스가 필요합니다.")?;
            let mut seen=std::collections::BTreeSet::new();
            let mut verified=Vec::new();
            for entry in files {
                let path=entry.as_str().filter(|p|unique.contains(*p))
                    .ok_or("구성요소별 수정 파일은 실제 targetFiles에 있어야 합니다.")?;
                if !seen.insert(path){
                    return Err("구성요소별 소스 파일이 중복됐습니다.".into());
                }
                verified.push(path.to_string());
            }
            component_files.insert((*role).to_string(),json!(verified));
        }
    }else{
        component_files.insert(component.to_string(),json!(paths));
    }
    // Explicit machine-readable cross-component mechanism makes the
    // reciprocal claim falsifiable and reusable by the next binary. These
    // are design hypotheses until an independently adopted descendant
    // changes each declared component's actual source.
    let mut interaction_edges=Vec::new();
    if let Some(rows)=body["interactionEdges"].as_array(){
        if rows.len()>12 || (component!="system"&&!rows.is_empty()){
            return Err("상호 개선 관계는 전체 시스템 설계에서 최대 12개입니다.".into());
        }
        let mut seen=std::collections::BTreeSet::new();
        for edge in rows {
            let from=edge["fromRole"].as_str()
                .filter(|role|components.contains(role))
                .ok_or("상호 개선 시작 구성요소가 선언되지 않았습니다.")?;
            let to=edge["toRole"].as_str()
                .filter(|role|components.contains(role)&&*role!=from)
                .ok_or("상호 개선 대상 구성요소가 중복되거나 없습니다.")?;
            let from_path=edge["fromPath"].as_str()
                .filter(|path|component_files.get(from).and_then(Value::as_array)
                    .is_some_and(|paths|paths.iter().any(|row|row==*path)))
                .ok_or("상호 개선 원인 소스가 구성요소에 속하지 않습니다.")?;
            let to_path=edge["toPath"].as_str()
                .filter(|path|component_files.get(to).and_then(Value::as_array)
                    .is_some_and(|paths|paths.iter().any(|row|row==*path)))
                .ok_or("상호 개선 결과 소스가 구성요소에 속하지 않습니다.")?;
            if !seen.insert((from.to_string(),to.to_string())){
                return Err("중복된 방향성 개선 가설입니다.".into());
            }
            let mechanism=edge["mechanism"].as_str()
                .filter(|v|(20..=1200).contains(&v.chars().count()))
                .ok_or("구성요소 간 개선 작용 원리 20~1200자가 필요합니다.")?;
            let observable=edge["observable"].as_str()
                .filter(|v|(12..=800).contains(&v.chars().count()))
                .ok_or("후손의 개선 능력을 확인할 관찰 지표가 필요합니다.")?;
            interaction_edges.push(json!({"fromRole":from,"toRole":to,
                "fromPath":from_path,"toPath":to_path,
                "mechanism":mechanism,"observable":observable,
                "status":"unverified_causal_hypothesis",
                "qualityProven":false}));
        }
    }
    // A model can author the actual NEXT descendant's executable task
    // schedule. Merely declaring a stage does not prove improvement. The
    // rebuilt child binds each stage to current source SHA and controls the
    // real execution policy after a separately adopted source redesign.
    let mut successor_stages=Vec::new();
    if let Some(stages)=body["successorStages"].as_array() {
        if stages.len()>8{return Err("후손 개선 프로그램은 최대 8단계입니다.".into());}
        for stage in stages {
            let role=stage["role"].as_str()
                .filter(|role|named.contains(*role))
                .ok_or("후손 단계 지능 구성요소가 올바르지 않습니다.")?;
            let domain=stage["domain"].as_str()
                .filter(|domain|crate::engine::rsi_evolve::DOMAINS.contains(domain))
                .ok_or("후손 단계 개선 분야가 올바르지 않습니다.")?;
            let path=stage["path"].as_str()
                .filter(|p|crate::engine::rsi::safe_path(p))
                .ok_or("후손 단계 소스 경로가 올바르지 않습니다.")?;
            let objective=stage["objective"].as_str()
                .filter(|s|(25..=1200).contains(&s.chars().count()))
                .ok_or("후손 단계에는 구체적 실행 목표 25~1200자가 필요합니다.")?;
            let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
            let tools=stage["toolPolicy"].as_str().unwrap_or("allow_new_modules");
            let execution=stage["executionPolicy"].as_str().unwrap_or("explore_then_edit");
            if !matches!(learning,"avoid_failed"|"retry_failed")
                || !matches!(tools,"patch_only"|"allow_new_modules")
                || !matches!(execution,"grounded_edit"|"explore_then_edit") {
                return Err("후손 단계의 실행·학습·도구 정책이 올바르지 않습니다.".into());
            }
            let file=root.join(path);
            let safe_existing=fs::canonicalize(&file).ok()
                .is_some_and(|file|file.starts_with(&root)&&file.is_file());
            // A newly invented intelligence module does not exist in G1
            // until the model writes it. It can nevertheless be the real
            // first editing assignment of the independently adopted G2.
            let planned_new=new_components.iter().any(|component|
                component["role"]==role
                && component["sourcePaths"].as_array().is_some_and(|files|
                    files.iter().any(|entry|entry==path)))
                && unique.contains(path)
                && file.parent().and_then(|parent|fs::canonicalize(parent).ok())
                    .is_some_and(|parent|parent.starts_with(&root));
            if !safe_existing && !planned_new{
                return Err("후손 프로그램은 실재하거나 실제 생성할 예정인 자기 소스만 선택할 수 있습니다.".into());
            }
            successor_stages.push(json!({"role":role,"domain":domain,
                "path":path,"objective":objective,
                "learningPolicy":learning,"toolPolicy":tools,
                "executionPolicy":execution,"qualityProven":false}));
        }
    }
    // Invented algorithmic principles must be backed by ACTUAL editable
    // implementation paths. Their downstream impact is learned by compiled
    // descendants, never inferred from their description alone.
    let mut improvement_principles=Vec::<Value>::new();
    if let Some(rows)=body["improvementPrinciples"].as_array(){
        if rows.len()>8 {return Err("한 세대의 새로운 개선 원리는 최대 8개입니다.".into());}
        let mut seen=std::collections::BTreeSet::new();
        for row in rows {
            let id=row["id"].as_str().filter(|id|
                (4..=64).contains(&id.len()) && id.bytes().all(|c|
                    c.is_ascii_alphanumeric()||c==b'_'||c==b'-'))
                .ok_or("개선 원리 식별자가 올바르지 않습니다.")?;
            if !seen.insert(id){return Err("개선 원리 ID가 중복됩니다.".into());}
            let mechanism=row["mechanism"].as_str().filter(|s|
                (25..=1800).contains(&s.chars().count()))
                .ok_or("새 개선 원리에는 실행 메커니즘이 필요합니다.")?;
            let falsifier=row["falsifier"].as_str().filter(|s|
                (20..=1000).contains(&s.chars().count()))
                .ok_or("원리의 실패를 판정할 관찰 조건이 필요합니다.")?;
            let prediction=row["prediction"].as_str().filter(|s|
                (20..=1000).contains(&s.chars().count()))
                .ok_or("다음 세대에서 확인할 예측이 필요합니다.")?;
            let methods=row["methodPaths"].as_array().filter(|rows|
                !rows.is_empty() && rows.len()<=8 && rows.iter().all(|path|
                    path.as_str().is_some_and(|p|unique.contains(p))))
                .ok_or("원리의 실제 구현 소스가 변경 대상에 포함돼야 합니다.")?;
            if !methods.iter().any(|path|path.as_str().is_some_and(|p|
                p.starts_with("src/engine/") && p.ends_with(".rs"))){
                return Err("다음 후손이 실행할 Rust 개선 프로그램 소스가 필요합니다.".into());
            }
            let roles=row["targetRoles"].as_array().filter(|items|
                !items.is_empty() && items.len()<=24 && items.iter().all(|name|
                    name.as_str().is_some_and(|role|named.contains(role))))
                .ok_or("개선 원리 대상 지능 구성요소가 올바르지 않습니다.")?;
            let domains=row["transferDomains"].as_array().filter(|items|
                !items.is_empty() && items.len()<=8 && items.iter().all(|v|
                    v.as_str().is_some_and(|d|crate::engine::rsi_evolve::DOMAINS.contains(&d))))
                .ok_or("독립 실험을 전이할 분야가 올바르지 않습니다.")?;
            let parents=row["composedFrom"].as_array().filter(|items|
                items.len()<=8 && items.iter().all(|v|
                    v.as_str().is_some_and(|s|s.len()<=64)))
                .cloned().unwrap_or_default();
            improvement_principles.push(json!({
                "source":"model_authored_executable_principle_v1",
                "id":id,"mechanism":mechanism,"falsifier":falsifier,
                "prediction":prediction,"methodPaths":methods,
                "targetRoles":roles,"transferDomains":domains,
                "composedFrom":parents,"causalEffectProven":false
            }));
        }
    }
    // A source-modifying descendant may invent and pass on a NEW
    // experimental search policy, but it cannot claim improved performance
    // until the planner module source is independently measured and adopted.
    let experiment_policy=if body["experimentalPolicy"].is_object(){
        if !components.contains(&"improvement_methods")
            || !unique.contains("src/engine/rsi/evaluation/intervention_lab.rs"){
            return Err("실험 정책을 계승하려면 실제 개선기 소스를 수정해야 합니다.".into());
        }
        let policy=&body["experimentalPolicy"];
        let mode=policy["mode"].as_str().filter(|mode|matches!(*mode,
            "coverage_first"|"synergy_search"|"counterexample_repair"))
            .ok_or("후손 실험 선택 알고리즘이 올바르지 않습니다.")?;
        let explore=policy["explorationWeight"].as_f64()
            .filter(|v|v.is_finite()&&*v>=0.0&&*v<=3.0)
            .ok_or("실험 탐색 가중치가 올바르지 않습니다.")?;
        let interaction=policy["interactionWeight"].as_f64()
            .filter(|v|v.is_finite()&&*v>=0.0&&*v<=3.0)
            .ok_or("상호작용 학습 가중치가 올바르지 않습니다.")?;
        let max_pairs=policy["maxPairs"].as_u64()
            .filter(|v|(2..=40).contains(v))
            .ok_or("실험 비교 후보 수가 올바르지 않습니다.")?;
        json!({"source":"model_authored_causal_experiment_policy_v1",
            "mode":mode,"explorationWeight":explore,
            "interactionWeight":interaction,"maxPairs":max_pairs,
            "trialQualityProven":false})
    }else{Value::Null};
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,components,new_components,component_files,interaction_edges,improvement_principles,experiment_policy,next,hypothesis,design,paths,successor_stages]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,
        "components":components,"newComponents":new_components,
        "componentFiles":component_files,
        "interactionEdges":interaction_edges,
        "improvementPrinciples":improvement_principles,
        "experimentalPolicy":experiment_policy,
        "successorStages":successor_stages,
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

/// Factorial feedback is reconstructed from a stored, measured candidate
/// and the original generation-job input. A descendant cannot teach its next
/// generation a fabricated experiment phase by changing a history JSON flag.
/// This remains only experimental evidence, not proof of causal improvement.
fn verified_factorial_receipt(
    generation:&Value,candidate:&Value,job:&Value
)->(Value,bool){
    let plan=&generation["factorialExperiment"];
    let profile=crate::engine::rsi_lineage_transfer::profile(&job["input"]);
    let bound=job["kind"]=="rsi_generation"
        && job["id"]==generation["jobId"]
        && job["input"]["rsiGeneratorCommit"]==generation["generatorCommit"]
        && plan["source"]=="compiled_open_topology_counterfactual_lab_v1"
        && plan["generatorCommit"]==generation["generatorCommit"]
        && profile.as_ref().is_some_and(|hash|
            plan["comparisonProfileSha256"].as_str()==Some(hash.as_str()))
        && job["input"]["rsiFactorialExperiment"]==*plan
        && candidate["baseCommit"]==generation["generatorCommit"]
        && candidate["evaluation"]["metrics"]["measurements"].as_array()
            .is_some_and(|cases|cases.iter().any(|case|
                case["holdout"]==true
                && case["id"].as_str().is_some_and(|id|!id.is_empty())
                && case["baselinePass"].is_boolean()
                && case["candidatePass"].is_boolean()));
    if !bound{return (Value::Null,false);}
    let paths=candidate["paths"].as_array().cloned().unwrap_or_default();
    let compliance=crate::engine::rsi_intervention_lab::compliance(plan,&paths);
    let eligible=compliance["eligibleForFactorial"]==true;
    (compliance,eligible)
}

/// The planner must derive its adoption flags from the original paired
/// receipts, not from a mutable generation record or a model's claims.
pub(crate) fn revalidated_history(
    state:&crate::state::AppState,history:&[Value]
)->Vec<Value>{
    history.iter().map(|generation|{
        let mut checked=generation.clone();
        // Revalidate the frozen raw paired receipt for BOTH accepted and
        // rejected candidates. Rejected attempts carry useful prequential
        // negative labels; their mutable history flags are not evidence.
        let candidate=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id));
        let measured=candidate.as_ref().is_some_and(|candidate|{
            candidate["pairedComparisonComplete"]==true
                && candidate["id"]==generation["candidateId"]
                && candidate["baseCommit"]==generation["generatorCommit"]
                && candidate["evaluation"]["source"]
                    =="server_invoked_local_paired_git_worktrees"
                && candidate["evaluation"]["evidenceSha256"]
                    ==generation["evaluation"]["evidenceSha256"]
                && candidate["evaluation"]["metrics"]
                    ==generation["evaluation"]["metrics"]
                && candidate["evaluation"]["verdict"]
                    ==generation["evaluation"]["verdict"]
                && crate::engine::rsi::replayed_pair_metrics(state,candidate)
                    .is_some_and(|metrics|metrics==candidate["evaluation"]["metrics"])
        });
        checked["pairedMeasured"]=json!(measured);
        // Bind the frozen-case profile and source-writing actor to the
        // original job rather than trusting editable lineage annotations.
        let job=generation["jobId"].as_str()
            .and_then(|id|state.store.get("job",id))
            .filter(|job|job["kind"]=="rsi_generation"
                && job["id"]==generation["jobId"]
                && job["input"]["rsiGeneratorCommit"]==generation["generatorCommit"]);
        if let Some(original)=job.as_ref(){
            checked["comparisonProfileSha256"]=json!(
                crate::engine::rsi_lineage_transfer::profile(&original["input"]));
            checked["modelEcologyExecuted"]=original["rsiModelEcologyExecuted"].clone();
            checked["generatorProviderId"]=original["input"]["providerId"].clone();
            checked["generatorModel"]=original["input"]["model"].clone();
        }else{
            checked["comparisonProfileSha256"]=Value::Null;
            checked["modelEcologyExecuted"]=Value::Null;
            checked["generatorProviderId"]=Value::Null;
            checked["generatorModel"]=Value::Null;
        }
        if measured {
            // The actual changed paths originate in the persisted evaluated
            // candidate, not model-written generation JSON.
            if let Some(candidate)=&candidate {
                checked["changedSourcePaths"]=candidate["paths"].clone();
            }
        }
        let (compliance,factorial_measured)=match (
            measured.then_some(()).and(candidate.as_ref()),job.as_ref()
        ){
            (Some(candidate),Some(job))=>
                verified_factorial_receipt(generation,candidate,job),
            _=>(Value::Null,false),
        };
        checked["factorialCompliance"]=compliance;
        checked["factorialMeasured"]=json!(factorial_measured);
        let valid=if generation["accepted"]==true {
            measured && candidate.as_ref().is_some_and(|candidate|{
                candidate["status"]=="verified"
                    && candidate["evaluation"]["metrics"]["eligible"]==true
                    && candidate["experimentAdoption"]["commit"]==generation["acceptedCommit"]
                    && candidate["experimentAdoption"]["generatorCommit"]==generation["generatorCommit"]
                    && candidate["experimentAdoption"]["productionDeployed"]==false
            })
        }else{false};
        checked["pairedRevalidated"]=json!(valid);
        // Rewrite recorded prediction labels from the still-present original
        // paired receipt and executed plan on every history load. A stale or
        // hand-edited Brier label cannot change the next compiled planner.
        if generation["metaLearning"]["source"]
            =="source_bound_prequential_feedback_v1"{
            checked["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
                &checked["metaPolicy"],&checked);
        }
        checked
    }).collect()
}


/// A compiled descendant inherits a model-written executable task program
/// only from a paired, source-backed adopted predecessor. It rebinds the
/// intended file to its CURRENT architecture evidence, and advances a stage
/// only when a generation has really attempted the previous stage.
pub(crate) fn inherited_stage(history:&[Value],frontier:&[Value])->Value {
    // A new adopted design may supersede an older method. If it has no
    // explicit program, however, do not silently erase a still-incomplete
    // independently adopted A->B->A loop from an earlier generation.
    for (position,parent) in history.iter().enumerate().rev(){
        if !valid_adopted_system_design(parent){continue;}
        let design=&parent["systemDesign"];
        let mut stages=design["successorStages"].as_array()
            .filter(|v|!v.is_empty()).cloned()
            .unwrap_or_else(||crate::engine::rsi_open_topology::derived_cycle_stages(design));
        if stages.is_empty(){continue;}
        // A bounded, causal next-generation program can be evolved by source
        // changes in descendants; it is not required to keep the old policy.
        stages.truncate(8);
        let design_id=&design["id"];
        if !design_id.is_string(){continue;}
        let later=&history[position+1..];
        let mut cursor=0usize;
        let mut repeated=0usize;
        for executed in later.iter().filter(|g|
            g["successorStage"]["designId"]==*design_id
                && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary") {
            if executed["successorStage"]["stageIndex"].as_u64()!=Some(cursor as u64){
                continue;
            }
            repeated+=1;
            let successful=executed["accepted"]==true
                && executed["pairedRevalidated"]==true;
            let retry=executed["successorStage"]["learningPolicy"]=="retry_failed"
                && !successful;
            let max_retries=executed["successorStage"]["maxRetries"].as_u64()
                .unwrap_or(2).min(8) as usize;
            if !retry || repeated>max_retries {
                cursor+=1;
                repeated=0;
            }
        }
        let Some(stage)=stages.get(cursor) else{continue};
        let Some(role)=stage["role"].as_str() else{continue};
        let Some(domain)=stage["domain"].as_str() else{continue};
        if !crate::engine::rsi_evolve::DOMAINS.contains(&domain){continue;}
        let Some(original_path)=stage["path"].as_str()
            .filter(|p|crate::engine::rsi::safe_path(p)) else{continue};
        let candidate=frontier.iter().find(|row|
            row["role"]==role && row["domain"]==domain
                && row["path"]==original_path)
            .or_else(||frontier.iter().find(|row|
                row["role"]==role && row["domain"]==domain));
        let Some(target)=candidate else{continue};
        let Some(hash)=target["sourceSha256"].as_str().filter(|sha|
            sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
            else{continue};
        let mut bound=stage.clone();
        bound["originalPlannedPath"]=json!(original_path);
        bound["path"]=target["path"].clone();
        bound["sourceSha256"]=json!(hash);
        bound["designId"]=design_id.clone();
        bound["stageIndex"]=json!(cursor);
        bound["acceptedParentCommit"]=parent["acceptedCommit"].clone();
        bound["source"]="rebound_verified_model_successor_program_v1".into();
        bound["methodOrigin"]=stage["source"].clone();
        bound["actualRole"]=target["role"].clone();
        bound["qualityProven"]=json!(false);
        return bound;
    }
    Value::Null
}

/// Reuse the model's target only when its *source change*, not its
/// explanation, was independently evaluated and adopted.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    let previous=last_verified_system_design(history)?;
    let domain=previous["systemDesign"]["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&domain)
        .then(||domain.to_string())
}

#[cfg(test)]
#[path = "../../tests/unit/engine_rsi_system_tests.rs"]
mod tests;
