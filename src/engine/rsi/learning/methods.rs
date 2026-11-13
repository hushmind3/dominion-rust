//! executable method inheritance and feedback.
//! Only method_learning owns these implementations. Other RSI stages use explicit
//! names imported through the stable rsi_evolve facade.
use super::{AppState, BTreeSet, DOMAINS, Digest, Path, Sha256, Value, json, research_scope, tools};
/// A learned method is an existing procedure DAG, not a name or a prompt.
/// It runs through the normal tool runtime (approvals, effects and budgets).
pub(super) fn method_session_running(state: &AppState, session: &Value) -> bool {
    let Some(campaign)=session["rsiCampaignId"].as_str() else {return false};
    session["rsiRun"] == true
        && state.store.get("job",campaign).is_some_and(|job|job["kind"] == "rsi_evolve"
            && job["status"] == "running" && job["cancelRequested"] != true)
        && session["rsiGenerationJobId"].as_str().and_then(|id|state.store.get("job",id))
            .is_some_and(|job|job["kind"] == "rsi_generation" && job["input"]["rsiCampaignId"] == campaign
                && job["status"] == "running" && job["cancelRequested"] != true)
}

pub(crate) fn propose_method(state: &AppState, session_id: &str, body: &Value) -> Result<Value, String> {
    let mut session = state.store.get("session", session_id).ok_or("세션 없음")?;
    if !method_session_running(state,&session) {
        return Err("진행 중인 RSI 세대에서만 절차를 제안할 수 있습니다.".into());
    }
    let name = body["name"].as_str().filter(|s| !s.trim().is_empty() && s.chars().count() <= 100)
        .ok_or("100자 이내의 절차 이름이 필요합니다.")?;
    let domain = body["domain"].as_str().filter(|d| DOMAINS.contains(d)).ok_or("유효한 분야가 필요합니다.")?;
    let verification = crate::engine::procedure::normalize(&body["verification"]);
    if verification["type"] != "procedure"
        || verification["nodes"].as_array().is_none_or(Vec::is_empty)
        || verification["after"].as_array().is_none_or(Vec::is_empty)
        || verification["appliesTo"]["requestContains"].as_array().is_none_or(|terms|
            terms.is_empty() || terms.iter().any(|v| v.as_str().is_none_or(|s| s.trim().is_empty())))
    {
        return Err("적용할 목표(requestContains), 실행 노드, 검증 가능한 후속 조건이 필요합니다.".into());
    }
    for node in verification["nodes"].as_array().unwrap() {
        let tool = node["tool"].as_str().unwrap_or("");
        if !tools::rsi_allowed(tool) || matches!(tool, "rsi_method_propose" | "algorithm_propose") {
            return Err(format!("학습 절차에서 실행할 수 없는 도구: {tool}"));
        }
    }
    for condition in verification["after"].as_array().unwrap() {
        if (condition["kind"] == "file_contains"
            && condition["text"].as_str().is_none_or(str::is_empty))
            || (condition["kind"] == "file_equals" && !condition["text"].is_string())
        {
            return Err("파일 내용 검증에는 명시적인 기대 값이 필요합니다.".into());
        }
    }
    let revision = format!("{:x}", Sha256::digest(json!([domain, &verification]).to_string().as_bytes()));
    let method = json!({"id":format!("rsi-method-{revision}"),"revision":revision,"name":name,
        "domain":domain,"verification":verification,"status":"candidate",
        "assistanceMode":"harness_assisted","description":body["description"],
        "sourceGenerationJobId":session["rsiGenerationJobId"]});
    if !session["rsiMethodProposals"].is_array() { session["rsiMethodProposals"] = json!([]); }
    let proposals = session["rsiMethodProposals"].as_array_mut().unwrap();
    if !proposals.iter().any(|m| m["id"] == method["id"]) { proposals.push(method.clone()); }
    state.store.put("session", &session).ok_or("절차 제안 저장 실패")?;
    Ok(json!({"method":method,"next":"다음 예정된 개발에서 실제 도구로 실행하고 후속 조건과 개발 결과를 확인합니다. 등록은 성과가 아닙니다."}))
}

