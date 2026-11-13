//! Independent handoff and quality evidence aggregation (not the mutable planner).
use super::{design_implemented,design_roles};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

/// Whole-system handoff evidence is separate from a source patch's score.
/// Coverage means source-backed redesigned roles, not measured intelligence
/// in every role. The actual downstream A/B remains the performance evidence.
pub(crate) fn handoff_evidence(state:&crate::state::AppState, history:&[Value])->Value {
    let profile=history.iter().find_map(|generation|state.store.get("job",generation["campaignId"].as_str()?)
        .and_then(|job|crate::engine::rsi_profile::from_input(&job["input"]).ok()))
        .unwrap_or_else(crate::engine::rsi_profile::default_profile);
    let required=profile["requiredComponents"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>();
    let mut chain=0usize;let mut handoffs=0usize;let mut prior_commit=String::new();
    let mut binaries=BTreeSet::new();let mut covered=BTreeSet::new();
    let mut methods=BTreeMap::<String,String>::new();let mut method_transfer=false;
    let mut meta_gain=false;let mut verified=0usize;
    for generation in history {
        if generation["accepted"]!=true {continue;}
        let Some(candidate)=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id)) else{continue;};
        let design=&generation["systemDesign"];
        let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
        let generator=generation["generatorCommit"].as_str().unwrap_or("");
        let accepted=generation["acceptedCommit"].as_str().unwrap_or("");
        let binary=generation["generatorBinarySha256"].as_str().unwrap_or("");
        let controller=&generation["controllerPlan"];
        let valid=generation["systemEvolution"]==true
            && (generation["evaluationProfileSha256"].is_null() || generation["evaluationProfileSha256"]==profile["sha256"])
            && candidate["status"]=="verified" && candidate["origin"]=="model_driven_detached_worktree"
            && candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
            && candidate["baseCommit"]==generator && candidate["experimentAdoption"]["commit"]==accepted
            && crate::engine::rsi::verified_pair_receipt(state,&candidate)
            && generation["systemBlueprintVerified"]==true
            && generation["systemBlueprint"]["generatorCommit"]==generator
            && controller["generatorCommit"]==generator
            && controller["source"]=="compiled_descendant_system_controller_v2"
            && controller["systemBlueprint"]==generation["systemBlueprint"]
            && design["source"]=="model_authored_system_design"
            && design["generatorCommit"]==generator && design["generationJobId"]==generation["jobId"]
            && design_implemented(design,&changed)
            && design["targetFiles"].as_array().is_some_and(|paths|paths.iter().all(|path|
                path.as_str().is_some_and(|path|generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|sha|sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
            && generation["tokenUsage"]["calls"].as_u64().unwrap_or(0)>0
            && generator.len()==40 && accepted.len()==40 && binary.len()==64;
        if !valid {continue;}
        verified+=1;
        if chain>0 && (generator!=prior_commit || binaries.contains(binary)) {
            chain=0;handoffs=0;covered.clear();binaries.clear();methods.clear();
            method_transfer=false;meta_gain=false;
        }
        if chain>0 {handoffs+=1;}
        for execution in generation["methodExecutions"].as_array().into_iter().flatten() {
            if execution["result"]["verified"]!=true || execution["result"]["followed"]!=true {continue;}
            if let (Some(id),Some(revision))=(execution["methodId"].as_str(),execution["revision"].as_str()) {
                if methods.get(id).is_some_and(|expected|expected==revision)
                    && state.store.all("experience").iter().any(|record|
                        record["executableMethods"].as_array().into_iter().flatten().any(|method|
                            method["id"]==id && method["revision"]==revision
                            && method["applications"].as_array().into_iter().flatten().any(|app|
                                app["jobId"]==generation["jobId"] && app["methodWroteAcceptedCandidate"]==true
                                && app["verificationVerdict"]=="passed")))
                    && state.store.get("job",generation["jobId"].as_str().unwrap_or(""))
                        .and_then(|job|job["sessionId"].as_str().map(str::to_string))
                        .is_some_and(|session|crate::engine::rsi_evolve::method_wrote_accepted_candidate(
                            state,generation,execution,&session)) {method_transfer=true;}
            }
        }
        for proposal in generation["methodProposals"].as_array().into_iter().flatten() {
            if proposal["sourceGenerationJobId"]==generation["jobId"] && proposal["synthesis"].is_null()
                && proposal["verification"]["type"]=="procedure"
                && proposal["verification"]["nodes"].as_array().is_some_and(|nodes|!nodes.is_empty()) {
                if let (Some(id),Some(revision))=(proposal["id"].as_str(),proposal["revision"].as_str()) {
                    methods.insert(id.to_string(),revision.to_string());
                }
            }
        }
        for role in design_roles(design){covered.insert(role);}
        meta_gain|=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0;
        chain+=1;prior_commit=accepted.into();binaries.insert(binary.to_string());
    }
    let missing=required.iter().filter(|role|!covered.contains(**role)).copied().collect::<Vec<_>>();
    let interaction=crate::engine::rsi_interaction::influence_graph(history,&[]);
    let reciprocal=crate::engine::rsi_mutual_program::completed_cycles(history);
    let reflexive=crate::engine::rsi_reflexive::completed(history);
    let open_programs=crate::engine::rsi_program_graph::handoff(history);
    let principle_learning=crate::engine::rsi_principle_learning::study(history);
    let approved_api_routes=history.iter().rev()
        .find_map(|g|g["apiModelPolicy"]["authorizedRoutes"].as_array()
            .map(|_|g["apiModelPolicy"]["authorizedRoutes"].clone()))
        .unwrap_or(Value::Null);
    let api_learning=crate::engine::rsi_api_route_learning::learn(
        history,&approved_api_routes);
    let verified_api_calls=history.iter().filter(|g|
        g["apiRouteExecutionVerified"]==true).count();
    let api_heldout_cases=api_learning["routes"].as_array().into_iter().flatten()
        .map(|r|r["independentlyMeasuredCases"].as_u64().unwrap_or(0))
        .sum::<u64>();
    let principle_rows=principle_learning["principles"].as_array().cloned().unwrap_or_default();
    let improved_task_cases=principle_rows.iter()
        .map(|r|r["improvedCases"].as_u64().unwrap_or(0)).sum::<u64>();
    let improved_descendant_cases=principle_rows.iter()
        .map(|r|r["nextGenerationImprovements"].as_u64().unwrap_or(0)).sum::<u64>();
    let experiment_methods=crate::engine::rsi_reflexive::completed_method_cycles(history);
    let transfer=crate::engine::rsi_mutual_program::transfer_graph(history);
    let edges=interaction["edges"].as_array().cloned().unwrap_or_default();
    let cross=edges.iter().filter(|edge|
        edge["sourceRole"]!=edge["targetRole"]
            && edge["isolatedGains"].as_u64().unwrap_or(0)>0).count();
    let reciprocal_pairs=edges.iter().filter(|edge|
        edge["sourceRole"]!=edge["targetRole"]
            && edge["isolatedGains"].as_u64().unwrap_or(0)>0
            && edges.iter().any(|other|
                other["sourceRole"]==edge["targetRole"]
                    && other["targetRole"]==edge["sourceRole"]
                    && other["isolatedGains"].as_u64().unwrap_or(0)>0))
        .count()/2;
    // Preserve the entire handoff evidence schema while avoiding the
    // compiler macro recursion limit on a monolithic JSON literal.
    let evidence_parts = [
        json!({"evaluationProfileId":profile["id"],"evaluationProfileSha256":profile["sha256"],"verifiedSystemGenerations":verified,"verifiedLineageLength":chain,"rebuiltHandoffs":handoffs,
        "sourceChangedComponents":covered,"requiredComponents":required,"missingComponents":missing,
        "metaImprovementObserved":meta_gain,"inventedMethodTransferred":method_transfer,
        "crossSubsystemPairedEffects":cross,
        "reciprocalSubsystemPairsObserved":reciprocal_pairs,
        "crossSubsystemEvidenceSha256":interaction["sha256"],
}),
        json!({"crossGenerationTransferGraphSha256":transfer["sha256"],
        "acceptedReciprocalRevisions":reciprocal["completedCycles"],
        "acceptedReciprocalLineageSha256":reciprocal["sha256"],
        "reciprocalCycleEvidence":reciprocal["cycles"],
        "acceptedReflexiveLoops":reflexive["completedLoops"],
        "reflexiveLineageSha256":reflexive["sha256"],
        "verifiedMetaCapabilityMetaCycles":reflexive["loops"],
        "completedExperimentMethodCycles":experiment_methods["completedMethodCycles"],
        "experimentMethodLineageSha256":experiment_methods["sha256"],
        "verifiedMethodRedesignAndExecution":experiment_methods["cycles"],
        "openImprovementAdoptedSteps":open_programs["independentlyAdoptedProgramSteps"],
        "openImprovementExecutedRewrites":open_programs["executedProgramRewrites"],
        "openImprovementLineageSha256":open_programs["sha256"],
        "openImprovementSourceEvidence":open_programs["steps"],
        "openImprovementProgramRewrites":open_programs["rewrites"],
}),
        json!({"learnedImprovementPrincipleCount":principle_rows.len(),
        "independentlySupportedTransferablePrinciples":
            principle_learning["transferFeatures"].as_array()
                .map(|features|features.iter().filter(|row|
                    row["transferEligible"]==true).count()).unwrap_or(0),
        "learnedPrincipleLedgerSha256":principle_learning["sha256"],
        "apiExecutionRouteLearningSha256":api_learning["sha256"],
        "verifiedApiCoderGenerationCalls":verified_api_calls,
        "apiRouteObservedHoldoutCases":api_heldout_cases,
        "apiRouteModelWeightUpdates":0,
        "apiRouteQualityEvidence":"observational_source_task_proxy_not_direct_model_AB",
        "independentlyImprovedTaskCases":improved_task_cases,
        "independentlyImprovedDescendantCases":improved_descendant_cases,
        "learnedPrincipleModels":principle_rows,
}),
        json!({"crossSubsystemAttribution":"independent_paired_observation_not_isolated_causal_proof",
        // Independent compiled-method handoff is an L2 lineage property;
        // cross-task improvements and executed method redesign are separate.
        "structuralHandoffVerified":chain>=3 && handoffs>=2
            && missing.is_empty() && method_transfer && meta_gain,
        "metaQualityEvidenceVerified":improved_descendant_cases>0
            && (open_programs["executedProgramRewrites"].as_u64().unwrap_or(0)>0
                || experiment_methods["completedMethodCycles"].as_u64().unwrap_or(0)>0),

        // A source adoption streak is not enough to demonstrate that the
        // improvement method actually learns to make BETTER descendants.
        // Require observed cross-role task improvement and an independently
        // executed improvement-method rewrite before claiming the closed loop.
        "demonstrated":chain>=3 && handoffs>=2 && missing.is_empty()
            && method_transfer && meta_gain
            && improved_descendant_cases>0
            && (open_programs["executedProgramRewrites"].as_u64().unwrap_or(0)>0
                || experiment_methods["completedMethodCycles"].as_u64().unwrap_or(0)>0),
        "coverageMeaning":"independently adopted source roles; performance is measured separately by downstream tasks"}),
    ];
    let mut evidence = json!({});
    for part in evidence_parts {
        evidence.as_object_mut().expect("handoff evidence object")
            .extend(part.as_object().expect("evidence segment").clone());
    }
    // Structural succession is not a measured recursive intelligence gain.
    let causal=crate::engine::rsi_causal_closure::audit(state,history);
    evidence["causalClosure"]=causal.clone();
    evidence["demonstrated"]=json!(evidence["demonstrated"]==true &&
        causal["demonstrated"]==true);
    evidence
}
