//! feedback: isolated RSI worktree responsibility; no automatic production deployment.
use super::*;

/// Return a short, grounded repair instruction. Do not repeat blind model
/// calls for provider errors, missing diffs or checkout integrity failures.
pub fn repair_feedback(result:&Value)->Option<String>{
    if result["status"]!="needs_review"
        ||!matches!(result["reason"].as_str(),
            Some("build_or_typecheck_failed"|"unsafe_safety_failed"))
        ||result["rsi"]["sourceClean"]!=true
        ||!result["rsi"]["captureError"].as_str().unwrap_or("").is_empty()
        ||!result["rsi"]["modelError"].as_str().unwrap_or("").is_empty(){
        return None;
    }
    let checks=result["rsi"]["checks"].as_array()?;
    let failures=checks.iter().filter(|check|check["passed"]!=true)
        .map(|check|{
            let label=check["label"].as_str().unwrap_or("verification");
            let detail=check["primaryDiagnostic"].as_str()
                .filter(|value|!value.is_empty())
                .or_else(||check["stderr"].as_str())
                .or_else(||check["error"].as_str())
                .unwrap_or("The verifier reported failure without further detail.");
            format!("{label}: {}",detail.chars().take(1100).collect::<String>())
        }).collect::<Vec<_>>();
    if failures.is_empty(){return None;}
    Some(format!(
        "이전 소스 변경 후 별도의 컴파일/타입 검사에서 오류가 발견됐습니다. 기존 변경을 읽고 실제 오류를 고쳐 다시 저장하세요. 검사 자체를 삭제하거나 우회하지 마세요. 수정 전과 무관한 파일은 건드리지 마세요. 테스트는 엔진이 다시 실행합니다.\n\n{}",
        failures.join("\n\n")
    ))
}
/// The model attempted to modify a grounded source file but the native tool
/// rejected its old revision or an inappropriate create-only write. Turn that
/// observed failure into ONE targeted retry in the same detached worktree.
/// The identical receipt also trains the following generation's repair DAG.
pub fn failed_edit_feedback(
    result:&Value,session:&Value,operations:&[Value]
)->Option<String>{
    if result["status"]!="needs_review" || result["reason"]!="no_valid_candidate"
        || result["rsi"]["sourceClean"]!=true
        || result["rsi"]["diffBytes"].as_u64().unwrap_or(0)!=0
        || !result["rsi"]["modelError"].as_str().unwrap_or("").is_empty()
        || session["rsiRun"]!=true || session["rsiExplicitFocus"]!=true {return None}
    let focus=session["rsiFocusFiles"].as_array()?;
    if focus.len()!=1 {return None}
    let path=focus[0].as_str().filter(|path|crate::engine::rsi::safe_path(path))?;
    let failure=operations.iter().rev().find(|op|
        op["sessionId"]==session["id"] && op["arguments"]["path"]==path
        && matches!(op["status"].as_str(),Some("completed"|"failed"))
        && matches!(op["tool"].as_str(),Some("workspace_replace"|"workspace_replace_function"|"workspace_write"))
        && op["result"]["written"]!=true
        && (op["result"]["staleVersion"]==true
            || op["result"]["preflightBlocked"]==true
            || op["result"]["error"].as_str().is_some_and(|error|
                error.contains("새 파일 생성 전용"))))?;
    let old=failure["arguments"]["newText"].as_str()
        .or_else(||failure["arguments"]["content"].as_str())
        .unwrap_or("");
    let excerpt=old.chars().take(1400).collect::<String>();
    let error=failure["result"]["error"].as_str().unwrap_or("");
    let advice=if failure["tool"]=="workspace_write" {
        "기존 파일에 생성 전용 workspace_write를 다시 사용하지 마세요. 현재 파일을 읽고 필요한 범위만 workspace_replace 하세요."
    } else {
        "이전 expectedSha256 또는 파일 관측이 오래됐습니다. 현재 파일의 정확한 변경 범위를 workspace_read_range로 다시 읽고 그 응답의 sourceSha256를 사용하세요."
    };
    Some(format!(
        "실제 도구 호출이 실패하여 소스 diff가 0건입니다. 실패했던 파일: {path}. \
        엔진이 기록한 이유: {}. {advice} \
        이미 의도한 수정 내용:\n~~~\n{excerpt}\n~~~\n\
        정확한 현재 소스 원문과 비교해 여전히 유효할 때만 120줄 이내 수정으로 적용하세요. \
        도구 실행이 끝나야 결과가 기록됩니다. 더 이상의 조사 반복으로 끝내지 마세요.",
        error.chars().take(380).collect::<String>()
    ))
}

