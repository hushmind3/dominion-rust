//! One cancellation authority for HTTP, native SDK and durable goal workers.
use crate::state::AppState;
use serde_json::{json,Value};

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum CancelJobError {
    Missing,
    NotRunning,
    Conflict,
}
impl std::fmt::Display for CancelJobError {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        f.write_str(match self {
            Self::Missing=>"작업이 없습니다.",
            Self::NotRunning=>"진행 중인 작업만 중단할 수 있습니다.",
            Self::Conflict=>"작업 상태가 변경되었습니다.",
        })
    }
}
impl std::error::Error for CancelJobError {}

/// Atomically seal cancellation before notifying any active turn.
/// A stale owner request never cancels the session of an already-settled job,
/// and a late worker result cannot overwrite a cancelled job.
pub fn cancel_job(state:&AppState,id:&str)->Result<Value,CancelJobError> {
    let job=state.store.get("job",id).ok_or(CancelJobError::Missing)?;
    if !matches!(job["status"].as_str(),Some("queued"|"running")) {
        return Err(CancelJobError::NotRunning);
    }
    let attached_goal=if job["kind"]=="goal" {
        job["input"]["goalId"].as_str().and_then(|goal_id|state.store.get("goal",goal_id))
    } else {None};
    let session=job["sessionId"].as_str()
        .or_else(||attached_goal.as_ref().and_then(|goal|goal["sessionId"].as_str()))
        .map(str::to_string);
    let now=chrono::Utc::now().to_rfc3339();
    let saved=state.store.patch_if("job",id,"status",&job["status"],
        &json!({"status":"cancelled","cancelRequested":true,"finishedAt":now}))
        .ok_or(CancelJobError::Conflict)?;
    state.bus.record("job",&saved);
    if let Some(session)=session {
        state.cancel_session(&session);
    }
    if let Some(goal)=attached_goal {
        if !matches!(goal["status"].as_str(),Some("completed"|"cancelled"|"failed"|"needs_review")) {
            if let Some(goal_id)=goal["id"].as_str() {
                if let Some(stopped)=state.store.patch_if("goal",goal_id,"status",&goal["status"],
                    &json!({"status":"cancelled","phase":"cancelled",
                        "finishedAt":chrono::Utc::now().to_rfc3339()})) {
                    state.bus.record("goal",&stopped);
                }
            }
        }
    }
    Ok(saved)
}
