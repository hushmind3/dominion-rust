//! Frozen, typed experiment envelope shared by compiled children and their
//! supervising evaluator. Mutable policy code may choose experiments, but may
//! not forge source bytes, target identity, evaluation or adoption rights.
use super::*;
use serde::{Deserialize,Serialize};
use std::path::Path;

const POLICY_SOURCE:&str="src/engine/rsi/experiment_policy.rs";

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub(crate) enum ExperimentProtocol {
    FrozenSourcePair,
    MethodOnOff,
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct ExperimentTarget {
    pub role:String,
    pub domain:String,
    pub path:String,
    pub source_sha256:String,
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct ExperimentDesign {
    pub schema:String,
    pub generator_commit:String,
    pub history_count:usize,
    pub policy_source_sha256:String,
    pub policy_revision:String,
    pub target:ExperimentTarget,
    pub protocol:ExperimentProtocol,
    pub question:String,
    pub parent_experiment_revision:Option<String>,
}

fn sha(s:&str,n:usize)->bool {
    s.len()==n && s.bytes().all(|byte|byte.is_ascii_hexdigit())
}

fn source_hash(root:&Path)->Result<String,String> {
    let root=std::fs::canonicalize(root).map_err(|e|e.to_string())?;
    let file=std::fs::canonicalize(root.join(POLICY_SOURCE))
        .map_err(|e|e.to_string())?;
    if !file.starts_with(&root)||!file.is_file(){
        return Err("실험 설계기가 현재 후손 소스에 없습니다.".into());
    }
    let bytes=std::fs::read(file).map_err(|e|e.to_string())?;
    Ok(format!("{:x}",Sha256::digest(bytes)))
}

/// Compare only immutable source facts and executable protocol availability.
/// Do not recompute the descendant's target scores using the old parent code:
/// that would prevent an accepted child policy from changing its own policy.
pub(crate) fn verify_experiment_design(
    value:&Value,commit:&str,history_count:usize,
    agenda:&Value,root:&Path,method_snapshot:&Value,
)->Result<(),String>{
    if value.is_null(){return Ok(());}
    let plan:ExperimentDesign=serde_json::from_value(value.clone())
        .map_err(|e|format!("실험 설계 계약 형식 오류: {e}"))?;
    if plan.schema!="compiled_evolvable_experiment_v1"
        || !sha(commit,40)
        || plan.generator_commit!=commit
        || plan.history_count!=history_count
        || !sha(&plan.policy_revision,64)
        || !sha(&plan.policy_source_sha256,64)
        || plan.policy_source_sha256!=source_hash(root)?
        || plan.question.is_empty() || plan.question.len()>500
        || !sha(&plan.target.source_sha256,64)
        || !crate::engine::rsi::safe_path(&plan.target.path)
        || !crate::engine::rsi_evolve::DOMAINS.contains(&plan.target.domain.as_str())
        || !plan.target.role.bytes().all(|b|
            b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
        || plan.target.role.is_empty()||plan.target.role.len()>64
        || plan.parent_experiment_revision.as_deref()
            .is_some_and(|v|!sha(v,64))
        || plan.protocol==ExperimentProtocol::MethodOnOff
            && (plan.target.domain=="improver"||!method_snapshot.is_object())
    {
        return Err("실험 설계가 수정 불가 출처·허용 실행 방식과 맞지 않습니다.".into());
    }
    let Some(rows)=agenda["frontier"].as_array() else{
        return Err("실험 대상의 유효한 소스 후보가 없습니다.".into());
    };
    let bound=rows.iter().any(|row|
        row["path"]==plan.target.path
        && row["sourceSha256"]==plan.target.source_sha256
        && row["domain"]==plan.target.domain
        && row["role"]==plan.target.role
        && row==&agenda["target"]
        && agenda["focus"].as_array().is_some_and(|focus|
            focus.iter().any(|entry|entry==row)));
    if !bound {
        return Err("실험 계획이 실제 후손 소스·선택 대상과 다릅니다.".into());
    }
    let canonical=serde_json::to_value(&plan).map_err(|e|e.to_string())?;
    if canonical!=*value {
        return Err("동결된 실험 계약의 정규 표현이 일치하지 않습니다.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "experiment_contract/tests.rs"]
mod tests;