/// Convert a grounded but read-only RSI model turn into a *single* concrete
/// follow-up edit attempt. Previously run_prompt only retried failed builds;
/// a generation that returned with zero writes was guaranteed to stop with
/// no_valid_candidate even after a verified source was already supplied.
/// This function is intentionally a decision algorithm, not an automatic
/// patch: the provider still authors the source change, and the detached
/// worktree still owns compile, diff capture and independent evaluation.
pub fn no_edit_feedback(
    result:&Value, session:&Value, operations:&[Value]
)->Option<String>{
    if result["status"]!="needs_review"
        || result["reason"]!="no_valid_candidate"
        || result["rsi"]["sourceClean"]!=true
        || result["rsi"]["diffBytes"].as_u64().unwrap_or(0)!=0
        || !result["rsi"]["modelError"].as_str().unwrap_or("").is_empty()
        || session["rsiRun"]!=true || session["rsiExplicitFocus"]!=true {return None}
    let focused=session["rsiFocusFiles"].as_array()?;
    if focused.len()!=1 {return None}
    let path=focused[0].as_str().filter(|p|crate::engine::rsi::safe_path(p))?;
    let own=operations.iter().filter(|op|op["sessionId"]==session["id"])
        .collect::<Vec<_>>();
    // Failed source changes are for the next failure-specific repair method,
    // not a second speculative "no edit" call in this same generation.
    if own.iter().any(|op|matches!(op["tool"].as_str(),
        Some("workspace_replace"|"workspace_replace_function"|"workspace_write"))) {return None}
    let grounded=own.iter().rev().find(|op|
        op["status"]=="completed" && op["tool"]=="workspace_read_range"
        && op["arguments"]["path"]==path && op["result"]["error"].is_null()
        && op["result"]["sourceSha256"].as_str().is_some_and(|s|
            s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit())));
    let indexed=(session["rsiPolicyFocus"]==true
            || session["rsiNoEditGroundedTarget"]==true)
        && session["rsiFocusContextReady"]==true
        && session["rsiCodeIndexId"].as_str().is_some_and(|s|!s.is_empty());
    if grounded.is_none() && !indexed {return None}
    let source=grounded.and_then(|op|{
        let body=op["result"]["content"].as_str()
            .or_else(||op["result"]["text"].as_str())?;
        let sha=op["result"]["sourceSha256"].as_str()?;
        Some(format!("\n최근 검증된 원문:\nsourceSha256: {sha}\n~~~\n{}\n~~~\n",
            body.chars().take(2600).collect::<String>()))
    }).unwrap_or_default();
    Some(format!(
        "이전 실행은 이미 검증된 소스를 관찰했지만 실제 편집을 수행하지 않아 후보가 0건입니다. \
         탐색을 반복하는 대신 이번 턴에는 파일 {path}에서 목표에 맞는 작고 구체적인 코드 수정을 완성하세요. \
         이미 제공된 소스 원문과 SHA를 사용하고, 추가 확인이 꼭 필요한 경우 그 범위만 읽으세요. \
         120줄 이내의 기존 파일 변경은 workspace_replace의 startLine/endLine/newText/expectedSha256 \
         인자로 실행하세요. 예상 SHA를 모르면 현재 파일을 읽어 확인하세요. \
         실행 가능한 코드 변화가 없다면 억지 패치를 만들지 말고 이유를 설명하세요. \
         이 호출은 최대 한 번이며, 성공 보상은 소스 diff 및 독립 평가 이후에만 주어집니다.{source}"
    ))
}

/// A restored failed source patch creates a nonempty git diff *before* the
/// descendant model acts. Do not use total diff bytes as a proxy for an edit.
/// Give the descendant one concrete correction attempt when its output is
/// byte-identical to the rejected parent patch, grounded on compiler evidence
/// and the actual restored source. This does not manufacture a new candidate.
pub fn inherited_unchanged_feedback(
    result:&Value,session:&Value,failed_patch:&Value,operations:&[Value]
)->Option<String>{
    if result["status"]!="needs_review"
        || result["reason"]!="no_valid_candidate"
        || result["rsi"]["inheritedPatchUnchanged"]!=true
        || result["rsi"]["sourceClean"]!=true
        || result["rsi"]["candidateId"].is_string()
        || !result["rsi"]["modelError"].as_str().unwrap_or("").is_empty()
        || session["rsiRun"]!=true
        || session["rsiRecoveredPatch"]["status"]!="restored_unverified"
        || session["rsiRecoveredPatch"]["candidateId"]!=failed_patch["candidateId"]{
        return None
    }
    let paths=session["rsiRecoveredPatch"]["paths"].as_array()?;
    let path=paths.iter().filter_map(Value::as_str)
        .find(|path|crate::engine::rsi::safe_path(path)
            && session["rsiFocusFiles"].as_array().is_some_and(|focus|
                focus.iter().any(|file|file.as_str()==Some(*path))))?;
    let diagnostic=failed_patch["checks"].as_array()
        .into_iter().flatten()
        .filter_map(|check|check["diagnostic"].as_str())
        .find(|text|text.contains(path))
        .or_else(||failed_patch["checks"].as_array()
            .into_iter().flatten()
            .filter_map(|check|check["diagnostic"].as_str())
            .next()).unwrap_or("");
    let error=operations.iter().rev().find(|op|
        op["sessionId"]==session["id"]
        && op["arguments"]["path"]==path
        && matches!(op["tool"].as_str(),Some("workspace_replace"|"workspace_replace_function"|"workspace_write"))
        && op["result"]["written"]!=true)
        .and_then(|op|op["result"]["error"].as_str())
        .unwrap_or("");
    Some(format!(
        "이번 후손은 부모의 실패 패치를 복원했지만 실제로 새 코드를 수정하지 않았습니다. \
         부모의 Git diff를 새로운 성과로 제출할 수 없습니다. \
         작업 대상: {path}. 최초 컴파일 진단: {}. \
         이 세대에서 관측된 도구 실패: {}. \
         현재 복원된 파일을 workspace_read_range로 읽어 최신 sourceSha256와 정확한 줄 범위를 얻으세요. \
         그 후 workspace_replace(startLine,endLine,expectedSha256,newText)로 \
         컴파일 오류를 해결하는 *새 수정*을 실행하세요. 기존 실패 패치를 그대로 재적용하지 마세요. \
         한 번만 재시도하며 결과는 독립 검증합니다.",
        diagnostic.chars().take(900).collect::<String>(),
        error.chars().take(350).collect::<String>()
    ))
}

