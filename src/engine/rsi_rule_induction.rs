//! Derive new executable RSI judgments from contrasting descendant traces.
//!
//! A model is never asked to invent success evidence. We take pre-edit
//! snapshots from two or more *independently adopted* descendants and
//! counterexamples from two independently evaluated, non-adopted descendants.
//! Bounded predicate search discovers a separation over observed source read,
//! post-ground search and call budget. Output is only a trial controller rule,
//! never a claim that a new descendant has improved.
use serde_json::{json,Value};
use std::collections::BTreeSet;

#[derive(Clone,Copy,Debug)]
struct Snapshot { reads:u64, searches:u64, calls:u64 }
fn metric(snapshot:Snapshot,name:&str)->u64 {
    match name {
        "completed:workspace_read_range"=>snapshot.reads,
        "completed:code_search"=>snapshot.searches,
        _=>snapshot.calls,
    }
}

fn read_snapshot(ops:&[Value], stop:usize, run:&str)->Option<Snapshot> {
    let mut reads=0u64;
    let mut searches=0u64;
    let mut calls=0u64;
    let mut attempted_edit=false;
    for op in ops.iter().take(stop) {
        if op["runId"].as_str()!=Some(run) {continue;}
        calls+=1;
        if matches!(op["tool"].as_str(),
            Some("workspace_replace_range"|"workspace_replace"|"workspace_write")) {
            attempted_edit=true;
        }
        if op["status"]!="completed" {continue;}
        match op["tool"].as_str().unwrap_or("") {
            "workspace_read_range"=>reads+=1,
            "code_search"=>searches+=1,
            _=>{}
        }
    }
    // All induced rules require edit_attempts == 0: their training positives
    // must satisfy that condition when the learned action would fire.
    (reads>0 && !attempted_edit && calls<=48)
        .then_some(Snapshot{reads,searches,calls})
}

fn accepted_edit(row:&Value, ops:&[Value])->Option<(String,Snapshot)> {
    if row["accepted"]!=true || row["evidenceVerified"]!=true
        || row["evaluation"]["metrics"]["eligible"]!=true {return None;}
    let adopted=row["acceptedSourceHashes"].as_object()?;
    for (index,op) in ops.iter().enumerate() {
        let name=op["tool"].as_str()?;
        if !matches!(name,"workspace_replace_range"|"workspace_replace") ||
            op["status"]!="completed" || op["result"]["written"]!=true
            || op["toolReceipt"]["verified"]!=true {continue;}
        let Some(path)=op["path"].as_str() else {continue;};
        let Some(digest)=adopted.get(path).and_then(Value::as_str) else {continue;};
        if digest.len()!=64 || op["result"]["postSha256"].as_str()!=Some(digest) {
            continue;
        }
        let Some(run)=op["runId"].as_str().filter(|s|!s.is_empty()) else {continue;};
        if let Some(snapshot)=read_snapshot(ops,index,run) {
            // The source observation must precede the edit in this run and
            // cover the same file. No cross-run "successful algorithm".
            let has_same_source=ops.iter().take(index).any(|read|
                read["runId"].as_str()==Some(run) && read["status"]=="completed"
                    && read["tool"]=="workspace_read_range"
                    && read["path"].as_str()==Some(path));
            if has_same_source {return Some((name.to_string(),snapshot));}
        }
    }
    None
}

