//! A compiled, source-evolvable experiment designer, NOT a closure stage
//! machine. Candidate method edits can rewrite these weights and algorithms.
//! Frozen paired adoption and the original assessor remain outside this module.
//! This pure policy has no AppState, disk IO, provider, or evaluation authority.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;

const POLICY_SOURCE:&str="compiled_source_evolvable_experiment_policy_v1";
const MAX_HISTORY:usize=24;

#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub(crate) enum ExperimentProtocol {
    PairedSource,
    MatchedMethodControl,
}

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct FrozenProtocol {
    pub(crate) profile_sha256:String,
    pub(crate) owner_commit:String,
    pub(crate) cases_count:usize,
    pub(crate) amendment_allowed_in_trial:bool,
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct ExperimentOption {
    pub(crate) protocol:ExperimentProtocol,
    pub(crate) source:String,
    pub(crate) observed_trials:u64,
    pub(crate) independent_gains:u64,
    pub(crate) independent_losses:u64,
    pub(crate) mean_tokens:u64,
    pub(crate) score:i64,
    pub(crate) eligible:bool,
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct SourceTarget {
    pub(crate) role:String,
    pub(crate) domain:String,
    pub(crate) path:String,
    pub(crate) source_sha256:String,
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct TargetOption {
    pub(crate) target:SourceTarget,
    pub(crate) attempted:u64,
    pub(crate) independently_adopted:u64,
    pub(crate) independently_rejected:u64,
    pub(crate) priority:i64,
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct ExperimentPolicy {
    pub(crate) schema_version:u32,
    pub(crate) source:String,
    pub(crate) generator_commit:String,
    pub(crate) observed_history:usize,
    pub(crate) frozen:FrozenProtocol,
    pub(crate) choices:Vec<ExperimentOption>,
    pub(crate) selected_protocol:ExperimentProtocol,
    pub(crate) ranked_targets:Vec<TargetOption>,
    pub(crate) preferred_focus:Option<SourceTarget>,
    pub(crate) selected_target:Option<SourceTarget>,
    pub(crate) redesign_trigger:Option<String>,
    pub(crate) signature_sha256:String,
}

fn sha(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn serialize<T:Serialize>(value:&T)->Vec<u8>{
    serde_json::to_vec(value).unwrap_or_default()
}
fn source_target(row:&Value)->Option<SourceTarget>{
    let path=row["path"].as_str()
        .filter(|s|crate::engine::rsi::safe_path(s))?;
    let source_sha=row["sourceSha256"].as_str()
        .filter(|s|s.len()==64 && s.bytes().all(|x|x.is_ascii_hexdigit()))?;
    let role=row["role"].as_str().filter(|s|!s.is_empty())?;
    let domain=row["domain"].as_str().filter(|s|!s.is_empty())?;
    Some(SourceTarget{role:role.to_string(),domain:domain.to_string(),
        path:path.to_string(),source_sha256:source_sha.to_string()})
}
fn method_eligible(algebra:&Value,request:&Value)->bool{
    let selected=&algebra["selected"];
    let path=selected["target"]["path"].as_str();
    let method=selected["rule"]["method"]["path"].as_str();
    request["rsiMethodAblation"]==true && request["rsiAutonomous"]==true
        && request["rsiMetaOperatorActive"]==true
        && path.zip(method).is_some_and(|(p,m)|p!=m
            && crate::engine::rsi::safe_path(p)
            && crate::engine::rsi::safe_path(m)
            && request["rsiFocusFiles"].as_array()
                .and_then(|a|a.first()).and_then(Value::as_str)==Some(p))
        && request["rsiArchitectureTarget"]==selected["target"]
        && request["rsiSelectedDomain"]==selected["target"]["domain"]
        && selected["target"].is_object()
}
fn observed_trials(history:&[Value])->BTreeMap<&'static str,(u64,u64,u64,u64)>{
    let mut map=BTreeMap::<&'static str,(u64,u64,u64,u64)>::new();
    for g in history.iter().rev().take(MAX_HISTORY){
        // Only evidence from already replayed frozen original receipts
        // can modify the next method/experiment-design search distribution.
        if g["pairedReceiptReplayed"]!=true{continue;}
        let control=&g["methodAblation"];
        // A failed/missing control is still a negative experiment-design
        // observation IF the compiled ON worker actually selected the
        // control protocol. The job replayer already binds that selection
        // to the original worker and sealed evaluation profile.
        let method_selected=g["experimentPolicyExecuted"]["source"]
                =="running_compiled_descendant_experiment_designer_v1"
            && g["experimentPolicyExecuted"]["protocol"]=="matched_method_control"
            && g["experimentPolicyExecuted"]["policySha256"]
                ==g["experimentPolicy"]["signatureSha256"]
            && g["experimentPolicy"]["selectedProtocol"]=="matched_method_control";
        let key=if method_selected{"method"}else{"pair"};
        let entry=map.entry(key).or_default();
        entry.0+=1;
        entry.3+=g["tokens"].as_u64().unwrap_or(0);
        if key=="method" {
            if control["methodAssociationObserved"]==true
                && control["qualityProven"]==true {
                entry.1+=1
            } else{entry.2+=1}
        }else if g["accepted"]==true && g["pairedRevalidated"]==true{
            entry.1+=1
        }else {entry.2+=1}
    }
    map
}
fn target_options(history:&[Value],frontier:&[Value])->Vec<TargetOption>{
    let mut targets=Vec::new();
    for target in frontier.iter().filter_map(source_target){
        let mut tried=0u64;let mut adopted=0u64;let mut rejected=0u64;
        for row in history.iter().rev().take(MAX_HISTORY) {
            let touched=row["focusFiles"].as_array().is_some_and(|a|
                a.iter().any(|p|p.as_str()==Some(target.path.as_str())));
            if !touched{continue;}
            tried+=1;
            // Independently evaluated, source-matching receipts are the
            // ONLY positive/negative payoffs. Unmeasured attempts affect
            // exploration pressure, never the observed improvement count.
            if row["pairedReceiptReplayed"]!=true{continue;}
            if row["accepted"]==true && row["pairedRevalidated"]==true
                && row["acceptedSourceHashes"][&target.path]==target.source_sha256{
                adopted+=1;
            }else if row["evaluation"]["metrics"].is_object() {rejected+=1}
        }
        // Experimental target selection is part of the evolvable code
        // itself. No named A/B generation or hand-coded list of editable
        // domains is required; the compiled source frontier is the roster.
        let novelty=(360/(tried+1)) as i64;
        let failure_pressure=(rejected.min(5)*85) as i64;
        let observed_gain=(adopted.min(5)*65) as i64;
        let repetition_cost=(tried.saturating_sub(4).min(12)*55) as i64;
        targets.push(TargetOption{target,attempted:tried,
            independently_adopted:adopted,independently_rejected:rejected,
            priority:novelty+failure_pressure+observed_gain-repetition_cost});
    }
    targets.sort_by(|a,b|b.priority.cmp(&a.priority)
        .then_with(||a.target.path.cmp(&b.target.path)));
    targets.truncate(48);
    targets
}
fn experiment_score(kind:ExperimentProtocol,stats:(u64,u64,u64,u64),
    eligible:bool,confidence_gate:bool)->i64{
    if !eligible{return i64::MIN/4;}
    let (n,gains,losses,tokens)=stats;
    // These exploration/novelty priors are executable Rust, intentionally
    // mutable by the agent as a source-change candidate in later versions.
    let prior=match kind {
        ExperimentProtocol::PairedSource=>220i64,
        ExperimentProtocol::MatchedMethodControl=>350i64,
    };
    let posterior=((gains+1)*1000/(n+3)) as i64;
    let downside=((losses)*300/(n+2)) as i64;
    let novelty=if n==0{250}else{(200/(n+1)) as i64};
    let average_tokens=if n>0{tokens/n}else{0};
    let token_cost=(average_tokens/2000).min(300) as i64;
    let uncertainty=if confidence_gate && n>0 && gains==0{
        400
    }else{0};
    prior+posterior+novelty-downside-token_cost-uncertainty
}

/// Policy is recomputed by the *running compiled descendant* from the same
/// source/version and replayed history, never accepted from model-generated JSON.
pub(crate) fn synthesize(history:&[Value],frontier:&[Value],algebra:&Value,
    request:&Value,commit:&str)->Value {
    let source_revision=request["rsiBaseCommit"].as_str()
        .unwrap_or(commit);
    let frozen=FrozenProtocol{
        profile_sha256:sha(&serialize(&json!([
            request["rsiFrozenCases"],request["rsiEvaluationConfig"],
            request["rsiMetaCases"]]))),
        owner_commit:source_revision.to_string(),
        cases_count:request["rsiFrozenCases"].as_array().map(Vec::len).unwrap_or(0),
        amendment_allowed_in_trial:false
    };
    let observed=observed_trials(history);
    let pair=observed.get("pair").copied().unwrap_or_default();
    let method=observed.get("method").copied().unwrap_or_default();
    let possible=method_eligible(algebra,request);
    let losses=method.2;
    let method_score=experiment_score(ExperimentProtocol::MatchedMethodControl,
        method,possible,true);
    let pair_score=experiment_score(ExperimentProtocol::PairedSource,pair,true,false);
    let mut choices=vec![
        ExperimentOption{protocol:ExperimentProtocol::PairedSource,
            source:"frozen_server_pair".to_string(),
            observed_trials:pair.0,independent_gains:pair.1,
            independent_losses:pair.2,mean_tokens:if pair.0>0{pair.3/pair.0}else{0},
            score:pair_score,eligible:true},
        ExperimentOption{protocol:ExperimentProtocol::MatchedMethodControl,
            source:"compiled_worker_method_on_off".to_string(),
            observed_trials:method.0,independent_gains:method.1,
            independent_losses:losses,mean_tokens:if method.0>0{method.3/method.0}else{0},
            score:method_score,eligible:possible}
    ];
    choices.sort_by(|a,b|b.score.cmp(&a.score));
    let selected_protocol=choices[0].protocol;
    let ranked_targets=target_options(history,frontier);
    let preferred_focus=ranked_targets.first().map(|row|row.target.clone());
    // A failed experiment-selection principle can become its OWN next
    // source-edit target. These are observed counterexamples, not a
    // hard-coded generation number or a fabricated A/B result.
    let controls_unproductive=method.0>=2 && method.1==0 && method.2>=2;
    let pair_unproductive=pair.0>=3 && pair.1==0 && pair.2>=3;
    let trigger=if controls_unproductive {
        Some("measured_method_control_design_failure")
    }else if pair_unproductive {
        Some("repeated_frozen_pair_selection_failure")
    }else{None};
    let target=if trigger.is_some(){
        frontier.iter().filter_map(source_target)
            .filter(|entry|entry.role=="experimental_design"
                || entry.role=="scientific_method"
                || entry.role=="improvement_methods")
            // Revising the policy itself is a choice of editable source,
            // not an edit of the independent frozen evaluator.
            .min_by_key(|entry|if entry.path.contains("experiment_policy.rs"){0}else{1})
    }else{None};
    let mut result=ExperimentPolicy{
        schema_version:1,source:POLICY_SOURCE.to_string(),
        generator_commit:commit.to_string(),observed_history:history.len(),
        frozen,choices,selected_protocol,ranked_targets,preferred_focus,
        selected_target:target,redesign_trigger:trigger.map(str::to_string),
        signature_sha256:String::new()
    };
    result.signature_sha256=sha(&serialize(&result));
    serde_json::to_value(result).unwrap_or(Value::Null)
}
pub(crate) fn decode(value:&Value,commit:&str,request:&Value)->Option<ExperimentPolicy>{
    let envelope=serde_json::from_value::<ExperimentPolicy>(value.clone()).ok()?;
    if envelope.schema_version!=1||envelope.source!=POLICY_SOURCE
        || envelope.generator_commit!=commit
        || envelope.frozen.owner_commit!=request["rsiBaseCommit"].as_str()
            .unwrap_or(commit)
        || envelope.frozen.amendment_allowed_in_trial
        || envelope.frozen.profile_sha256!=sha(&serialize(&json!([
            request["rsiFrozenCases"],request["rsiEvaluationConfig"],
            request["rsiMetaCases"]])))
        || envelope.frozen.cases_count!=request["rsiFrozenCases"]
            .as_array().map(Vec::len).unwrap_or(0)
        || envelope.choices.len()!=2
        || envelope.ranked_targets.len()>48
        || envelope.choices[0].protocol!=envelope.selected_protocol
        || envelope.preferred_focus!=envelope.ranked_targets.first()
            .map(|row|row.target.clone()) {
        return None
    }
    let signature=&envelope.signature_sha256;
    if signature.len()!=64 || !signature.bytes().all(|b|b.is_ascii_hexdigit()){
        return None;
    }
    let mut unsigned=envelope.clone();
    unsigned.signature_sha256.clear();
    if &sha(&serialize(&unsigned))!=signature {return None}
    Some(envelope)
}
pub(crate) fn policy_controls_method(value:&Value,commit:&str,request:&Value)->bool{
    decode(value,commit,request).is_some_and(|policy|
        policy.selected_protocol==ExperimentProtocol::MatchedMethodControl)
}
pub(crate) fn design_target(value:&Value,commit:&str,request:&Value)->Option<Value>{
    let decision=decode(value,commit,request)?;
    let target=decision.selected_target?;
    Some(json!({"role":target.role,"domain":target.domain,
        "path":target.path,"sourceSha256":target.source_sha256,
        "source":"compiled_evolvable_experiment_redesign",
        "reason":decision.redesign_trigger,
        "evidenceIsNotQualityProof":true}))
}

pub(crate) fn preferred_focus(value:&Value,commit:&str,request:&Value)->Option<Value>{
    let policy=decode(value,commit,request)?;
    let target=policy.preferred_focus?;
    Some(json!({"role":target.role,"domain":target.domain,
        "path":target.path,"sourceSha256":target.source_sha256}))
}
#[cfg(test)]
#[path="experiment_policy/tests.rs"]
mod tests;
