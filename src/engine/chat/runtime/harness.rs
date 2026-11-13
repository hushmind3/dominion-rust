//! harness responsibilities of the chat runtime.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_harness_nodes(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    strategy_candidates: &[Value],
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::super::budget::BudgetLedger,
    op_seq: &mut u64,
    generation: u64,
) -> Result<Vec<Value>, String> {
    let mut synthetic = Vec::new();
    for strategy in strategy_candidates {
        if strategy["assistanceMode"].as_str() != Some("harness_assisted") {
            continue;
        }
        if !crate::engine::procedure::applies(strategy, &request.input, None, None) {
            continue;
        }
        let mut results = std::collections::HashMap::new();
        for _ in 0..16 {
            if state.is_cancelled(session_id)
                || state.cancellation_generation(session_id) != generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let operations = state
                .store
                .all("execution_operation")
                .into_iter()
                .filter(|operation| operation["runId"].as_str() == Some(execution_id))
                .collect::<Vec<_>>();
            for operation in &operations {
                if operation["origin"].as_str() == Some("procedure")
                    && operation["procedureStrategyId"].as_str() == strategy["id"].as_str()
                    && operation["procedureRevision"].as_str() == strategy["revision"].as_str()
                    && crate::engine::procedure::operation_succeeded(operation)
                {
                    if let Some(node) = operation["procedureNodeId"].as_str() {
                        results.insert(node.to_string(), operation["result"].clone());
                    }
                }
            }
            let Some((node, tool, arguments)) = crate::engine::procedure::ready_intent(
                strategy,
                &operations,
                &request.input,
                &results,
            ) else {
                break;
            };
            if !budget.charge_tool() {
                return Err(format!(
                    "harness 절차 도구 예산({MAX_TOOL_CALLS}회)을 초과했습니다."
                ));
            }
            *op_seq += 1;
            let call = ToolCall {
                id: format!("procedure-{node}-{}", uuid::Uuid::new_v4()),
                name: tool,
                arguments: arguments.to_string(),
            };
            let (result, ok, content) = dispatch_tool_call(
                state,
                tx,
                session,
                request,
                session_id,
                execution_id,
                &call,
                arguments,
                metrics,
                *op_seq,
                0,
                0,
                true,
                "procedure",
                Some(&node),
                strategy["id"].as_str(),
                strategy["revision"].as_str(),
            )
            .await?;
            results.insert(node.clone(), result);
            synthetic.push(json!({
                "role": "system",
                "content": format!(
                    "Harness procedure node {node} executed (origin=procedure). result: {}",
                    content.chars().take(1200).collect::<String>()
                )
            }));
            if !ok {
                break;
            }
        }
    }
    Ok(synthetic)
}
