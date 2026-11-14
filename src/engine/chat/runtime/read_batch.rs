//! Model-order parallel read execution. A write/control call is an exclusive
//! barrier; malformed JSON and failures still produce one response per ID.
use super::*;
use futures_util::StreamExt;

/// Number of leading tool calls eligible to overlap before the first
/// exclusive/unknown side-effect barrier. Preserves the model's ordering.
pub(super) fn parallel_read_prefix_len<T>(
    calls: &[T], is_read: impl Fn(&T) -> bool,
) -> usize {
    calls.iter().position(|call| !is_read(call)).unwrap_or(calls.len())
}

/// Form one tool response per model tool-call ID in **model order**, even if
/// some arguments are invalid or individual parallel tasks return earlier.
fn parallel_tool_messages<T>(
    calls: &[T],
    responses: Vec<Option<String>>,
    call_id: impl Fn(&T) -> &str,
) -> Vec<Value> {
    let missing = json!({"error":"missing parallel tool result"}).to_string();
    calls.iter().enumerate().map(|(index, call)| json!({
        "role":"tool",
        "tool_call_id":call_id(call),
        "content":responses.get(index).and_then(Option::as_deref).unwrap_or(
            missing.as_str()
        )
    })).collect()
}

#[cfg(test)]
mod parallel_read_order_tests {
    use super::parallel_read_prefix_len;
    use serde_json::json;

    #[test]
    fn parallel_reads_respect_first_write_barrier() {
        let calls = ["read", "read", "write", "read", "read"];
        assert_eq!(parallel_read_prefix_len(&calls, |call| *call == "read"), 2);
        assert_eq!(parallel_read_prefix_len(&calls[2..], |call| *call == "read"), 0);
        assert_eq!(parallel_read_prefix_len(&calls[3..], |call| *call == "read"), 2);
    }

    #[test]
    fn malformed_and_successful_parallel_results_preserve_model_call_id_order() {
        let calls = ["a", "b", "c"];
        let messages = super::parallel_tool_messages(&calls,
            vec![Some("result-a".into()), None, Some("result-c".into())],
            |call| *call);
        assert_eq!(messages.len(),3);
        assert_eq!(messages[0]["tool_call_id"],"a");
        assert_eq!(messages[1]["tool_call_id"],"b");
        assert_eq!(messages[2]["tool_call_id"],"c");
        assert_eq!(messages[0]["content"],"result-a");
        assert!(messages[1]["content"].as_str().unwrap().contains("missing parallel tool result"));
        assert_eq!(messages[2]["content"],"result-c");
        let bad = json!({"error":"bad arguments"}).to_string();
        let mixed = super::parallel_tool_messages(&calls,
            vec![Some(bad.clone()), Some("success".into()),
                Some("later".into())],|call|*call);
        assert_eq!(mixed[0]["tool_call_id"],"a");
        assert_eq!(mixed[1]["tool_call_id"],"b");
        assert_eq!(mixed[0]["content"],bad);
    }

    #[test]
    fn unknown_or_exclusive_calls_stay_serial() {
        let calls = ["mcp_unknown", "read", "read"];
        assert_eq!(parallel_read_prefix_len(&calls, |call| *call == "read"), 0);
        let reads = ["read", "read", "read"];
        assert_eq!(parallel_read_prefix_len(&reads, |call| *call == "read"), 3);
        let empty: [&str; 0] = [];
        assert_eq!(parallel_read_prefix_len(&empty, |call| *call == "read"), 0);
    }
}


