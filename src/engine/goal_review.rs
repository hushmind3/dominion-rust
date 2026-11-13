//! Durable boundary between an agent's possibly side-effecting tool turn and
//! its read-only review phase. A checkpoint must contain the complete inputs
//! needed to repeat verification WITHOUT repeating an external operation.
use serde_json::{json, Value};

pub(crate) const VERSION: u64 = 1;

/// Legacy/partial checkpoints are never optimistically resumed.
pub(crate) fn replayable(job: &Value) -> bool {
    let state = &job["goalState"];
    state["reviewPending"] == true
        && state["inFlight"] == false
        && state["reviewVersion"].as_u64() == Some(VERSION)
        && state["nextIteration"].as_u64().is_some_and(|n| n > 0)
        && state["sessionId"].as_str().is_some_and(|s| !s.is_empty())
        && state["evidence"].is_array()
        && state["transcript"].is_string()
        && state["gaps"].as_array().is_some_and(|xs| xs.iter().all(Value::is_string))
        && state["toolErrors"].as_array().is_some_and(|xs| xs.iter().all(Value::is_string))
        && (state["turnError"].is_null() || state["turnError"].is_string())
}

pub(crate) fn replay_iteration(job: &Value) -> Option<usize> {
    replayable(job).then(|| {
        job["goalState"]["nextIteration"].as_u64().unwrap_or(1).saturating_sub(1) as usize
    })
}

pub(crate) fn recorded_errors(job: &Value) -> (Option<String>, Vec<String>) {
    let state = &job["goalState"];
    let error = state["turnError"].as_str().map(str::to_owned);
    let tools = state["toolErrors"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_owned).collect();
    (error, tools)
}

/// The store commits this document ONLY after the tool turn has finished.
/// It deliberately contains no tool instructions, authority, or replay cursor
/// that could authorize a second execution of the same side effect.
pub(crate) fn snapshot(
    session_id: &str, iteration: usize, evidence: &[Value], transcript: &str,
    gaps: &[String], turn_error: Option<&str>, tool_errors: &[String],
) -> Value {
    json!({
        "sessionId": session_id,
        "nextIteration": iteration + 1,
        "inFlight": false,
        "reviewPending": true,
        "reviewVersion": VERSION,
        "evidence": evidence,
        "transcript": transcript,
        "gaps": gaps,
        "turnError": turn_error,
        "toolErrors": tool_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_read_only_review_snapshots_can_resume() {
        let state = snapshot("s", 2, &[], "transcript", &[], None, &[]);
        let job = json!({"goalState": state});
        assert!(replayable(&job));
        assert_eq!(replay_iteration(&job), Some(2));
        assert_eq!(recorded_errors(&job), (None, vec![]));
        for patch in [
            json!({"reviewPending":false}),
            json!({"inFlight":true}),
            json!({"reviewVersion":0}),
            json!({"toolErrors":null}),
            json!({"evidence":null}),
            json!({"nextIteration":0}),
        ] {
            let mut damaged = job.clone();
            for (key, value) in patch.as_object().unwrap() {
                damaged["goalState"][key] = value.clone();
            }
            assert!(!replayable(&damaged), "unsafe checkpoint: {patch}");
        }
        assert!(!replayable(&json!({"goalState":{"reviewPending":true,"inFlight":false}})));
    }

    #[test]
    fn reviewer_replay_retains_previous_tool_failure_diagnostics() {
        let stage = snapshot("s", 0, &[], "a", &["gap".into()],
            Some("provider error"), &["shell_exec failed".into()]);
        let job = json!({"goalState":stage});
        assert_eq!(recorded_errors(&job), (
            Some("provider error".into()), vec!["shell_exec failed".to_string()]
        ));
    }
}
