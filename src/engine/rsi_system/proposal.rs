//! Model-authored architecture proposals; a proposal never grants itself adoption.
use super::valid_role;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,fs};

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
    let new_components=crate::engine::rsi_component_birth::validate_declarations(
        body,&Value::Object(assignments.clone()),&components,paths)?;
    // Model-authored open graph: arbitrary roles, feedback branches and
    // higher-order self-rewrites. It becomes executable only if the actual
    // source revision is independently accepted by the frozen evaluator.
    let open_program=crate::engine::rsi_program_graph::normalize(&body["openProgram"])?;
    // Meta-evolution is also an adopted, source-backed program: the agent
    // can redesign experiment choice and the rules that choose experiments.
    let evolution_protocol=crate::engine::rsi_evolve::meta_evolution::normalize(
        &body["evolutionProtocol"])?;
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
        commit,component,components,assignments,next,hypothesis,design,paths,new_components,open_program,evolution_protocol,successor_stages]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,"components":components,"componentFiles":assignments,
        "nextDomain":next,"hypothesis":hypothesis,
        "design":design,"targetFiles":paths,"newComponents":new_components,
        "openProgram":open_program,"evolutionProtocol":evolution_protocol,
        "successorStages":successor_stages,
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
