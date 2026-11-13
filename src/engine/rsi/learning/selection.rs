//! selection: executable, independently observed RSI method-learning responsibilities.
use super::*;

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
pub(crate) fn adopted_structural_descendant(method:&Value, methods:&[Value])->bool {
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
pub(crate) fn method_selection_feedback(methods:&[Value])->(usize,f64) {
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

pub(crate) fn method_observed_selection_score(method:&Value,attempts:usize,exploration:f64)->f64 {
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

pub(crate) fn method_reuse_decision(state: &AppState, input: &Value, domain: &str) -> Value {
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

