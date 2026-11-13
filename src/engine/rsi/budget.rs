//! Cohesive source-level RSI coordinator subsystem.
//! This is code that runs in the compiled child, not model-weight training.
use super::*;

pub(super) fn session_research_fingerprint(state: &AppState, session_id: &str) -> String {
    let mut parts = Vec::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        let tool = operation["tool"].as_str().unwrap_or("");
        if !matches!(
            tool,
            "code_search" | "workspace_read_range" | "web_search" | "http_fetch"
        ) {
            continue;
        }
        parts.push(format!("{tool}:{}", operation["arguments"]));
    }
    parts.sort();
    let refs = parts.iter().map(String::as_str).collect::<Vec<_>>();
    crate::engine::memory::stable_id(&refs)
}

/// Research can continue inside the owner's existing budgets while it adds
/// concrete findings. Repeated calls or different queries alone are not progress.
pub(super) fn generation_stop_reason(job:&Value,worker_error:Option<&str>)->Option<&'static str>{
    let error=job["result"]["rsi"]["modelError"].as_str().or(worker_error).or_else(||job["error"].as_str())?;
    if error.starts_with("rsi_token_budget:") || error=="token_budget" {Some("token_budget")}
    else if error=="time_budget" {Some("time_budget")}
    else {None}
}

/// Budget the actual prompt rather than the short owner request. A caller
/// reserves space for the native worker's required improvement directive.
pub(super) fn remaining_generation_tokens(record:&Value)->Option<u64> {
    record["limits"]["tokenBudget"].as_u64().map(|budget|
        budget.saturating_sub(record["tokens"].as_u64().unwrap_or(0)))
}

/// One failed real-model generation previously spent the entire campaign
/// budget, so no descendant could inherit its compiler failure. Allocate
/// the remaining tokens across the remaining actual generation slots.
pub(super) fn generation_token_quota(record:&Value)->Option<u64> {
    let available=remaining_generation_tokens(record)?;
    let attempts_left=record["limits"]["maxAttempts"].as_u64().unwrap_or(10)
        .saturating_sub(record["attempts"].as_u64().unwrap_or(0)).max(1);
    Some(available.div_ceil(attempts_left).max(1).min(available))
}
pub(super) fn prompt_fits_allowance(prompt:&str,available:Option<u64>,reserve:u64)->bool {
    available.is_none_or(|limit|
        crate::engine::chat::estimate_text_tokens(prompt).saturating_add(reserve)<=limit)
}

/// Persisted research is an implementation input, not expendable decoration.
/// Remove duplicated source excerpts before removing the research digest; keep
/// the owner request intact in every variant and never truncate a JSON receipt.
pub(super) fn fit_parent_generation_context(
    base:&str,inherited:&str,digest:&str,source_excerpt:&str,available:Option<u64>
)->Option<(String,Value)> {
    for (mode,context,research,source) in [
        ("complete",inherited,digest,source_excerpt),
        ("source_deferred",inherited,digest,""),
        ("general_context_deferred","",digest,source_excerpt),
        ("research_only","",digest,""),
        ("prior_experience_only",inherited,"",""),
        ("owner_core","","",""),
    ] {
        let prompt=format!("{base}\n\n{context}{research}{source}");
        // The descendant generator also appends its strategy and measurements.
        // The full final prompt is measured independently in that process.
        if prompt_fits_allowance(&prompt,available,1280) {
            let estimated=crate::engine::chat::estimate_text_tokens(&prompt);
            return Some((prompt,json!({"mode":mode,
                "priorExperienceDeferred":!inherited.is_empty() && context.is_empty(),
                "researchDigestDeferred":!digest.is_empty() && research.is_empty(),
                "sourceExcerptDeferred":!source_excerpt.is_empty() && source.is_empty(),
                "estimatedInputTokens":estimated})));
        }
    }
    None
}

