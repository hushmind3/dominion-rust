//! Executable proposal lifecycle, failure response, and inherited domains.
use super::*;

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
                    Some("rs"|"ts"|"tsx"|"js"|"jsx"|"py")) {
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
    let introductions=crate::engine::rsi_open_topology::validate_proposal(
        body,&root,&roles,paths,&assignments)?;
    // Explicit source-bound A->B interactions are hypotheses until two actual
    // edits and an independent source-adoption receipt exist. Disjoint
    // componentFiles prevent claiming the same changed file for both roles.
    let mut edges=Vec::<Value>::new();
    if let Some(rows)=body["interactionEdges"].as_array(){
        if component!="system" || rows.len()>16 {
            return Err("상호 개선 계획은 2개 이상 역할의 결합 설계여야 합니다.".into());
        }
        let mut unique_edges=std::collections::BTreeSet::new();
        for edge in rows {
            let from=edge["fromRole"].as_str()
                .filter(|r|roles.iter().any(|x|x.as_str()==Some(*r)))
                .ok_or("상호개선 출발 역할이 구성요소에 없습니다.")?;
            let to=edge["toRole"].as_str()
                .filter(|r|roles.iter().any(|x|x.as_str()==Some(*r)))
                .ok_or("상호개선 도착 역할이 구성요소에 없습니다.")?;
            let from_path=edge["fromPath"].as_str()
                .filter(|p|crate::engine::rsi::safe_path(p)
                    && assignments[from].as_array().is_some_and(|items|
                        items.iter().any(|item|item.as_str()==Some(*p))))
                .ok_or("출발 구성요소의 실제 수정 파일이 필요합니다.")?;
            let to_path=edge["toPath"].as_str()
                .filter(|p|crate::engine::rsi::safe_path(p)
                    && assignments[to].as_array().is_some_and(|items|
                        items.iter().any(|item|item.as_str()==Some(*p))))
                .ok_or("도착 구성요소의 실제 수정 파일이 필요합니다.")?;
            let mechanism=edge["mechanism"].as_str()
                .filter(|v|(25..=1600).contains(&v.chars().count()))
                .ok_or("상호개선 관계의 구체적 메커니즘이 필요합니다.")?;
            let expected=edge["expectedEffect"].as_str()
                .filter(|v|(15..=1200).contains(&v.chars().count()))
                .ok_or("상호개선의 관찰 가능한 예상 효과가 필요합니다.")?;
            if from==to || from_path==to_path
                || !unique_edges.insert((from.to_string(),to.to_string())) {
                return Err("상호개선 관계는 서로 다른 두 구성요소와 소스여야 합니다.".into());
            }
            edges.push(json!({"fromRole":from,"toRole":to,
                "fromPath":from_path,"toPath":to_path,
                "mechanism":mechanism,"expectedEffect":expected,
                "qualityProven":false}));
        }
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
    // Immutable model proposal forecast precedes any edit or evaluation.
    let mut expectations=serde_json::Map::new();
    if let Some(predicted)=body["expectedOutcomes"].as_object() {
        if predicted.is_empty() || predicted.len()>4 {
            return Err("예측 결과는 1~4개 지표가 필요합니다.".into());
        }
        for (name,p) in predicted {
            if !matches!(name.as_str(),"candidateCaptured"|"adoption"|"metaImprovement"|"regression")
                || p.as_f64().is_none_or(|value|!value.is_finite()||!(0.0..=1.0).contains(&value)) {
                return Err("결과 예측은 허용된 지표와 0~1 확률이어야 합니다.".into());
            }
            expectations.insert(name.clone(),p.clone());
        }
    }
    // A principle is experimental DATA only when the model binds it to
    // distinct task-intelligence and self-improvement source revisions.
    let improvement_principle=crate::engine::rsi_principle_lab::validate_authored(
        &body["improvementPrinciple"],&assignments,paths)?;
    // A descendant may invent or replace its ENTIRE improvement program.
    // The graph is a model-authored hypothesis until actual source changes
    // pass the frozen independent evaluator.
    let open_program=crate::engine::rsi_open_program::normalize(
        &body["openProgram"])?;
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,roles,assignments,next,hypothesis,design,paths,
        edges,introductions,stages,expectations,improvement_principle,open_program
    ]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,
        "components":roles,"componentFiles":assignments,
        "interactionEdges":edges,
        "moduleIntroductions":introductions,
        "successorStages":stages,
        "expectedOutcomes":expectations,
        "improvementPrinciple":improvement_principle,
        "openProgram":open_program,
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

