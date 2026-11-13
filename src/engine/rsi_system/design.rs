//! Authored architectural proposals and validated source-bound component
//! design state. Proposal creation does not adopt source without paired A/B.
use super::*;

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
    // A newly invented subsystem is not merely a renamed design role:
    // require its OWN implementation paths and distinct executable runtime
    // integration paths within the same model-authored and edited proposal.
    let mut new_components=Vec::new();
    if let Some(proposals)=body["newComponents"].as_array(){
        if component!="system" || proposals.len()>8 {
            return Err("새 지능 구성요소는 최대 8개인 전체 시스템 설계에서만 생성합니다.".into());
        }
        let mut seen=BTreeSet::new();
        for proposal in proposals {
            let role=proposal["role"].as_str()
                .filter(|r|crate::engine::rsi_open_topology::valid_role(r)
                    && assignments.contains_key(*r)
                    && !COMPONENTS.iter().any(|(original,_)|*original==*r))
                .ok_or("새 지능 구성요소가 유효한 신규 역할이 아닙니다.")?;
            if !seen.insert(role){return Err("중복 신규 지능 역할입니다.".into());}
            let domain=proposal["domain"].as_str()
                .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
                .ok_or("신규 지능의 발전 영역이 유효하지 않습니다.")?;
            let purpose=proposal["purpose"].as_str()
                .filter(|p|(30..=1200).contains(&p.len()))
                .ok_or("새 지능이 해결할 능력과 실행 목적을 설명해야 합니다.")?;
            let sources=proposal["sourcePaths"].as_array()
                .filter(|xs|!xs.is_empty()&&xs.len()<=4)
                .ok_or("신규 지능은 실제 구현 소스 경로가 필요합니다.")?;
            let wires=proposal["integrationPaths"].as_array()
                .filter(|xs|!xs.is_empty()&&xs.len()<=4)
                .ok_or("신규 지능을 실행기에 연결할 별도 경로가 필요합니다.")?;
            if !sources.iter().all(|p|p.as_str().is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && assignments[role].as_array().is_some_and(|xs|
                    xs.iter().any(|x|x==path)))) {
                return Err("새 지능 소스가 해당 구성요소에 배정되지 않았습니다.".into());
            }
            if !wires.iter().all(|p|p.as_str().is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && paths.iter().any(|x|x==path)
                && !sources.iter().any(|s|s==path))) {
                return Err("신규 지능의 실행 연결 파일이 소스 설계에 포함되지 않았습니다.".into());
            }
            let improves=proposal["improvesRole"].as_str()
                .filter(|r|*r!=role && valid_role(r)
                    && (assignments.contains_key(*r)
                        || COMPONENTS.iter().any(|(name,_)|*name==*r)))
                .unwrap_or("improvement_methods");
            new_components.push(json!({"role":role,"domain":domain,
                "purpose":purpose,"sourcePaths":sources,
                "integrationPaths":wires,"improvesRole":improves,
                "qualityProven":false}));
        }
    }
    // Model-authored hypotheses are not causal evidence until an adopted
    // source change and a separately observed compiled descendant support them.
    let mut interaction_edges=Vec::new();
    if let Some(edges)=body["interactionEdges"].as_array() {
        if edges.len()>12 || (component!="system" && !edges.is_empty()){
            return Err("상호개선 가설은 다중 영역 설계에서 최대 12개입니다.".into());
        }
        let mut seen=BTreeSet::new();
        for edge in edges {
            let from=edge["fromRole"].as_str()
                .filter(|role|assignments.contains_key(*role))
                .ok_or("상호개선 시작 역할이 선언된 설계에 없습니다.")?;
            let to=edge["toRole"].as_str()
                .filter(|role|*role!=from && assignments.contains_key(*role))
                .ok_or("상호개선 대상 역할이 설계에 없거나 동일합니다.")?;
            let from_path=edge["fromPath"].as_str()
                .filter(|path|assignments[from].as_array()
                    .is_some_and(|files|files.iter().any(|file|file.as_str()==Some(*path))))
                .ok_or("상호개선 출발 소스가 해당 역할에 배정되지 않았습니다.")?;
            let to_path=edge["toPath"].as_str()
                .filter(|path|assignments[to].as_array()
                    .is_some_and(|files|files.iter().any(|file|file.as_str()==Some(*path))))
                .ok_or("상호개선 도착 소스가 해당 역할에 배정되지 않았습니다.")?;
            if !seen.insert((from.to_string(),to.to_string())){
                return Err("중복된 방향성 개선 가설입니다.".into());
            }
            let mechanism=edge["mechanism"].as_str()
                .filter(|text|(20..=1200).contains(&text.chars().count()))
                .ok_or("영역 간 개선이 작동하는 원리 20~1200자가 필요합니다.")?;
            let observable=edge["observable"].as_str()
                .filter(|text|(12..=800).contains(&text.chars().count()))
                .ok_or("후손이 바뀌었는지 측정할 관찰 지표가 필요합니다.")?;
            interaction_edges.push(json!({
                "fromRole":from,"toRole":to,"fromPath":from_path,"toPath":to_path,
                "mechanism":mechanism,"observable":observable,
                "qualityProven":false,"status":"unverified_causal_hypothesis"
            }));
        }
    }
    // Unlike a fixed stage list, improvementRules are authored by the model
    // as source-implemented transformation operators over arbitrary
    // intelligence roles. They can later compose into any-length role cycles.
    // A declared operator remains a hypothesis until a paired-adopted source
    // revision proves its method path was really changed.
    let mut improvement_rules=Vec::new();
    if let Some(rules)=body["improvementRules"].as_array(){
        if rules.len()>12 || component!="system" {
            return Err("개선 규칙은 다중 지능 재설계에서 최대 12개를 제안합니다.".into());
        }
        let mut seen=BTreeSet::new();
        for rule in rules {
            let name=rule["name"].as_str()
                .filter(|s|!s.is_empty()&&s.len()<=64
                    && s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'))
                .ok_or("개선 규칙 이름이 유효하지 않습니다.")?;
            if !seen.insert(name) {return Err("중복 개선 규칙 이름".into());}
            let method_role=rule["methodRole"].as_str()
                .filter(|role|assignments.contains_key(*role))
                .ok_or("규칙의 실제 실행 방법 역할이 설계에서 누락됐습니다.")?;
            let method_path=rule["methodPath"].as_str()
                .filter(|p|crate::engine::rsi::safe_path(p)
                    && assignments[method_role].as_array().is_some_and(|rows|
                        rows.iter().any(|row|row==p))
                    && matches!(Path::new(p).extension().and_then(|x|x.to_str()),
                        Some("rs"|"ts"|"tsx"|"js"|"jsx"|"py")))
                .ok_or("개선 규칙을 구현하는 실제 소스 파일이 필요합니다.")?;
            let parse_roles=|key:&str|->Result<Vec<Value>,String>{
                let rows=rule[key].as_array()
                    .filter(|v|!v.is_empty()&&v.len()<=8)
                    .ok_or("개선 규칙의 역할 목록이 비어 있거나 너무 깁니다.")?;
                let mut unique=BTreeSet::new();
                for row in rows{
                    let role=row.as_str()
                        .filter(|r|crate::engine::rsi_open_topology::valid_role(r))
                        .ok_or("개선 규칙 역할 이름 오류")?;
                    if !unique.insert(role){
                        return Err("개선 규칙의 동일 역할 중복".into());
                    }
                }
                Ok(rows.clone())
            };
            let premises=parse_roles("premises")?;
            let products=parse_roles("products")?;
            let hypothesis=rule["hypothesis"].as_str()
                .filter(|s|(25..=1600).contains(&s.len()))
                .ok_or("개선 규칙에는 관찰 가능한 메커니즘 가설이 필요합니다.")?;
            let observable=rule["observable"].as_str()
                .filter(|s|(12..=800).contains(&s.len()))
                .ok_or("개선 규칙에는 독립 평가할 관측량이 필요합니다.")?;
            improvement_rules.push(json!({
                "name":name,"methodRole":method_role,"methodPath":method_path,
                "premises":premises,"products":products,
                "hypothesis":hypothesis,"observable":observable,
                "qualityProven":false
            }));
        }
    }
    // Stages are an evolvable REBUILD-BINARY programme. They are not
    // executable merely because a model declared them: adoption, current
    // source SHA and the descendant's actual target must all match.
    let mut successor_stages=Vec::new();
    if let Some(stages)=body["successorStages"].as_array(){
        if stages.len()>8{return Err("후손 개선 프로그램은 최대 8단계입니다.".into());}
        for stage in stages {
            let role=stage["role"].as_str().filter(|name|valid_role(name)&&*name!="system")
                .ok_or("후손 개선 단계의 구성요소 이름 오류")?;
            let domain=stage["domain"].as_str()
                .filter(|domain|crate::engine::rsi_evolve::DOMAINS.contains(domain))
                .ok_or("후손 개선 단계의 분야 오류")?;
            let path=stage["path"].as_str()
                .filter(|path|crate::engine::rsi::safe_path(path)
                    && matches!(std::path::Path::new(path).extension().and_then(|x|x.to_str()),
                        Some("rs"|"ts"|"tsx"|"js"|"jsx"|"py")))
                .ok_or("후손 개선 단계가 지원하는 실제 소스 경로가 아닙니다.")?;
            let objective=stage["objective"].as_str()
                .filter(|text|(25..=1200).contains(&text.chars().count()))
                .ok_or("후손 단계에는 실제 구현 목표 25~1200자가 필요합니다.")?;
            let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
            let execution=stage["executionPolicy"].as_str().unwrap_or("explore_then_edit");
            if !matches!(learning,"avoid_failed"|"retry_failed")
                || !matches!(execution,"grounded_edit"|"explore_then_edit") {
                return Err("후손 개선 단계의 학습·편집 정책 오류".into());
            }
            // New modules are allowed when this generation created them;
            // existing paths remain bound to the future checkout digest.
            let proposed=root.join(path);
            if proposed.exists(){
                let canonical=fs::canonicalize(&proposed).map_err(|e|e.to_string())?;
                if !canonical.starts_with(&root)||!canonical.is_file(){
                    return Err("후손 단계가 실제 작업공간 외부를 가리킵니다.".into());
                }
            }else if !paths.iter().any(|file|file.as_str()==Some(path)){
                return Err("새 후손 단계 모듈은 이번 세대의 수정 대상에 포함돼야 합니다.".into());
            }
            successor_stages.push(json!({
                "role":role,"domain":domain,"path":path,"objective":objective,
                "learningPolicy":learning,"executionPolicy":execution,
                "qualityProven":false
            }));
        }
    }
    let action_program=if body["actionProgram"].is_null() {Value::Null}
        else{
            compile_action_program(&body["actionProgram"])
                .ok_or("개선 프로그램은 실제 소스 읽기 후 편집하는 허용된 도구 2~8개여야 합니다.")?;
            body["actionProgram"].clone()
        };
    let method_hypothesis=body["methodHypothesis"].as_str().unwrap_or("");
    if method_hypothesis.len()>1200 {
        return Err("개선 방법의 인과 가설은 1200자 이하여야 합니다.".into());
    }
    let commit=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 원본 revision 없음")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        commit,component,components,assignments,next,hypothesis,design,paths,
        action_program,method_hypothesis,interaction_edges,successor_stages,
        new_components,improvement_rules
    ]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),"status":"proposed",
        "source":"model_authored_system_design",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":commit,"component":component,"components":components,"componentFiles":assignments,
        "nextDomain":next,"hypothesis":hypothesis,
        "design":design,"targetFiles":paths,
        "actionProgram":action_program,"methodHypothesis":method_hypothesis,
        "interactionEdges":interaction_edges,"successorStages":successor_stages,
        "newComponents":new_components,
        "improvementRules":improvement_rules,
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

