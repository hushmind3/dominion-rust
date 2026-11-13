//! Real post-evaluation policy and method feedback for the next descendant.
//! Model proposals only queue trials; paired results decide activation.
use super::*;

pub(super) fn record_generation_feedback(
    context:&JobContext,
    record:&mut Value,
    input:&Value,
    generation:&mut Value,
    session:Option<&str>,
    attempt:u64,
    head:&str,
){
    let prior_history = record["history"].as_array().cloned().unwrap_or_default();
    if let Some(session)=session {
        // Queue identity and next-generation lookup must use one target
        // objective. Autonomous system prompts are not owner constraints.
        let trial_objective=selection_objective(input);
        let proposals = proposed_algorithms(&context.state, session);
        let active = crate::engine::algorithms::active(&context.state, "selfimprove.search")
            .unwrap_or_else(|| json!({"id":Value::Null,"revision":Value::Null,"implementation":"failure-clusters","parameters":{}}));
        let mut queued = Vec::new();
        let had_pending_trial=record["nextTrialAlgorithmId"].is_string();
        for candidate in proposals
            .iter()
            .filter(|candidate| candidate["kind"] == "selfimprove.search")
        {
            let report = queue_search_trial(
                &context.state,
                candidate,
                &active,
                &prior_history,
                &trial_objective,
                &context.job_id,
                attempt,
            );
            if report["status"] == "pending_trial" {
                if let Some(id) = report["candidateId"].as_str() {
                    queued.push(id.to_string());
                }
            }
        }
        if !queued.is_empty() {
            generation["searchTrialsQueued"] = json!(queued);
        }
        // Autonomous RSI must be capable of evolving its own search
        // policy even when a small model never calls algorithm_propose.
        // No new generation or paid model invocation is created here.
        if queued.is_empty() && !had_pending_trial
            && input["rsiAutonomous"]==true
            && generation["domainSelection"]!="algorithm_trial" {
            let mut complete=prior_history.clone();
            complete.push(generation.clone());
            if let Some(report)=queue_adaptive_search_trial(
                &context.state,&complete,&active,
                &trial_objective,
                &context.job_id,attempt,
            ){
                if let Some(id)=report["candidateId"].as_str() {
                    queued.push(id.to_string());
                    generation["autoEvolvedSearch"]=json!({
                        "candidateId":id,
                        "hypothesis":report["candidateRevision"],
                        "source":report["hypothesisSource"],
                        "status":"pending_future_real_trial_not_improvement_proof"
                    });
                }
            }
        }
        record["nextTrialAlgorithmId"] = Value::Null;
        if generation["domainSelection"].as_str() == Some("algorithm_trial") {
            if let Some(id) = generation["searchStrategy"]["id"].as_str() {
                let outcome =
                    record_trial_outcome(&context.state, id, &generation, &prior_history);
                // A research continuation did not run the formula. Keep the
                // candidate pending for the next already scheduled generation.
                if outcome["status"] == "not_used" {
                    record["nextTrialAlgorithmId"] = json!(id);
                } else if outcome["status"] == "pending_second_trial" {
                    record["nextTrialAlgorithmId"] = json!(id);
                }
                generation["trialOutcome"] = outcome;
            }
        }
        if record["nextTrialAlgorithmId"].is_null() {
            if let Some(id) = queued.into_iter().next() {
                record["nextTrialAlgorithmId"] = json!(id);
            }
        }
    }
    if input["rsiSystemEvolution"]==true {
        generation["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
            &generation["metaPolicy"],&generation);
        context.state.store.audit("rsi.meta_controller_feedback",json!({
            "campaignId":context.job_id,"generationJobId":generation["jobId"],
            "generatorCommit":head,
            "metaPolicyRevision":generation["metaPolicy"]["revision"],
            "independentObservation":generation["metaLearning"]["independentObservation"],
            "brierError":generation["metaLearning"]["brierError"],
            "taskQualityBrierError":generation["metaLearning"]["taskQualityBrierError"],
            "observedTaskCases":generation["metaLearning"]["taskQualityObservations"]
        }));
    }
}
