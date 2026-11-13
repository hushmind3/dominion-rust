//! Structural source/lineage validation outside the mutable planner.
use super::{source_inventory,valid_role};
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::{fs,path::Path};

/// The resident supervisor validates MATERIAL facts, not the old
/// generation's ranking, self-model taxonomy, or improvement equation.
/// Otherwise an improved descendant metacontroller would be rejected merely
/// for making a different architectural decision from its parent.
pub(crate) fn validate_plan_evidence(
    state:&crate::state::AppState,root:&Path,commit:&str,
    index_id:&str,plan:&Value,history:&[Value]
)->Result<(),String>{
    use std::collections::{BTreeMap,BTreeSet};
    // Verify real source bytes, not equality to the parent's old source
    // discovery algorithm. Descendants may enlarge their source taxonomy
    // without bypassing the immutable filesystem or hash boundaries.
    let actual=source_inventory(root);
    let blueprint=&plan["systemBlueprint"];
    let declared=blueprint["sourceInventory"].as_array()
        .filter(|rows|!rows.is_empty()&&rows.len()<=4096)
        .ok_or("후손의 지능 소스 목록 오류")?;
    let digest=format!("{:x}",Sha256::digest(
        serde_json::to_vec(declared).map_err(|e|e.to_string())?));
    if blueprint["generatorCommit"]!=commit
        || blueprint["sourceTreeSha256"]!=digest
        || plan["sourceTreeSha256"]!=digest
        || blueprint["sourceFileCount"].as_u64()!=Some(declared.len() as u64){
        return Err("후손 소스 계보와 전체 소스 스냅샷이 다릅니다.".into());
    }
    let base=fs::canonicalize(root).map_err(|e|e.to_string())?;
    let mut known=BTreeMap::<String,String>::new();
    for row in declared {
        let path=row["path"].as_str()
            .filter(|p|crate::engine::rsi::safe_path(p))
            .ok_or("후손이 제시한 소스 경로가 안전하지 않습니다.")?;
        let sha=row["sha256"].as_str().filter(|s|
            s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit()))
            .ok_or("후손의 소스 해시가 유효하지 않습니다.")?;
        let file=fs::canonicalize(base.join(path))
            .map_err(|_|"후손이 제시한 소스가 실제 저장소에 없습니다.".to_string())?;
        if !file.starts_with(&base)||!file.is_file(){
            return Err("소스 경로가 격리된 저장소 밖에 있습니다.".into());
        }
        let bytes=fs::read(&file).map_err(|e|e.to_string())?;
        if bytes.len()>2_000_000
            || row["bytes"].as_u64()!=Some(bytes.len() as u64)
            || format!("{:x}",Sha256::digest(&bytes))!=sha
            || known.insert(path.to_string(),sha.to_string()).is_some(){
            return Err("후손 소스 목록이 실제 파일 내용과 다릅니다.".into());
        }
        if crate::engine::code_index::indexed_source_hash(state,index_id,path)
            .is_some_and(|indexed|indexed!=sha){
            return Err("후손 소스와 기존 인덱스가 일치하지 않습니다.".into());
        }
    }
    // Previously observed sources remain a factual minimum: an evolved
    // scanner may ADD new programming languages and intelligence modules,
    // but cannot hide any old source or forge its contents.
    if actual.iter().any(|row|{
        let Some(path)=row["path"].as_str() else{return true;};
        known.get(path).map(String::as_str)!=row["sha256"].as_str()
    }){
        return Err("기존 실행 소스가 후손의 자기모형에서 누락됐습니다.".into());
    }
    let components=blueprint["architecture"].as_array()
        .filter(|c|!c.is_empty()&&c.len()<=64)
        .ok_or("후손의 지능 구성요소 목록 오류")?;
    let mut names=BTreeSet::new();
    let mut ownership=BTreeMap::<String,BTreeSet<String>>::new();
    for component in components{
        let role=component["role"].as_str().filter(|r|valid_role(r))
            .ok_or("자기모형 구성요소 이름 오류")?;
        if !names.insert(role.to_string()){
            return Err("자기모형의 구성요소가 중복됐습니다.".into());
        }
        let domain=component["domain"].as_str()
            .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
            .ok_or("자기모형 구성요소 분야 오류")?;
        let _=domain;
        let entries=component["representativeSources"].as_array()
            .ok_or("자기모형 구성요소 소스 목록 오류")?;
        let mut mapped=BTreeSet::new();
        for entry in entries{
            let path=entry["path"].as_str().filter(|path|
                crate::engine::rsi::safe_path(path))
                .ok_or("자기모형 구성요소 소스 경로 오류")?;
            let sha=entry["sha256"].as_str().ok_or("자기모형 소스 해시 누락")?;
            if !mapped.insert(path.to_string())
                || known.get(path).map(String::as_str)!=Some(sha)
                || crate::engine::code_index::indexed_source_hash(state,index_id,path)
                    .as_deref().is_some_and(|indexed|indexed!=sha){
                return Err("구성요소 소스와 현재 색인의 SHA-256이 다릅니다.".into());
            }
        }
        ownership.insert(role.to_string(),mapped);
    }
    let frontier=plan["architectureFrontier"].as_array()
        .filter(|rows|rows.len()<=128).ok_or("후손 탐색 근거 수 오류")?;
    let mut available=BTreeSet::new();
    for item in frontier{
        let role=item["role"].as_str().ok_or("탐색 구성요소 이름 누락")?;
        let path=item["path"].as_str()
            .filter(|path|crate::engine::rsi::safe_path(path))
            .ok_or("탐색 소스 경로 오류")?;
        let sha=item["sourceSha256"].as_str()
            .ok_or("탐색 소스 해시 누락")?;
        let domain=item["domain"].as_str().filter(|d|
            crate::engine::rsi_evolve::DOMAINS.contains(d))
            .ok_or("탐색 분야 오류")?;
        if !names.contains(role)
            || !ownership.get(role).is_some_and(|files|files.contains(path))
            || known.get(path).map(String::as_str)!=Some(sha)
            || crate::engine::code_index::indexed_source_hash(state,index_id,path)
                .as_deref().is_some_and(|indexed|indexed!=sha)
            || !available.insert((role.to_string(),domain.to_string(),
                path.to_string(),sha.to_string())){
            return Err("후손 탐색 결정의 소스·역할·인덱스 근거 오류".into());
        }
    }
    let bound=|value:&Value|->bool{
        let (Some(role),Some(domain),Some(path),Some(sha))=(
            value["role"].as_str(),value["domain"].as_str(),
            value["path"].as_str(),value["sourceSha256"].as_str()) else{
            return false;
        };
        available.contains(&(role.to_string(),domain.to_string(),
            path.to_string(),sha.to_string()))
    };
    if !plan["architectureTarget"].is_null()
        && !bound(&plan["architectureTarget"]){
        return Err("후손이 선택한 아키텍처 대상이 실제 소스에 없습니다.".into());
    }
    let meta=&plan["metaPolicy"];
    if !meta.is_object() || (meta["target"].is_object() && !bound(&meta["target"])){
        return Err("후손 메타 제어기의 선택 대상이 실제 소스에 없습니다.".into());
    }
    if plan["metaActionPolicy"]!=meta["controlPolicy"]{
        return Err("후손이 실행할 메타 정책이 계획과 다릅니다.".into());
    }
    let policy=&plan["metaActionPolicy"];
    if policy.is_object(){
        if !matches!(policy["toolPolicy"].as_str(),
            Some("patch_only"|"allow_new_modules"))
            || !matches!(policy["learningPolicy"].as_str(),
                Some("avoid_failed"|"retry_failed"))
            || !matches!(policy["executionPolicy"].as_str(),
                Some("grounded_edit"|"explore_then_edit"))
            || !available.iter().any(|(role,domain,path,sha)|
                policy["role"]==*role && policy["domain"]==*domain
                    && policy["path"]==*path && policy["sourceSha256"]==*sha){
            return Err("후손의 실행·학습·도구 정책이 실제 소스에 연결되지 않았습니다.".into());
        }
    }
    // Check the *facts* behind a paired source-edit programme, never impose
    // the older resident controller's partner-selection scoring function.
    let mutual=&plan["mutualCurriculum"];
    if !mutual.is_null() {
        let pair=&mutual["pair"];
        if mutual["source"]!="compiled_reciprocal_intelligence_curriculum_v1"
            || mutual["causalityProven"]!=false
            || mutual["qualityProven"]!=false
            || !mutual["revision"].as_str().is_some_and(|s|
                s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit())) {
            return Err("상호 지능 개선 프로그램의 출처와 증거 범위가 다릅니다.".into());
        }
        if pair.is_object(){
            let a=frontier.iter().find(|row|row["role"]==pair["fromRole"]
                && row["path"]==pair["fromPath"]
                && row["sourceSha256"]==pair["fromSourceSha256"]);
            let b=frontier.iter().find(|row|row["role"]==pair["toRole"]
                && row["path"]==pair["toPath"]
                && row["sourceSha256"]==pair["toSourceSha256"]);
            if a.is_none() || b.is_none()
                || pair["fromRole"]==pair["toRole"]
                || pair["fromPath"]==pair["toPath"]
                || a.is_none_or(|row|&mutual["primary"]!=row)
                || mutual["phases"].as_array().is_none_or(|p|p.len()!=3) {
                return Err("상호 발전 계획의 2개 실제 소스·단계가 일치하지 않습니다.".into());
            }
        }else if mutual["phases"].as_array()
            .is_none_or(|phases|!phases.is_empty()){
            return Err("대상이 없는 상호개선 계획에 실행 단계가 남았습니다.".into());
        }
    }
    let science_active=plan["recursiveScienceActive"].as_bool()
        .ok_or("후손 재귀 발명 프로그램의 실행 여부가 없습니다.")?;
    if science_active && (!plan["recursiveDiscovery"]["target"].is_object()
        || plan["focusFiles"].as_array().is_none_or(|files|
            !files.iter().any(|file|
                file==&plan["recursiveDiscovery"]["target"]["path"]))) {
        return Err("재귀 발명의 실행 목표와 실제 소스 편집 대상이 다릅니다.".into());
    }
    if !crate::engine::rsi_recursive_discovery::validate(
        &plan["recursiveDiscovery"],frontier,commit,history,
        &blueprint["openTopology"],&plan["factorialCurriculum"],
        &plan["mutualCurriculum"]) {
        return Err("후손의 재귀 지능 발명·자기수정 계획 근거가 일치하지 않습니다.".into());
    }
    if !crate::engine::rsi_causal_curriculum::valid(
        &plan["factorialCurriculum"],frontier,commit,history.len(),
        &plan["mutualCurriculum"]) {
        return Err("요인별 실험 계획의 독립 소스·계보가 일치하지 않습니다.".into());
    }
    // Validate the model-authored graph as source-grounded *data*, while
    // leaving its editable ranking algorithm inside the compiled successor.
    if !crate::engine::rsi_open_program::validate(
        &plan["openProgramPlan"],history,frontier,commit) {
        return Err("모델 작성 열린 실험 그래프의 계보 또는 소스 근거 오류".into());
    }
    let stage=&plan["successorStage"];
    if stage.is_object(){
        let open_stage=stage["source"]=="model_authored_open_program_stage"
            && plan["openProgramPlan"]["stage"]==*stage
            && plan["openProgramPlan"]["target"]["path"]==stage["path"];
        if !bound(stage)
            || (stage["programmeStageIndex"].is_number()
                && !crate::engine::rsi_mutual_evolution::valid_programme_lineage(
                    history,stage))
            || (!open_stage && !history.iter().any(|g|
            g["accepted"]==true && g["pairedRevalidated"]==true
            && g["systemDesign"]["id"]==stage["sourceDesignId"]
            && g["acceptedCommit"]==stage["predecessorCommit"]
            && (g["systemDesign"]["successorStages"].as_array().is_some_and(|rows|
                rows.iter().any(|row|row["role"]==stage["role"]
                    && row["domain"]==stage["domain"]
                    && row["path"]==stage["path"]))
                || crate::engine::rsi_mutual_evolution::valid_return_lineage(
                    history,stage)))) {
            return Err("후손에게 계승된 설계 단계의 채택 계보가 다릅니다.".into());
        }
    }
    Ok(())
}

