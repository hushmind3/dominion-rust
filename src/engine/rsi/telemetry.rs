//! Cohesive source-level RSI coordinator subsystem.
//! This is code that runs in the compiled child, not model-weight training.
use super::*;

pub(super) fn session_tool_usage(state: &AppState, session_id: &str) -> Vec<Value> {
    let mut counts = BTreeMap::<String, u64>::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        // Historical tool-call counts remain a throughput metric, never
        // evidence that a filesystem mutation actually happened.
        let tool = operation["tool"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(tool).or_insert(0) += 1;
    }
    counts.into_iter()
        .map(|(tool, calls)| json!({"tool":tool,"calls":calls}))
        .collect()
}

/// A successful *call* is not the same as a successful source *write*.
/// Both failed operations and legacy Ok({"error": ...}) responses must
/// survive candidate collection and reach the next generation.
pub(super) fn session_write_receipts(state:&AppState, session_id:&str) -> Vec<Value> {
    let mut rows = Vec::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || !matches!(operation["tool"].as_str(),Some("workspace_write"|"workspace_replace"|"workspace_replace_range"|"workspace_replace_function"|"workspace_replace_function"))
        {
            continue;
        }
        let actual_write=operation["status"]=="completed"
            && operation["result"]["written"]==true
            && operation["toolReceipt"]["verified"]==true;
        let error=operation["error"].as_str()
            .or_else(||operation["result"]["error"].as_str())
            .unwrap_or(if actual_write {""} else {"tool call has no confirmed file write"});
        rows.push(json!({
            "tool":operation["tool"],"path":operation["arguments"]["path"],
            "status":operation["status"],"written":actual_write,
            "error":error.chars().take(650).collect::<String>(),
        }));
    }
    rows.into_iter().rev().take(12).collect()
}

/// Preserve failed or interrupted *real* candidates as corrective evidence.
/// Only checksPassed candidates enter the frozen A/B evaluator; the worker
/// must never silently turn a compile failure into a research-only attempt.
pub(super) fn candidate_capture_feedback(result:&Value) -> Option<String> {
    let id=result["candidateId"].as_str()?;
    if result["checksPassed"]==true {return None;}
    let mut failures=Vec::new();
    for check in result["checks"].as_array().into_iter().flatten()
        .filter(|check|check["passed"]!=true).take(3) {
        let label=check["label"].as_str().unwrap_or("source verification");
        let details=check["stderr"].as_str()
            .filter(|s|!s.trim().is_empty())
            .or_else(||check["error"].as_str())
            .unwrap_or("the check was not successful");
        failures.push(format!("{label}: {}",
            details.chars().take(900).collect::<String>()));
    }
    if failures.is_empty() {
        failures.push("검사 완료 증거가 없어 비교 실험으로 넘기지 못함".into());
    }
    Some(format!(
        "실제 소스 변경 후보 {id}가 수집됐지만 검증이 실패했습니다. 다음 세대에서 해당 소스를 교정하고 다시 편집해야 합니다. {}",
        failures.join(" | ")
    ))
}

/// Each campaign keeps its compiled generator. Two concurrent campaigns
/// must never replace one another's process between identity check and spawn.
pub(super) fn generator_target_dir(data_dir:&Path, campaign_id:&str) -> PathBuf {
    let safe_id=crate::engine::memory::stable_id(&[campaign_id]);
    data_dir.join("rsi-build").join("generator").join(format!("campaign-{safe_id}"))
}

/// Once a clean native hypothesis has been independently measured and
/// rejected as a plateau, exploring a *different* native operator is more
/// useful than restoring the identical patch for a model that was never used.
/// The receipt gate in the caller still has to validate the measured plateau.
pub(super) fn native_plateau_prefers_next_operator(input:&Value,candidate:&Value)->bool {
    input["rsiNativeInduction"]==true
        && input["selectionPolicy"]=="automatic"
        && candidate["origin"]=="native_inductive_source_mutation"
        && candidate["status"]=="inconclusive"
}

/// Choose a verified next domain from accepted native source mutations.
/// The choice is an execution target, never a substitute for paired quality.
pub(super) fn native_descendant_target(input:&Value,history:&[Value])->Option<&'static str> {
    if input["rsiNativeInduction"]!=true || input["rsiExpandAfterGain"]!=true {
        return None;
    }
    let accepted=history.iter().filter(|row|row["accepted"]==true
        && row["candidateOrigin"]=="native_inductive_source_mutation"
        && row["evaluation"]["metrics"]["eligible"]==true)
        .collect::<Vec<_>>();
    if accepted.is_empty() {return None;}
    let meta_gain=accepted.iter().any(|row|row["candidateTarget"]=="improver"
        && row["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0);
    Some(if meta_gain {"memory"} else {"improver"})
}

