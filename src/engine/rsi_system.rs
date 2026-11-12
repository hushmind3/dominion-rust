//! Source-grounded self-model produced by the actual compiled RSI descendant.
//! This is a mutable planning component; it does not claim model intelligence
//! or any measured gain. Only independent paired evaluation can adopt edits.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,fs};

const COMPONENTS: &[(&str,&[&str])] = &[
    ("algorithms",&["src/engine/algorithms.rs","src/engine/rsi_evolve.rs","src/engine/reasoning.rs"]),
    ("harness",&["src/engine/jobs.rs","src/engine/rsi_worktree.rs","src/engine/chat/runtime.rs","src/engine/decompose.rs"]),
    ("memory",&["src/engine/memory.rs","src/engine/room_state.rs","src/engine/room_knowledge.rs"]),
    ("learning",&["src/engine/learning.rs","src/engine/experience.rs","src/engine/procedure.rs"]),
    ("tools",&["src/engine/tools.rs","src/engine/browser.rs","src/engine/mcp.rs"]),
    ("improvement_methods",&["src/engine/rsi_evolve.rs","src/engine/rsi_mutation.rs","src/engine/rsi_system.rs"]),
    ("frontend",&["web/src","web/src/App.tsx","web/src/main.tsx"])
];

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
    json!({
        "source":"compiled_descendant_system_blueprint_v1",
        "generatorCommit":revision,
        "architecture":components,
        "historyCount":history.len(),
        "inheritedDesign":history.last().filter(|g|g["accepted"]==true && g["pairedRevalidated"]==true && g["evaluation"]["metrics"]["eligible"]==true).map(|g|g["systemDesign"].clone()).unwrap_or(Value::Null),
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
        cross-module Rust changes with workspace_replace_range/workspace_replace/workspace_write. \
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
        let candidate=root.join(path);
        if fs::symlink_metadata(&candidate).is_ok() {
            // An existing file is allowed only if it resolves inside this
            // exact rebuilt descendant's checkout. Never follow outside links.
            let file=fs::canonicalize(&candidate)
                .map_err(|_|"설계 대상 소스를 확인할 수 없습니다.".to_string())?;
            if !file.starts_with(&root)||!file.is_file() {
                return Err("외부 파일은 설계 범위가 아닙니다.".into());
            }
        } else {
            // Genuine architectural redesign may CREATE a new Rust module.
            // Pin the closest existing parent to the worktree, including
            // when the new module is in a new subdirectory.
            let mut ancestor=candidate.parent()
                .ok_or("새 소스의 상위 디렉터리가 없습니다.")?;
            while !ancestor.exists() {
                ancestor=ancestor.parent()
                    .ok_or("새 소스의 작업공간 상위 경로가 없습니다.")?;
            }
            let parent=fs::canonicalize(ancestor).map_err(|e|e.to_string())?;
            if !parent.starts_with(&root)||!parent.is_dir() {
                return Err("새 소스를 외부 디렉터리에 만들 수 없습니다.".into());
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
    if !paths.iter().filter_map(Value::as_str).any(|path|
        previous["acceptedSourceHashes"][path].as_str().is_some_and(|hash|hash.len()==64)){
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
            && design["component"].as_str()
                .is_some_and(|role|COMPONENTS.iter().any(|r|r.0==role))
            && target.is_some_and(|files|files.iter().filter_map(Value::as_str)
                .any(|path|edits.is_some_and(|all|all.get(path)
                    .and_then(Value::as_str).is_some_and(|hash|hash.len()==64))));
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
        let roles=chain.iter().filter_map(|g|
            g["systemDesign"]["component"].as_str())
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
