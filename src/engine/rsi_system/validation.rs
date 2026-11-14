//! Frozen source identity and pinned descendant plan validation.
//! Supervisor-owned check: validates facts only, never recomputes editable ranking.
use super::*;

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
    if !actual.starts_with(&base) || !actual.is_file() {
        return Err(format!("소스 경계 밖의 탐색 근거: {path}"));
    }
    let actual_sha=format!("{:x}",Sha256::digest(
        fs::read(&actual).map_err(|e|e.to_string())?));
    if actual_sha!=sha {
        // The actual pinned file differs. No index rebuild can make this
        // model-generated evidence trustworthy: reject the plan.
        return Err(format!("현재 소스와 다른 탐색 근거: {path} (on-disk SHA differs)"));
    }
    let indexed=crate::engine::code_index::indexed_source_hash(state,index_id,path);
    if indexed.as_deref()!=Some(sha){
        // A recorded index can be stale or missing after a persistent store
        // migration. Reconstruct it from the SAME pinned checkout, then
        // compare both the index identity and the actual source bytes again.
        // This is a consistency repair, never an acceptance override.
        let refreshed=crate::engine::code_index::refresh(state,&base)?;
        let rebuilt=refreshed["id"].as_str()==Some(index_id)
            && crate::engine::code_index::indexed_source_hash(state,index_id,path)
                .as_deref()==Some(sha);
        let final_sha=format!("{:x}",Sha256::digest(
            fs::read(&actual).map_err(|e|e.to_string())?));
        if !rebuilt || final_sha!=sha {
            return Err(format!("현재 소스와 다른 탐색 근거: {path} (index mismatch after reindex)"));
        }
    }
    Ok(())
}

