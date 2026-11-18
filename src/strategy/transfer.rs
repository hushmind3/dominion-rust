//! HarnessFix/Harness2 (2026) style receipt-linked recovery transfer.
//! Read-only evidence gate, not a new optimizer, executor or keyword scorer.
use super::StrategyEvidence;
use crate::domain::{Run, TimelineEvent, RunStatus};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
pub struct RecoveryProof {
    pub failure_kind:String,
    pub failed_tool:String,
    pub affected_path:String,
    pub failed_call_id:String,
    pub repair_call_id:String,
    pub postflight_call_id:String,
    pub failure_seq:u64,
    pub repair_seq:u64,
    pub postflight_seq:u64,
    pub verification_seq:u64,
    pub claim:String,
}
#[derive(Clone)]
struct Failure{
    kind:String, tool:String, path:String, call_id:String, seq:u64
}
fn started<'a>(r:&'a Run,id:&str,before:u64)->Option<&'a TimelineEvent>{
    r.events.iter().rev().find(|e|e.seq<before &&e.kind=="tool_started"
        &&e.detail.as_ref().is_some_and(|d|d["id"]==id))
}
fn path(e:&TimelineEvent)->Option<String>{
    e.detail.as_ref()?["arguments"]["path"].as_str()
        .filter(|s|!s.is_empty()).map(str::to_owned)
}
fn failure(r:&Run,e:&TimelineEvent)->Option<Failure>{
    let kind=crate::repair_guidance::failure_family(e)?;
    if matches!(kind,"permission_denied"|"external_outcome_unknown"|"transport_uncertain"){
        return None;
    }
    let id=e.detail.as_ref()?["call_id"].as_str()?;
    let intent=started(r,id,e.seq)?;
    Some(Failure{kind:kind.into(),
        tool:intent.detail.as_ref()?["name"].as_str()?.into(),
        path:path(intent)?,call_id:id.into(),seq:e.seq})
}
fn success<'a>(r:&'a Run,before:u64)->impl Iterator<Item=&'a TimelineEvent>{
    r.events.iter().filter(move|e|e.seq<before&&e.kind=="tool_result"
        &&e.detail.as_ref().is_some_and(|d|d["succeeded"]==true))
}
/// Require a real failed call, same-path mutation, same-path postflight and a
/// subsequent verifier PASS. This is associated evidence, not causal proof
/// or a held-out task score; no successful narrative becomes provenance.
pub fn recovery_proof(r:&Run)->Option<RecoveryProof>{
    let audit=r.events.iter().rev().find(|e|e.kind=="verification")?;
    if !crate::verification::strict_pass(&audit.message){return None;}
    let repairs=success(r,audit.seq).filter(|e|e.detail.as_ref()
        .is_some_and(|d|matches!(d["name"].as_str(),
            Some("write_file"|"edit_file")))).collect::<Vec<_>>();
    for edit in repairs.into_iter().rev(){
        let edit_id=edit.detail.as_ref()?["call_id"].as_str()?;
        let resource=path(started(r,edit_id,edit.seq)?)?;
        let post=success(r,audit.seq).find(|e|{
            if e.seq<=edit.seq||e.detail.as_ref()
                .is_none_or(|d|d["name"]!="read_file"){return false;}
            e.detail.as_ref().and_then(|d|d["call_id"].as_str())
                .and_then(|id|started(r,id,e.seq)).and_then(path)
                .is_some_and(|p|p==resource)
        });
        let Some(post)=post else{continue};
        let post_id=post.detail.as_ref()?["call_id"].as_str()?;
        let bad=r.events.iter().rev().filter(|e|e.seq<edit.seq)
            .filter_map(|e|failure(r,e)).find(|f|f.path==resource);
        let Some(bad)=bad else{continue};
        if r.events.iter().any(|e|e.seq>audit.seq &&
            (e.kind=="tool_error"||(e.kind=="tool_result"
                &&e.detail.as_ref().is_some_and(|d|matches!(
                    d["name"].as_str(),Some("write_file"|"edit_file")))))){
            return None;
        }
        return Some(RecoveryProof{
            failure_kind:bad.kind,failed_tool:bad.tool,
            affected_path:resource,failed_call_id:bad.call_id,
            repair_call_id:edit_id.into(),postflight_call_id:post_id.into(),
            failure_seq:bad.seq,repair_seq:edit.seq,postflight_seq:post.seq,
            verification_seq:audit.seq,
            claim:"linked failure and same-file mutation/read before auditor PASS; transfer not independently proven".into()
        });
    }
    None
}
fn valid(record:&StrategyEvidence,repo:&str)->bool{
    record.repository==repo&&matches!(record.outcome.as_str(),"verified"|"adopted")
        &&record.recovery.is_some()&&record.cost_krw.is_finite()
        &&record.cost_krw>=0.0
}
fn frontier(records:Vec<StrategyEvidence>,limit:usize)->Vec<StrategyEvidence>{
    let mut chosen=records.iter().enumerate().filter(|(i,a)|{
        !records.iter().enumerate().any(|(j,b)|{
            if i==&j{return false}
            let (Some(x),Some(y))=(&a.recovery,&b.recovery)else{return false};
            x.failed_tool==y.failed_tool&&x.failure_kind==y.failure_kind
                &&x.affected_path==y.affected_path
                &&b.cost_krw<=a.cost_krw&&b.attempts<=a.attempts
                &&(b.cost_krw<a.cost_krw||b.attempts<a.attempts)
        })
    }).map(|(_,x)|x.clone()).collect::<Vec<_>>();
    chosen.sort_by(|a,b|b.updated_at_ms.cmp(&a.updated_at_ms)
        .then_with(||a.source_id.cmp(&b.source_id)));
    chosen.truncate(limit.min(3));chosen
}
/// Public SDK search accepts exact resource paths or an explicit typed
/// failure contract. Generic prose does not rank strategies by word count.
pub fn rank(records:Vec<StrategyEvidence>,repo:&str,query:&str,limit:usize)
    ->Vec<StrategyEvidence>{
    if repo.is_empty()||repo=="global"||limit==0{return Vec::new();}
    let records=records.into_iter().filter(|e|valid(e,repo))
        .collect::<Vec<_>>();
    let q=query.trim();
    if q.is_empty(){return frontier(records,limit)}
    if let Ok(c)=serde_json::from_str::<Value>(q){
        let (Some(tool),Some(kind),Some(resource))=(
            c["failed_tool"].as_str(),c["failure_kind"].as_str(),
            c["affected_path"].as_str()) else{return Vec::new()};
        return frontier(records.into_iter().filter(|e|e.recovery
            .as_ref().is_some_and(|p|p.failed_tool==tool
                &&p.failure_kind==kind&&p.affected_path==resource)).collect(),limit);
    }
    let exact_paths=q.split_whitespace()
        .map(|t|t.trim_matches(|c:char|matches!(c,'"'|','|':'|'('|')')))
        .filter(|p|p.contains('/')&&p.len()<240).collect::<HashSet<_>>();
    if exact_paths.is_empty(){return Vec::new()}
    frontier(records.into_iter().filter(|e|e.recovery.as_ref()
        .is_some_and(|p|exact_paths.contains(p.affected_path.as_str())))
        .collect(),limit)
}
/// Only read exact, persisted failure surfaces from the current Run, and
/// require a past audited repair of the SAME tool, failure kind and resource.
pub fn rank_for_run(records:Vec<StrategyEvidence>,run:&Run,limit:usize)
    ->Vec<StrategyEvidence>{
    let Some(repo)=run.repository.as_deref()else{return Vec::new()};
    if run.status!=RunStatus::Running||limit==0{return Vec::new()}
    let latest_model=run.events.iter().rev().find(|e|e.kind=="model")
        .map(|e|e.seq).unwrap_or(0);
    let current=run.events.iter().rev().take_while(|e|e.seq>latest_model)
        .find_map(|e|failure(run,e));
    let Some(current)=current else{return Vec::new()};
    frontier(records.into_iter().filter(|e|valid(e,repo)&&e.source_id!=run.id
        &&e.recovery.as_ref().is_some_and(|p|
            p.failed_tool==current.tool&&p.failure_kind==current.kind
                &&p.affected_path==current.path)).collect(),limit)
}
pub fn prompt(records:&[StrategyEvidence])->String{
    records.iter().filter_map(|e|{
        let p=e.recovery.as_ref()?;
        Some(format!("Past run {}: observed {} of {} at {} (failure call {} seq {}), \
            subsequent repair call {} seq {}, same-file postflight call {} seq {}, \
            auditor PASS seq {}; observed cost KRW {:.2}, model rounds {}. \
            Reference only. Inspect the current file and approvals; do not \
            adopt or repeat any past effect without current evidence.",
            e.source_id,p.failure_kind,p.failed_tool,p.affected_path,
            p.failed_call_id,p.failure_seq,p.repair_call_id,p.repair_seq,
            p.postflight_call_id,p.postflight_seq,p.verification_seq,
            e.cost_krw,e.attempts))
    }).take(3).collect::<Vec<_>>().join("\n")
}
