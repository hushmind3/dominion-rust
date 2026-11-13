//! Evidence binding from compiled operator selection to an actual model edit.
//! The frozen paired evaluator still owns all success decisions.
use crate::{engine::{code_index, rsi, rsi_improvement_algebra},state::AppState};
use serde_json::{json,Value};

fn sha(s:&str,n:usize)->bool{
    s.len()==n && s.bytes().all(|c|c.is_ascii_hexdigit())
}
/// A pre-model-call dispatch receipt, emitted only by the rebuilt child
/// when this operator (not an overriding planner) controls its edit target.
pub(crate) fn dispatch(state:&AppState,job:&Value,commit:&str,active:bool)
    ->Result<Value,String> {
    if !active{return Ok(Value::Null)}
    let plan=&job["input"]["rsiImprovementAlgebra"];
    let selected=&plan["selected"];
    let rule=&selected["rule"];
    let id=selected["ruleId"].as_str().filter(|s|sha(s,64))
        .ok_or("후손 개선 규칙 ID 없음")?;
    let family=rule["methodFamilyId"].as_str().filter(|s|sha(s,64))
        .ok_or("후손 개선법 계보 없음")?;
    if rule["id"]!=id
        || rsi_improvement_algebra::method_family_id(rule).as_deref()!=Some(family)
        || plan["generatorCommit"]!=commit
        || job["input"]["rsiControllerPlan"]["improvementAlgebra"]!=*plan
        || job["input"]["rsiArchitectureTarget"]!=selected["target"] {
        return Err("실행 규칙·개선법·계획의 소스 계보가 불일치합니다.".into())
    }
    let target=selected["target"]["path"].as_str()
        .filter(|p|rsi::safe_path(p)).ok_or("개선 대상 파일 없음")?;
    let target_sha=selected["target"]["sourceSha256"].as_str()
        .filter(|s|sha(s,64)).ok_or("개선 대상 원본 해시 없음")?;
    let method=rule["method"]["path"].as_str()
        .filter(|p|rsi::safe_path(p)).ok_or("실제 개선법 소스 없음")?;
    let method_sha=rule["method"]["sourceSha256"].as_str()
        .filter(|s|sha(s,64)).ok_or("실제 개선법 원본 해시 없음")?;
    let index=job["input"]["rsiIndexId"].as_str()
        .ok_or("소스 인덱스 없음")?;
    let focus=job["input"]["rsiFocusFiles"].as_array()
        .and_then(|v|v.first()).and_then(Value::as_str);
    if focus!=Some(target)
        || job["input"]["rsiSelectedDomain"]!=selected["target"]["domain"]
        || code_index::indexed_source_hash(state,index,target).as_deref()!=Some(target_sha)
        || code_index::indexed_source_hash(state,index,method).as_deref()!=Some(method_sha) {
        return Err("실행 후손의 실제 편집 대상·개선법 해시가 다릅니다.".into())
    }
    Ok(json!({"source":"compiled_descendant_operator_dispatch_v1",
        "generationJobId":job["id"],"generatorCommit":commit,
        "ruleId":id,"methodFamilyId":family,"methodPath":method,
        "methodSourceSha256":method_sha,"targetPath":target,
        "targetSourceSha256":target_sha,"sourceIndexId":index,
        "qualityProven":false}))
}
/// Read the original worker, candidate, provider and operation ledger.
/// A reported plan or model-generated JSON does not constitute execution.
pub(crate) fn observe(state:&AppState,g:&Value)->Value{
    let Some(job_id)=g["jobId"].as_str()else{return Value::Null};
    let Some(job)=state.store.get("job",job_id)else{return Value::Null};
    let d=&job["rsiOperatorDispatch"];
    if d["source"]!="compiled_descendant_operator_dispatch_v1"
        || d["generationJobId"]!=job_id || d["generatorCommit"]!=g["generatorCommit"]
        || d["qualityProven"]!=false
        || job["rsiCompiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || job["rsiCompiledPlanExecuted"]["generatorCommit"]!=g["generatorCommit"]
        || g["compiledPlanExecuted"]!=job["rsiCompiledPlanExecuted"]
        || g["improvementAlgebraPlan"]!=job["input"]["rsiImprovementAlgebra"]
        || g["controllerPlan"]!=job["input"]["rsiControllerPlan"]
        || g["improvementAlgebraPlan"]["selected"]["ruleId"]!=d["ruleId"]
        || g["improvementAlgebraPlan"]["selected"]["target"]["path"]!=d["targetPath"]
        || g["improvementAlgebraPlan"]["selected"]["rule"]["method"]["sourceSha256"]
            !=d["methodSourceSha256"]
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || !rsi::model_route_audited(state,g){return Value::Null}
    // Replay the source-index identity, rather than trusting a stored
    // dispatch that could have been recorded before a superseding choice.
    if dispatch(state,&job,g["generatorCommit"].as_str().unwrap_or(""),true)
        .ok().as_ref()!=Some(d){return Value::Null}
    let Some(target)=d["targetPath"].as_str().filter(|p|rsi::safe_path(p))
        else{return Value::Null};
    let Some(candidate_id)=g["candidateId"].as_str()else{return Value::Null};
    let Some(candidate)=state.store.get("rsi_candidate",candidate_id)
        else{return Value::Null};
    let Some(session)=job["sessionId"].as_str()else{return Value::Null};
    if candidate["baseCommit"]!=g["generatorCommit"]
        || candidate["rsiJobId"]!=job_id
        || candidate["modelSessionId"]!=session
        || candidate["paths"]!=g["changedSourcePaths"]
        || !candidate["paths"].as_array().is_some_and(|v|v.iter().any(|p|p==target))
        || job["input"]["rsiFocusFiles"].as_array()
            .and_then(|v|v.first()).and_then(Value::as_str)!=Some(target) {
        return Value::Null
    }
    // Search the complete durable tool ledger, not its truncated UI summary.
    let mut operations=state.store.all("execution_operation").into_iter().filter(|op|
        op["sessionId"]==session && op["origin"]=="model"
        && op["arguments"]["path"]==target && op["status"]=="completed"
        && op["result"]["written"]==true && op["toolReceipt"]["verified"]==true
        && op["result"]["postSha256"].as_str().is_some_and(|s|sha(s,64))
        && matches!(op["tool"].as_str(),
            Some("workspace_replace"|"workspace_replace_range"|"workspace_write")))
        .collect::<Vec<_>>();
    operations.sort_by(|a,b|a["startedAt"].as_str().cmp(&b["startedAt"].as_str())
        .then_with(||a["opSeq"].as_u64().cmp(&b["opSeq"].as_u64())));
    let Some(op)=operations.last()else{return Value::Null};
    // An adopted method must own the FINAL bytes, not merely an earlier edit
    // later overwritten by a repair or unrelated tool.
    if g["accepted"]==true
        && g["acceptedSourceHashes"][target]!=op["result"]["postSha256"] {
        return Value::Null;
    }
    json!({"source":"verified_compiled_operator_tool_edit_v1",
        "generationJobId":job_id,"generatorCommit":g["generatorCommit"],
        "candidateId":candidate_id,"sessionId":session,
        "ruleId":d["ruleId"],"methodFamilyId":d["methodFamilyId"],
        "methodSourceSha256":d["methodSourceSha256"],
        "targetPath":target,"targetSourceSha256":d["targetSourceSha256"],
        "algebraRevision":g["improvementAlgebraPlan"]["sha256"],
        "toolRunId":op["runId"],"toolOpSeq":op["opSeq"],
        "toolPostSha256":op["result"]["postSha256"],
        "modelEditObserved":true,"qualityProven":false})
}
/// Fully recheck attribution before independently replayed paired outcomes
/// can update an operator's future source-edit priority.
pub(crate) fn verified(state:&AppState,g:&Value,rule:&Value)->bool{
    let observed=observe(state,g);
    observed.is_object() && g["operatorExecution"]==observed
        && rsi_improvement_algebra::method_family_id(rule).as_deref()
            ==observed["methodFamilyId"].as_str()
        && g["improvementAlgebraPlan"]["selected"]["ruleId"]==observed["ruleId"]
}

#[cfg(test)]
#[path = "rsi_operator_provenance/tests.rs"]
mod tests;
