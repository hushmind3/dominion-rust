//! Editable, descendant-owned scientific experiment allocation policy.
//! It learns from original prior generation outcomes and chooses BOTH a
//! source target and a bounded intervention. Frozen evaluators are separate.
use super::*;
use serde_json::{json,Value};
use crate::engine::rsi_evolve::proof::experiment_contract::{
    ExperimentDesign,ExperimentProtocol,ExperimentTarget
};
use std::path::Path;

const POLICY_SOURCE:&str="src/engine/rsi/experiment_policy.rs";

fn touched(g:&Value,path:&str)->bool {
    g["experimentDesign"]["target"]["path"]==path
        || g["focusFiles"].as_array().is_some_and(|rows|
            rows.iter().any(|row|row==path))
}

fn score(row:&Value,history:&[Value],meta:&Value)->f64{
    let Some(path)=row["path"].as_str() else{return f64::NEG_INFINITY};
    let relevant=history.iter().filter(|g|touched(g,path)).collect::<Vec<_>>();
    let attempts=relevant.len() as f64;
    let successes=relevant.iter().filter(|g|
        g["accepted"]==true
            && g["pairedRevalidated"]==true
            && g["evaluation"]["metrics"]["eligible"]==true
            && g["evaluation"]["verdict"]=="improved"
            && g["experimentOutcome"]["sourceEditObserved"]==true
            && g["experimentOutcome"]["actualWorkerExecuted"]==true
            && g["experimentOutcome"]["policyRevision"]==
                g["experimentDesign"]["policyRevision"]).count() as f64;
    let regressions=relevant.iter().filter(|g|
        g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0).count() as f64;
    let novelty=(2.0*(history.len() as f64+2.0).ln()/(attempts+1.0)).sqrt();
    let measured=(successes+1.0)/(attempts+2.0);
    let source_cost=row["sourceBytes"].as_u64().unwrap_or(0) as f64/200_000.0;
    let meta_agreement=if meta["target"]["path"]==path{0.25}else{0.0};
    // When the scientific procedure stops yielding actual adopted changes,
    // consider rewriting THE EXPERIMENT POLICY, not a prescribed A-B-A turn.
    let recent=history.iter().rev().take(6).collect::<Vec<_>>();
    let fruitless=recent.iter().filter(|g|g["accepted"]!=true).count();
    let stale=recent.len()>=3 && fruitless*2>=recent.len();
    let method_redesign=if row["role"]=="experiment_design" {
        if stale {0.7} else {0.08}
    }else{0.0};
    measured+0.22*novelty+meta_agreement+method_redesign
        -0.25*regressions/(attempts+1.0)-0.025*source_cost.min(4.0)
}

fn parent_revision(history:&[Value])->Option<String>{
    history.iter().rev().find(|g|
        g["accepted"]==true && g["pairedRevalidated"]==true
        && g["experimentOutcome"]["sourceEditObserved"]==true
        && g["experimentOutcome"]["actualWorkerExecuted"]==true
        && g["experimentOutcome"]["verifiedQualityGain"]==true
        && g["experimentOutcome"]["policyRevision"]==
            g["experimentDesign"]["policyRevision"])
        .and_then(|g|g["experimentDesign"]["policyRevision"].as_str())
        .map(str::to_string)
}

/// Policy itself is ordinary editable Rust. Its original bytes are pinned by
/// the independent source-bound contract, not trusted as a success receipt.
pub(super) fn design(
    lab:&Path,commit:&str,history:&[Value],agenda:&Value,
    meta:&Value,method_snapshot:&Value
)->Value {
    let Some(rows)=agenda["frontier"].as_array() else{return Value::Null};
    let grounded=rows.iter().filter(|row|
        row["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
            && row["sourceSha256"].as_str().is_some_and(|hash|
                hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()))
            && row["role"].is_string() && row["domain"].is_string())
        .collect::<Vec<_>>();
    if grounded.is_empty(){return Value::Null;}
    // An accepted descendant's authored stage and verified task-to-method
    // feedback are obligations. The adaptive policy explores otherwise.
    let locked=agenda["inheritedStage"].is_object()
        || agenda["downstreamFeedbackFocus"].is_object();
    let picked=if locked {
        grounded.iter().copied().find(|row|*row==&agenda["target"])
    }else{
        grounded.into_iter().max_by(|a,b|
            score(a,history,meta).total_cmp(&score(b,history,meta))
                .then_with(||b["path"].as_str().cmp(&a["path"].as_str())))
    };
    let Some(target)=picked else{return Value::Null};
    let Some(path)=target["path"].as_str() else{return Value::Null};
    let bytes=std::fs::read(lab.join(POLICY_SOURCE)).ok();
    let Some(bytes)=bytes else{return Value::Null};
    let policy_source_sha256=format!("{:x}",Sha256::digest(bytes));
    let Some(sha)=target["sourceSha256"].as_str() else{return Value::Null};
    let protocol=if target["domain"]!="improver"
        && method_snapshot.is_object() {
        ExperimentProtocol::MethodOnOff
    }else{ExperimentProtocol::FrozenSourcePair};
    let question=if target["role"]=="experiment_design"{
        "Which experiment allocation or comparison strategy produces more independently improved descendants?"
    }else if target["domain"]=="improver"{
        "Can the source-improvement algorithm improve independent task quality and transfer?"
    }else{
        "Can this intelligence subsystem improve held-out task quality under its inherited method?"
    };
    let parent=parent_revision(history);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([
            commit,history.len(),policy_source_sha256,path,sha,
            &protocol,parent,question
        ])).unwrap_or_default()));
    serde_json::to_value(ExperimentDesign{
        schema:"compiled_evolvable_experiment_v1".to_string(),
        generator_commit:commit.to_string(),history_count:history.len(),
        policy_source_sha256,policy_revision:revision,
        target:ExperimentTarget{
            role:target["role"].as_str().unwrap_or("").to_string(),
            domain:target["domain"].as_str().unwrap_or("").to_string(),
            path:path.to_string(),source_sha256:sha.to_string()
        },
        protocol,question:question.to_string(),
        parent_experiment_revision:parent
    }).unwrap_or(Value::Null)
}

pub(super) fn apply(agenda:&mut Value,design:&Value){
    let Some(path)=design["target"]["path"].as_str() else{return};
    let chosen=agenda["frontier"].as_array().and_then(|rows|rows.iter()
        .find(|row|row["path"]==path
            && row["sourceSha256"]==design["target"]["sourceSha256"]
            && row["role"]==design["target"]["role"]
            && row["domain"]==design["target"]["domain"])).cloned();
    if let Some(chosen)=chosen{
        agenda["target"]=chosen.clone();
        agenda["focus"]=json!([chosen]);
        agenda["experimentDesign"]=design.clone();
    }
}

#[cfg(test)]
#[path = "experiment_policy/tests.rs"]
mod tests;