/// Execute only the contiguous read prefix already selected by the runtime.
/// Tool-operation receipts, token/proposal budgets, cancellation and model
/// transcript order are owned by the same native session.
pub(super) async fn execute_parallel_read_prefix(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut budget::BudgetLedger,
    generation: u64,
    op_seq: &mut u64,
    round: usize,
    compat: bool,
    read_calls: &[ToolCall],
    messages: &mut Vec<Value>,
) -> Result<(),String> {
    debug_assert!(read_calls.len() >= 2);
                // Proposed reads are real tool calls and consume the same
                // per-turn proposal budget as every serial tool dispatch.
                for _ in read_calls {
                    if !budget.charge_proposal() {
                        mark_budget_exhausted(state, execution_id);
                        return Err(format!(
                            "도구 제안 예산({}회)을 초과했습니다.",
                            MAX_TOOL_CALLS.saturating_mul(2)
                        ));
                    }
                }
                if state.is_cancelled(session_id)
                    || state.cancellation_generation(session_id) != generation {
                    return Err("실행이 중단되었습니다.".into());
                }
                // Only this applicable run owns a combination usage record;
                // a single read is not a combination measurement.
                let _ = crate::engine::algorithms::choose_for_agent(
                    state,
                    "tool.combination",
                    request.agent_id.as_deref(),
                    Some(execution_id),
                );
                let base_seq = *op_seq;
                *op_seq += read_calls.len() as u64;
                let mut futures = Vec::new();
                let mut valid_calls = Vec::new();
                // Tool response slots correspond to model call indices,
                // including invalid JSON. Completion order and early errors
                // must not reorder the transcript sent to the next model.
                let mut responses = vec![None::<String>; read_calls.len()];
                for (offset, call) in read_calls.iter().enumerate() {
                    match serde_json::from_str::<Value>(&call.arguments) {
                        Ok(arguments) => {
                            valid_calls.push((call.clone(), offset));
                            futures.push(dispatch_tool_call(
                                state,
                                tx,
                                &session,
                                request,
                                session_id,
                                execution_id,
                                call,
                                arguments,
                                metrics,
                                base_seq + offset as u64 + 1,
                                round,
                                offset,
                                compat,
                                "model",
                                None,
                                None,
                                None,
                            ));
                        }
                        Err(error) => {
                            // Every tool_call needs a matching tool message.
                            // Dropping a malformed read would leave the next
                            // provider round with an unanswered call id.
                            responses[offset] = Some(json!({
                                "error":format!("invalid tool arguments: {error}")
                            }).to_string());
                            crate::engine::algorithms::mark_signal(
                                state, execution_id, "tool.selection", false,
                            );
                            crate::engine::algorithms::mark_signal(
                                state, execution_id, "recovery.strategy", false,
                            );
                            mark_tool_argument_signal(state, request, execution_id, false);
                        }
                    }
                }
                // A bounded pool prevents model-generated read fan-out
                // from overwhelming local files, web tools or provider limits.
                // buffered() preserves the original call order of results.
                let results = futures_util::stream::iter(futures)
                    .buffered(4).collect::<Vec<_>>().await;
                // A dispatch can complete while the tool itself failed
                // (`Ok((value, false, content))`). Combination credit requires
                // every combined read to actually succeed.
                crate::engine::algorithms::mark_signal(
                    state,
                    execution_id,
                    "tool.combination",
                    valid_calls.len() == read_calls.len() && results.iter().all(|result| {
                        result.as_ref().is_ok_and(|(_, ok, _)| *ok)
                    }),
                );
                for ((_call, offset), result) in valid_calls.iter().zip(results.into_iter()) {
                    let (content, ok) = match result {
                        Ok((_value, ok, content)) => (content, ok),
                        Err(error) => (json!({"error":error}).to_string(), false),
                    };
                    responses[*offset] = Some(content);
                    crate::engine::algorithms::mark_signal(
                        state, execution_id, "tool.selection", ok,
                    );
                    crate::engine::algorithms::mark_signal(
                        state, execution_id, "recovery.strategy", ok,
                    );
                }
                messages.extend(parallel_tool_messages(read_calls, responses, |call| &call.id));
                // Drain only executed prefix. Later reads remain behind the
                // intervening write/control call, preserving model order.

    Ok(())
}
