//! Typed experiment envelope shared by compiled children and independent
//! supervising evaluator. Mutable policy code may choose experiments, but may
//! not forge source bytes, target identity, evaluation or adoption rights.
use serde::{Deserialize,Serialize};
use std::path::Path;
use serde_json::Value;
use sha2::{Digest,Sha256};

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

impl ExperimentDesign {
    /// Stable, domain-neutral content identity: selection policy may evolve,
    /// but every reported decision must name its exact immutable bytes.
    pub(crate) fn revision(&self)->String {
        format!("{:x}",Sha256::digest(
            serde_json::to_vec(&serde_json::json!([
                self.generator_commit,self.history_count,
                self.policy_source_sha256,self.target.path,
                self.target.source_sha256,self.protocol,
                self.parent_experiment_revision,self.question
            ])).unwrap_or_default()))
    }
    pub(crate) fn seal(&mut self) {
        self.policy_revision=self.revision();
    }
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
    value:&Value,commit:&str,history:&[Value],
    agenda:&Value,root:&Path,method_snapshot:&Value,
)->Result<(),String>{
    if value.is_null(){return Ok(());}
    let plan:ExperimentDesign=serde_json::from_value(value.clone())
        .map_err(|e|format!("실험 설계 계약 형식 오류: {e}"))?;
    if plan.schema!="compiled_evolvable_experiment_v1"
        || !sha(commit,40)
        || plan.generator_commit!=commit
        || plan.history_count!=history.len()
        || !sha(&plan.policy_revision,64)
        || plan.policy_revision!=plan.revision()
        || !sha(&plan.policy_source_sha256,64)
        || plan.policy_source_sha256!=source_hash(root)?
        || plan.question.is_empty() || plan.question.len()>500
        || !sha(&plan.target.source_sha256,64)
        || !crate::engine::rsi::safe_path(&plan.target.path)
        || plan.target.domain.is_empty()||plan.target.domain.len()>80
        || !plan.target.domain.bytes().all(|b|
            b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
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
    // Provenance is a versioned relation to an actual accepted predecessor,
    // never just a 64-character model-authored label. No fixed depth or
    // "previous two generations" schedule is imposed by this contract.
    if let Some(previous)=plan.parent_experiment_revision.as_deref() {
        let authentic=history.iter().any(|g|
            g["accepted"]==true
            && g["pairedRevalidated"]==true
            && g["evaluation"]["metrics"]["eligible"]==true
            && g["experimentDesign"]["policyRevision"]==previous
            && g["experimentOutcome"]["sourceEditObserved"]==true
            && g["experimentOutcome"]["actualWorkerExecuted"]==true
            && g["experimentOutcome"]["policyRevision"]==previous);
        if !authentic{return Err("실제 채택되지 않은 실험 정책 계승입니다.".into());}
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
#[path = "experiment/tests.rs"]
mod tests;
