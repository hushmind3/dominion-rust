//! Tool-facing instructions derived from the compiled descendant policy.
use serde_json::Value;

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
    let mut message=format!(
        "[EDITABLE META-IMPROVER revision={}]\n\
        Selection mode={}; chosen intelligence subsystem={} domain={} source={} hash={}. \
        Estimated adoption chance={} uncertainty={}; this is an unproven forecast.\n\
        Predicted downstream outcomes={} and proposed executable control policy={}.\n\
        Next mutually developing subsystem link={} (observed succession, NOT causal proof).\n\
        If mode=repair_own_prediction_mechanism, redesign the editable prediction/search rule ITSELF using observed prediction errors, not the benchmark scorer. If mode=repair_own_transfer_method, redesign the executable donor-method matching, experimental isolation, or negative-transfer selection rule in src/engine/rsi_meta_controller/peer_transfer.rs using measured recipient and matched-control outcomes; do not edit the frozen evaluator. Before editing call rsi_system_plan with expectedOutcomes probabilities (candidateCaptured, adoption, metaImprovement, regression), then let independent assessment calibrate this proposal.\n\
        Compare MULTIPLE independent earlier experiments, not one trajectory. \
        Valid successes and their descendant productivity:\n{}\n\
        Counterexamples to avoid repeating:\n{}\n\
        Author the novel executable redesign yourself, including new modules or \
        multi-component changes. Explain how one changed subsystem could \
        improve the ability of ANOTHER subsystem to make subsequent changes. \
        For a coupled change, use component=system, components, componentFiles \
        and successorStages so the rebuilt child can carry out the next link. You may also edit src/engine/rsi_meta_controller.rs, \
        src/engine/rsi_system.rs and the task/memory/learning/tool harness, so \
        the next compiled child changes HOW it designs improvements. \
        Explain a counterfactual or failure mode for your proposal and predict \
        observable changes before editing. After an accepted change the NEXT \
        binary must generate the new improvement plan. Frozen verification is \
        external and cannot be changed.",
        policy["revision"],policy["mode"],target["role"],target["domain"],
        target["path"],target["sourceSha256"],
        policy["forecast"]["acceptanceProbability"],policy["forecast"]["uncertainty"],
        policy["taskOutcomeForecast"],policy["controlPolicy"],
        policy["nextMutualRedesign"],refs,failures
    );
    if policy["mode"]=="repair_inherited_method_from_downstream_failures" {
        message.push_str(&format!(
            "\nINHERITED METHOD FAILURE RECOVERY (observational, not causal proof): {}. \
             Your actual rebuilt method was used by independently assessed \
             successor attempts without source adoption. Read this current \
             executable method, redesign the failure diagnosis, experiment \
             selection or edit-control algorithm itself, then produce a REAL \
             model-authored source edit. The frozen paired evaluator alone \
             decides adoption.\n",
            policy["inheritedMethodRepairEvidence"]
        ));
    }
    if policy["peerMethodTransfer"]["active"]==true {
        let rows=policy["peerMethodTransfer"]["references"].as_array()
            .into_iter().flatten()
            .filter(|r|r["candidateId"]==policy["peerMethodTransfer"]["activeDonorCandidateId"])
            .take(1)
            .map(|r|format!(
                "isolated donor={} source roles={} target={} current file={} \
                 measured recipient trials={} actual source adoptions={} \
                 smoothed portability={} matched control trials={} control adoptions={} negative transfer={}",
                r["candidateId"],r["fromRoles"],r["toRole"],
                r["toSourcePath"],r["independentlyMeasuredTransferTrials"],
                r["recipientSourceAdoptions"],r["empiricalTransferPosterior"],
                r["matchedControlTrials"],r["matchedControlAdoptions"],
                r["negativeTransferSignal"]
            )).collect::<Vec<_>>();
        message.push_str(&format!(
            "\nPORTABLE METHOD HYPOTHESIS (not a performance claim): \
             re-derive the improvement METHOD from these independently paired \
             donor experiments and test it on the newly chosen component. \
             Do not reuse their patch or label this transfer successful. \
             Make a new source edit, predict the result, and allow the frozen \
             evaluator to measure it:\n{}\n",rows.join("\n")
        ));
    }
    let negative=policy["peerMethodLearning"]["observations"].as_array()
        .into_iter().flatten()
        .filter(|row|row["negativeTransferSignal"]==true)
        .take(4)
        .map(|row|format!("donor={} toRole={} transfer {}/{} vs matched non-transfer {}/{}",
            row["donorCandidateId"],row["recipientRole"],
            row["adoptedTransferTrials"],row["observedTransferTrials"],
            row["matchedControlAdoptions"],row["matchedControlTrials"]))
        .collect::<Vec<_>>();
    if !negative.is_empty(){
        message.push_str(&format!(
            "\nOBSERVATIONAL NEGATIVE TRANSFER (not causal proof):\n{}\n\
             Prefer redesigning the method or testing a clean alternative; \
             do not reward a failing foreign method merely for being cited.\n",
             negative.join("\n")));
    }
    message
}

/// Bounded, independently evaluated ideas from compatible foreign lineages.
pub(crate) fn peer_instruction(peers:&Value,policy:&Value)->String {
    // The non-transfer control must not see foreign method prompts.
    // Transferred variants see only the one assigned, source-matched donor.
    if policy["peerMethodTransfer"]["active"]!=true
        || peers["comparisonProfileSha256"]
            !=policy["peerMethodTransfer"]["comparisonProfileSha256"] {
        return String::new();
    }
    let Some(donor)=policy["peerMethodTransfer"]["activeDonorCandidateId"].as_str()
        else{return String::new()};
    let lines=peers["references"].as_array().into_iter().flatten()
        .filter(|row|row["candidateId"]==donor
            && row["comparisonProfileSha256"]==peers["comparisonProfileSha256"])
        .take(1)
        .map(|row|format!(
            "single assigned donor={} roles={} hypothesis={} design={} aligned={}",
            row["campaignId"],row["roles"],row["hypothesis"],
            row["designSummary"],row["alignedSourceFiles"]))
        .collect::<Vec<_>>();
    if lines.is_empty(){String::new()}else{
        format!("\nISOLATED PEER METHOD REFERENCE (ideas only, never automatic patch reuse):\n{}\n",
            lines.join("\n"))
    }
}

