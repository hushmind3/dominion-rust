//! budgeted generation and prompt assembly.
//! Only generation_execution owns these implementations. Other RSI stages use explicit
//! names imported through the stable rsi_evolve facade.
use super::{AppState, BTreeMap, BTreeSet, DOMAINS, Value, experience, json, research_scope};
/// Research can continue inside the owner's existing budgets while it adds
/// concrete findings. Repeated calls or different queries alone are not progress.
pub(super) fn generation_stop_reason(job:&Value,worker_error:Option<&str>)->Option<&'static str>{
    let error=job["result"]["rsi"]["modelError"].as_str().or(worker_error).or_else(||job["error"].as_str())?;
    if error.starts_with("rsi_token_budget:") || error=="token_budget" {Some("token_budget")}
    else if error=="time_budget" {Some("time_budget")}
    else {None}
}

pub(super) fn generation_fits_remaining_budget(record:&Value,input:&Value)->bool {
    record["limits"]["tokenBudget"].as_u64().is_none_or(|budget|{
        let minimum=crate::engine::chat::estimate_text_tokens(input["prompt"].as_str().unwrap_or(""))+256;
        budget.saturating_sub(record["tokens"].as_u64().unwrap_or(0))>=minimum
    })
}

/// Never splice a fixed number of characters out of serialized JSON:
/// an incomplete receipt wastes model tokens and can corrupt later feedback.
/// Shrink OPTIONAL telemetry as a whole and keep the objective and directive.
/// Independent paired cases that still miss their quality objective are
/// the actionable counterexamples for the next source-edit generation.
/// A neutral 1.0 -> 1.0 case is a satisfied guard, not a repair target.
pub(super) fn unresolved_quality_case(case:&Value)->bool {
    case["metrics"].as_array().is_some_and(|metrics|metrics.iter().any(|metric|
        metric["key"]=="quality" && metric["improved"]==false
            && metric["regressed"]==false
            && metric["baseline"].as_f64().is_some_and(|value|value<1.0)
            && metric["candidate"].as_f64()==metric["baseline"].as_f64()))
}

pub(super) fn fit_worker_prompt(
    base: &str, domain: &str, capability: &str, history: &[Value],
    remaining_tokens: Option<u64>,
) -> Result<(String, Value), String> {
    let directive = format!(
        "\n이번 탐색 분야: {domain}. 사용자 목표와 연결된 능력: {capability}. 전체 제품 기능·화면·외부 연결도 개선 대상입니다. 과거 성공·실패와 측정값을 참고하되 관측되지 않은 성능 향상을 주장하지 마세요. 이전 paired 사례의 baseline과 candidate가 같았다면 성공했다고 반복하지 말고, 그 사례를 바꿀 다른 소스 수정 가설을 실제로 구현하세요."
    );
    let full = history.iter().rev().take(8).map(|row| {
        let measured=row["evaluation"]["metrics"]["measurements"].as_array();
        let altered=measured.into_iter().flatten().filter(|case|case["metrics"].as_array()
            .is_some_and(|metrics|metrics.iter().any(|m|
                m["improved"]==true || m["regressed"]==true))).take(3).cloned();
        // Zero-gain is evidence too. Previously a successful compilation
        // followed by 13 unchanged paired cases was handed to the successor
        // as an EMPTY measurement set, causing blind repeat attempts.
        let mut cases=altered.collect::<Vec<_>>();
        // Prioritize a genuinely unsolved quality task before saturated
        // guards (e.g. 0.75 -> 0.75 before 1.0 -> 1.0).
        for case in measured.into_iter().flatten()
            .filter(|case|unresolved_quality_case(case))
            .chain(measured.into_iter().flatten().filter(|case|
                case["metrics"].as_array().is_some_and(|metrics|!metrics.is_empty()))) {
            if cases.len()>=3 {break;}
            if !cases.iter().any(|previous|previous["id"]==case["id"]) {
                cases.push(case.clone());
            }
        }
        json!({
            "domain":row["domain"],"accepted":row["accepted"],
            "tokens":row["tokens"],"elapsedMs":row["elapsedMs"],
            "verdict":row["evaluation"]["verdict"],"measurements":cases,
            "error":row["error"].as_str().map(|error|
                error.chars().take(240).collect::<String>()),
            // A descendant must know the real compiler failure from the
            // parent run, not merely that "no candidate" was adopted.
            "compilerDiagnostic":row["verification"]["checks"].as_array()
                .into_iter().flatten().find(|check|check["passed"]!=true)
                .and_then(|check|check["detail"].as_str())
                .map(|error|error.chars().take(780).collect::<String>()),
            "repairAttempts":row["verification"]["repairAttempts"]
        })
    }).collect::<Vec<_>>();
    let compact = history.iter().rev().take(2).map(|row|json!({
        "domain":row["domain"],"accepted":row["accepted"],
        "tokens":row["tokens"],"verdict":row["evaluation"]["verdict"],
        // Compact mode must still disclose at least one unchanged measured
        // case; otherwise low token budgets silently erase failure feedback.
        "pairedCase":row["evaluation"]["metrics"]["measurements"].as_array()
            .and_then(|cases|cases.iter().find(|case|unresolved_quality_case(case))
                .or_else(||cases.iter().find(|case|
                    case["metrics"].as_array().is_some_and(|metrics|!metrics.is_empty()))))
            .map(|case|json!({"id":case["id"],"domain":case["domain"],
                "metrics":case["metrics"]})),
        "error":row["error"].as_str().map(|error|
            error.chars().take(120).collect::<String>()),
        "compilerDiagnostic":row["verification"]["checks"].as_array()
            .into_iter().flatten().find(|check|check["passed"]!=true)
            .and_then(|check|check["detail"].as_str())
            .map(|error|error.chars().take(450).collect::<String>())
    })).collect::<Vec<_>>();
    for (mode, rows) in [("full",full),("compact",compact),("deferred",Vec::new())] {
        let mut prompt = format!("{base}{directive}");
        if !rows.is_empty() {
            prompt.push_str("\n이 실험 계보의 과거 결과: ");
            prompt.push_str(&json!(rows).to_string());
        } else if !history.is_empty() {
            prompt.push_str("\n과거 세대의 상세 지표는 작업 저장소에 보존되어 있습니다.");
        }
        let estimate = crate::engine::chat::estimate_text_tokens(&prompt);
        if remaining_tokens.is_none_or(|limit|estimate.saturating_add(256)<=limit) {
            return Ok((prompt,json!({
                "mode":mode,"feedbackRows":rows.len(),
                "availableRows":history.len(),"estimatedInputTokens":estimate,
                "ownerObjectivePreserved":true
            })));
        }
    }
    Err("rsi_token_budget: 목표와 핵심 지시문을 유지할 입력 예산이 부족합니다.".into())
}