pub(super) fn applicable_methods(state: &AppState, partition: &str, domain: &str, objective: &str) -> Vec<Value> {
    let mut methods = Vec::new();
    let records = state.store.all("experience").into_iter()
        .filter(|record|record["partitionKey"] == partition && record["status"] == "active")
        .collect::<Vec<_>>();
    // Re-proposing the identical program in another generation must not erase
    // its failed application history. A revised method has a new content hash.
    let blocked = records.iter().flat_map(|record|record["executableMethods"].as_array().into_iter().flatten())
        .filter(|m|matches!(m["status"].as_str(),Some("suspended" | "held")))
        .filter_map(|m|m["id"].as_str()).collect::<BTreeSet<_>>();
    for record in &records {
        for method in record["executableMethods"].as_array().into_iter().flatten() {
            if method["domain"] != domain
                || blocked.contains(method["id"].as_str().unwrap_or(""))
                || !matches!(method["status"].as_str(), Some("candidate" | "executable"))
                || !crate::engine::procedure::applies(method, objective, None, None) { continue; }
            let mut method = method.clone();
            method["rsiMethodExperienceId"] = record["id"].clone();
            methods.push(method);
        }
    }
    methods.sort_by(|a,b| b["successfulApplications"].as_u64().unwrap_or(0)
        .cmp(&a["successfulApplications"].as_u64().unwrap_or(0))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    let mut ids = BTreeSet::new();
    methods.retain(|m| ids.insert(m["id"].as_str().unwrap_or("").to_string()));
    methods.truncate(3);
    methods
}

pub(super) fn method_reuse_decision(state: &AppState, input: &Value, domain: &str) -> Value {
    let methods = applicable_methods(state, &research_scope(input).partition_key, domain,
        input["rsiObjective"].as_str().unwrap_or(""));
    json!({"action":if methods.is_empty(){"research_only"}else{"execute_methods"},
        "methodIds":methods.iter().map(|m|m["id"].clone()).collect::<Vec<_>>()})
}

pub(crate) fn configure_method_session(state: &AppState, input: &Value, session: &mut Value) {
    let domain = input["rsiDomain"].as_str().unwrap_or("");
    session["rsiResearchPartition"] = json!(research_scope(input).partition_key);
    session["rsiDomain"] = json!(domain);
    let mut decision = input["rsiDecisionInput"].clone();
    if !decision.is_object() {decision=json!({});}
    if decision["nextDomain"] != domain || !decision["reuseDecision"].is_object() {
        decision["reuseDecision"] = method_reuse_decision(state,input,domain);
    }
    session["rsiDecisionInput"] = decision;
}

/// Called by the normal chat runtime. Only the parent's concrete reuse choice
/// can inject a learned method; ordinary chats and other repositories cannot.
pub(crate) fn method_strategies(state: &AppState, session: &Value) -> Vec<Value> {
    if !method_session_running(state,session) || session["rsiDecisionInput"]["reuseDecision"]["action"] != "execute_methods" {
        return Vec::new();
    }
    let ids = session["rsiDecisionInput"]["reuseDecision"]["methodIds"].as_array().cloned().unwrap_or_default();
    let prior = state.store.all("execution_operation");
    applicable_methods(state, session["rsiResearchPartition"].as_str().unwrap_or(""),
        session["rsiDomain"].as_str().unwrap_or(""), session["rsiObjective"].as_str().unwrap_or(""))
        .into_iter().filter(|m| ids.contains(&m["id"]) && !prior.iter().any(|op|
            op["sessionId"] == session["id"] && op["origin"] == "procedure"
                && op["procedureStrategyId"] == m["id"]))
        .collect()
}

/// Evaluate the actual tool operations while the detached workspace still
/// exists. Neither a model assertion nor a `loaded` label is execution evidence.
pub(crate) fn collect_method_executions(state: &AppState, session_id: &str, workspace: &Path) {
    let Some(mut session) = state.store.get("session", session_id) else { return };
    let mut receipts = Vec::new();
    for execution in state.store.all("execution").into_iter().filter(|e| e["sessionId"] == session_id) {
        let Ok(mut operations) = state.store.execution_operations(execution["id"].as_str().unwrap_or("")) else {continue};
        crate::engine::procedure::sort_operations(&mut operations);
        for method in execution["strategyCandidates"].as_array().into_iter().flatten()
            .filter(|m| m["rsiMethodExperienceId"].is_string()) {
            let used = operations.iter().filter(|op|op["origin"] == "procedure"
                && op["procedureStrategyId"] == method["id"] && op["procedureRevision"] == method["revision"])
                .cloned().collect::<Vec<_>>();
            if used.is_empty() { continue; }
            let result = crate::engine::procedure::evaluate(method, &operations, workspace);
            receipts.push(json!({"methodId":method["id"],"revision":method["revision"],
                "experienceId":method["rsiMethodExperienceId"],"executionId":execution["id"],
                "operationIds":used.iter().map(|op|op["id"].clone()).collect::<Vec<_>>(),
                "result":result}));
        }
    }
    session["rsiMethodExecutions"] = json!(receipts);
    state.store.put("session", &session);
}

/// Exact source contribution by an inherited procedure, checked against
/// independently adopted bytes. Co-occurring reads or an execution claim do
/// not establish that the learned method produced the accepted source.
pub(crate) fn method_wrote_accepted_candidate(state:&AppState,generation:&Value,receipt:&Value,session_id:&str)->bool {
    if generation["accepted"]!=true {return false;}
    let Some(candidate)=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id)) else{return false;};
    if candidate["status"]!="verified" || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {return false;}
    let ids=receipt["operationIds"].as_array().cloned().unwrap_or_default();
    state.store.execution_operations(receipt["executionId"].as_str().unwrap_or(""))
        .unwrap_or_default().iter().any(|operation| {
            let Some(path)=operation["arguments"]["path"].as_str() else{return false;};
            let Some(sha)=generation["acceptedSourceHashes"][path].as_str() else{return false;};
            ids.contains(&operation["id"]) && operation["sessionId"]==session_id
                && operation["origin"]=="procedure" && operation["procedureStrategyId"]==receipt["methodId"]
                && operation["procedureRevision"]==receipt["revision"] && operation["status"]=="completed"
                && matches!(operation["tool"].as_str(),Some("workspace_write"|"workspace_replace"|"workspace_replace_range"))
                && operation["result"]["written"]==true && operation["toolReceipt"]["verified"]==true
                && operation["toolReceipt"]["observedVersion"]==format!("sha256:{sha}")
                && candidate["paths"].as_array().is_some_and(|paths|paths.iter().any(|entry|entry==path))
        })
}

