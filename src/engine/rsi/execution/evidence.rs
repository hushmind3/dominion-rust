//! Collect actual child-model/tool usage and source-edit observations.
//! This module observes durable execution. It does not choose experiments,
//! change the evaluator, decide adoption or claim recursive closure.
use super::*;

pub(super) fn collect_generation_evidence(
    context:&JobContext,record:&mut Value,current:&Value,
    attempt:u64,child_id:&str,head:&str,worker_elapsed_ms:u64,
)->Value{
    let session=current["sessionId"].as_str();
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
                    Some("code_search" | "workspace_read_range" | "workspace_replace_range" | "workspace_replace" | "workspace_replace_function")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let write_receipts = session
            .map(|s| session_write_receipts(&context.state, s))
            .unwrap_or_default();
        let action_trace = session
            .map(|s| session_action_trace(&context.state,s))
            .unwrap_or_default();
        let method_actions=session.map(|session_id|context.state.store.all("execution")
            .into_iter().filter(|execution|execution["sessionId"]==session_id
                && (execution["rsiMethodDecision"].is_object()
                    || execution["rsiActionProgramDecision"].is_object()
                    || execution["rsiActionProgramDecisionHistory"].is_array()))
            .flat_map(|execution|{
                let decisions=execution["rsiActionProgramDecisionHistory"]
                    .as_array().cloned().filter(|rows|!rows.is_empty())
                    .unwrap_or_else(||execution["rsiActionProgramDecision"]
                        .is_object().then(||vec![execution["rsiActionProgramDecision"].clone()])
                        .unwrap_or_default());
                if decisions.is_empty() {
                    return vec![json!({"executionId":execution["id"],
                        "decision":execution["rsiMethodDecision"],
                        "inducedProgramDecision":Value::Null})];
                }
                decisions.into_iter().map(|choice|json!({
                    "executionId":execution["id"],
                    "decision":execution["rsiMethodDecision"],
                    "inducedProgramDecision":choice,
                })).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()).unwrap_or_default();
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
            "focusFiles":current["input"]["rsiFocusFiles"],
            "focusSelection":current["input"]["rsiFocusSelection"],
            "knowledgePartition":research_scope(&current["input"]).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&[
                current["input"]["rsiObjective"].as_str().unwrap_or("")
            ]),
            "tokenUsage":{"promptTokens":usage["promptTokens"],"completionTokens":usage["completionTokens"],
                "calls":usage["calls"],"estimatedCalls":usage["estimatedCalls"]},
            "sourceContextChars":current["input"]["rsiFocusContextChars"],
            "totalPromptChars":current["input"]["rsiPromptContextChars"],
            "parentContextFit":current["input"]["rsiParentContextFit"],
            "workerContextFit":current["rsiWorkerContextFit"],
            "generatorBinarySha256":record["generatorBinarySha256"],"generatorModel":current["input"]["model"],"generatorProviderId":current["input"]["providerId"],
            "domainSelection":current["rsiDomainSelection"],"searchStrategy":current["rsiSearchStrategy"],
            "decisionInput":current["rsiDecisionInput"],"selectedDomainInput":current["input"]["rsiSelectedDomain"],
            "nativePlan":current["input"]["rsiNativePlan"],
            "improvementDirective":current["input"]["rsiImprovementDirective"],
            "researchDelivered":current["rsiResearchDelivered"],"workerMarker":current["result"]["rsi"]["fixturePhase"],
            "boundaryResult":current["rsiBoundaryResult"],
            "elapsedMs":worker_elapsed_ms,"domain":current["rsiDomain"],
            "toolsUsed":tools_used,"builtInToolsUsed":capabilities_used,
            "writeReceipts":write_receipts,
            "actionTrace":action_trace,
            "executionMethod":current["input"]["rsiExecutionMethod"],
            "methodActions":method_actions,
            "inheritedImplementations":current["input"]["rsiInheritedImplementations"],
            "hypothesisMessageId":current["result"]["rsi"]["modelMessageId"],
            "researchFingerprint":research_fingerprint,
            "accepted":false,"createdAt":chrono::Utc::now().to_rfc3339()});

    generation
}
