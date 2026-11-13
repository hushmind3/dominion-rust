//! Compiled, source-grounded RSI self-model: design responsibility.
use super::*;

/// frozen evaluator or steer successors unless source changes are adopted.
/// A descendant-authored meta-design is persisted as an experiment contract
/// with exact source observations. The agent, not its supervisor, invents
/// components, techniques, and executable exploration programs.
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
        || child["cancelRequested"]==true {
        return Err("진행 중인 자기재설계 후손만 설계할 수 있습니다.".into());
    }
    let component=body["component"].as_str()
        .filter(|name|valid_role(name))
        .ok_or("시스템 구성요소 이름 오류")?;
    let mut roles=body["components"].as_array()
        .map(|items|items.iter().filter_map(Value::as_str)
            .map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_else(||vec![component.to_string()]);
    if component=="system" && roles.len()<2 {
        return Err("전체 시스템 결합 설계는 2개 이상의 지능 구성요소를 지정해야 합니다.".into());
    }
    if roles.is_empty() || roles.len()>12 || roles.iter().any(|role|!valid_role(role)){
        return Err("지능 구성요소 목록 형식 오류".into());
    }
    let mut unique_roles=BTreeSet::new();
    if roles.iter().any(|role|!unique_roles.insert(role.clone())){
        return Err("중복된 설계 구성요소".into());
    }
    if component!="system" && roles.len()>1 && !roles.iter().any(|r|r==component){
        return Err("복합 설계에서 주 구성요소가 누락됐습니다.".into());
    }
    if component!="system" && roles.len()==1 && roles[0]!=component {
        return Err("주 구성요소와 설계 범위가 일치하지 않습니다.".into());
    }
    let domain=body["nextDomain"].as_str()
        .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
        .ok_or("후손의 다음 탐색 분야 오류")?;
    let hypothesis=body["hypothesis"].as_str()
        .filter(|v|v.trim().len()>=20 && v.len()<=2000)
        .ok_or("인과적인 개선 가설 20~2000자를 입력하세요.")?;
    let design=body["design"].as_str()
        .filter(|v|v.trim().len()>=30 && v.len()<=6000)
        .ok_or("실제 구현할 구조 또는 학습 방법 30~6000자가 필요합니다.")?;
    let targets=body["targetFiles"].as_array()
        .filter(|files|!files.is_empty()&&files.len()<=12)
        .ok_or("변경할 소스 파일은 1~12개여야 합니다.")?;
    let root=fs::canonicalize(&state.config.workspace).map_err(|e|e.to_string())?;
    let mut target_set=BTreeSet::new();
    let mut observed=serde_json::Map::new();
    for file in targets {
        let path=file.as_str().ok_or("소스 경로는 문자열이어야 합니다.")?;
        if !intelligence_editable_path(path)
            || !target_set.insert(path.to_string())
            || !(path.starts_with("src/")||path.starts_with("web/src/")
                || path.starts_with("src-tauri/src/")) {
            return Err("허용되지 않은 구성요소 파일 또는 중복 경로".into());
        }
        let target=root.join(path);
        if target.exists() {
            let real=fs::canonicalize(&target).map_err(|e|e.to_string())?;
            if !real.starts_with(&root)||!real.is_file(){
                return Err("작업공간 외부 소스를 변경할 수 없습니다.".into());
            }
            let bytes=fs::read(real).map_err(|e|e.to_string())?;
            observed.insert(path.to_string(),json!({
                "sha256":format!("{:x}",Sha256::digest(&bytes)),"bytes":bytes.len()
            }));
        } else {
            // New code modules are essential for real architectural novelty.
            // Only an existing canonical parent under the detached checkout
            // can authorize a new source. A proposal does NOT create it.
            let parent=target.parent().ok_or("새 소스의 상위 경로 없음")?;
            let canonical=fs::canonicalize(parent)
                .map_err(|_|"새 소스는 기존 소스 디렉터리 아래에 만드세요.".to_string())?;
            if !canonical.starts_with(&root)||!canonical.is_dir(){
                return Err("새 소스의 상위 폴더가 실험 루트 밖입니다.".into());
            }
            observed.insert(path.to_string(),json!({"plannedNewSource":true}));
        }
    }
    let mut component_files=body["componentFiles"].clone();
    if roles.len()==1 && !component_files.is_object(){
        let mut map=serde_json::Map::new();
        map.insert(roles[0].clone(),json!(targets));
        component_files=Value::Object(map);
    }
    for role in &roles {
        let files=component_files[role.as_str()].as_array()
            .filter(|rows|!rows.is_empty())
            .ok_or("각 구성요소에 실제 변경 대상 파일을 배정하세요.")?;
        if files.iter().any(|file|file.as_str()
            .is_none_or(|path|!target_set.contains(path))) {
            return Err("구성요소 파일이 전체 수정 대상에 포함되지 않습니다.".into());
        }
    }
    if let Some(program)=body["actionProgram"].as_array(){
        if program.len()<2 || program.len()>8
            || !program.iter().all(|step|step.as_str().is_some_and(|tool|
                matches!(tool,"code_search"|"workspace_read_range"|"workspace_replace"
                    |"workspace_replace_range"|"workspace_replace_function"
                    |"workspace_write"|"rsi_system_plan"|"rsi_method_propose"))) {
            return Err("새 개선 프로그램에는 허용된 도구 2~8개가 필요합니다.".into());
        }
        let read=program.iter().position(|step|step=="workspace_read_range");
        let write=program.iter().position(|step|matches!(step.as_str(),Some(
            "workspace_replace"|"workspace_replace_range"
            |"workspace_replace_function"|"workspace_write")));
        if read.zip(write).is_none_or(|(r,w)|r>=w) {
            return Err("개선 프로그램은 실제 소스 관찰 후 소스 편집을 수행해야 합니다.".into());
        }
    }
    // An invented intelligence role is admissible only with a changed
    // implementation AND a separately changed runtime integration point.
    let births=body["newComponents"].as_array().cloned().unwrap_or_default();
    if births.len()>8 {return Err("새 지능 구성요소는 한 세대에 최대 8개입니다.".into());}
    let mut birth_names=BTreeSet::new();
    for item in &births {
        let role=item["role"].as_str().filter(|r|valid_role(r) && *r!="system")
            .ok_or("새 지능 구성요소 이름 오류")?;
        if !birth_names.insert(role.to_string()) || !roles.iter().any(|r|r==role){
            return Err("새 구성요소는 유일하며 실제 설계 역할에 포함돼야 합니다.".into());
        }
        if !item["domain"].as_str().is_some_and(|d|
            crate::engine::rsi_evolve::DOMAINS.contains(&d)){
            return Err("새 지능 구성요소의 기능 도메인 오류".into());
        }
        let paths=item["sourcePaths"].as_array()
            .filter(|p|!p.is_empty()&&p.len()<=4)
            .ok_or("새 구성요소의 구현 소스가 필요합니다.")?;
        let wired=item["integrationPaths"].as_array()
            .filter(|p|!p.is_empty()&&p.len()<=4)
            .ok_or("새 구성요소의 실제 런타임 연결 소스가 필요합니다.")?;
        let mapped=component_files[role].as_array()
            .ok_or("새 구성요소의 소스 역할 매핑이 없습니다.")?;
        if paths.iter().any(|f|f.as_str().is_none_or(|path|
                !target_set.contains(path)||!mapped.iter().any(|m|m==path)))
            || wired.iter().any(|w|w.as_str().is_none_or(|path|
                !target_set.contains(path)||paths.iter().any(|p|p==path))){
            return Err("새 구성요소와 런타임 연결은 변경 파일로 명시돼야 합니다.".into());
        }
        if paths.iter().any(|p|p.as_str().is_some_and(|name|
                name.starts_with("src/engine/")&&name.ends_with(".rs")))
            && (!wired.iter().any(|p|p=="src/engine/mod.rs")
                || !wired.iter().any(|p|p!="src/engine/mod.rs")) {
            return Err("새 Rust 지능은 모듈 등록과 별도의 실제 실행 호출 소스를 모두 수정해야 합니다.".into());
        }
        if !item["purpose"].as_str().is_some_and(|p|
            p.trim().len()>=15 && p.len()<=1000){
            return Err("새 지능 구성요소의 실질적인 기능 설명이 필요합니다.".into());
        }
        if let Some(improved)=item["improvesRole"].as_str(){
            if improved==role||!roles.iter().any(|known|known==improved){
                return Err("새 지능은 실제로 개선할 다른 구성요소를 지정해야 합니다.".into());
            }
        }
    }
    let edges=body["interactionEdges"].as_array().cloned().unwrap_or_default();
    if edges.len()>12{return Err("상호개선 관계는 최대 12개입니다.".into());}
    for edge in &edges{
        let from=edge["fromRole"].as_str().filter(|role|
            roles.iter().any(|r|r==role)).ok_or("상호개선 출발 역할 오류")?;
        let to=edge["toRole"].as_str().filter(|role|
            roles.iter().any(|r|r==role) && *role!=from)
            .ok_or("상호개선 대상 역할 오류")?;
        for (key,role) in [("fromPath",from),("toPath",to)]{
            let path=edge[key].as_str().filter(|p|target_set.contains(*p))
                .ok_or("상호개선은 실제 변경 소스를 연결해야 합니다.")?;
            if !component_files[role].as_array().is_some_and(|a|
                a.iter().any(|p|p==path)){
                return Err("상호개선 소스의 구성요소 소유 관계 오류".into());
            }
        }
        if !edge["mechanism"].as_str().is_some_and(|s|
            s.len()>=20&&s.len()<=1200)
            || !edge["observable"].as_str().is_some_and(|s|
                s.len()>=10&&s.len()<=1000){
            return Err("상호개선 관계의 메커니즘과 관찰 가능한 결과가 필요합니다.".into());
        }
    }
    // The running descendant's self-research curriculum is a source-
    // changing contract, not a narrative. Its claimed stage must be present
    // in the actual edited files before the method can survive adoption.
    let laboratory=&child["input"]["rsiRecursiveLaboratory"];
    if laboratory["source"]=="descendant_editable_recursive_laboratory_v1"{
        let step=laboratory["phase"].as_str().unwrap_or("");
        let required=laboratory["target"]["path"].as_str()
            .filter(|path|intelligence_editable_path(path));
        if matches!(step,"develop_invented_intelligence"|
            "return_to_origin"|"rewrite_scientific_method"|
            "rewrite_stalled_scientific_method"|"repair_measured_negative_experiment")
            && required.is_none_or(|path|!target_set.contains(path)){
            return Err("후손 연구 단계가 요구한 실제 소스 수정이 설계에 없습니다.".into());
        }
        if step=="invent_new_intelligence" {
            let expected=laboratory["invention"]["role"].as_str()
                .ok_or("새로 발명할 지능 역할이 지정되지 않았습니다.")?;
            if !births.iter().any(|item|item["role"]==expected){
                return Err("후손이 발명한 지능 역할을 실행 가능한 코드 설계에 포함하세요.".into());
            }
        }
        if matches!(step,"rewrite_scientific_method"|
            "rewrite_stalled_scientific_method"){
            // The inherited action program is used by the *next* binary's
            // actual native tool runner. No source-only description qualifies.
            if !body["actionProgram"].as_array().is_some_and(|seq|seq.len()>=2)
                || !body["methodHypothesis"].as_str().is_some_and(|note|
                    note.trim().len()>=20){
                return Err("실험 알고리즘 재작성에는 계승 가능한 실제 편집 절차와 방법 가설이 필요합니다.".into());
            }
        }
    }
    // General improvement principles are invented by the MODEL and backed
    // by editable source, not a closed list of operator types or role pairs.
    // Descendants learn their value only from later source-changing children.
    let principles=body["improvementPrinciples"].as_array()
        .cloned().unwrap_or_default();
    if principles.len()>12{
        return Err("한 세대의 새 개선 원리는 최대 12개입니다.".into());
    }
    let mut principle_names=BTreeSet::new();
    for principle in &principles{
        let name=principle["name"].as_str().filter(|name|
            !name.is_empty()&&name.len()<=80
            && name.bytes().all(|b|b.is_ascii_alphanumeric()
                ||b==b'_'||b==b'-'))
            .ok_or("개선 원리 이름 형식 오류")?;
        if !principle_names.insert(name.to_string()){
            return Err("새 개선 원리의 이름이 중복됐습니다.".into());
        }
        let method_role=principle["methodRole"].as_str()
            .filter(|role|roles.iter().any(|entry|entry==role))
            .ok_or("개선 원리를 실제 수정하는 구성요소가 누락됐습니다.")?;
        let paths=principle["methodSourcePaths"].as_array()
            .filter(|paths|!paths.is_empty()&&paths.len()<=8)
            .ok_or("개선 원리의 실제 구현 소스가 필요합니다.")?;
        if paths.iter().any(|file|file.as_str().is_none_or(|path|
            !target_set.contains(path)
                || !component_files[method_role].as_array()
                    .is_some_and(|rows|rows.iter().any(|p|p==path)))){
            return Err("개선 원리는 이번 후보가 실제로 수정하는 소스를 가리켜야 합니다.".into());
        }
        let targets=principle["improvesRoles"].as_array()
            .filter(|targets|!targets.is_empty()&&targets.len()<=12)
            .ok_or("새 개선 원리가 발전시킬 지능 역할이 필요합니다.")?;
        if targets.iter().any(|target|target.as_str()
            .is_none_or(|role|!valid_role(role)||role=="system")){
            return Err("개선 원리의 대상 지능 이름이 유효하지 않습니다.".into());
        }
        if !principle["mechanism"].as_str().is_some_and(|v|
            v.trim().len()>=25&&v.len()<=1600)
            || !principle["falsifiableOutcome"].as_str().is_some_and(|v|
                v.trim().len()>=20&&v.len()<=1000){
            return Err("새 개선 원리에는 실행 원리와 반증 가능한 다음 세대 결과가 필요합니다.".into());
        }
        let parents=principle["composedFrom"].as_array().cloned()
            .unwrap_or_default();
        let mut seen_parents=BTreeSet::<String>::new();
        if parents.len()>6 || parents.iter().any(|item|item.as_str()
            .is_none_or(|name|name.is_empty()||name.len()>80))
            || parents.iter().filter_map(Value::as_str)
                .any(|parent|!seen_parents.insert(parent.to_string())||parent==name){
            return Err("새 원리는 중복되지 않은 실제 선행 원리만 합성해야 합니다.".into());
        }
        if !parents.is_empty(){
            let history=state.store.all("rsi_generation");
            let observed=history.iter().filter(|past|
                accepted_model_design(past).is_some()
                    && past["changedSourcePaths"].as_array().is_some())
                .flat_map(|past|past["systemDesign"]["improvementPrinciples"]
                    .as_array().cloned().unwrap_or_default())
                .filter_map(|row|row["name"].as_str().map(str::to_string))
                .collect::<BTreeSet<_>>();
            if parents.iter().filter_map(Value::as_str).any(|parent|
                !observed.contains(parent)){
                return Err("합성하려는 선행 원리는 독립 채택된 실제 소스 계보에 있어야 합니다.".into());
            }
        }
    }
    // The compiled child made an EVIDENCE-LEARNED capability selection.
    // A proposal that ignores its actual target, or merely claims a new
    // improvement principle without rewriting its method, cannot inherit.
    let principle_execution=&child["input"]["rsiSystemAgenda"]["principleCurriculum"];
    if principle_execution["source"]==
        "compiled_adaptive_improvement_principle_curriculum_v1"{
        let target_path=principle_execution["target"]["path"].as_str()
            .ok_or("학습된 지능 개선 대상 소스가 없습니다.")?;
        let target_role=principle_execution["target"]["role"].as_str()
            .ok_or("학습된 지능 구성요소가 없습니다.")?;
        if !target_set.contains(target_path)
            || !component_files[target_role].as_array().is_some_and(|rows|
                rows.iter().any(|p|p==target_path)){
            return Err("학습한 지능 개선 대상이 실제 수정 소스 설계에 반영되지 않았습니다.".into());
        }
        if let (Some(method_path),Some(method_role))=(
            principle_execution["methodSource"]["path"].as_str(),
            principle_execution["methodSource"]["role"].as_str()){
            if method_path!=target_path
                && (!target_set.contains(method_path)
                    || !component_files[method_role].as_array()
                        .is_some_and(|rows|rows.iter().any(|p|p==method_path))
                    || !principles.iter().any(|p|
                        p["methodRole"]==method_role
                            && p["methodSourcePaths"].as_array()
                                .is_some_and(|rows|rows.iter()
                                    .any(|path|path==method_path)))){
                return Err("새 개선 원리를 실행할 소스 수정과 원리 선언이 일치하지 않습니다.".into());
            }
        }
        if principle_execution["phase"]=="synthesize_new_improvement_principle" {
            let required=principle_execution["compositionHypothesis"]["composedFrom"]
                .as_array().ok_or("합성할 선행 개선 원리가 없습니다.")?;
            if required.len()<2 || !principles.iter().any(|p|
                p["composedFrom"].as_array().is_some_and(|rows|
                    required.iter().all(|name|rows.contains(name)))){
                return Err("실행할 합성 원리가 후손의 실제 원리 설계에 반영되지 않았습니다.".into());
            }
        }
    }
    // The child's actual WHOLE-DOMINION growth target is an executable
    // source commitment. It is never satisfied by changing a planner,
    // declaring a capability, selecting a provider, or training an unrelated
    // local adapter. The target intelligence implementation must be changed.
    let whole=&child["input"]["rsiSystemAgenda"]["wholeDominionGrowth"];
    if whole["source"]=="descendant_whole_dominion_growth_v1"{
        let path=whole["target"]["path"].as_str()
            .ok_or("차세대 도미니언의 지능 개선 소스가 없습니다.")?;
        let role=whole["target"]["role"].as_str()
            .ok_or("차세대 도미니언의 실제 지능 역할이 없습니다.")?;
        if !target_set.contains(path)
            || !component_files[role].as_array().is_some_and(|files|
                files.iter().any(|file|file==path)) {
            return Err("도미니언 자신의 미해결 지능을 개선하려면 선택한 실제 구현 소스를 수정해야 합니다.".into());
        }
        if whole["phase"]=="invent_better_improvement_method_for_failed_intelligence" {
            let method_path=whole["improvementMethodSource"]["path"]
                .as_str().ok_or("실패한 지능을 위한 새로운 개선기 소스가 필요합니다.")?;
            let method_role=whole["improvementMethodSource"]["role"]
                .as_str().ok_or("개선 방법의 지능 역할이 필요합니다.")?;
            if !target_set.contains(method_path)
                || !component_files[method_role].as_array()
                    .is_some_and(|files|files.iter().any(|file|file==method_path))
                || !principles.iter().any(|p|
                    p["methodRole"]==method_role
                        && p["methodSourcePaths"].as_array().is_some_and(|files|
                            files.iter().any(|file|file==method_path))
                        && p["improvesRoles"].as_array().is_some_and(|roles|
                            roles.iter().any(|r|r==role))){
                return Err("같은 지능을 반복해서 개선하지 못했다면, 원래 지능과 새로운 개선 방법의 실행 소스를 함께 변경하고 원리를 명시해야 합니다.".into());
            }
        }
    }
    let next_files=body["nextTargetFiles"].as_array()
        .cloned().unwrap_or_default();
    if next_files.len()>3 || next_files.iter().any(|file|file.as_str()
        .is_none_or(|path|!intelligence_editable_path(path))) {
        return Err("다음 세대의 목표 파일은 허용된 소스 3개 이하여야 합니다.".into());
    }
    let stages=body["successorStages"].as_array().cloned().unwrap_or_default();
    if stages.len()>10 {
        return Err("후손의 새로운 개선 프로그램은 최대 10단계입니다.".into());
    }
    let mut stage_ids=BTreeSet::new();
    for stage in &stages{
        let role=stage["role"].as_str().filter(|r|valid_role(r))
            .ok_or("후손 단계의 지능 구성요소가 유효하지 않습니다.")?;
        let domain=stage["domain"].as_str().filter(|d|
            crate::engine::rsi_evolve::DOMAINS.contains(d))
            .ok_or("후손 단계의 탐색 분야가 유효하지 않습니다.")?;
        let file=stage["path"].as_str().filter(|p|intelligence_editable_path(p))
            .ok_or("후손 단계의 소스 경로가 유효하지 않습니다.")?;
        if !stage_ids.insert((role.to_string(),domain.to_string(),file.to_string())){
            return Err("중복된 개선 단계".into());
        }
        let path=root.join(file);
        if !path.is_file() && !target_set.contains(file){
            return Err("후손의 개선 단계는 실재하거나 이번 세대가 생성할 소스를 가리켜야 합니다.".into());
        }
        let learning=stage["learningPolicy"].as_str().unwrap_or("avoid_failed");
        if !matches!(learning,"avoid_failed"|"repair_failed"|"explore_novel"|"repeat_verified")
            || !matches!(stage["executionPolicy"].as_str().unwrap_or("grounded_edit"),
                "grounded_edit"|"meta_rewrite_first"|"cross_component_revision"
                |"repair_then_edit") {
            return Err("후손 개선 단계의 학습·실행 정책 오류".into());
        }
        if !stage["objective"].as_str().is_some_and(|v|v.len()>=15 && v.len()<=1200)
            || !stage["priority"].as_i64().is_none_or(|v|(-20..=20).contains(&v))
            || !stage["maxRetries"].as_i64().is_none_or(|v|(0..=8).contains(&v)) {
            return Err("후손 단계의 가설·우선순위 또는 재시도 한도 오류".into());
        }
    }
    let method_hypothesis=body["methodHypothesis"].as_str().unwrap_or("");
    if method_hypothesis.len()>1200 {
        return Err("개선 방법의 인과 설명이 너무 깁니다.".into());
    }
    let generator=child["input"]["rsiGeneratorCommit"].as_str()
        .ok_or("후손 소스 버전이 없습니다.")?;
    let revision=format!("{:x}",Sha256::digest(json!([
        generator,component,&roles,domain,hypothesis,design,targets,
        &component_files,&body["actionProgram"],&stages,&next_files,method_hypothesis,
        &births,&edges,&principles
    ]).to_string().as_bytes()));
    let proposal=json!({
        "id":format!("system-design-{revision}"),
        "source":"model_authored_system_design","status":"proposed",
        "campaignId":campaign_id,"generationJobId":child_id,
        "generatorCommit":generator,
        "component":component,"components":roles,"componentFiles":component_files,
        "nextDomain":domain,"nextTargetFiles":next_files,
        "hypothesis":hypothesis,"design":design,"targetFiles":targets,
        "methodHypothesis":method_hypothesis,
        "actionProgram":body["actionProgram"],
        "successorStages":stages,
        "newComponents":births,"interactionEdges":edges,
        "improvementPrinciples":principles,
        "observedSources":observed,
        "qualityProven":false,"adopted":false
    });
    session["rsiSystemDesign"]=proposal.clone();
    state.store.put("session",&session).ok_or("개선 설계 기록 저장 실패")?;
    state.store.audit("rsi.system_design_proposed",json!({
        "campaignId":campaign_id,"generationJobId":child_id,
        "designId":proposal["id"],"roles":proposal["components"],
        "targetFiles":proposal["targetFiles"],"qualityProven":false
    }));
    Ok(json!({"proposal":proposal,
        "next":"이제 실제 Rust 소스를 수정하세요. 변경된 모든 targetFiles가 채택되기 전에는 설계나 개선 프로그램을 다음 세대에 계승하지 않습니다."}))
}

/// The model's accepted goal remains an input to its own next binary.
/// An unadopted written design cannot dictate the descendant's behavior.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    let design=history.iter().rev().find_map(accepted_model_design)?;
    let next=design["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&next)
        .then(||next.to_string())
}

