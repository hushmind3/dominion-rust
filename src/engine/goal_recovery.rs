//! Distinguish a replay-unsafe tool turn from an idempotent review phase.
//! A completed executor turn has already emitted and durably stored evidence;
//! restarting the read-only model review must never execute its tools again.
use serde_json::Value;

pub(crate) fn safe_review_checkpoint(job: &Value) -> bool {
    let state=&job["goalState"];
    job["kind"]=="goal"
        && state["inFlight"]==false
        && state["reviewPending"]==true
        && state["nextIteration"].as_u64().is_some_and(|n|n>0)
        && state["sessionId"].as_str().is_some_and(|s|!s.is_empty())
        && state["evidence"].is_array()
        && state["transcript"].is_string()
        && state["gaps"].is_array()
        && state["toolErrors"].as_array().is_some_and(|list|list.iter().all(Value::is_string))
        && (state["turnError"].is_null() || state["turnError"].is_string())
        && job["cancelRequested"]!=true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn safe() -> Value {
        json!({"kind":"goal","status":"running","goalState":{
            "sessionId":"s","nextIteration":1,"inFlight":false,
            "reviewPending":true,"evidence":[],"transcript":"",
            "gaps":[],"toolErrors":[],"turnError":null
        }})
    }
    #[test]
    fn only_immutable_review_phase_is_replayable(){
        let checkpoint=safe();
        assert!(safe_review_checkpoint(&checkpoint));
        let mut tool=checkpoint.clone();
        tool["goalState"]["inFlight"]=json!(true);
        assert!(!safe_review_checkpoint(&tool));
        let mut corrupt=checkpoint.clone();
        corrupt["goalState"]["evidence"]=Value::Null;
        assert!(!safe_review_checkpoint(&corrupt));
        let mut cancelled=checkpoint.clone();
        cancelled["cancelRequested"]=json!(true);
        assert!(!safe_review_checkpoint(&cancelled));
    }
}
