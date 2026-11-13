//! Child-binary planning process boundary. Does not execute model tools,
//! evaluate candidates, or adopt source. Separate from worker runtime.
use super::*;

pub async fn plan_generation(path:&str)->Result<(),String>{
    plan_generation_inner(path,false).await
}

/// The control is compiled from the earlier source revision while reading the
/// SAME CURRENT source tree, frozen cases and replayed campaign history.
/// It can generate a plan but can never adopt a candidate or move Git refs.
pub async fn plan_counterfactual(path:&str)->Result<(),String>{
    plan_generation_inner(path,true).await
}

async fn plan_generation_inner(path:&str,control_mode:bool)->Result<(),String>{
    let snapshot:Value=serde_json::from_slice(&std::fs::read(path)
        .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let commit=option_env!("DOMINION_BUILD_COMMIT")
        .ok_or("계획 생성기 바이너리의 커밋 식별자가 없습니다.")?;
    if snapshot["generatorCommit"]!=commit
        || snapshot["input"]["rsiGeneratorCommit"]!=commit
        || snapshot["input"]["rsiSystemEvolution"]!=true {
        return Err("소스 스냅샷과 실제 후손 바이너리가 다릅니다.".into());
    }
    let campaign_id=snapshot["campaignId"].as_str().ok_or("캠페인 ID 없음")?;
    let job=state.store.get("job",campaign_id).ok_or("캠페인 작업 없음")?;
    let campaign=state.store.get("rsi_campaign",campaign_id)
        .ok_or("연속 캠페인 계보 없음")?;
    let legitimate_control=control_mode
        && snapshot["controlExperiment"]==true
        && snapshot["sourceHeadCommit"]==campaign["headCommit"]
        && snapshot["input"]["rsiBaseCommit"]==campaign["headCommit"]
        && snapshot["input"]["rsiControlExperiment"]==true
        && snapshot["input"]["rsiSourceDir"]==campaign["labDir"]
        && snapshot["input"]["rsiGeneratorCommit"]==commit
        && snapshot["sourceHeadCommit"]!=commit;
    if job["kind"]!="rsi_evolve" || job["status"]!="running"
        || job["cancelRequested"]==true
        || (control_mode && !legitimate_control)
        || (!control_mode && campaign["headCommit"]!=commit) {
        return Err("정지되거나 다른 소스의 캠페인은 계획할 수 없습니다.".into());
    }
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]==campaign_id).collect::<Vec<_>>();
    history.sort_by_key(|row|row["attempt"].as_u64().unwrap_or(0));
    if snapshot["historyCount"].as_u64()!=Some(history.len() as u64)
        || snapshot["attempt"].as_u64()!=Some(history.len() as u64+1)
        || campaign["history"].as_array().map(Vec::len)!=Some(history.len()) {
        return Err("후손이 읽은 이전 세대의 실험 기록이 일치하지 않습니다.".into());
    }
    let plan=compiled_system_generation_plan(
        &state,&snapshot["input"],&history,commit)?;
    println!("{plan}");
    Ok(())
}

// Executes only one durable generation, without starting a server, scheduler,
// migrations or recovering another process's jobs in the shared ledger.