pub(super) fn record_method_feedback(state: &AppState, generation: &Value, session_id: &str) -> Vec<Value> {
    let receipts = state.store.get("session", session_id)
        .and_then(|s|s["rsiMethodExecutions"].as_array().cloned()).unwrap_or_default();
    for receipt in &receipts {
        let Some(mut record) = receipt["experienceId"].as_str().and_then(|id|state.store.get("experience",id)) else {continue};
        let Some(method) = record["executableMethods"].as_array_mut().and_then(|methods|
            methods.iter_mut().find(|m|m["id"] == receipt["methodId"] && m["revision"] == receipt["revision"])) else {continue};
        // Recovered coordinator steps must not count a completed application twice.
        if method["applications"].as_array().is_some_and(|rows|rows.iter().any(|r|r["executionId"]==receipt["executionId"])) {continue;}
        let result = &receipt["result"];
        let unknown = result["unknown"] == true || result["status"] == "unknown"
            || result["verified"] != true;
        // One unknown condition must not hide a separately confirmed violation.
        let violated = result["verified"] == true && (result["status"] == "violated"
            || result["violations"].as_array().is_some_and(|rows| !rows.is_empty()));
        let ok = !unknown && !violated && result["followed"] == true;
        let progress = generation["accepted"] == true || generation["newFindings"].as_u64().unwrap_or(0)>0;
        method["status"] = json!(if violated {"suspended"} else if ok && progress {"executable"} else {"held"});
        if ok && generation["accepted"] == true {
            method["successfulApplications"] = json!(method["successfulApplications"].as_u64().unwrap_or(0)+1);
        }
        let application = json!({"jobId":generation["jobId"],"executionId":receipt["executionId"],
            "operationIds":receipt["operationIds"],
            "methodWroteAcceptedCandidate":ok && method_wrote_accepted_candidate(state,generation,receipt,session_id),
            "postconditionsPassed":if !ok && !violated {Value::Null}else{json!(ok)},
            "verificationVerdict":if violated {"violated"}else if ok {"passed"}else{"inconclusive"},
            "accepted":generation["accepted"],"newFindings":generation["newFindings"],
            "tokens":generation["tokens"],"elapsedMs":generation["elapsedMs"],
            "reuse":method["status"],"gain":"observed_association_not_causal_proof"});
        if !method["applications"].is_array() {method["applications"]=json!([]);}
        method["applications"].as_array_mut().unwrap().push(application);
        // Champion recovery synthesis is causal only to the failed procedure
        // OPERATION, not to a model-authored error summary. A revision can
        // acquire a new executable DAG candidate, but no quality credit is
        // awarded until a later run executes and independently evaluates it.
        let method_snapshot=method.clone();
        let operations=state.store.execution_operations(
            receipt["executionId"].as_str().unwrap_or("")
        ).unwrap_or_default();
        let ids=receipt["operationIds"].as_array().cloned().unwrap_or_default();
        let mut candidates=Vec::new();
        for operation in operations.iter().filter(|op|
            ids.contains(&op["id"]) && op["status"]=="failed"
                && op["origin"]=="procedure") {
            for proposed in crate::engine::rsi_repair_synthesis::propose(
                &method_snapshot, operation) {
                if !candidates.iter().any(|existing:&Value| existing["id"]==proposed["id"]) {
                    candidates.push(proposed);
                }
                if candidates.len()>=4 {break;}
            }
            if candidates.len()>=4 {break;}
        }
        let mut added=Vec::new();
        if let Some(methods)=record["executableMethods"].as_array_mut() {
            for candidate in candidates {
                if methods.iter().any(|method|method["id"]==candidate["id"]) {continue;}
                added.push(candidate["id"].clone());
                methods.push(candidate);
            }
        }
        if !added.is_empty() {
            state.store.audit("rsi.executable_recovery_synthesized",
                json!({"executionId":receipt["executionId"],
                    "parentMethodId":receipt["methodId"],"candidateIds":added,
                    "qualityProven":false}));
        }
        state.store.put("experience", &record);
    }
    receipts
}
