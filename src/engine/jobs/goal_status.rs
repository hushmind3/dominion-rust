//! Synchronize terminal job errors with their parent autonomous goal.
//! A failed worker must not leave the dashboard claiming a goal is still running.
use crate::state::AppState;
use serde_json::{json,Value};

pub(super) fn on_error(state:&AppState,job:&Value,status:&str,error:&str){
    if job["kind"]!="goal"{return;}
    let Some(id)=job["input"]["goalId"].as_str() else{return;};
    let Some(goal)=state.store.get("goal",id) else{return;};
    if matches!(goal["status"].as_str(),
        Some("completed"|"cancelled"|"failed"|"needs_review")){return;}
    let final_status=if status=="cancelled"{"cancelled"}
        else if status=="needs_review"{"needs_review"}else{"failed"};
    if let Some(saved)=state.store.patch_if("goal",id,"status",&goal["status"],
        &json!({"status":final_status,"phase":final_status,
            "error":error,"gaps":[format!("작업 실행 실패: {error}")],
            "finishedAt":chrono::Utc::now().to_rfc3339()})){
        state.bus.record("goal",&saved);
        state.store.audit("goal.worker_failed",json!({"goalId":id,"jobId":job["id"],
            "status":final_status}));
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn ordinary_jobs_cannot_mutate_goals(){
        assert_ne!(json!({"kind":"prompt"})["kind"],"goal");
    }
}
