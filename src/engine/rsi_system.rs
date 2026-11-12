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
        components.push(json!({"role":role,"representativeSources":inventory}));
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
        "inheritedDesign":history.last().filter(|g|g["accepted"]==true && g["evaluation"]["metrics"]["eligible"]==true).map(|g|g["systemDesign"].clone()).unwrap_or(Value::Null),
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
        the actual code, including new modules when justified. A changed \
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

/// Reuse the model's target only when its *source change*, not its
/// explanation, was independently evaluated and adopted.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    let previous=history.last()?;
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
    if !paths.iter().filter_map(Value::as_str).any(|path|
        previous["acceptedSourceHashes"][path].as_str().is_some_and(|hash|hash.len()==64)){
        return None;
    }
    let domain=design["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&domain)
        .then(||domain.to_string())
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