fn negative_snapshot(row:&Value,ops:&[Value])->Option<Snapshot> {
    if row["accepted"]==true || row["candidateId"].as_str().is_none()
        || row["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || !row["evaluation"]["metrics"].is_object(){return None;}
    // Observe a candidate's decision state immediately BEFORE its first
    // source edit, not the subsequent tool calls produced by a failed patch.
    for (index,op) in ops.iter().enumerate(){
        if !matches!(op["tool"].as_str(),
            Some("workspace_replace_range"|"workspace_replace")) {continue;}
        if let Some(run)=op["runId"].as_str().filter(|s|!s.is_empty()) {
            if let Some(snapshot)=read_snapshot(ops,index,run) {return Some(snapshot);}
        }
    }
    None
}

fn predicate(metric:&str,cmp:&str,value:u64)->Value {
    json!({"metric":metric,"cmp":cmp,"value":value})
}

fn passes(s:Snapshot, pred:&Value)->bool {
    let current=metric(s,pred["metric"].as_str().unwrap_or(""));
    let value=pred["value"].as_u64().unwrap_or(u64::MAX);
    match pred["cmp"].as_str().unwrap_or("") {
        "ge"=>current>=value,
        "lt"=>current<value,
        _=>false,
    }
}

/// Search discriminative guards rather than selecting from a template table.
/// Only a trial controller is produced. To promote it, the independent
/// tournament must observe actual descendant source patches and comparisons.
pub fn induce(history:&[&Value])->Vec<Value> {
    // Never allow the first historical worker's provider/model to hide
    // evidence from a later coherent cohort. Search each homogeneous cohort
    // independently, prioritizing the one containing the latest worker.
    let mut cohorts=std::collections::BTreeMap::<String,Vec<&Value>>::new();
    for row in history {
        let mut key=Vec::new();
        for field in ["objectiveId","knowledgePartition","generatorProviderId",
            "generatorModel"] {
            let Some(value)=row[field].as_str().filter(|s|!s.is_empty()) else {
                key.clear();break;
            };
            key.push(value);
        }
        if key.len()==4 {cohorts.entry(json!(key).to_string()).or_default().push(*row);}
    }
    let mut grouped=cohorts.into_values().collect::<Vec<_>>();
    grouped.sort_by(|a,b|b.len().cmp(&a.len()));
    for group in grouped {
        let rules=induce_cohort(&group);
        if !rules.is_empty() {return rules;}
    }
    Vec::new()
}
fn induce_cohort(history:&[&Value])->Vec<Value> {
    let mut successes=Vec::<(String,Snapshot)>::new();
    let mut negatives=Vec::<Snapshot>::new();
    let mut seen=BTreeSet::new();
    for row in history {
        let Some(job)=row["jobId"].as_str().filter(|s|!s.is_empty()) else {continue;};
        if !seen.insert(job.to_string()) {continue;}
        let Some(ops)=row["toolTimeline"].as_array() else {continue;};
        if let Some(success)=accepted_edit(row,ops) {
            successes.push(success);
        }else if let Some(failure)=negative_snapshot(row,ops){
            negatives.push(failure);
        }
    }
    if successes.len()<2 || negatives.len()<2 {return Vec::new();}
    let mut result=Vec::new();
    for tool in ["workspace_replace_range","workspace_replace"] {
        let positive=successes.iter().filter(|(name,_)|name==tool)
            .map(|(_,s)|*s).collect::<Vec<_>>();
        if positive.len()<2 {continue;}
        // Enforce a fresh source observation and no previous edit attempt.
        // Search all measured threshold/polarity combinations that cover
        // every positive example; choose the one excluding most failures.
        let mut best=None::<(usize,Value)>;
        for name in ["completed:workspace_read_range",
            "completed:code_search","tool_calls"] {
            let lo=positive.iter().map(|s|metric(*s,name)).min().unwrap_or(0);
            let hi=positive.iter().map(|s|metric(*s,name)).max().unwrap_or(0);
            for (cmp,value) in [("ge",lo),("lt",hi.saturating_add(1))] {
                if value>48 || (name=="completed:workspace_read_range" && value==0) {continue;}
                let pred=predicate(name,cmp,value);
                if !positive.iter().all(|s|passes(*s,&pred)) {continue;}
                let false_positives=negatives.iter().filter(|s|passes(**s,&pred)).count();
                if best.as_ref().is_none_or(|(prior,_)|false_positives<*prior) {
                    best=Some((false_positives,pred));
                }
            }
        }
        let Some((false_positive,first))=best else {continue;};
        // The learned decision must discriminate at least one independently
        // measured failure. Otherwise it merely duplicates the base policy.
        if false_positive==negatives.len(){continue;}
        let remaining=negatives.iter().copied().filter(|s|passes(*s,&first))
            .collect::<Vec<_>>();
        let mut clauses=vec![
            predicate("completed:workspace_read_range","ge",1),
            predicate("edit_attempts","lt",1),
            first,
        ];
        if !remaining.is_empty() {
            let mut second=None::<(usize,Value)>;
            for name in ["completed:workspace_read_range",
                "completed:code_search","tool_calls"] {
                let lo=positive.iter().map(|s|metric(*s,name)).min().unwrap_or(0);
                let hi=positive.iter().map(|s|metric(*s,name)).max().unwrap_or(0);
                for (cmp,value) in [("ge",lo),("lt",hi.saturating_add(1))] {
                    if value>48 {continue;}
                    let pred=predicate(name,cmp,value);
                    if !positive.iter().all(|s|passes(*s,&pred)) {continue;}
                    let leaked=remaining.iter().filter(|s|passes(**s,&pred)).count();
                    if second.as_ref().is_none_or(|(prior,_)|leaked<*prior) {
                        second=Some((leaked,pred));
                    }
                }
            }
            let Some((left,extra))=second else {continue;};
            if left>0 {continue;}
            clauses.push(extra);
        }
        result.push(json!({"guard":{"all":clauses},"allow":[tool],
            "sourceJobId":"contrasting_independent_descendant_executions",
            "parentRuleId":null}));
    }
    crate::engine::rsi_decision_rules::normalize(&json!(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn child(id:&str,accepted:bool,searches:u64,tool:&str)->Value {
        let mut operations=vec![json!({"runId":id,"opSeq":1,
            "tool":"workspace_read_range","status":"completed",
            "path":"src/engine/example.rs"})];
        for n in 0..searches {
            operations.push(json!({"runId":id,"opSeq":n+2,"tool":"code_search",
                "status":"completed"}));
        }
        operations.push(json!({"runId":id,"opSeq":searches+2,
            "tool":tool,"status":"completed","path":"src/engine/example.rs",
            "result":{"written":true,"postSha256":"a".repeat(64)},
            "toolReceipt":{"verified":true}}));
        json!({"jobId":id,"objectiveId":"o","knowledgePartition":"p",
            "generatorProviderId":"api","generatorModel":"m",
            "accepted":accepted,"evidenceVerified":accepted,
            "acceptedSourceHashes":{"src/engine/example.rs":"a".repeat(64)},
            "candidateId":id,
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":accepted}},
            "toolTimeline":operations})
    }
    #[test]
    fn discovers_new_threshold_that_separates_two_adoptions_from_failures(){
        let rows=[
            child("good-1",true,0,"workspace_replace_range"),
            child("bad-1",false,3,"workspace_replace_range"),
            child("good-2",true,0,"workspace_replace_range"),
            child("bad-2",false,4,"workspace_replace_range"),
        ];
        let refs=rows.iter().collect::<Vec<_>>();
        let new=induce(&refs);
        assert_eq!(new.len(),1);
        assert_eq!(new[0]["allow"],json!(["workspace_replace_range"]));
        let good=rows[0]["toolTimeline"].as_array().unwrap();
        assert!(crate::engine::rsi_decision_rules::choose(
            &json!({"rules":new.clone()}),&good[..1],1,true,false).is_some());
        let bad=rows[1]["toolTimeline"].as_array().unwrap();
        assert!(crate::engine::rsi_decision_rules::choose(
            &json!({"rules":new}),&bad[..4],4,true,false).is_none());
    }
    #[test]
    fn no_independent_adoption_or_no_counterexamples_produces_no_trial(){
        let rows=[child("good",true,0,"workspace_replace_range"),
            child("bad",false,3,"workspace_replace_range")];
        assert!(induce(&rows.iter().collect::<Vec<_>>()).is_empty());
        let rows=[child("good-1",true,0,"workspace_replace_range"),
            child("good-2",true,0,"workspace_replace_range")];
        assert!(induce(&rows.iter().collect::<Vec<_>>()).is_empty());
    }
    #[test]
    fn a_different_earlier_model_does_not_hide_an_independent_usable_cohort(){
        let mut unrelated=child("other-model",false,1,"workspace_replace_range");
        unrelated["generatorModel"]=json!("different");
        let rows=[
            unrelated,
            child("good-1",true,0,"workspace_replace_range"),
            child("bad-1",false,3,"workspace_replace_range"),
            child("good-2",true,0,"workspace_replace_range"),
            child("bad-2",false,4,"workspace_replace_range")
        ];
        assert_eq!(induce(&rows.iter().collect::<Vec<_>>()).len(),1);
    }
    #[test]
    fn learned_rule_must_have_been_executable_before_its_training_edit(){
        let mut rows=[
            child("g1",true,0,"workspace_replace_range"),
            child("g2",true,0,"workspace_replace_range"),
            child("bad1",false,3,"workspace_replace_range"),
            child("bad2",false,4,"workspace_replace_range")];
        rows[0]["toolTimeline"].as_array_mut().unwrap().insert(1,
            json!({"runId":"g1","opSeq":2,"tool":"workspace_replace_range",
                "status":"failed"}));
        assert!(induce(&rows.iter().collect::<Vec<_>>()).is_empty(),
            "a rule requiring zero previous edits cannot learn from a retry");
    }
    #[test]
    fn adoption_with_no_prior_matching_file_read_is_not_success_evidence(){
        let mut rows=[child("g1",true,0,"workspace_replace_range"),
            child("g2",true,0,"workspace_replace_range"),
            child("bad1",false,3,"workspace_replace_range"),
            child("bad2",false,3,"workspace_replace_range")];
        rows[0]["toolTimeline"][0]["path"]=json!("different.rs");
        assert!(induce(&rows.iter().collect::<Vec<_>>()).is_empty());
    }
}
