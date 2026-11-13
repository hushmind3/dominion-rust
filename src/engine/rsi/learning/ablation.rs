//! ablation: executable, independently observed RSI method-learning responsibilities.
use super::*;

/// own successful write appears in the verified candidate's changed paths.
/// A generation can succeed because some *other* model/tool edited the source;
/// that event must not train an unrelated method as a successful descendant.
/// This is effect attribution, not yet causal paired evaluation of the method.
pub(super) fn method_wrote_accepted_candidate(
    state:&AppState,generation:&Value,receipt:&Value,operations:&[Value]
)->bool {
    if generation["accepted"]!=true {return false}
    let Some(candidate)=generation["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{return false};
    if candidate["status"]!="verified"
        || candidate["evaluation"]["metrics"]["eligible"]!=true {return false}
    let Some(paths)=candidate["paths"].as_array() else{return false};
    let Some(operation_ids)=receipt["operationIds"].as_array() else{return false};
    operations.iter().filter(|op|
        operation_ids.contains(&op["id"])
        && op["origin"]=="procedure"
        && op["procedureStrategyId"]==receipt["methodId"]
        && op["procedureRevision"]==receipt["revision"]
        && matches!(op["tool"].as_str(),Some("workspace_replace"|"workspace_replace_function"|"workspace_write"))
        && crate::engine::procedure::operation_succeeded(op)
        && op["result"]["written"]==true)
        .any(|op|{
            let Some(path)=op["arguments"]["path"].as_str() else{return false};
            if !paths.iter().any(|item|item.as_str()==Some(path)){return false}
            let Some(adopted_hash)=generation["acceptedSourceHashes"][path].as_str()
                else{return false};
            let actual_post=op["result"]["postSha256"].as_str().map(str::to_string)
                .or_else(||if op["tool"]=="workspace_write"{
                    op["arguments"]["content"].as_str().map(|content|format!(
                        "{:x}",Sha256::digest(content.as_bytes())))
                }else{None});
            actual_post.as_deref().is_some_and(|hash|hash.eq_ignore_ascii_case(adopted_hash))
        })
}

/// Cross-domain transfer is learned from an *adopted descendant*, not from
/// the mere creation of an optimization proposal. The donor and target must
/// belong to the same repository partition. This permits using a verified
/// algorithmic transformation (read-node elimination) on a new capability
/// without importing the donor's source text or claiming a target gain.
pub(super) fn proven_read_ablation_donor(
    state:&AppState, generation_job:&Value, target_domain:&str
)->Option<Value> {
    let partition=research_scope(&generation_job["input"]).partition_key;
    let mut donors=state.store.all("experience").into_iter()
        .filter(|record|record["partitionKey"]==partition && record["status"]=="active")
        .flat_map(|record|record["executableMethods"].as_array().cloned().unwrap_or_default())
        .filter(|method|matches!(method["synthesis"]["kind"].as_str(),
            Some("successful_read_ablation"|"transferred_read_ablation"))
            && method["synthesis"]["transferDepth"].as_u64().unwrap_or(0)<3
            && method["domain"].as_str().is_some_and(|d|d!=target_domain)
            && method["successfulApplications"].as_u64().unwrap_or(0)>0
            && method["status"]=="executable"
            && method["applications"].as_array().is_some_and(|apps|apps.iter().any(|row|
                row["accepted"]==true && row["postconditionsPassed"]==true)))
        .collect::<Vec<_>>();
    donors.sort_by(|a,b|b["successfulApplications"].as_u64().unwrap_or(0)
        .cmp(&a["successfulApplications"].as_u64().unwrap_or(0))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    donors.into_iter().next().map(|method|json!({
        "methodId":method["id"],"revision":method["revision"],
        "domain":method["domain"],"adoptedApplications":method["successfulApplications"],
        "transferDepth":method["synthesis"]["transferDepth"].as_u64().unwrap_or(0)
    }))
}

/// A *successful* generation can improve its own method-generation program:
/// propose a smaller DAG by removing a redundant source read. The original
/// accepted program remains active. The new one is a candidate until its own
/// isolated run and held-out evaluation pass; this is not a gain claim.
///
/// The source read may be removed only when its sole purpose was to bind the
/// expected SHA of exactly one versioned line edit. DOMINION's RSI executor
/// already implements an equivalent fresh-index CAS for a null SHA. Real
/// operation receipts, a generation-bound source index and acceptance of the
/// original implementation are all required before this mutation is formed.
pub(crate) fn synthesize_successful_dag_ablation(
    state:&AppState, method:&Value, receipt:&Value, operations:&[Value], generation:&Value
)->Option<Value> {
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || receipt["result"]["verified"]!=true
        || receipt["result"]["followed"]!=true
        || receipt["result"]["status"]=="violated" {return None}
    if generation["accepted"]==true && !method_wrote_accepted_candidate(
        state,generation,receipt,operations) {return None}
    let job=state.store.get("job",generation["jobId"].as_str()?)?;
    let transfer=if generation["accepted"]==true {None}
        else {proven_read_ablation_donor(state,&job,method["domain"].as_str()?)};
    if generation["accepted"]!=true && transfer.is_none(){return None}
    let index_id=job["input"]["rsiIndexId"].as_str()?;
    let index=state.store.get("code_index",index_id)?;
    if index["sourceDir"]!=job["input"]["rsiSourceDir"]{return None}
    let nodes=method["verification"]["nodes"].as_array()?;
    if nodes.len()<2 {return None}
    let ids=receipt["operationIds"].as_array()?;
    for read in nodes.iter().filter(|n|n["tool"]=="workspace_read_range") {
        let read_id=read["id"].as_str()?;
        let path=read["arguments"]["path"].as_str()?;
        if !crate::engine::rsi::safe_path(path)
            || crate::engine::code_index::indexed_source_hash(
                state,index_id,path).is_none() {continue}
        let binding=format!("{}{}.sourceSha256",char::from(36),read_id);
        let edits=nodes.iter().filter(|n|
            n["tool"]=="workspace_replace"
            && n["arguments"]["path"]==path
            && n["arguments"]["oldText"].as_str().is_none_or(str::is_empty)
            && n["arguments"]["expectedSha256"]==binding
            && n["arguments"]["startLine"].as_u64().unwrap_or(0)>0
            && n["arguments"]["endLine"].as_u64().unwrap_or(0)>=
                n["arguments"]["startLine"].as_u64().unwrap_or(1))
            .collect::<Vec<_>>();
        if edits.len()!=1 {continue}
        let edit_id=edits[0]["id"].as_str()?;
        // Removing a read must not strand other dynamic arguments: only the
        // expected-SHA binding may depend on this node.
        let mut edited_arguments=edits[0]["arguments"].clone();
        edited_arguments["expectedSha256"]=Value::Null;
        if edited_arguments.to_string().contains(
            &format!("{}{}.",char::from(36),read_id)) {continue}
        let edges=method["verification"]["edges"].as_array()?;
        // Read cannot have any other consumer or a prerequisite that must
        // remain satisfied. A DAG rewrite must preserve all remaining edges.
        if edges.iter().any(|e|
            (e["from"]==read_id && e["to"]!=edit_id)
            || e["to"]==read_id) {continue}
        if !edges.iter().any(|e|e["from"]==read_id && e["to"]==edit_id){continue}
        if nodes.iter().any(|n|n["id"]!=edit_id && n["id"]!=read_id
            && n["arguments"].to_string().contains(&format!("{}{}.",char::from(36),read_id))) {
            continue
        }
        if method["verification"]["before"].as_array().into_iter().flatten()
            .any(|v|v.to_string().contains(read_id)) {continue}
        if method["verification"]["after"].as_array().into_iter().flatten()
            .any(|v|v["tool"]=="workspace_read_range"
                || v.to_string().contains(&format!("{}{}.",char::from(36),read_id))) {continue}
        let read_hash=operations.iter().filter(|op|
            ids.contains(&op["id"]) && op["origin"]=="procedure"
                && op["procedureStrategyId"]==method["id"]
                && op["procedureRevision"]==method["revision"]
                && op["procedureNodeId"]==read_id
                && op["tool"]=="workspace_read_range"
                && crate::engine::procedure::operation_succeeded(op))
            .filter_map(|op|op["result"]["sourceSha256"].as_str())
            .find(|hash|hash.len()==64
                && hash.bytes().all(|byte|byte.is_ascii_hexdigit()));
        let Some(observed_sha)=read_hash else {continue};
        let edit_ok=operations.iter().any(|op|
            ids.contains(&op["id"]) && op["origin"]=="procedure"
                && op["procedureStrategyId"]==method["id"]
                && op["procedureRevision"]==method["revision"]
                && op["procedureNodeId"]==edit_id
                && op["tool"]=="workspace_replace"
                && crate::engine::procedure::operation_succeeded(op)
                && op["result"]["written"]==true
                && op["arguments"]["path"]==path
                && op["arguments"]["expectedSha256"].as_str()
                    .is_some_and(|hash|hash.eq_ignore_ascii_case(observed_sha)));
        if !edit_ok {continue}
        let mut program=method["verification"].clone();
        program["nodes"].as_array_mut()?.retain(|n|n["id"]!=read_id);
        let edit=program["nodes"].as_array_mut()?.iter_mut()
            .find(|n|n["id"]==edit_id)?;
        edit["arguments"]["expectedSha256"]=Value::Null;
        program["edges"].as_array_mut()?.retain(|e|
            e["from"]!=read_id && e["to"]!=read_id);
        let program=crate::engine::procedure::normalize(&program);
        if program["type"]!="procedure" {continue}
        let domain=method["domain"].as_str()?;
        let revision=format!("{:x}",Sha256::digest(
            json!([domain,&program]).to_string().as_bytes()));
        if revision==method["revision"]{continue}
        return Some(json!({
            "id":format!("rsi-method-{revision}"),"revision":revision,
            "name":format!("{} · success-ablation",method["name"].as_str()
                .unwrap_or("Learned method").chars().take(65).collect::<String>()),
            "domain":domain,"verification":program,"status":"candidate",
            "assistanceMode":"harness_assisted","successfulApplications":0,
            "synthesis":{
                "kind":if transfer.is_some(){"transferred_read_ablation"}
                    else {"successful_read_ablation"},
                "transferDepth":transfer.as_ref().map(|t|
                    t["transferDepth"].as_u64().unwrap_or(0)+1).unwrap_or(0),
                "transferDonor":transfer,
                "parentMethodId":method["id"],"parentRevision":method["revision"],
                "sourceExecutionId":receipt["executionId"],
                "sourceGenerationJobId":generation["jobId"],
                "removedReadNode":read_id,"retainedEditNode":edit_id,
                "sourceIndexId":index_id,"sourcePath":path,
                "hypothesis":"remove_redundant_read_using_generation_bound_index",
                "qualityGain":Value::Null,
                "causalProof":"awaiting_separate_descendant_trial"
            }
        }))
    }
    None
}

/// A proved method *transformation* can be transplanted across independent
/// capability domains, but only within the same repository partition and
/// only after the donor itself was used in a verified adopted patch. This is
/// operator inheritance, not an inference of target quality improvement.
pub(super) fn adopted_pruning_donor(
    state:&AppState, generation:&Value, target_domain:&str
)->Option<Value>{
    let job=state.store.get("job",generation["jobId"].as_str()?)?;
    let partition=research_scope(&job["input"]).partition_key;
    let mut donors=state.store.all("experience").into_iter()
        .filter(|e|e["status"]=="active" && e["partitionKey"]==partition)
        .flat_map(|e|e["executableMethods"].as_array().cloned().unwrap_or_default())
        .filter(|m|matches!(m["synthesis"]["kind"].as_str(),
            Some("successful_observation_pruning"|"transferred_observation_pruning"))
            && m["domain"].as_str().is_some_and(|d|d!=target_domain)
            && m["synthesis"]["transferDepth"].as_u64().unwrap_or(0)<3
            && m["status"]=="executable"
            && m["successfulApplications"].as_u64().unwrap_or(0)>0
            && m["applications"].as_array().is_some_and(|apps|apps.iter().any(|app|
                app["accepted"]==true && app["postconditionsPassed"]==true)))
        .collect::<Vec<_>>();
    donors.sort_by(|a,b|b["successfulApplications"].as_u64().unwrap_or(0)
        .cmp(&a["successfulApplications"].as_u64().unwrap_or(0))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    donors.into_iter().next().map(|m|json!({
        "methodId":m["id"],"revision":m["revision"],"domain":m["domain"],
        "adoptedApplications":m["successfulApplications"],
        "transferDepth":m["synthesis"]["transferDepth"].as_u64().unwrap_or(0)
    }))
}

/// Derive a second, genuinely different meta-operator from an accepted source
/// improvement: remove an observed but dataflow-dead exploration node, then
/// rewire its predecessor/successor dependencies. Unlike source-SHA ablation,
/// this works with code search, web fetch and broad research observations.
/// It does NOT infer that searching is universally useless: the smaller DAG
/// remains a candidate and must be executed and compared independently.
pub(crate) fn synthesize_observed_dag_pruning(
    state:&AppState, method:&Value, receipt:&Value, operations:&[Value], generation:&Value
)->Option<Value> {
    if method["assistanceMode"]!="harness_assisted"
        || method["verification"]["type"]!="procedure"
        || receipt["result"]["verified"]!=true || receipt["result"]["followed"]!=true
        || receipt["result"]["unknown"]==true {return None}
    let local_adoption=method_wrote_accepted_candidate(
        state,generation,receipt,operations);
    let transfer=if local_adoption {None}
        else {adopted_pruning_donor(state,generation,method["domain"].as_str()?)};
    if !local_adoption && transfer.is_none() {return None}
    let nodes=method["verification"]["nodes"].as_array()?;
    if nodes.len()<2 || !nodes.iter().any(|n|
        matches!(n["tool"].as_str(),Some("workspace_replace"|"workspace_write"))) {
        return None
    }
    let edges=method["verification"]["edges"].as_array()?;
    let operation_ids=receipt["operationIds"].as_array()?;
    for node in nodes {
        // A source read may be a stateful preflight prerequisite even if
        // its JSON result has no consumers. Search/fetch nodes are the
        // observation-only operators this general pruning rule handles.
        if !matches!(node["tool"].as_str(),
            Some("code_search"|"web_search"|"http_fetch")) {
            continue
        }
        let Some(id)=node["id"].as_str() else {continue};
        let binding=format!("{}{}.",char::from(36),id);
        if nodes.iter().filter(|n|n["id"]!=id)
            .any(|n|n["arguments"].to_string().contains(&binding)) {continue}
        if method["verification"]["bindings"].to_string().contains(&binding)
            || method["verification"]["before"].to_string().contains(&binding)
            || method["verification"]["after"].to_string().contains(&binding) {continue}
        if method["verification"]["after"].as_array().into_iter().flatten().any(|c|
            c["kind"]=="tool_called" && c["tool"]==node["tool"]) {continue}
        // A pruned node must have actually run successfully within the
        // source-changing method's own execution; a plan-only step is not
        // evidence that an operator can be removed.
        let observed=operations.iter().any(|op|
            operation_ids.contains(&op["id"])
            && op["origin"]=="procedure"
            && op["procedureStrategyId"]==method["id"]
            && op["procedureRevision"]==method["revision"]
            && op["procedureNodeId"]==id
            && op["tool"]==node["tool"]
            && crate::engine::procedure::operation_succeeded(op));
        if !observed {continue}
        let predecessors=edges.iter()
            .filter(|edge|edge["to"]==id)
            .filter_map(|edge|edge["from"].as_str())
            .collect::<Vec<_>>();
        let successors=edges.iter()
            .filter(|edge|edge["from"]==id)
            .filter_map(|edge|edge["to"].as_str())
            .collect::<Vec<_>>();
        if predecessors.len()*successors.len()>16 {continue}
        let mut program=method["verification"].clone();
        program["nodes"].as_array_mut()?.retain(|n|n["id"]!=id);
        let rewritten=program["edges"].as_array_mut()?;
        rewritten.retain(|edge|edge["from"]!=id && edge["to"]!=id);
        for from in &predecessors {
            for to in &successors {
                if from==to {continue}
                if !rewritten.iter().any(|edge|
                    edge["from"]==*from && edge["to"]==*to) {
                    rewritten.push(json!({"from":from,"to":to}));
                }
            }
        }
        let normalized=crate::engine::procedure::normalize(&program);
        if normalized["type"]!="procedure" {continue}
        let domain=method["domain"].as_str()?;
        let revision=format!("{:x}",Sha256::digest(
            json!([domain,&normalized]).to_string().as_bytes()));
        if revision==method["revision"] {continue}
        return Some(json!({
            "id":format!("rsi-method-{revision}"),"revision":revision,
            "name":format!("{} · pruned-observation",
                method["name"].as_str().unwrap_or("Learned method")
                    .chars().take(60).collect::<String>()),
            "domain":domain,"verification":normalized,"status":"candidate",
            "assistanceMode":"harness_assisted","successfulApplications":0,
            "synthesis":{
                "kind":if transfer.is_some() {"transferred_observation_pruning"}
                    else {"successful_observation_pruning"},
                "transferDepth":transfer.as_ref().map(|donor|
                    donor["transferDepth"].as_u64().unwrap_or(0)+1).unwrap_or(0),
                "transferDonor":transfer,
                "parentMethodId":method["id"],"parentRevision":method["revision"],
                "sourceExecutionId":receipt["executionId"],
                "sourceGenerationJobId":generation["jobId"],
                "prunedNodeId":id,"prunedTool":node["tool"],
                "rewiredIncoming":predecessors.len(),"rewiredOutgoing":successors.len(),
                "hypothesis":"observed_research_node_has_no_runtime_consumers",
                "qualityGain":Value::Null,
                "causalProof":"pending_independent_descendant_trial"
            }
        }))
    }
    None
}

/// Bootstrap the self-improvement method pool from an *actual failed model
