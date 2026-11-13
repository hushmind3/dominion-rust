//! failed_edit: executable, independently observed RSI method-learning responsibilities.
use super::*;

/// edit*, without requiring the model to have first written a procedure
/// definition. A zero-diff generation can therefore teach the next
/// generation a different executable source-edit program. Its code change
/// remains the model's observed intention, not a fabricated patch.
pub(crate) fn synthesize_failed_model_edit(
    session:&Value,op:&Value,generation:&Value
)->Option<Value>{
    if session["rsiRun"]!=true
        || session["rsiGenerationJobId"]!=generation["jobId"]
        || op["sessionId"]!=session["id"]
        || op["origin"]=="procedure"
        || op["tool"]!="workspace_replace"
        || !matches!(op["status"].as_str(),Some("completed"|"failed"))
        || (op["result"]["staleVersion"]!=true
            && op["result"]["preflightBlocked"]!=true) {return None}
    let path=op["arguments"]["path"].as_str()
        .filter(|path|crate::engine::rsi::safe_path(path))?;
    if !session["rsiFocusFiles"].as_array().is_some_and(|paths|
        paths.iter().any(|item|item.as_str()==Some(path))){return None}
    let start=op["arguments"]["startLine"].as_u64()
        .or_else(||op["arguments"]["startLine"].as_str()?.parse::<u64>().ok())?;
    let end=op["arguments"]["endLine"].as_u64()
        .or_else(||op["arguments"]["endLine"].as_str()?.parse::<u64>().ok())?;
    if start==0 || end<start || end-start>=120 {return None}
    if op["arguments"]["oldText"].as_str().is_some_and(|s|!s.is_empty()) {return None}
    let new_text=op["arguments"]["newText"].as_str()
        .filter(|text|!text.trim().is_empty() && text.chars().count()<=2800)?;
    let objective=session["rsiObjective"].as_str()
        .filter(|s|!s.trim().is_empty())?;
    // The reuse engine uses requestContains rather than a file-path guess to
    // determine whether a learned program applies to the next objective.
    let applicability=objective.chars().take(100).collect::<String>();
    let fingerprint=format!("{:x}",Sha256::digest(
        json!([op["id"],generation["jobId"],path,start,end,new_text]).to_string().as_bytes()));
    let read_id=format!("observe_{}",&fingerprint[..12]);
    let edit_id=format!("apply_{}",&fingerprint[..12]);
    let program=crate::engine::procedure::normalize(&json!({
        "type":"procedure",
        "appliesTo":{"requestContains":[applicability]},
        "nodes":[
            {"id":read_id,"tool":"workspace_read_range",
                "arguments":{"path":path,"startLine":start,"endLine":end}},
            {"id":edit_id,"tool":"workspace_replace",
                "arguments":{"path":path,"startLine":start,"endLine":end,
                    "expectedSha256":format!("{}{}.sourceSha256",char::from(36),read_id),
                    "newText":new_text}}
        ],
        "edges":[{"from":read_id,"to":edit_id}],
        "after":[{"kind":"file_contains","path":path,
            "text":new_text.trim()}]
    }));
    if program["type"]!="procedure" {return None}
    let domain=focus_domain_hint(path);
    let revision=format!("{:x}",Sha256::digest(
        json!([domain,&program]).to_string().as_bytes()));
    Some(json!({
        "id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("Observed model edit · {}",path.chars().take(65).collect::<String>()),
        "domain":domain,"verification":program,
        "status":"candidate","assistanceMode":"harness_assisted",
        "successfulApplications":0,
        "synthesis":{"kind":"failed_model_tool_rebase",
            "failedOperationId":op["id"],"sourceGenerationJobId":generation["jobId"],
            "observedModelIntent":true,"qualityGain":Value::Null,
            "causalProof":"awaiting_descendant_tool_and_paired_evaluation"}
    }))
}

