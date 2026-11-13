//! repairs: executable, independently observed RSI method-learning responsibilities.
use super::*;

/// A repair discovered from a failed procedure is an executable program, not
/// a suggested prompt. The candidate receives no improvement credit until
/// its own future application passes the ordinary procedure evaluator.
pub(crate) fn is_synthesized_source_repair(method:&Value)->bool {
    matches!(method["synthesis"]["kind"].as_str(),
        Some("stale_source_rebase"|"indexed_source_rebase"|"existing_file_to_versioned_edit"|"indexed_path_retarget"|"successful_read_ablation"|"transferred_read_ablation"|"successful_observation_pruning"|"transferred_observation_pruning"|"failed_model_tool_rebase"|"failed_model_reversed_span"))
}

/// Synthesize a new executable procedure, not just an action ranking.
/// A witnessed stale source edit gains a read node, a DAG dependency and a
/// dynamically bound fresh hash; acceptance requires later real execution.
pub(crate) fn synthesize_stale_edit_repair(method:&Value, op:&Value)->Option<Value> {
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || op["origin"]!="procedure" || op["tool"]!="workspace_replace"
        || op["status"]!="failed" || op["result"]["staleVersion"]!=true
        || op["procedureStrategyId"]!=method["id"]
        || op["procedureRevision"]!=method["revision"] {return None}
    let path=op["arguments"]["path"].as_str()?;
    if !crate::engine::rsi::safe_path(path) {return None}
    let node_id=op["procedureNodeId"].as_str()?;
    let nodes=method["verification"]["nodes"].as_array()?;
    if nodes.is_empty()||nodes.len()>=16 {return None}
    let node=nodes.iter().find(|n|n["id"]==node_id && n["tool"]=="workspace_replace"
        && n["arguments"]["path"]==path)?;
    if !node["arguments"]["newText"].is_string()
        || node["arguments"]["expectedSha256"].as_str()
            .is_some_and(|s|s.starts_with(char::from(36))) {return None}
    let text_mode=node["arguments"]["oldText"].as_str()
        .is_some_and(|s|!s.is_empty());
    // A precise oldText edit can also fail because its whole-file hash is
    // stale. Re-read a single line for the fresh hash while retaining
    // exact-text uniqueness checks in the actual tool executor.
    let (start,end)=if text_mode {
        if node["arguments"]["oldText"]!=op["arguments"]["oldText"] {return None}
        (1,1)
    } else {
        let coordinate=|v:&Value|v.as_u64()
            .or_else(||v.as_str()?.parse::<u64>().ok());
        let start=coordinate(&node["arguments"]["startLine"])?;
        let end=coordinate(&node["arguments"]["endLine"])?;
        if start==0 || end<start || end-start>=120
            || coordinate(&op["arguments"]["startLine"])?!=start
            || coordinate(&op["arguments"]["endLine"])?!=end {return None}
        (start,end)
    };
    let fingerprint=format!("{:x}",Sha256::digest(
        json!([method["revision"],node_id,"stale-rebase-v1"]).to_string().as_bytes()));
    let read_id=format!("source_rebase_{}",&fingerprint[..12]);
    if nodes.iter().any(|n|n["id"]==read_id) {return None}
    let mut program=method["verification"].clone();
    program["nodes"].as_array_mut()?.insert(0,json!({
        "id":read_id,"tool":"workspace_read_range",
        "arguments":{"path":path,"startLine":start,"endLine":end}
    }));
    let replace=program["nodes"].as_array_mut()?.iter_mut()
        .find(|n|n["id"]==node_id)?;
    replace["arguments"]["expectedSha256"]=json!(format!(
        "{}{}.sourceSha256",char::from(36),read_id));
    if !program["edges"].is_array() {program["edges"]=json!([])}
    program["edges"].as_array_mut()?.push(json!({"from":read_id,"to":node_id}));
    let program=crate::engine::procedure::normalize(&program);
    if program["type"]!="procedure" {return None}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(json!([domain,&program]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · source-rebase",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":program,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"stale_source_rebase",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"changedNodeId":node_id,
            "newReadNodeId":read_id}}))
}

