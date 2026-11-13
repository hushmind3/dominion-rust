//! feedback: executable, independently observed RSI method-learning responsibilities.
use super::*;

pub(crate) fn record_method_feedback(state: &AppState, generation: &Value, session_id: &str) -> Vec<Value> {
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