/// Real small-model traces may contain a reversed pair of *absolute* source
/// line endpoints. Unlike inventing a missing oldText or guessing a relative
/// line count, sorting both witnessed endpoints describes the identical line
/// set. Learn a read -> version-bound edit DAG from the failed operation, and
/// independently test it in a later descendant. Never grant improvement credit
/// for constructing the candidate.
pub(crate) fn synthesize_failed_reversed_span(
    session:&Value, op:&Value, generation:&Value
)->Option<Value> {
    if session["rsiRun"]!=true
        || session["rsiGenerationJobId"]!=generation["jobId"]
        || op["sessionId"]!=session["id"]
        || op["origin"]=="procedure"
        || op["tool"]!="workspace_replace"
        || !matches!(op["status"].as_str(),Some("completed"|"failed")) {
        return None;
    }
    let error=op["result"]["error"].as_str().or_else(||op["error"].as_str())?;
    if !error.contains("endLine") || !error.contains("startLine") {return None}
    let path=op["arguments"]["path"].as_str()
        .filter(|path|crate::engine::rsi::safe_path(path))?;
    if !session["rsiFocusFiles"].as_array().is_some_and(|files|
        files.iter().any(|file|file.as_str()==Some(path))) {return None}
    if op["arguments"]["oldText"].as_str().is_some_and(|s|!s.is_empty()) {
        return None;
    }
    let coordinate=|key:&str|op["arguments"][key].as_u64()
        .or_else(||op["arguments"][key].as_str()?.parse::<u64>().ok());
    let raw_start=coordinate("startLine")?;
    let raw_end=coordinate("endLine")?;
    if raw_end==0 || raw_start<=raw_end || raw_start-raw_end>=120 {return None}
    let new_text=op["arguments"]["newText"].as_str()
        .filter(|text|!text.trim().is_empty() && text.chars().count()<=2800)?;
    let objective=session["rsiObjective"].as_str()
        .filter(|text|!text.trim().is_empty())?;
    let fingerprint=format!("{:x}",Sha256::digest(json!([
        op["id"],generation["jobId"],path,raw_start,raw_end,
        new_text,"reordered-absolute-endpoints-v1"
    ]).to_string().as_bytes()));
    let read_id=format!("observe_{}",&fingerprint[..12]);
    let edit_id=format!("apply_{}",&fingerprint[..12]);
    let program=crate::engine::procedure::normalize(&json!({
        "type":"procedure",
        "appliesTo":{"requestContains":[objective.chars().take(100).collect::<String>()]},
        "nodes":[
            {"id":read_id,"tool":"workspace_read_range",
                "arguments":{"path":path,"startLine":raw_end,"endLine":raw_start}},
            {"id":edit_id,"tool":"workspace_replace",
                "arguments":{"path":path,"startLine":raw_end,"endLine":raw_start,
                    "expectedSha256":format!("{}{}.sourceSha256",char::from(36),read_id),
                    "newText":new_text}}
        ],
        "edges":[{"from":read_id,"to":edit_id}],
        "after":[{"kind":"file_contains","path":path,"text":new_text.trim()}]
    }));
    if program["type"]!="procedure" {return None}
    let domain=focus_domain_hint(path);
    let revision=format!("{:x}",Sha256::digest(
        json!([domain,&program]).to_string().as_bytes()));
    Some(json!({
        "id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("Observed model reversed-span · {}",
            path.chars().take(60).collect::<String>()),
        "domain":domain,"verification":program,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"failed_model_reversed_span",
            "failedOperationId":op["id"],
            "sourceGenerationJobId":generation["jobId"],
            "originalStartLine":raw_start,"originalEndLine":raw_end,
            "derivedStartLine":raw_end,"derivedEndLine":raw_start,
            "operator":"sort_absolute_endpoints_then_bind_fresh_source_hash",
            "qualityGain":Value::Null,
            "causalProof":"awaiting_independent_descendant_application"}
    }))
}