/// Competing repair operator for a stale *line-mode* edit. Instead of adding
/// an extra read, permit the RSI runtime to recover the version from its
/// freshly indexed source. This is allowed only for version-checked line mode:
/// the tool itself refuses edits without an indexed SHA or on a mismatch.
/// The cheaper method and read-rebase method are independently evaluated by
/// their subsequent applications rather than both editing the same source.
pub(crate) fn synthesize_indexed_edit_repair(method:&Value,op:&Value)->Option<Value> {
    let _validated=synthesize_stale_edit_repair(method,op)?;
    let node_id=op["procedureNodeId"].as_str()?;
    let nodes=method["verification"]["nodes"].as_array()?;
    let node=nodes.iter().find(|node|node["id"]==node_id)?;
    if node["arguments"]["oldText"].as_str().is_some_and(|s|!s.is_empty()){
        return None;
    }
    let mut program=method["verification"].clone();
    let target=program["nodes"].as_array_mut()?.iter_mut()
        .find(|node|node["id"]==node_id)?;
    // A null hash in line mode is resolved exclusively against the bound
    // repository index; there is no analogous safe fallback for oldText mode.
    target["arguments"]["expectedSha256"]=Value::Null;
    let program=crate::engine::procedure::normalize(&program);
    if program["type"]!="procedure"{return None}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(
        json!([domain,&program]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · indexed-rebase",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":program,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"indexed_source_rebase",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"changedNodeId":node_id}}))
}

/// A failed attempt to overwrite an existing source file exposes a reusable
/// execution-method transformation: create-only -> versioned replacement.
/// Subsequent execution re-reads the source hash and observed line count.
/// The ordinary 120-line limit still prevents oversized source overwrites.
pub(crate) fn synthesize_existing_file_write_repair(method:&Value,op:&Value)->Option<Value> {
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || op["origin"]!="procedure" || op["tool"]!="workspace_write"
        || op["status"]!="failed"
        || op["procedureStrategyId"]!=method["id"]
        || op["procedureRevision"]!=method["revision"] {return None}
    let error=op["result"]["error"].as_str()
        .or_else(||op["error"].as_str())?;
    if !error.contains("workspace_write") || !error.contains("새 파일 생성 전용") {
        return None;
    }
    let path=op["arguments"]["path"].as_str()?;
    if !crate::engine::rsi::safe_path(path) {return None}
    let node_id=op["procedureNodeId"].as_str()?;
    let nodes=method["verification"]["nodes"].as_array()?;
    if nodes.is_empty() || nodes.len()>=16 {return None}
    let node=nodes.iter().find(|node|node["id"]==node_id
        && node["tool"]=="workspace_write"
        && node["arguments"]["path"]==path
        && node["arguments"]["content"]==op["arguments"]["content"])?;
    // The program interpreter limits any argument string to 4096 chars.
    let content=node["arguments"]["content"].as_str()
        .filter(|text|!text.is_empty() && text.chars().count()<=4096)?;
    // A create-only failure is not permission to destroy an existing source.
    // Only a method that already specified exact full-file equality can be
    // transformed into a versioned whole-file rewrite. File-contains alone
    // cannot establish that removing the rest of the file is intentional.
    if !method["verification"]["after"].as_array()?.iter().any(|check|
        check["kind"]=="file_equals" && check["path"]==path
            && check["text"]==content) {return None}
    let fingerprint=format!("{:x}",Sha256::digest(
        json!([method["revision"],node_id,"existing-file-v1"]).to_string().as_bytes()));
    let read_id=format!("source_rebase_{}",&fingerprint[..12]);
    if nodes.iter().any(|node|node["id"]==read_id){return None}
    let mut program=method["verification"].clone();
    program["nodes"].as_array_mut()?.insert(0,json!({
        "id":read_id,"tool":"workspace_read_range",
        "arguments":{"path":path,"startLine":1,"endLine":120}
    }));
    let changed=program["nodes"].as_array_mut()?.iter_mut()
        .find(|node|node["id"]==node_id)?;
    changed["tool"]=json!("workspace_replace");
    changed["arguments"]=json!({
        "path":path,"startLine":1,
        "endLine":format!("{}{}.totalLines",char::from(36),read_id),
        "expectedSha256":format!("{}{}.sourceSha256",char::from(36),read_id),
        "newText":content
    });
    // Change the matching postcondition too; an unchanged tool_called check
    // would make the reconstructed program impossible to satisfy.
    for after in program["after"].as_array_mut()? {
        if after["kind"]=="tool_called" && after["tool"]=="workspace_write"
            && after["path"]==path {
            after["tool"]=json!("workspace_replace");
        }
    }
    if !program["edges"].is_array(){program["edges"]=json!([])}
    program["edges"].as_array_mut()?.push(json!({"from":read_id,"to":node_id}));
    let program=crate::engine::procedure::normalize(&program);
    if program["type"]!="procedure" {return None}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(json!([domain,&program]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · versioned-edit",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":program,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"existing_file_to_versioned_edit",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"changedNodeId":node_id,
            "newReadNodeId":read_id}}))
}