/// History compression is semantic and reversible: no partial JSON, no loss of
/// owner objective or the causal action directive on a tight campaign budget.
pub(super) fn fit_worker_generation_context(
    base:&str,domain:&str,capability:&str,history:&[Value],
    directive:&Value,available:Option<u64>,
)->Result<(String,Value),String> {
    // The durable directive contains full compiler stdout and quality
    // receipts. Repeating those large objects with every model turn consumed
    // descendant quotas before a single source read. Preserve the original
    // receipt in the job, but supply a compact executable repair contract.
    let failures=directive["candidateCapture"]["checks"].as_array()
        .into_iter().flatten().filter(|check|check["passed"]==false)
        .take(2).map(|check|json!({
            "label":check["label"],
            "diagnostic":check["stderr"].as_str().or_else(||check["error"].as_str())
                .unwrap_or("").chars().take(700).collect::<String>()
        })).collect::<Vec<_>>();
    let compact_directive=json!({
        "mode":directive["mode"],
        "sourceGenerationId":directive["sourceGenerationId"],
        "sourceJobId":directive["sourceJobId"],
        "domain":directive["domain"],
        "candidateId":directive["candidateId"],
        "attemptedCandidateId":directive["attemptedCandidateId"],
        "previousVerdict":directive["previousVerdict"],
        "previousError":directive["previousError"],
        "previousFocusFiles":directive["previousFocusFiles"],
        "failureCaseIds":directive["failureCaseIds"],
        "checksPassed":directive["candidateCapture"]["checksPassed"],
        "failedChecks":failures,
        "evidenceHashes":directive["evidenceHashes"].as_array().map(|hashes|
            hashes.iter().take(8).filter_map(|hash|hash.as_str())
                .filter(|hash|hash.len()<=128).collect::<Vec<_>>()).unwrap_or_default()
    });
    let mandatory=format!(
        "\n이번 탐색 분야: {domain}. 사용자 목표와 연결된 능력: {capability}. 전체 제품 기능·화면·외부 연결도 개선 대상입니다. 과거 성공·실패와 측정값을 참고하되 관측되지 않은 성능 향상을 주장하지 마세요.\n\n### 이번 세대의 개선 실행 방식\n{}\n근거가 되는 실제 세대 기록: {compact_directive}\n이번 작업에서 코드 변경과 검증 여부를 분명히 기록하세요.",
        improvement_work_contract(directive));
    let full=history.iter().rev().take(8).map(|row|{
        let measured=row["evaluation"]["metrics"]["measurements"].as_array()
            .into_iter().flatten().filter(|case|case["holdout"]==true)
            .take(3).cloned().collect::<Vec<_>>();
        json!({"domain":row["domain"],"accepted":row["accepted"],
            "tokens":row["tokens"],"elapsedMs":row["elapsedMs"],
            "verdict":row["evaluation"]["verdict"],"measurements":measured,
            "error":row["error"].as_str().unwrap_or("").chars().take(200).collect::<String>()})
    }).collect::<Vec<_>>();
    let compact=history.iter().rev().take(2).map(|row|json!({
        "domain":row["domain"],"accepted":row["accepted"],
        "verdict":row["evaluation"]["verdict"],"tokens":row["tokens"],
        "error":row["error"].as_str().unwrap_or("").chars().take(100).collect::<String>()
    })).collect::<Vec<_>>();
    for (mode,rows) in [("measured",full),("compact",compact),("on_demand",Vec::new())] {
        let mut prompt=format!("{base}{mandatory}");
        if !rows.is_empty() {
            prompt.push_str("\n이 실험 계보의 과거 결과: ");
            prompt.push_str(&json!(rows).to_string());
        } else if !history.is_empty() {
            prompt.push_str("\n상세 세대 기록은 저장소에 보존되어 있습니다.");
        }
        if prompt_fits_allowance(&prompt,available,256) {
            let estimated=crate::engine::chat::estimate_text_tokens(&prompt);
            return Ok((prompt,json!({"mode":mode,"rows":rows.len(),
                "availableRows":history.len(),
                "estimatedInputTokens":estimated,
                "improvementDirectivePreserved":true})));
        }
    }
    Err("rsi_token_budget: 소유자 목표와 실제 개선 지시를 유지할 입력 예산이 부족합니다.".into())
}

pub(super) fn generation_fits_remaining_budget(record:&Value,input:&Value)->bool {
    record["limits"]["tokenBudget"].as_u64().is_none_or(|budget|{
        let minimum=crate::engine::chat::estimate_text_tokens(input["prompt"].as_str().unwrap_or(""))+256;
        budget.saturating_sub(record["tokens"].as_u64().unwrap_or(0))>=minimum
    })
}

pub(super) fn should_stop_no_progress(streak: u64, research: bool, changed: bool) -> bool {
    streak>=1 && (!research || !changed)
}

/// A failed attempt to turn real research into code deserves one bounded
/// corrective generation. Without this exception no_progress terminates
/// before the newly learned change_approach action can ever execute.
/// The normal maxAttempts, token and time budgets remain authoritative.
pub(super) fn corrective_retry_allowed(history:&[Value], no_edit_streak:u64) -> bool {
    if no_edit_streak>2 {return false;}
    let Some(last)=history.last() else {return false;};
    last["accepted"]!=true
        && last["candidateId"].is_null()
        && last["newFindings"].as_u64().unwrap_or(0)==0
        && matches!(last["improvementDirective"]["mode"].as_str(),
            Some("implement_research"|"repair_counterexample"|"revise_candidate"))
}

pub(super) fn new_research_findings(state:&AppState,input:&Value,job_id:&str,hashes:&[Value],history:&[Value])->usize{
    let mut known=history.iter().flat_map(|g|g["knowledgeArtifactHashes"].as_array().into_iter().flatten()).filter_map(Value::as_str).map(str::to_string).collect::<BTreeSet<_>>();
    for experience in state.store.all("experience").into_iter().filter(|e|
        e["status"]=="active" && e["generationJobId"]!=job_id
            && e["partitionKey"]==research_scope(input).partition_key) {
        known.extend(experience["researchArtifacts"].as_array().into_iter().flatten().filter_map(|a|a["hash"].as_str().map(str::to_string)));
    }
    hashes.iter().filter(|hash|hash.as_str().is_some_and(|hash|!known.contains(hash))).count()
}

