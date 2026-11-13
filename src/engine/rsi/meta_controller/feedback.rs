//! feedback stage of the compiled, revisable meta-improvement controller.
//! These observations propose experiments; only frozen independent evaluation may adopt them.
use super::*;
use super::provenance::OUTCOME_CLASSES;

/// The model—not this controller—authors the actual design, programs and
/// reusable procedures. Comparative observations are bounded and source
/// grounded; they carry no adoption rights.
pub(crate) fn instruction(policy:&Value)->String {
    let target=&policy["target"];
    let refs=policy["referenceDesigns"].as_array().into_iter().flatten()
        .take(2).map(|g|format!(
            "verified roles={} causal hypothesis={} design={} next generation gains={}",
            g["roles"],g["hypothesis"],g["design"],g["productiveDescendants"]
        )).collect::<Vec<_>>().join("\n");
    let failures=policy["counterexamples"].as_array().into_iter().flatten()
        .take(3).map(|g|format!(
            "failed roles={} cause={} source={} observed error={}",
            g["roles"],g["failureKind"],g["sourceTarget"],g["error"]
        )).collect::<Vec<_>>().join("\n");
    let mut prompt=format!(
        "[EDITABLE META-IMPROVER revision={}]\n\
        Selection mode={}; chosen intelligence subsystem={} domain={} source={} hash={}. \
        Estimated adoption chance={} uncertainty={}; this is an unproven forecast.\n\
        Compare MULTIPLE independent earlier experiments, not one trajectory. \
        Valid successes and their descendant productivity:\n{}\n\
        Counterexamples to avoid repeating:\n{}\n\
        Author the novel executable redesign yourself, including new modules or \
        multi-component changes. Executable meta-methods live in \
        src/engine/rsi/meta_controller/ and the editable source-model lives \
        in src/engine/rsi/system/. Edit the actual relevant files and the \
        task/memory/learning/tool harness so the next compiled child changes \
        HOW it designs improvements. \
        Explain a counterfactual or failure mode for your proposal and predict \
        observable changes before editing. After an accepted change the NEXT \
        binary must generate the new improvement plan. Frozen verification is \
        external and cannot be changed.",
        policy["revision"],policy["mode"],target["role"],target["domain"],
        target["path"],target["sourceSha256"],
        policy["forecast"]["acceptanceProbability"],policy["forecast"]["uncertainty"],
        refs,failures
    );
    let pair=&policy["reciprocalExperiment"];
    if pair["partner"].is_object() {
        prompt.push_str(&format!(
            "\n[EXECUTABLE RECIPROCAL SYSTEM EXPERIMENT]\n\
            Primary role={} file={} SHA={}. Partner role={} file={} SHA={}. \
            Observed parent-to-child transitions forward={}, reverse={}, jointly adopted designs={}. \
            Independently paired isolated holdout gains A-to-B={}, B-to-A={}; \
            observed regressions A-to-B={}, B-to-A={}. \
            These observations are NOT proof of a causal improvement. \
            When automatic whole-system focus includes both files, inspect \
            both exact source versions, invent an interaction in which each \
            capability improves the other's future improvement procedure, \
            and WRITE both implementations. Test with the original frozen \
            evaluator; only the next rebuilt binary may inherit adopted code.\n",
            pair["primary"]["role"],pair["primary"]["path"],
            pair["primary"]["sourceSha256"],pair["partner"]["role"],
            pair["partner"]["path"],pair["partner"]["sourceSha256"],
            pair["observedForwardTransfers"],pair["observedReverseTransfers"],
            pair["observedJointAdoptions"],
            pair["measuredForwardHoldoutGains"],pair["measuredReverseHoldoutGains"],
            pair["measuredForwardRegressions"],pair["measuredReverseRegressions"]
        ));
    }
    let influence=policy["interactionDynamics"]["edges"].as_array().into_iter()
        .flatten().take(8).map(|edge|format!(
            "{} -> {}: isolated paired gains={}, regressions={}, joint-only gains={}",
            edge["sourceRole"],edge["targetRole"],
            edge["isolatedPairedGains"],edge["isolatedPairedRegressions"],
            edge["jointPairedGains"]
        )).collect::<Vec<_>>().join("\n");
    if !influence.is_empty(){
        prompt.push_str(&format!(
            "\n[EVOLVABLE CROSS-SYSTEM INFLUENCE MODEL]\n\
            Based on actual frozen holdout comparisons, the source-role to \
            performance-domain relationships are:\n{}\n\
            Treat these as hypotheses, not isolated causal interventions. \
            You may invent a new graph learner, new interaction roles and \
            a better method to choose and execute future multi-role changes.\n",
            influence
        ));
    }
    if policy["reciprocalContinuation"]["source"]==
        "verified_descendant_reverse_redesign_v1" {
        prompt.push_str(&format!(
            "\n[RECURSIVE REVERSAL AFTER VERIFIED TWO-SOURCE ADOPTION]\n\
            The last compiled descendant adopted both role implementations. \
            Now select role {} and redesign its source so the prior role {} \
            gains a BETTER future improvement procedure. This is a new trial, \
            not a proven recursive gain.\n",
            policy["reciprocalContinuation"]["nextRole"],
            policy["reciprocalContinuation"]["priorRole"]
        ));
    }
    prompt
}

/// New prediction errors are durable observations available to later compiled
/// meta-controllers. No model statement can turn a prediction into evidence.
pub(crate) fn feedback(policy:&Value,generation:&Value)->Value {
    if policy["source"]!="compiled_editable_meta_controller_v1" {
        return Value::Null;
    }
    // An inherited, independently adopted method stage may legitimately
    // override the generic meta-controller. Its real outcome must not be
    // used to calibrate a forecast made for a different source/role.
    if generation["systemAgenda"]["target"]!=policy["target"] {
        return json!({"policyRevision":policy["revision"],
            "selectedByInheritedStage":true,
            "actualRole":generation["systemAgenda"]["target"]["role"],
            "forecastRole":policy["target"]["role"],
            "independentlyMeasured":false,
            "brierError":null,"categoricalBrier":null,
            "qualityProven":false});
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
    let class=observed_class(generation);
    let categorical_brier=class.and_then(|actual|{
        let probs=&policy["forecast"]["failurePosterior"];
        let mut error=0.0;
        for label in OUTCOME_CLASSES {
            let probability=probs[label].as_f64().filter(|p|p.is_finite()
                && *p>=0.0&&*p<=1.0)?;
            let truth=if label==actual{1.0}else{0.0};
            error+=(probability-truth).powi(2);
        }
        Some((error/2.0).min(1.0))
    });
    json!({
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "predictedProbability":probability,
        "independentlyMeasured":observed.is_some(),
        "actualModelAuthoredAdoption":observed,
        "brierError":brier,
        "categoricalBrier":categorical_brier,
        "observedFailureClass":class,
        "failureKind":if independent_adoption(generation){"adopted"}
            else{failure_kind(generation)},
        "attemptedSourceChange":generation["candidateId"].is_string(),
        "nextPolicyCanTrainFromObservation":observed.is_some(),
        "verifiedMetaImplementationChanged":meta_source_changed(generation),
        "qualityProven":observed==Some(true)
    })
}

