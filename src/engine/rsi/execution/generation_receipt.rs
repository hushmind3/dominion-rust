//! Constructs a generation's bounded original worker receipt. This module
//! serializes execution facts only; it does NOT decide adoption, evidence
//! validity, candidate quality, or campaign completion.
use super::*;

pub(super) fn assemble(
    context:&JobContext,input:&Value,current:&Value,record:&Value,
    head:&str,child_id:&str,attempt:u64,worker_started:Instant,
    observation:GenerationObservation<'_>,
)->Value {
    let session=current["sessionId"].as_str();
    let GenerationObservation {
        tools_used,research_fingerprint,capabilities_used,program_stages,
        tool_timeline,rule_executions,usage,selected_provider,selected_model,
        api_route_usage,api_route_exclusive,api_route_verified,tokens
    }=observation;
        let mut generation = json!({"id":format!("{}-{attempt}",context.job_id),"campaignId":context.job_id,
            "attempt":attempt,"generatorCommit":head,"jobId":child_id,"tokens":tokens,
            "tokenUsage":{"promptTokens":usage["promptTokens"],"completionTokens":usage["completionTokens"],
                "calls":usage["calls"],"estimatedCalls":usage["estimatedCalls"]},
            "sourceContextChars":current["input"]["rsiFocusContextChars"],
            "totalPromptChars":current["input"]["rsiPromptContextChars"],
            "focusFiles":current["input"]["rsiFocusFiles"],
            "scopeExpanded":current["input"]["rsiSourceScopeExpanded"],
            "residualEvidence":current["input"]["rsiResidualEvidence"],
            "costOpportunities":current["input"]["rsiCostOpportunities"],
            "metaTransferTarget":current["input"]["rsiMetaTransferTarget"],
            "focusSelection":current["input"]["rsiFocusSelection"],
            "targetReplan":current["rsiTargetReplan"],
            "knowledgePartition":research_scope(&current["input"]).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&[
                target_focus_objective(&current["input"])]),
            "generatorBinarySha256":record["generatorBinarySha256"],
            "generatorModel":current["input"]["model"],
            "generatorProviderId":current["input"]["providerId"],
            "domainSelection":current["rsiDomainSelection"],"searchStrategy":current["rsiSearchStrategy"],
            "decisionInput":current["rsiDecisionInput"],"selectedDomainInput":current["input"]["rsiSelectedDomain"]});
        let generation_details = json!({
            "researchDelivered":current["rsiResearchDelivered"],"workerMarker":current["result"]["rsi"]["fixturePhase"],
            "boundaryResult":current["rsiBoundaryResult"],
            "elapsedMs":worker_started.elapsed().as_millis() as u64,"domain":current["rsiDomain"],
            "toolsUsed":tools_used,"builtInToolsUsed":capabilities_used,
            "inheritedImplementations":current["input"]["rsiInheritedImplementations"],
            "hypothesisMessageId":current["result"]["rsi"]["modelMessageId"],
            "researchFingerprint":research_fingerprint,
            "editEvidence":current["result"]["rsi"]["editEvidence"],
            "editPolicy":current["input"]["rsiEditPolicy"],
            "programStages":program_stages,
            "editMethodReward":Value::Null,
            "diffBytes":current["result"]["rsi"]["diffBytes"],
            "captureError":current["result"]["rsi"]["captureError"],
            "repairFailure":current["result"]["repairError"],
            "nativeMutation":current["result"]["nativeMutation"],
            "candidateOrigin":current["result"]["rsi"]["candidateOrigin"],
            "compilerRepairReceipts":current["result"]["compilerRepairReceipts"],
            "accepted":false,"createdAt":chrono::Utc::now().to_rfc3339()});
        generation.as_object_mut().expect("generation object")
            .extend(generation_details.as_object().expect("generation details object").clone());
        generation["systemEvolution"]=current["input"]["rsiSystemEvolution"].clone();
        generation["systemBlueprint"]=current["input"]["rsiSystemBlueprint"].clone();
        generation["descendantPlan"]=current["input"]["rsiDescendantPlan"].clone();
        generation["controllerPlan"]=current["input"]["rsiControllerPlan"].clone();
        generation["architectureFrontier"]=current["input"]["rsiArchitectureFrontier"].clone();
        generation["architectureTarget"]=current["input"]["rsiArchitectureTarget"].clone();
        generation["metaPolicy"]=current["input"]["rsiMetaPolicy"].clone();
        generation["symbioticCurriculum"]=current["input"]["rsiSymbioticCurriculum"].clone();
        generation["interactionGraph"]=current["input"]["rsiInteractionGraph"].clone();
        generation["negativeTransferPlan"]=current["input"]["rsiNegativeTransferPlan"].clone();
        generation["coupledPortfolio"]=current["input"]["rsiCoupledPortfolio"].clone();
        generation["factorialExperiment"]=current["input"]["rsiFactorialExperiment"].clone();
        generation["recursiveScientificProgram"]=current["input"]["rsiRecursiveScientificProgram"].clone();
        generation["improvementAlgebraPlan"]=current["input"]["rsiImprovementAlgebra"].clone();
        generation["methodGeneralization"]=current["input"]["rsiMethodGeneralization"].clone();
        generation["experimentPolicy"]=current["input"]["rsiExperimentPolicy"].clone();
        generation["experimentPolicyExecuted"]=current["rsiExperimentPolicyExecuted"].clone();
        generation["modelEvolutionPlan"]=current["input"]["rsiModelEvolutionPlan"].clone();
        generation["modelEvolutionPlanExecuted"]=current["rsiModelEvolutionPlanExecuted"].clone();
        generation["modelSelectionActual"]=json!({
            "providerId":current["input"]["providerId"],
            "model":current["input"]["model"],
            "ownerOrigin":current["input"]["rsiModelOrigin"],
            "generationJobId":child_id
        });
        generation["requestedProviderId"]=json!(selected_provider);
        generation["requestedModel"]=json!(selected_model);
        generation["apiRouteSessionId"]=json!(session);
        generation["apiRouteUsage"]=api_route_usage;
        generation["apiRouteSessionExclusive"]=json!(api_route_exclusive);
        generation["apiRouteExecutionVerified"]=json!(api_route_verified);
        // Only compare holdout records from identical frozen case/profile
        // definitions and the same actual compiled generator commit.
        let frozen_profile=json!([
            input["rsiFrozenCases"],input["rsiEvaluationConfig"],
            input["rsiMetaCases"]
        ]);
        if !input["rsiFrozenCases"].is_null() {
            generation["evaluationProfileSha256"]=json!(format!("{:x}",
                Sha256::digest(serde_json::to_vec(&frozen_profile)
                    .unwrap_or_default())));
        }
        generation["successorStage"]=current["input"]["rsiSuccessorStage"].clone();
        generation["successorStageExecuted"]=current["rsiSuccessorStageExecuted"].clone();
        generation["metaPolicyExecuted"]=current["rsiMetaPolicyExecuted"].clone();
        generation["compiledPlanExecuted"]=current["rsiCompiledPlanExecuted"].clone();
        generation["inheritedMetaProgram"]=current["input"]["rsiInheritedMetaProgram"].clone();
        generation["inheritedMetaProgramBound"]=current["rsiInheritedMetaProgramBound"].clone();
        generation["crossDomainMetaMethodTransfer"]=current["input"]["rsiEditPolicy"]["crossDomainTransfer"].clone();
        generation["peerLineages"]=current["input"]["rsiPeerLineages"].clone();
        generation["comparativeMode"]=current["input"]["rsiComparativeMode"].clone();
        generation["comparisonProfileSha256"]=json!(
            crate::engine::rsi_meta_controller::comparison_profile(&current["input"]));
        generation["improvementDirective"]=current["input"]["rsiImprovementDirective"].clone();
        generation["systemBlueprintVerified"]=current["rsiSystemBlueprintVerified"].clone();
        generation["improvementDirectiveVerified"]=current["rsiImprovementDirectiveVerified"].clone();
        generation["systemDesign"]=session.and_then(|id|context.state.store.get("session",id))
            .map(|row|row["rsiSystemDesign"].clone()).unwrap_or(Value::Null);
        generation["candidateOrigin"]=current["result"]["rsi"]["candidateOrigin"].clone();
        generation["requestedModel"]=input["model"].clone();
        if generation["candidateOrigin"]=="native_inductive_source_mutation" {
            generation["generatorModel"]=json!(format!("dominion-native:{}",
                generation["nativeMutation"]["operator"].as_str().unwrap_or("unknown")));
            generation["generatorProviderId"]=json!("dominion-native");
        }
        // Keep the generation JSON macro below Rust's recursive expansion
        // ceiling and attach bounded evidence only after building the core
        // generation receipt.
        generation["toolTimeline"]=json!(tool_timeline);
        generation["ruleExecutions"]=json!(rule_executions);
        // A successful native write can still produce uncompilable Rust.
        // Keep the *real compiler diagnostic* and the pre-repair history
        // separate from independently eligible improvements. This makes
        // next-generation repair synthesis learn WHY a patch failed.
        let check_diagnostics=current["result"]["rsi"]["checks"].as_array()
            .into_iter().flatten().take(4).map(|check|json!({
                "label":check["label"],"passed":check["passed"],
                "exitCode":check["exitCode"],
                "detail":check["stderr"].as_str()
                    .or_else(||check["error"].as_str()).unwrap_or("")
                    .chars().take(1600).collect::<String>()
            })).collect::<Vec<_>>();
        let prior_diagnostics=current["result"]["previousAttempts"].as_array()
            .into_iter().flatten().take(3).map(|attempt|{
                let first=attempt["result"]["checks"].as_array()
                    .into_iter().flatten().find(|c|c["passed"]!=true);
                json!({
                    "attempt":attempt["attempt"],
                    "checksPassed":attempt["result"]["checksPassed"],
                    "error":first.and_then(|c|c["stderr"].as_str()
                        .or_else(||c["error"].as_str()))
                        .unwrap_or("").chars().take(1200).collect::<String>()
                })
            }).collect::<Vec<_>>();
        generation["inheritedCandidateId"]=current["input"]["rsiInheritedCandidateId"].clone();
        generation["archiveSelection"]=current["input"]["rsiArchiveSelection"].clone();
        generation["inheritedPatchChanged"]=current["result"]["rsi"]["inheritedPatchChanged"].clone();
        generation["verification"]=json!({
            "rawCandidateId":current["result"]["rsi"]["candidateId"],
            "checksPassed":current["result"]["rsi"]["checksPassed"],
            "reason":current["result"]["reason"],
            "checks":check_diagnostics,
            "repairAttempts":current["result"]["repairAttempts"],
            "repairHistory":prior_diagnostics,
        });
        generation
}
