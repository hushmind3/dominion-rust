//! Model-authored method selection, reuse, source-grounded repairs and execution feedback.
//! Private to the RSI coordinator; no frozen evaluator or model weight changes.
use super::{
    Digest, AppState, Value, BTreeSet, Path,
    Sha256, json, experience, rsi, tools,
    DOMAINS, limits, run, research_scope, focus_domain_hint,
};

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
        if !tools::rsi_allowed(tool) || matches!(tool, "rsi_method_propose" | "algorithm_propose" | "rsi_system_plan") {
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
    // Verified adoption dominates. Before adoption, favor untried programs
    // over repeatedly replaying a merely inconclusive method. Thus actual
    // observations change the next generation's repair-selection algorithm.
    methods.sort_by(|a,b| b["successfulApplications"].as_u64().unwrap_or(0)
        .cmp(&a["successfulApplications"].as_u64().unwrap_or(0))
        .then(a["applications"].as_array().map(Vec::len).unwrap_or(0)
            .cmp(&b["applications"].as_array().map(Vec::len).unwrap_or(0)))
        .then(repair_exploration_order(a).cmp(&repair_exploration_order(b)))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    let mut ids = BTreeSet::new();
    methods.retain(|m| ids.insert(m["id"].as_str().unwrap_or("").to_string()));
    methods.truncate(24);
    methods
}

pub(super) fn repair_exploration_order(method:&Value)->u8 {
    match method["synthesis"]["kind"].as_str() {
        Some("stale_source_rebase")=>0,
        Some("existing_file_to_versioned_edit")=>1,
        Some("indexed_source_rebase")=>2,
        Some("indexed_path_retarget")=>3,
        Some("successful_read_ablation"|"transferred_read_ablation"|"successful_observation_pruning"|"transferred_observation_pruning")=>4,
        Some("failed_model_tool_rebase")=>5,
        _=>6,
    }
}

/// One exploration trial after an observed accepted application. This mutates
/// the *method-selection algorithm* itself: the new structural candidate gets
/// one trial against its accepted parent's future performance, then an
/// unadopted candidate relinquishes priority. Previous acceptance is not
/// automatically transferred to its child.
pub(super) fn pending_success_ablation(method:&Value, methods:&[Value])->bool {
    if !matches!(method["synthesis"]["kind"].as_str(),
        Some("successful_read_ablation"|"transferred_read_ablation"|"successful_observation_pruning"|"transferred_observation_pruning"))
        || method["applications"].as_array().is_some_and(|a|!a.is_empty())
        || method["successfulApplications"].as_u64().unwrap_or(0)>0 {
        return false
    }
    (matches!(method["synthesis"]["kind"].as_str(),
        Some("transferred_read_ablation"|"transferred_observation_pruning"))
        && method["synthesis"]["transferDonor"]["methodId"].is_string()
        && method["synthesis"]["transferDonor"]["adoptedApplications"]
            .as_u64().unwrap_or(0)>0)
        || methods.iter().any(|parent|
            parent["id"]==method["synthesis"]["parentMethodId"]
            && parent["revision"]==method["synthesis"]["parentRevision"]
            && parent["successfulApplications"].as_u64().unwrap_or(0)>0)
}

/// An actually adopted structural descendant should supersede its parent
/// even if the parent amassed more historic successes. Cumulative counts
/// otherwise make the algorithm resistant to its own verified upgrades.
pub(super) fn adopted_structural_descendant(method:&Value, methods:&[Value])->bool {
    if !is_synthesized_source_repair(method)
        || method["successfulApplications"].as_u64().unwrap_or(0)==0
        || method["status"]!="executable"
        || !method["applications"].as_array().is_some_and(|apps|apps.iter().any(|row|
            row["accepted"]==true && row["postconditionsPassed"]==true)) {
        return false
    }
    let parent=methods.iter().any(|prior|
        prior["id"]==method["synthesis"]["parentMethodId"]
        && prior["revision"]==method["synthesis"]["parentRevision"]);
    let transferred=matches!(method["synthesis"]["kind"].as_str(),
        Some("transferred_read_ablation"|"transferred_observation_pruning"))
        && method["synthesis"]["transferDonor"]["adoptedApplications"]
            .as_u64().unwrap_or(0)>0;
    parent || transferred
}

/// Learn the search-vs-exploitation coefficient from *method application
/// receipts*, then rank alternative DAGs by an uncertainty-aware posterior.
/// A large lifetime adoption count cannot hide recent repeated failed trials.
/// This is an observational scheduler; it never labels an unpaired comparison
/// as a causal improvement in model or system quality.
pub(super) fn method_selection_feedback(methods:&[Value])->(usize,f64) {
    let attempts=methods.iter().map(|m|m["applications"].as_array()
        .map(Vec::len).unwrap_or(0)).sum::<usize>();
    let matched=methods.iter().map(|m|
        m["successfulApplications"].as_u64().unwrap_or(0)
            .min(m["applications"].as_array().map(Vec::len).unwrap_or(0) as u64))
        .sum::<u64>() as f64;
    let posterior=(matched+1.0)/(attempts as f64+2.0);
    // When previous attempts rarely produced adopted source, explore more
    // alternative programs. Once repeatedly effective, exploit more.
    let exploration=0.20+0.25*(1.0-posterior);
    (attempts,exploration)
}

pub(super) fn method_observed_selection_score(method:&Value,attempts:usize,exploration:f64)->f64 {
    let apps=method["applications"].as_array();
    let n=apps.map(Vec::len).unwrap_or(0);
    let adopted=method["successfulApplications"].as_u64().unwrap_or(0)
        .min(n as u64) as f64;
    let passed=apps.into_iter().flatten()
        .filter(|entry|entry["postconditionsPassed"]==true).count() as f64;
    // Non-adopted procedure success counts only as evidence of executability,
    // never as a demonstrated quality gain.
    let posterior=(adopted+0.15*(passed-adopted).max(0.0)+0.5)
        /(n as f64+1.0);
    let curiosity=exploration*
        (((attempts as f64+2.0).ln()/(n as f64+1.0)).sqrt());
    posterior+curiosity
}

pub(super) fn method_reuse_decision(state: &AppState, input: &Value, domain: &str) -> Value {
    let mut candidates = applicable_methods(state, &research_scope(input).partition_key, domain,
        input["rsiObjective"].as_str().unwrap_or(""));
    let exploratory=candidates.iter().filter(|m|pending_success_ablation(m,&candidates))
        .filter_map(|m|m["id"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let adopted=candidates.iter().filter(|m|adopted_structural_descendant(m,&candidates))
        .filter_map(|m|m["id"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let (observed_trials,exploration)=method_selection_feedback(&candidates);
    candidates.sort_by(|a,b|
        exploratory.contains(b["id"].as_str().unwrap_or(""))
            .cmp(&exploratory.contains(a["id"].as_str().unwrap_or("")))
        .then(adopted.contains(b["id"].as_str().unwrap_or(""))
            .cmp(&adopted.contains(a["id"].as_str().unwrap_or(""))))
        .then_with(||{
            let sa=method_observed_selection_score(a,observed_trials,exploration);
            let sb=method_observed_selection_score(b,observed_trials,exploration);
            sb.total_cmp(&sa)
        }));
    // Mutually exclusive *writing methods* must never edit the same file
    // in one generation. This applies to inherited parent methods too, not
    // only newly synthesized candidates; otherwise a compact child may edit
    // the file after its parent and falsely appear to perform better.
    // Sequential trials are exploratory, not a causal paired A/B proof.
    let mut occupied=BTreeSet::new();
    let mut methods=Vec::new();
    for method in candidates {
        let target=method["verification"]["nodes"].as_array()
            .and_then(|nodes|nodes.iter().find(|n|
                matches!(n["tool"].as_str(),Some("workspace_replace"|"workspace_write"))))
            .and_then(|node|node["arguments"]["path"].as_str());
        if is_synthesized_source_repair(&method) && target.is_none(){continue}
        if let Some(path)=target {
            if !occupied.insert(path.to_string()){continue}
        }
        methods.push(method);
        if methods.len()>=3{break}
    }
    // An observed-failure repair is an executable next-generation experiment.
    // Preserve the exact target and provenance rather than only a method ID.
    let repair=methods.iter().find(|m|is_synthesized_source_repair(m));
    let repair_path=repair.and_then(|m|m["verification"]["nodes"].as_array())
        .and_then(|nodes|nodes.iter().find(|n|n["tool"]=="workspace_replace"))
        .and_then(|node|node["arguments"]["path"].as_str())
        .filter(|path|crate::engine::rsi::safe_path(path));
    json!({"action":if methods.is_empty(){"research_only"}else{"execute_methods"},
        "methodIds":methods.iter().map(|m|m["id"].clone()).collect::<Vec<_>>(),
        "repairMethodId":repair.map(|m|m["id"].clone()),
        "repairRevision":repair.map(|m|m["revision"].clone()),
        "repairTarget":repair_path,
        "repairSelectionEvidence":repair.map(|m|json!({
            "operator":m["synthesis"]["kind"],
            "parentRevision":m["synthesis"]["parentRevision"],
            "attemptedApplications":m["applications"].as_array().map(Vec::len).unwrap_or(0),
            "verifiedAdoptions":m["successfulApplications"].as_u64().unwrap_or(0),
            "selectionPolicy":"observed_method_outcomes_posterior_v1",
            "selectionPolicyTrials":observed_trials,
            "explorationCoefficient":exploration,
            "methodPosteriorUtility":method_observed_selection_score(
                m,observed_trials,exploration),
            "selection":if exploratory.contains(m["id"].as_str().unwrap_or("")) {
                "success_ablation_trial"
            } else if adopted.contains(m["id"].as_str().unwrap_or("")) {
                "adopted_structural_descendant"
            } else {"adopted_then_untried_sequential"},
            "causalGain":"not_measured_sequential_trials"
        }))})
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

/// A repair discovered from a failed procedure is an executable program, not
/// a suggested prompt. The candidate receives no improvement credit until
/// its own future application passes the ordinary procedure evaluator.
pub(super) fn is_synthesized_source_repair(method:&Value)->bool {
    matches!(method["synthesis"]["kind"].as_str(),
        Some("stale_source_rebase"|"indexed_source_rebase"|"existing_file_to_versioned_edit"|"indexed_path_retarget"|"successful_read_ablation"|"transferred_read_ablation"|"successful_observation_pruning"|"transferred_observation_pruning"|"failed_model_tool_rebase"|"failed_model_reversed_span"))
}

/// Synthesize a new executable procedure, not just an action ranking.
/// A witnessed stale source edit gains a read node, a DAG dependency and a
/// dynamically bound fresh hash; acceptance requires later real execution.
pub(super) fn synthesize_stale_edit_repair(method:&Value, op:&Value)->Option<Value> {
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
pub(super) fn synthesize_indexed_edit_repair(method:&Value,op:&Value)->Option<Value> {
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
pub(super) fn synthesize_existing_file_write_repair(method:&Value,op:&Value)->Option<Value> {
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
pub(super) fn synthesize_indexed_path_repair(
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
pub(super) fn synthesize_successful_dag_ablation(
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
pub(super) fn synthesize_observed_dag_pruning(
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
/// edit*, without requiring the model to have first written a procedure
/// definition. A zero-diff generation can therefore teach the next
/// generation a different executable source-edit program. Its code change
/// remains the model's observed intention, not a fabricated patch.
pub(super) fn synthesize_failed_model_edit(
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
pub(super) fn synthesize_failed_reversed_span(
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

pub(super) fn record_method_feedback(state: &AppState, generation: &Value, session_id: &str) -> Vec<Value> {
    let receipts = state.store.get("session", session_id)
        .and_then(|s|s["rsiMethodExecutions"].as_array().cloned()).unwrap_or_default();
    let operations=state.store.all("execution_operation");
    let mut repairs=Vec::<Value>::new();
    // Cold-start: an RSI generation with no learned procedure can still
    // observe an actual model-side edit failure and compile a next-generation
    // execution method. The method will not be credited until applied.
    if let Some(session)=state.store.get("session",session_id) {
        for op in operations.iter().filter(|op|op["sessionId"]==session_id) {
            for repair in [
                synthesize_failed_model_edit(&session,op,generation),
                synthesize_failed_reversed_span(&session,op,generation)
            ].into_iter().flatten() {
                if !repairs.iter().any(|m|m["id"]==repair["id"]) {
                    repairs.push(repair);
                }
                if repairs.len()>=2 {break}
            }
            if repairs.len()>=2 {break}
        }
    }
    for receipt in &receipts {
        let Some(mut record) = receipt["experienceId"].as_str().and_then(|id|state.store.get("experience",id)) else {continue};
        let Some(method) = record["executableMethods"].as_array_mut().and_then(|methods|
            methods.iter_mut().find(|m|m["id"] == receipt["methodId"] && m["revision"] == receipt["revision"])) else {continue};
        // Recovered coordinator steps must not count a completed application twice.
        if method["applications"].as_array().is_some_and(|rows|rows.iter().any(|r|r["executionId"]==receipt["executionId"])) {continue;}
        for op in operations.iter().filter(|op|receipt["operationIds"].as_array()
            .is_some_and(|ids|ids.contains(&op["id"]))) {
            // A single witnessed failure can produce different *executable*
            // repairs. They remain candidates until their real trials prove
            // which method works; do not credit synthesis as improvement.
            for mut repair in [
                synthesize_stale_edit_repair(method,op),
                synthesize_indexed_edit_repair(method,op),
                synthesize_existing_file_write_repair(method,op),
                synthesize_indexed_path_repair(state,method,op,generation)
            ].into_iter().flatten() {
                repair["sourceGenerationJobId"]=generation["jobId"].clone();
                if !repairs.iter().any(|r|r["id"]==repair["id"]) {repairs.push(repair);}
            }
        }
        let result = &receipt["result"];
        let unknown = result["unknown"] == true || result["status"] == "unknown"
            || result["verified"] != true;
        // One unknown condition must not hide a separately confirmed violation.
        let violated = result["verified"] == true && (result["status"] == "violated"
            || result["violations"].as_array().is_some_and(|rows| !rows.is_empty()));
        let ok = !unknown && !violated && result["followed"] == true;
        let progress = generation["accepted"] == true || generation["newFindings"].as_u64().unwrap_or(0)>0;
        let credited=ok && method_wrote_accepted_candidate(
            state,generation,receipt,&operations);
        if ok {
            for mut variant in [
                synthesize_successful_dag_ablation(
                    state,method,receipt,&operations,generation),
                synthesize_observed_dag_pruning(
                    state,method,receipt,&operations,generation)
            ].into_iter().flatten() {
                variant["sourceGenerationJobId"]=generation["jobId"].clone();
                if !repairs.iter().any(|r|r["id"]==variant["id"]) {
                    repairs.push(variant);
                }
            }
        }
        method["status"] = json!(if violated {"suspended"} else if ok && progress {"executable"} else {"held"});
        if credited {
            method["successfulApplications"] = json!(method["successfulApplications"].as_u64().unwrap_or(0)+1);
        }
        let application = json!({"jobId":generation["jobId"],"executionId":receipt["executionId"],
            "operationIds":receipt["operationIds"],
            "postconditionsPassed":if !ok && !violated {Value::Null}else{json!(ok)},
            "verificationVerdict":if violated {"violated"}else if ok {"passed"}else{"inconclusive"},
            "accepted":generation["accepted"],"newFindings":generation["newFindings"],
            "tokens":generation["tokens"],"elapsedMs":generation["elapsedMs"],
            "reuse":method["status"],
            "methodWroteAcceptedCandidate":credited,
            "gain":if credited {"matched_adopted_source_effect_not_paired_causal_proof"}
                else {"unattributed_observation_no_improvement_credit"}});
        if !method["applications"].is_array() {method["applications"]=json!([]);}
        method["applications"].as_array_mut().unwrap().push(application);
        state.store.put("experience", &record);
    }
    if !repairs.is_empty() {
        if let Some(mut session)=state.store.get("session",session_id) {
            if !session["rsiMethodProposals"].is_array() {session["rsiMethodProposals"]=json!([]);}
            let methods=session["rsiMethodProposals"].as_array_mut().unwrap();
            for repair in repairs {
                if !methods.iter().any(|m|m["id"]==repair["id"]) {methods.push(repair);}
            }
            state.store.put("session",&session);
        }
    }
    receipts
}
