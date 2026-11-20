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
    /// Distinguish a separate auditor verdict from a deterministic exact-file
    /// postcondition so no assistant claims model-audit results that never ran.
    pub verification_basis:String,
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
    let audit=r.events.iter().rev().find(|e|e.kind=="verification");
    if audit.is_some_and(|e|!crate::verification::strict_pass(&e.message)){
        return None;
    }
    let until=audit.map(|e|e.seq).unwrap_or(u64::MAX);
    let repairs=success(r,until).filter(|e|e.detail.as_ref()
        .is_some_and(|d|matches!(d["name"].as_str(),
            Some("write_file"|"edit_file")))).collect::<Vec<_>>();
    for edit in repairs.into_iter().rev(){
        let edit_id=edit.detail.as_ref()?["call_id"].as_str()?;
        let resource=path(started(r,edit_id,edit.seq)?)?;
        let post=success(r,until).find(|e|{
            if e.seq<=edit.seq||e.detail.as_ref()
                .is_none_or(|d|d["name"]!="read_file"){return false;}
            e.detail.as_ref().and_then(|d|d["call_id"].as_str())
                .and_then(|id|started(r,id,e.seq)).and_then(path)
                .is_some_and(|p|p==resource)
        });
        let Some(post)=post else{continue};
        let post_id=post.detail.as_ref()?["call_id"].as_str()?;
        let post_intent=started(r,post_id,post.seq)?;
        if post_intent.detail.as_ref().is_some_and(|d|
            d["arguments"].get("start_line").is_some()
                || d["arguments"].get("end_line").is_some()) {
            continue; // A line-window excerpt is not a full artifact postflight.
        }
        let direct_postflight=||->bool{
            let expected=edit_id;
            let Some(receipt)=r.messages.iter().rev().find(|m|
                m["role"]=="tool" && m["tool_call_id"]==post_id)
                .and_then(|m|m["content"].as_str()) else{return false};
            let Some(intent)=started(r,expected,edit.seq)else{return false};
            let args=&intent.detail.as_ref().map(|d|&d["arguments"]);
            let Some(args)=args else{return false};
            match edit.detail.as_ref().and_then(|d|d["name"].as_str()){
                Some("write_file")=>args["content"].as_str()
                    .is_some_and(|content|!content.is_empty()&&receipt==content),
                Some("edit_file")=>{
                    let new=args["new_text"].as_str().unwrap_or("");
                    let old=args["old_text"].as_str().unwrap_or("");
                    !new.is_empty()&&!old.is_empty()&&receipt.contains(new)
                        &&!receipt.contains(old)
                },
                _=>false
            }
        };
        let exact=direct_postflight();
        // Without a paid model auditor, content equality is the only
        // accepted source of an artifact-level claim. It is not equivalent
        // to correctness of the complete user instruction.
        if audit.is_none() && !exact {continue;}
        let bad=r.events.iter().rev().filter(|e|e.seq<edit.seq)
            .filter_map(|e|failure(r,e)).find(|f|f.path==resource);
        let Some(bad)=bad else{continue};
        if r.events.iter().any(|e|e.seq>post.seq &&
            (e.kind=="tool_error" || e.kind=="tool_result"
                && e.detail.as_ref().is_some_and(|d|
                    matches!(d["name"].as_str(),Some("write_file"|"edit_file"))))){
            return None;
        }
        return Some(RecoveryProof{
            failure_kind:bad.kind,failed_tool:bad.tool,
            affected_path:resource,failed_call_id:bad.call_id,
            repair_call_id:edit_id.into(),postflight_call_id:post_id.into(),
            failure_seq:bad.seq,repair_seq:edit.seq,postflight_seq:post.seq,
            verification_seq:audit.map(|e|e.seq).unwrap_or(post.seq),
            verification_basis:if audit.is_some(){
                "auditor_pass_with_postflight".into()
            }else{"deterministic_exact_artifact".into()},
            claim:if audit.is_some(){
                "linked failure and same-file mutation/read before auditor PASS; held-out transfer unmeasured".into()
            }else{
                "actual same-path failed tool, repair and exact content postcondition; no auditor or full-task proof".into()
            }
        });
    }
    None
}
fn valid(record:&StrategyEvidence,repo:&str)->bool{
    record.repository==repo&&matches!(record.outcome.as_str(),"verified"|"artifact_verified"|"adopted")
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
            verification basis {} at seq {}; observed cost KRW {:.2}, model rounds {}. \
            Reference only. Inspect the current file and approvals; do not \
            adopt or repeat any past effect without current evidence.",
            e.source_id,p.failure_kind,p.failed_tool,p.affected_path,
            p.failed_call_id,p.failure_seq,p.repair_call_id,p.repair_seq,
            p.postflight_call_id,p.postflight_seq,p.verification_basis,
            p.verification_seq,e.cost_krw,e.attempts))
    }).take(3).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CreateRun;
    use serde_json::json;
    fn run()->Run{
        let mut r=Run::new(CreateRun{instruction:"Repair src/logic.rs".into(),
            budget_krw:Some(200.0),max_steps:Some(8)},8).unwrap();
        r.strategy_learning=true;
        r.repository=Some("repo".into());
        r.status=RunStatus::Completed;
        r.steps=4;r.cost_krw=12.0;
        r
    }
    fn intent(r:&mut Run,id:&str,name:&str,source:&str){
        r.event("tool_started","",Some(json!({
            "id":id,"name":name,"arguments":{"path":source}})));
    }
    fn settled(r:&mut Run,id:&str,name:&str,ok:bool){
        r.event(if ok{"tool_result"}else{"tool_error"},"",Some(json!({
            "call_id":id,"name":name,"succeeded":ok,
            "result":if ok{"read receipt"}else{"path does not exist"}})));
    }
    fn audited()->Run{
        let mut r=run();
        intent(&mut r,"f","read_file","src/logic.rs");
        settled(&mut r,"f","read_file",false);
        intent(&mut r,"fix","write_file","src/logic.rs");
        settled(&mut r,"fix","write_file",true);
        intent(&mut r,"check","read_file","src/logic.rs");
        settled(&mut r,"check","read_file",true);
        r.event("verification","PASS",None);
        r
    }
    #[test]
    fn real_failure_and_read_after_write_are_required_for_reusable_evidence(){
        let r=audited();
        let proof=recovery_proof(&r).unwrap();
        assert_eq!(proof.failed_call_id,"f");
        assert_eq!(proof.repair_call_id,"fix");
        assert_eq!(proof.postflight_call_id,"check");
        assert!(proof.failure_seq<proof.repair_seq);
        assert!(proof.repair_seq<proof.postflight_seq);
        assert!(proof.postflight_seq<proof.verification_seq);
        assert!(r.plan.is_none(),"no separate plan model call is necessary");
        assert_eq!(crate::strategy::from_run(&r).unwrap().files,
            vec!["src/logic.rs".to_owned()]);
    }
    #[test]
    fn direct_file_postcondition_transfers_without_paying_for_a_second_model_audit(){
        let mut r=run();
        intent(&mut r,"f","read_file","src/logic.rs");
        settled(&mut r,"f","read_file",false);
        r.event("tool_started","",Some(json!({
            "id":"fix","name":"write_file",
            "arguments":{"path":"src/logic.rs","content":"pub fn repaired() {}"}
        })));
        settled(&mut r,"fix","write_file",true);
        intent(&mut r,"check","read_file","src/logic.rs");
        settled(&mut r,"check","read_file",true);
        r.messages.push(json!({"role":"tool","tool_call_id":"check",
            "content":"pub fn repaired() {}"}));
        assert!(r.events.iter().all(|e|e.kind!="verification"));
        let evidence=crate::strategy::from_run(&r).unwrap();
        assert_eq!(evidence.outcome,"artifact_verified");
        assert_eq!(evidence.recovery.as_ref().unwrap().verification_basis,
            "deterministic_exact_artifact");
        let mut bad=r;
        bad.messages.last_mut().unwrap()["content"]=json!("completely different");
        assert!(crate::strategy::from_run(&bad).is_none(),
            "simply executing read_file is not evidence of the requested content");
    }
    #[test]
    fn model_narration_stale_pass_and_missing_postflight_abstain(){
        let mut r=audited();
        r.events.retain(|e|e.kind!="verification");
        assert!(recovery_proof(&r).is_none());
        r.event("verification","PASS maybe",None);
        assert!(recovery_proof(&r).is_none());
        let mut r=audited();
        r.events.retain(|e|e.detail.as_ref().is_none_or(|d|d["call_id"]!="check"));
        assert!(recovery_proof(&r).is_none());
        let mut r=audited();
        r.event("tool_result","late mutation",Some(json!({
            "name":"edit_file","call_id":"late","succeeded":true})));
        assert!(recovery_proof(&r).is_none());
    }
    #[test]
    fn same_resource_failure_transfers_but_unrelated_queries_and_repositories_do_not(){
        let historical=crate::strategy::from_run(&audited()).unwrap();
        let mut current=run();
        current.status=RunStatus::Running;
        intent(&mut current,"new","read_file","src/logic.rs");
        settled(&mut current,"new","read_file",false);
        let found=rank_for_run(vec![historical.clone()],&current,2);
        assert_eq!(found.len(),1);
        assert!(prompt(&found).contains("do not"));
        assert!(rank(vec![historical.clone()],"repo","generic bug fix",3).is_empty());
        assert_eq!(rank(vec![historical.clone()],"repo",
            "Repair src/logic.rs",3).len(),1);
        assert!(rank(vec![historical.clone()],"repo",
            "Repair src/unknown.rs",3).is_empty());
        current.repository=Some("another".into());
        assert!(rank_for_run(vec![historical],&current,2).is_empty());
    }
    #[test]
    fn prior_denial_is_not_transferable_and_no_autorun_is_possible(){
        let mut r=run();
        intent(&mut r,"p","write_file","src/logic.rs");
        r.event("tool_denied","owner denied",Some(json!({"call_id":"p"})));
        assert!(r.events.iter().rev().all(|e|failure(&r,e).is_none()));
        assert!(crate::strategy::from_run(&r).is_none());
    }
    #[test]
    fn conflicting_cost_and_rounds_preserve_alternatives_without_fake_winner(){
        let a=crate::strategy::from_run(&audited()).unwrap();
        let mut b=a.clone();b.source_id=uuid::Uuid::new_v4();
        b.attempts=2;b.cost_krw=80.0;
        let ranked=rank(vec![a,b],"repo","",3);
        assert_eq!(ranked.len(),2,"one is faster, one is cheaper; neither dominates");
    }
}