/// A previously valid source path can disappear when a module moves. A failed
/// read is enough to search the current generation's *own* repository index
/// for exactly one file with the same basename. Rewrite the procedure DAG,
/// its postconditions and the dependent edit's SHA binding. Ambiguous paths
/// produce no candidate: the engine never guesses which file to mutate.
pub(crate) fn synthesize_indexed_path_repair(
    state:&AppState,method:&Value,op:&Value,generation:&Value
)->Option<Value> {
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || op["origin"]!="procedure" || op["tool"]!="workspace_read_range"
        || op["status"]!="failed"
        || op["procedureStrategyId"]!=method["id"]
        || op["procedureRevision"]!=method["revision"] {return None}
    let error=op["result"]["error"].as_str()
        .or_else(||op["error"].as_str())?;
    if !error.contains("No such file") && !error.contains("os error 2")
        && !error.contains("파일이나 디렉터리가 없습니다") {return None}
    let old=op["arguments"]["path"].as_str()?;
    if !crate::engine::rsi::safe_path(old) {return None}
    let read_id=op["procedureNodeId"].as_str()?;
    let nodes=method["verification"]["nodes"].as_array()?;
    if !nodes.iter().any(|node|node["id"]==read_id
        && node["tool"]=="workspace_read_range"
        && node["arguments"]["path"]==old) {return None}
    // Preserve the precise source partition; never apply a similarly named
    // file from another indexed repository or a stale global index.
    let job=state.store.get("job",generation["jobId"].as_str()?)?;
    let index_id=job["input"]["rsiIndexId"].as_str()?;
    let index=state.store.get("code_index",index_id)?;
    if index["sourceDir"]!=job["input"]["rsiSourceDir"] {return None}
    let filename=std::path::Path::new(old).file_name()?.to_str()?;
    let mut matching=state.store.all("code_index_file").into_iter()
        .filter(|file|file["indexId"].as_str()==Some(index_id))
        .filter_map(|file|file["path"].as_str().map(str::to_string))
        .filter(|path|path.as_str()!=old && crate::engine::rsi::safe_path(path)
            && std::path::Path::new(path).file_name()
                .and_then(|name|name.to_str())==Some(filename))
        .collect::<Vec<_>>();
    matching.sort();
    matching.dedup();
    if matching.len()!=1 {return None}
    let target=&matching[0];
    let mut program=method["verification"].clone();
    let mut edited=Vec::new();
    for node in program["nodes"].as_array_mut()? {
        if node["arguments"]["path"]!=old {continue}
        // Clone only the tool tag: mutation below must not alias a borrowed
        // string field in the same serde_json::Value.
        let tool=node["tool"].as_str().map(str::to_string);
        match tool.as_deref() {
            Some("workspace_read_range")=>{
                node["arguments"]["path"]=json!(target);
            },
            Some("workspace_replace")=>{
                node["arguments"]["path"]=json!(target);
                node["arguments"]["expectedSha256"]=json!(format!(
                    "{}{}.sourceSha256",char::from(36),read_id));
                if let Some(id)=node["id"].as_str(){edited.push(id.to_string());}
            },
            _=>return None
        }
    }
    if !program["edges"].is_array(){program["edges"]=json!([])}
    for id in edited {
        if id==read_id {return None}
        let edges=program["edges"].as_array_mut()?;
        if !edges.iter().any(|edge|edge["from"]==read_id && edge["to"]==id) {
            edges.push(json!({"from":read_id,"to":id}));
        }
    }
    for after in program["after"].as_array_mut()? {
        if after["path"]==old {after["path"]=json!(target);}
    }
    for before in program["before"].as_array_mut().into_iter().flatten() {
        for key in ["when","require"] {
            if before[key]["path"]==old {before[key]["path"]=json!(target);}
        }
    }
    let program=crate::engine::procedure::normalize(&program);
    if program["type"]!="procedure" {return None}
    let domain=method["domain"].as_str()?;
    let revision=format!("{:x}",Sha256::digest(
        json!([domain,&program]).to_string().as_bytes()));
    Some(json!({"id":format!("rsi-method-{revision}"),"revision":revision,
        "name":format!("{} · indexed-retarget",method["name"].as_str()
            .unwrap_or("Recovered procedure").chars().take(70).collect::<String>()),
        "domain":domain,"verification":program,"status":"candidate",
        "assistanceMode":"harness_assisted","successfulApplications":0,
        "synthesis":{"kind":"indexed_path_retarget",
            "parentMethodId":method["id"],"parentRevision":method["revision"],
            "failedOperationId":op["id"],"sourceIndexId":index_id,
            "oldPath":old,"targetPath":target,"changedReadNodeId":read_id}}))
}

/// A method is associated with an adopted source improvement only when its
