//! Independently observed outcomes used to calibrate later meta-policy.
use super::{independent_adoption,failure_kind};
use serde_json::{json,Value};

/// New prediction errors are durable observations available to later compiled
/// meta-controllers. No model statement can turn a prediction into evidence.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    if policy["source"]!="compiled_editable_meta_controller_v1" {
        return Value::Null;
    }
    let probability=policy["forecast"]["acceptanceProbability"]
        .as_f64().filter(|p|p.is_finite()&&*p>=0.0&&*p<=1.0);
    let assessed=generation["evaluation"]["verdict"].as_str().is_some()
        && generation["evaluation"]["metrics"]["eligible"].is_boolean();
    let observed=if assessed{Some(independent_adoption(generation))}else{None};
    let brier=match(probability,observed){
        (Some(p),Some(success))=>Some((p-if success{1.0}else{0.0}).powi(2)),
        _=>None
    };
    // Capture the model's PROPOSAL-time forecast before editing. The
    // frozen evaluator supplies later observations, and missing evidence
    // must never become a fabricated regression or success.
    let proposal=&generation["systemDesign"]["expectedOutcomes"];
    let forecast=if proposal.is_object(){proposal}else{&policy["taskOutcomeForecast"]};
    let actuals=[
        ("candidateCaptured",Some(generation["candidateId"].is_string())),
        ("adoption",observed),
        ("metaImprovement",if assessed{Some(generation["evaluation"]["metrics"]
            ["metaHoldoutGains"].as_u64().unwrap_or(0)>0)}else{None}),
        ("regression",if assessed{Some(generation["evaluation"]["metrics"]
            ["regressions"].as_u64().unwrap_or(0)>0)}else{None})
    ];
    let mut errors=serde_json::Map::new();
    let mut losses=Vec::new();
    for (name,actual) in actuals{
        if let (Some(p),Some(result))=(forecast[name].as_f64()
            .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p)),actual){
            let loss=(p-if result{1.0}else{0.0}).powi(2);
            errors.insert(name.into(),json!({"predicted":p,"observed":result,
                "brierError":loss,"independentAssessment":name=="candidateCaptured"||assessed}));
            losses.push(loss);
        }
    }
    let proposal_error=if losses.is_empty(){Value::Null}else{
        json!(losses.iter().sum::<f64>()/losses.len() as f64)
    };
    // Compare only the visible training case predictions against actual
    // measurements. Held-out case IDs and results are never used as
    // predictive training labels.
    let mut public_losses=Vec::new();
    let mut public_observations=Vec::new();
    if assessed{
        for expected in policy["taskOutcomeForecast"]["publicCaseForecasts"]
            .as_array().into_iter().flatten().take(16){
            let Some(id)=expected["caseId"].as_str() else{continue;};
            let Some(p)=expected["candidatePassProbability"].as_f64()
                .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p))
                else{continue;};
            let actual=generation["evaluation"]["metrics"]["measurements"].as_array()
                .into_iter().flatten().find(|row|row["id"]==id
                    && row["holdout"]==false)
                .and_then(|row|row["candidatePass"].as_bool());
            if let Some(passed)=actual{
                let error=(p-if passed{1.0}else{0.0}).powi(2);
                public_losses.push(error);
                public_observations.push(json!({
                    "caseId":id,"predicted":p,"observed":passed,
                    "brierError":error,"holdout":false
                }));
            }
        }
    }
    let public_brier=if public_losses.is_empty(){Value::Null}else{
        json!(public_losses.iter().sum::<f64>()/public_losses.len() as f64)
    };
    // A peer-guided *attempt* is distinct from both donor success and
    // recipient adoption. Future generations learn method portability only
    // from attempts that actually reached the chosen indexed source.
    let transfer=&policy["peerMethodTransfer"];
    let assigned_donor=transfer["activeDonorCandidateId"].as_str()
        .filter(|id|!id.is_empty());
    let matching_assignment=assigned_donor.is_some_and(|id|
        generation["metaPolicy"]["peerMethodTransfer"]["activeDonorCandidateId"]==id
            && generation["metaPolicy"]["revision"]==policy["revision"]
            && transfer["references"].as_array().is_some_and(|refs|
                refs.iter().any(|row|row["candidateId"]==id)));
    let source_focus=transfer["active"]==true && matching_assignment
        && transfer["comparisonProfileSha256"]==generation["comparisonProfileSha256"]
        && transfer["selectedRecipient"]["role"].as_str().is_some_and(|role|
            transfer["selectedRecipient"]["path"].as_str().is_some_and(|path|
                transfer["selectedRecipient"]["sourceSha256"].as_str().is_some_and(
                    |sha|executed_recipient_context(generation,role,path,sha,
                        &transfer["comparisonProfileSha256"]))));
    let source_adopted=source_focus && transfer["selectedRecipient"]["path"].as_str()
        .zip(transfer["selectedRecipient"]["sourceSha256"].as_str())
        .is_some_and(|(path,sha)|recipient_source_adopted(generation,path,sha));
    let transfer_observation=json!({
        "source":"measured_cross_lineage_recipient_observation_v1",
        "hypothesisRevision":policy["revision"],
        "fromPeerCandidates":transfer["references"].as_array()
            .into_iter().flatten()
            .filter_map(|row|row["candidateId"].as_str())
            .take(4).collect::<Vec<_>>(),
        "recipient":transfer["selectedRecipient"],
        "attributableDonorCandidateId":if source_focus{json!(assigned_donor)}else{Value::Null},
        "singleDonorSourceSuiteMatched":source_focus,
        "sourceFocusExecuted":source_focus,
        "independentlyAssessed":source_focus && assessed,
        "recipientSourceIndependentlyAdopted":source_adopted,
        "portableMethodSuperiorityProven":false,
        "automaticPatchReuse":false
    });
    json!({
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "portableMethodObservation":transfer_observation,
        "taskPredictionErrors":errors,
        "proposalBrierError":proposal_error,
        "publicCasePredictionErrors":public_observations,
        "publicCaseBrierError":public_brier,
        "publicCasePredictionsAssessed":public_losses.len(),
        "forecastOrigin":if proposal.is_object(){"model_authored_pre_edit"}
            else{"compiled_meta_prior"},
        "predictedProbability":probability,
        "independentlyMeasured":observed.is_some(),
        "actualModelAuthoredAdoption":observed,
        "brierError":brier,
        "failureKind":if independent_adoption(generation){"adopted"}
            else{failure_kind(generation)},
        "attemptedSourceChange":generation["candidateId"].is_string(),
        "nextPolicyCanTrainFromObservation":observed.is_some(),
        "qualityProven":observed==Some(true)
    })
}