/// An opted-in broad RSI campaign must not keep optimizing a now-satisfied
/// single-file fixture after a verified adoption. Choose the next challenge
/// from REAL frozen paired measurements and rescan the descendant's source.
/// Neither the new goal nor a quality gap counts as an adopted generation.
pub(super) fn post_adoption_objective(input: &Value, history: &[Value]) -> Option<Value> {
    if input["rsiExpandAfterGain"] != true { return None; }
    let accepted = history.iter().rev().find(|row|
        row["accepted"] == true && row["evaluation"]["metrics"]["eligible"] == true)?;
    let residuals = accepted["evaluation"]["metrics"]["measurements"].as_array()
        .into_iter().flatten()
        .filter_map(|case| {
            let gaps = case["metrics"].as_array()?.iter()
                .filter(|metric|metric["key"] == "quality"
                    && metric["candidate"].as_f64().is_some_and(|value|value < 1.0)
                    && metric["improved"] != true)
                .map(|metric|json!({"metric":"quality",
                    "observed":metric["candidate"],"ceiling":1.0}))
                .collect::<Vec<_>>();
            if gaps.is_empty() {return None;}
            Some(json!({"caseId":case["id"],"domain":case["domain"],
                "gaps":gaps}))
        }).take(5).collect::<Vec<_>>();
    // Most fixed-quality oracles saturate at 1.0 after the first gain.
    // Rank real unoptimized cost measurements instead of blindly asking
    // the descendant to improve an already-satisfied accuracy fixture.
    let mut costs=Vec::<Value>::new();
    for case in accepted["evaluation"]["metrics"]["measurements"]
        .as_array().into_iter().flatten() {
        let frozen=input["rsiFrozenCases"].as_array().into_iter().flatten()
            .find(|row|row["id"]==case["id"]);
        for metric in case["metrics"].as_array().into_iter().flatten() {
            let key=metric["key"].as_str().unwrap_or("");
            if metric["direction"]!="lower" || metric["improved"]==true {continue;}
            let Some(observed)=metric["candidate"].as_f64()
                .filter(|v|v.is_finite() && *v>0.0) else {continue;};
            let Some(spec)=frozen.and_then(|row|row["metrics"].as_array())
                .and_then(|metrics|metrics.iter().find(|row|row["key"]==key))
                else {continue;};
            let relative=spec["minRelativeGain"].as_f64().unwrap_or(0.05);
            let absolute=spec["minAbsoluteGain"].as_f64().unwrap_or(0.0);
            let needed=(observed*relative).max(absolute);
            if needed<=0.0 {continue;}
            costs.push(json!({"caseId":case["id"],"domain":case["domain"],
                "metric":key,"observed":observed,
                "requiredReduction":needed,"qualityGuard":"no_regression"}));
        }
    }
    // Token/context reduction has a more stable, directly attributable
    // counter than noisy wall-clock timing. Within a metric, prioritize
    // the largest measured costs; the fixed evaluator still decides adoption.
    costs.sort_by(|a,b| {
        let rank=|row:&Value|if row["metric"]=="contextChars" {0}else{1};
        rank(a).cmp(&rank(b)).then_with(||
            b["observed"].as_f64().unwrap_or(0.0)
                .total_cmp(&a["observed"].as_f64().unwrap_or(0.0)))
    });
    costs.truncate(5);
    // An actually recursive demonstration needs an improved improver,
    // followed by a new independently measured capability. A first gain in
    // ordinary application logic does not satisfy this causal transfer.
    // Use paired meta evidence only to select the next target, NEVER to
    // count an unverified adoption or loosen the frozen evaluation.
    let meta_seen=history.iter().rev().take_while(|row|row["accepted"]==true)
        .any(|row|row["evaluation"]["metrics"]["eligible"]==true
            && row["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0)>0);
    // Once the improver itself has a measured gain, prioritize a second
    // domain with a known source transformation, if the native source
    // generator was opted into the campaign. Its benefit is still entirely
    // decided by the independent frozen evaluator.
    if meta_seen && input["rsiNativeInduction"]==true {
        costs.sort_by_key(|row| {
            if row["domain"]=="memory" && row["metric"]=="elapsedMs" {0}
            else if row["domain"]=="context" && row["metric"]=="contextChars" {1}
            else {2}
        });
    }
    let meta_cost=if meta_seen {None} else {
        costs.iter().find(|row|row["domain"]=="improver")
    };
    let objective = if let Some(target)=meta_cost {
        format!("먼저 개선기 자체의 탐색·선택 알고리즘을 더 효율적으로 발전시킨다. 독립 동결 기준의 메타 병목은 {} 이며 비용 감소 목표는 최소 {}이다. src/engine/rsi_evolve.rs 등 실제 개선기 소스의 반복 계산과 증거 처리 방식을 측정 가능한 새 실행 방식으로 수정한다. 독립 메타 holdout 승격 전에는 성과라고 보고하지 않는다. 이후에야 다른 능력으로 전이한다.",
            json!(target),target["requiredReduction"])
    } else if !residuals.is_empty() {
        format!("동결 평가의 미해결 정확도 결함 {} 중 하나를 실제 구현으로 개선하고 이전 세대의 채택 성과를 유지한다.",
            json!(residuals))
    } else if let Some(target)=costs.first() {
        format!("동결 A/B에서 다음 비용 병목을 계측했다: {}. 해당 도메인의 실제 소스를 개선하여 비용을 최소 {}만큼 줄이고 다른 품질·기능 회귀 없이 독립 재검증한다. 실패할 경우 다른 독립 지표를 탐색한다.",
            json!(target),target["requiredReduction"])
    } else {
        "기존 채택을 유지하면서 동결된 정확도·비용 지표에서 실제 신규 개선 기회를 발견하고 실행한다. 포화된 품질의 반복 수정은 피한다.".to_string()
    };
    Some(json!({
        "objective":objective,
        "metaTransferTarget":meta_cost.is_some(),
        "residuals":residuals,
        "costOpportunities":costs,
        "previousAcceptedCommit":accepted["acceptedCommit"],
        "prompt":format!(
            "DOMINION 연속 자기개선의 후손 세대입니다. 직전 채택 커밋은 {} 입니다. 이진 실행기와 작업 기준 코드는 이미 이 커밋에서 재빌드되었습니다. {} 현재 소스 인덱스로 다른 분야의 원인 파일을 찾고, 안전한 분리 작업공간에서 실제 동작/비용을 바꾸는 최소 구현을 작성하세요. 소스 조사로 끝내지 말고 검증 가능한 패치를 남기세요. 동일한 분해 버그의 재수정은 하지 마세요. 패치가 새로운 동결 평가 개선을 입증하기 전에는 성능 향상이라고 주장하지 마세요.",
            accepted["acceptedCommit"].as_str().unwrap_or(""),
            objective)
    }))
}

pub(super) fn should_stop_no_progress(
    streak: u64, research: bool, changed: bool,
    pending_trial: bool, autonomous: bool,
) -> bool {
    // Explicit/manual campaigns keep their previous early-stop semantics.
    if !autonomous {return streak>=1 && (!research || !changed);}
    // A bounded autonomous campaign gets time to turn real, barren feedback
    // into a different strategy and run the pending trial. Never loop forever.
    if streak>=6 {return true;}
    if pending_trial {return false;}
    if research && changed {return false;}
    streak>=3
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

pub(super) fn session_tool_usage(state: &AppState, session_id: &str) -> Vec<Value> {
    let mut counts = BTreeMap::<String, u64>::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        let tool = operation["tool"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(tool).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(tool, calls)| json!({"tool": tool, "calls": calls}))
        .collect()
}

/// Turn a real, compiler-passing but neutral descendant into an editable
/// starting point for its successor. Rejection remains rejection: this never
/// changes the accepted HEAD or promotes an unmeasured patch.
/// A neutral inherited patch changes the disposable checkout *after* the
/// coordinator has indexed the pristine base. Source excerpts and old read
/// ranges therefore refer to the WRONG bytes for the descendant. Let the
/// descendant obtain a fresh source SHA from its actual inherited worktree.
/// Keep paired failure evidence, but never reuse stale source as edit context.
pub(super) fn generation_prompt_variants(
    original: &str, inherited: &str, digest: &str, focus: &str,
    neutral_id: Option<&str>,
) -> Vec<(&'static str, String, bool, bool)> {
    if let Some(id) = neutral_id {
        let note = format!(
            "\n\n### 실제 중립 소스 후보의 후손 교정\n후보 {id}의 컴파일 통과·독립 품질 무개선 패치가 이 세대 작업공간에 이미 적용됩니다. 채택된 패치는 아닙니다. 기존 기준 소스 발췌와 과거 읽기 범위의 줄 번호/SHA는 이 작업공간과 다를 수 있습니다. 먼저 workspace_read_range로 현재 함수 원문과 sourceSha256을 새로 읽으세요. 수정할 것은 이전에 실패한 행동이며, 부분적인 문법 수정이나 동일 패치 재제출이 아닙니다. 이전 후보를 보존한 채 실질적으로 다른 동작을 구현하고 검증하세요."
        );
        return vec![
            ("neutral_fresh_source", format!("{original}\n\n{inherited}{note}"), true, true),
            ("neutral_compact_fresh_source", format!("{original}{note}"), true, true),
        ];
    }
    vec![
        ("full", format!("{original}\n\n{inherited}{digest}{focus}"), false, false),
        ("source_on_demand", format!("{original}\n\n{inherited}{digest}"), true, false),
        ("research_on_demand", format!("{original}\n\n{inherited}"), true, true),
    ]
}

/// Development-only observations from the ACTUALLY RUNNING revision. No
/// replacement source, expected patch or confirmation examples are supplied.
/// The same probe is rerun in every rebuilt descendant before choosing a task.
pub fn diagnose_capabilities() -> Value {
    let cases = [
        ("파일을 만들고 이름을 알려줘", 2usize),
        ("문장을 고치고 결과를 보여줘", 2),
        ("캐시를 지우고 다시 시작해", 2),
    ];
    let observations=cases.iter().map(|(input,expected)| {
        let result=crate::engine::decompose::decompose(input,
            &json!({"implementation":"numbered-first","parameters":{"kernel":"numbered-first"}}));
        let observed=result["subtasks"].as_array().map_or(0,Vec::len);
        json!({"input":input,"expectedSegments":expected,"observedSegments":observed,
            "passed":observed==*expected})
    }).collect::<Vec<_>>();
    let failures=observations.iter().filter(|row|row["passed"]==false).count();
    json!({"version":1,"generatorCommit":option_env!("DOMINION_BUILD_COMMIT"),
        "domain":"tools","focusFiles":["src/engine/decompose.rs"],
        "failed":failures,"observations":observations})
}

/// Shared coordinator transition. Downstream evaluation executes this exact
/// transition too, so a candidate may improve either target binding or the
/// worker's interpretation of it. No evaluator-specific substitute selector.
pub(super) fn bind_inherited_target(input:&mut Value, next:&Value) {
    if let Some(domain)=next["nextDomain"].as_str().filter(|domain|DOMAINS.contains(domain)) {
        input["rsiSelectedDomain"]=json!(domain);
        input["rsiDecisionInput"]=next.clone();
    }else {
        input["rsiSelectedDomain"]=Value::Null;
        input["rsiDecisionInput"]=Value::Null;
    }
}
