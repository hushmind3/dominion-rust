//! Bounded, original-session-only tool and model receipts used by the
//! campaign; the mutable coordinator does not own telemetry collection.
//! Values are sourced from durable executions, not model-written summaries.
use super::*;

pub(super) struct GenerationObservation<'a> {
    pub(super) tools_used: Vec<Value>,
    pub(super) research_fingerprint: String,
    pub(super) capabilities_used: Vec<Value>,
    pub(super) program_stages: Vec<Value>,
    pub(super) tool_timeline: Vec<Value>,
    pub(super) rule_executions: Vec<Value>,
    pub(super) usage: Value,
    pub(super) selected_provider: &'a str,
    pub(super) selected_model: &'a str,
    pub(super) api_route_usage: Value,
    pub(super) api_route_exclusive: bool,
    pub(super) api_route_verified: bool,
    pub(super) tokens: u64,
}

pub(super) fn collect_generation_observation<'a>(
    state:&AppState,current:&'a Value,child_id:&str,
)->GenerationObservation<'a> {
    let session=current["sessionId"].as_str();
    let tools_used = session
        .map(|session_id| session_tool_usage(state, session_id))
        .unwrap_or_default();
    let research_fingerprint = session
        .map(|session_id| session_research_fingerprint(state, session_id))
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
    let mut program_stages = state.store.all("rsi_program_phase")
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
    let mut tool_timeline = session.map(|id| state.store.all("execution_operation")
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
    let mut rule_executions=state.store.all("rsi_rule_execution")
        .into_iter().filter(|row|
            row["generationJobId"].as_str()==Some(child_id)
            && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
        .collect::<Vec<_>>();
    rule_executions.sort_by(|a,b|a["recordedAt"].as_str().unwrap_or("")
        .cmp(b["recordedAt"].as_str().unwrap_or("")));
    if rule_executions.len()>32 {rule_executions.drain(..rule_executions.len()-32);}
    let usage = session
        .map(|s| state.store.session_usage(s))
        .unwrap_or(Value::Null);
    let selected_provider=current["input"]["providerId"].as_str().unwrap_or("");
    let selected_model=current["input"]["model"].as_str().unwrap_or("");
    let api_route_usage=session.map(|id|state.store.api_route_usage(
        id,selected_provider,selected_model)).unwrap_or(Value::Null);
    let observed_routes=session.and_then(|id|
        state.store.session_model_routes(id).ok());
    let api_route_exclusive=observed_routes.as_ref().is_some_and(|routes|
        routes.len()==1 && routes[0]["providerId"]==selected_provider
            && routes[0]["model"]==selected_model);
    let api_route_verified=api_route_exclusive
        && api_route_usage["matchedExactProviderModelSession"]==true
        && current["result"]["rsi"]["modelRouteVerified"]==true
        && current["result"]["rsi"]["modelSessionId"].as_str()==session
        && current["result"]["rsi"]["actualModelRoute"]["providerId"]==selected_provider
        && current["result"]["rsi"]["actualModelRoute"]["model"]==selected_model;
    let tokens = usage["totalTokens"].as_u64().unwrap_or(0);

    GenerationObservation {
        tools_used,
        research_fingerprint,
        capabilities_used,
        program_stages,
        tool_timeline,
        rule_executions,
        usage,
        selected_provider,
        selected_model,
        api_route_usage,
        api_route_exclusive,
        api_route_verified,
        tokens,
    }
}
