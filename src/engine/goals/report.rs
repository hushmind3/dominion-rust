//! Evidence-bound final report for durable autonomous goals.
//! The report is derived from the frozen acceptance contract and successful
//! tool observations, never from the executor's prose or the reviewer's claim.
use crate::state::AppState;
use crate::engine::verification::{acceptance_gaps_in, resolve_path_from, successful_evidence, tool_name};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) fn build(state:&AppState, goal:&Value)->Value {
    // Standalone verification remains available for ordinary non-isolated
    // goals and in-memory test contracts without a persisted goal ID.
    // Explicitly isolated goals never fall back to the shared checkout.
    let workdir=if goal["isolateWorktree"]!=true && goal["gitWorktree"].is_null(){
        state.config.workspace.clone()
    }else{
        match crate::engine::goal_git::workspace_for_goal(state,goal) {
            Ok(path)=>path,
            Err(reason)=>return json!({"accepted":false,"gaps":[reason],
                "checks":[],"artifacts":[],"operations":[],"gitChanges":null}),
        }
    };
    let evidence=goal["evidence"].as_array().cloned().unwrap_or_default();
    let intent=goal["goal"].as_str().unwrap_or("");
    let checks=&goal["acceptanceChecks"];
    let gaps=acceptance_gaps_in(&workdir,intent,checks,&evidence);
    let checks_report=checks.as_array().into_iter().flatten().map(|check|{
        // Passing the empty goal avoids counting a generic action gate once
        // per individual predicate. The complete goal gate is above.
        let failures=acceptance_gaps_in(&workdir,"",&json!([check]),&evidence);
        json!({"type":check["type"],"path":check["path"],"command":check["command"],
            "passed":failures.is_empty(),"gaps":failures})
    }).collect::<Vec<_>>();
    let successful=evidence.iter().filter(|item|successful_evidence(item)).count();
    let work=workdir.canonicalize().ok();
    let mut unique=std::collections::BTreeSet::new();
    let artifacts=checks.as_array().into_iter().flatten()
        .filter_map(|check|check["path"].as_str())
        .filter(|path|unique.insert(path.to_string()))
        .take(20)
        .filter_map(|path|{
            let resolved=resolve_path_from(&workdir,path).ok()?;
            let canonical=resolved.canonicalize().ok()?;
            if !canonical.is_file() || !work.as_ref().is_some_and(|root|canonical.starts_with(root)){
                return None;
            }
            let mut file=std::fs::File::open(&canonical).ok()?;
            let size=file.metadata().ok()?.len();
            if size>8*1024*1024{return Some(json!({"path":path,"bytes":size,"hashOmitted":true}));}
            let mut bytes=Vec::new();
            file.by_ref().take(8*1024*1024+1).read_to_end(&mut bytes).ok()?;
            Some(json!({"path":path,"bytes":size,
                "sha256":format!("{:x}",Sha256::digest(&bytes))}))
        }).collect::<Vec<_>>();
    let operations=evidence.iter().filter(|item|successful_evidence(item))
        .take(30).map(|item|{
            json!({"tool":tool_name(item),
                "path":item.pointer("/arguments/path").or_else(||item.pointer("/args/path")),
                "exitCode":item.pointer("/result/exitCode")})
        }).collect::<Vec<_>>();
    json!({
        "accepted":gaps.is_empty(),
        "checks":checks_report,
        "gaps":gaps,
        "artifacts":artifacts,
        "successfulToolEvidence":successful,
        "toolEvidenceRecorded":evidence.len(),
        "operations":operations,
        "gitChanges":super::git::report(&workdir,&goal["gitBaseline"]),
        "sessionUsage":goal["sessionId"].as_str().filter(|id|!id.is_empty())
            .map(|id|state.store.session_usage(id)).unwrap_or(Value::Null),
        "tokenBudget":goal["tokenBudget"],
        "tokenBudgetScope":"session executor and planner/reviewer/replanner model usage",
        "tokenBudgetIsExactBillingCap":false,
        "reportedAt":chrono::Utc::now().to_rfc3339()
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/goal_report_tests.rs"]
mod tests;
