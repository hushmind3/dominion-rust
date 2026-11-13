//! Independently auditable worker trace, model/tool receipt and bounded
//! execution evidence capture. Extracted from the campaign supervisor so
//! token bookkeeping and source provenance have one explicit owner.
use super::*;

pub(super) fn capture_generation_evidence(
    context:&JobContext,
    record:&mut Value,
    input:&Value,
    current:&Value,
    head:&str,
    attempt:u64,
    child_id:&str,
    session:Option<&str>,
    worker_started:Instant,
)->Result<Value,String>{
    let tools_used = session
        .map(|session_id| session_tool_usage(&context.state, session_id))
        .unwrap_or_default();
    let research_fingerprint = session
        .map(|session_id| session_research_fingerprint(&context.state, session_id))
        .unwrap_or_default();
    let capabilities_used = tools_used
        .iter()
        .filter(|row| {
            matches!(
                row["tool"].as_str(),
                Some("code_search" | "workspace_read_range" | "workspace_replace" | "workspace_replace_range")
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    // A reused session must never attribute an older worker's stage
    // transitions to this descendant. Match the worker and exact policy.
    let mut program_stages = context.state.store.all("rsi_program_phase")
        .into_iter().filter(|row|
            row["generationJobId"].as_str()==Some(child_id)
            && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
        .collect::<Vec<_>>();
    program_stages.sort_by(|left,right|
        left["recordedAt"].as_str().unwrap_or("").cmp(
            right["recordedAt"].as_str().unwrap_or("")));
    if program_stages.len()>48 {
        program_stages.drain(..program_stages.len()-48);
    }
    // The next generator needs actual ordered tool outcomes, not merely
    // a model-written description. Keep only bounded, non-secret fields.
    let mut tool_timeline = session.map(|id| context.state.store.all("execution_operation")
        .into_iter().filter(|op|op["sessionId"].as_str()==Some(id)
            && op["origin"]=="model")
        .map(|op| json!({
            "runId":op["runId"],"opSeq":op["opSeq"],
            "startedAt":op["startedAt"],
            "tool":op["tool"],"status":op["status"],
            "path":op["arguments"]["path"],
            "result":{"written":op["result"]["written"],
                "postSha256":op["result"]["postSha256"]},
            "toolReceipt":{"verified":op["toolReceipt"]["verified"]},
        })).collect::<Vec<_>>()).unwrap_or_default();
    tool_timeline.sort_by(|a,b|a["startedAt"].as_str().unwrap_or("")
        .cmp(b["startedAt"].as_str().unwrap_or(""))
        .then_with(||a["opSeq"].as_u64().unwrap_or(0)
            .cmp(&b["opSeq"].as_u64().unwrap_or(0))));
    if tool_timeline.len()>64 {tool_timeline.drain(..tool_timeline.len()-64);}
    let mut rule_executions=context.state.store.all("rsi_rule_execution")
        .into_iter().filter(|row|
            row["generationJobId"].as_str()==Some(child_id)
            && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
        .collect::<Vec<_>>();
    rule_executions.sort_by(|a,b|a["recordedAt"].as_str().unwrap_or("")
        .cmp(b["recordedAt"].as_str().unwrap_or("")));
    if rule_executions.len()>32 {rule_executions.drain(..rule_executions.len()-32);}
    let usage = session
        .map(|s| context.state.store.session_usage(s))
        .unwrap_or(Value::Null);
    let tokens = usage["totalTokens"].as_u64().unwrap_or(0);
    record["tokensEstimated"] = json!(
        record["tokensEstimated"] == true || usage["estimatedCalls"].as_u64().unwrap_or(0) > 0
    );
    record["tokens"] = json!(record["tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(tokens));
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
    let evaluation_profile_sha=
        crate::engine::rsi_profile::from_input(input)?["sha256"].clone();
    // Keep the serialized evidence schema unchanged. One giant json!
    // expansion overflows Rust's default macro recursion limit and
    // prevents a freshly rebuilt successor from compiling at all.
    // Independent fragments have the same keys and are flattened into
    // the generation journal before any evaluation or adoption.
    let generation_details = [
        json!({
            "researchDelivered":current["rsiResearchDelivered"],
            "workerMarker":current["result"]["rsi"]["fixturePhase"],
            "boundaryResult":current["rsiBoundaryResult"],
            "elapsedMs":worker_started.elapsed().as_millis() as u64,
            "domain":current["rsiDomain"],
            "toolsUsed":tools_used,
            "builtInToolsUsed":capabilities_used,
            "inheritedImplementations":current["input"]["rsiInheritedImplementations"],
            "systemEvolution":input["rsiSystemEvolution"],
            // The default frozen evaluator has an actual profile digest.
            "evaluationProfileSha256":evaluation_profile_sha,
            "architectureTarget":current["input"]["rsiArchitectureTarget"],
            "systemBlueprint":current["input"]["rsiSystemBlueprint"],
            "improvementDirective":current["input"]["rsiImprovementDirective"],
            "methodTransfer":current["input"]["rsiMethodTransfer"],
            "methodTransferVerified":current["rsiMethodTransferVerified"],
            "feedbackRepairSelected":current["input"]["rsiFeedbackRepairSelected"]
        }),
        json!({
            "lineageSha256":current["input"]["rsiLineageCapsule"]["sha256"],
            "lineageVerified":current["rsiLineageVerified"],
            "comparativeMode":current["input"]["rsiComparativeMode"],
            "successorStage":current["input"]["rsiSuccessorStage"],
            "interactionGraphSha256":current["input"]["rsiInteractionGraph"]["sha256"],
            "coDesignPortfolio":current["input"]["rsiCoDesignPortfolio"],
            "factorialExperiment":current["input"]["rsiFactorialExperiment"],
            "reflexiveProgram":current["input"]["rsiReflexiveProgram"],
            "openImprovementProgram":current["input"]["rsiOpenImprovementProgram"],
            "metaEvolutionProgram":current["input"]["rsiMetaEvolutionProgram"],
            "principleLearning":current["input"]["rsiPrincipleLearning"],
            "apiRoutePolicy":current["input"]["rsiApiRoutePolicy"],
            "mutualProgram":current["input"]["rsiMutualProgram"]
        }),
        json!({
            "compiledPlanExecuted":current["rsiCompiledPlanExecuted"],
            "experimentContract":current["input"]["rsiExperimentContract"],
            "behaviorCounterfactual":current["input"]["rsiBehaviorCounterfactual"],
            "peerReferenceCandidateIds":current["input"]["rsiPeerLineages"]["references"]
                .as_array().map(|rows|rows.iter().filter_map(|row|
                    row["candidateId"].as_str()).take(4).collect::<Vec<_>>()).unwrap_or_default(),
            "controllerPlan":current["input"]["rsiControllerPlan"],
            "systemBlueprintVerified":current["rsiSystemBlueprintVerified"],
            "methodProposals":session.and_then(|id|context.state.store.get("session",id))
                .map(|row|row["rsiMethodProposals"].clone()).unwrap_or(Value::Null),
            "systemDesign":session.and_then(|id|context.state.store.get("session",id))
                .map(|row|row["rsiSystemDesign"].clone()).unwrap_or(Value::Null),
            "hypothesisMessageId":current["result"]["rsi"]["modelMessageId"],
            "researchFingerprint":research_fingerprint,
            "editEvidence":current["result"]["rsi"]["editEvidence"],
            "editPolicy":current["input"]["rsiEditPolicy"],
            "programStages":program_stages
        }),
        json!({
            "editMethodReward":Value::Null,
            "diffBytes":current["result"]["rsi"]["diffBytes"],
            "captureError":current["result"]["rsi"]["captureError"],
            "repairFailure":current["result"]["repairError"],
            "nativeMutation":current["result"]["nativeMutation"],
            "candidateOrigin":current["result"]["rsi"]["candidateOrigin"],
            "compilerRepairReceipts":current["result"]["compilerRepairReceipts"],
            "accepted":false,
            "createdAt":chrono::Utc::now().to_rfc3339()
        }),
    ];
    for evidence in generation_details {
        generation.as_object_mut().expect("generation object")
            .extend(evidence.as_object().expect("evidence object").clone());
    }
    generation["requestedModel"]=current["input"]["model"].clone();
    generation["requestedProviderId"]=current["input"]["providerId"].clone();
    generation["apiModelPolicy"]=current["input"]["rsiApiRoutePolicy"].clone();
    let api_route_usage=session.map(|id|context.state.store.api_route_usage(
        id,current["input"]["providerId"].as_str().unwrap_or(""),
        current["input"]["model"].as_str().unwrap_or("")))
        .unwrap_or(Value::Null);
    generation["apiRouteUsage"]=api_route_usage.clone();
    // A session using any additional model is not clean evidence of
    // the selected coder route's contribution to source-task quality.
    let audited_routes=session.and_then(|id|
        context.state.store.session_model_routes(id).ok()).unwrap_or_default();
    generation["apiRouteSessionExclusive"]=json!(
        audited_routes.len()==1 && audited_routes[0]["providerId"]
            ==current["input"]["providerId"]
            && audited_routes[0]["model"]==current["input"]["model"]);
    // Bind usage evidence to the original worker session so the next
    // compiled descendant can replay it from the protected audit log.
    generation["apiRouteSessionId"]=json!(session);
    generation["apiRouteExecutionVerified"]=json!(
        current["input"]["rsiAdaptiveApiRouting"]==true
        && current["input"]["rsiApiRoutePolicy"]["chosen"]["model"]
            ==current["input"]["model"]
        && current["input"]["rsiApiRoutePolicy"]["chosen"]["providerId"]
            ==current["input"]["providerId"]
        && api_route_usage["matchedExactProviderModelSession"]==true
    );
    // The meta-policy was chosen by the compiled generator BEFORE it
    // observed this generation's result. Keep it separate from measured
    // feedback to avoid look-ahead and let later descendants learn.
    generation["metaPolicy"]=current["input"]["rsiMetaPolicy"].clone();
    generation["metaPolicyVerified"]=current["rsiMetaPolicyVerified"].clone();
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
    Ok(generation)
}
