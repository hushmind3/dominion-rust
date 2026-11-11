//! Self-evolving RSI judgement programs. These are executable rules over
//! witnessed operations, not another fixed stage list or language-model prose.
use crate::{engine::{memory,tools},state::AppState};
use serde_json::{json,Value};
use std::collections::BTreeSet;

fn editable(tool:&str)->bool {
    matches!(tool,"workspace_replace"|"workspace_replace_range"|"workspace_write")
}
fn allowed(tool:&str)->bool {
    matches!(tool,"code_search"|"workspace_read_range"|"workspace_replace"
        |"workspace_replace_range"|"workspace_write")
}
fn valid_guard(v:&Value,depth:usize)->bool{
    if depth>3{return false;}
    if let Some(all)=v.get("all").and_then(Value::as_array){
        return !all.is_empty()&&all.len()<=4
            && all.iter().all(|x|valid_guard(x,depth+1));
    }
    if let Some(any)=v.get("any").and_then(Value::as_array){
        return !any.is_empty()&&any.len()<=4
            && any.iter().all(|x|valid_guard(x,depth+1));
    }
    if let Some(events)=v.get("sequence").and_then(Value::as_array){
        return (2..=4).contains(&events.len()) && events.iter().all(|event|
            event["tool"].as_str().is_some_and(allowed)
            && matches!(event["status"].as_str(),Some("completed"|"failed")));
    }
    let metric=v["metric"].as_str().unwrap_or("");
    (matches!(metric,"grounded"|"calls"|"edit_attempts"|"pending_failure"
        |"verified_write"|"read_calls")
        || metric.strip_prefix("completed:").is_some_and(allowed)
        || metric.strip_prefix("failed:").is_some_and(allowed))
        && matches!(v["cmp"].as_str(),Some("eq"|"ge"|"lt"))
        && v["value"].as_u64().is_some_and(|n|n<=32)
}
pub fn normalize(raw:&Value)->Vec<Value>{
    let mut rules=Vec::new();
    let mut ids=BTreeSet::new();
    for rule in raw.as_array().into_iter().flatten().take(24){
        if !valid_guard(&rule["guard"],0){continue;}
        let Some(names)=rule["allow"].as_array() else{continue;};
        if names.is_empty()||names.len()>3{continue;}
        let Some(mut names)=names.iter().map(|name|
            name.as_str().filter(|name|allowed(name))
                .map(str::to_string)).collect::<Option<Vec<_>>>() else{continue;};
        names.sort();names.dedup();
        let id=memory::stable_id(&[
            "champion.runtime-judgement.v1",&rule["guard"].to_string(),
            &names.join(",")]);
        if ids.insert(id.clone()){
            rules.push(json!({"id":id,"guard":rule["guard"],"allow":names,
                "sourceJobId":rule["sourceJobId"],"parentRuleId":rule["parentRuleId"]}));
        }
        if rules.len()>=6{break;}
    }
    rules
}
fn count(metric:&str,ops:&[Value],calls:u64,source_ready:bool)->Option<u64>{
    let mut grounded=source_ready;
    let mut read=0u64;
    let mut edit_attempts=0u64;
    let mut pending=false;
    let mut verified_write=false;
    let mut matching=0u64;
    for op in ops{
        let tool=op["tool"].as_str().unwrap_or("");
        let status=op["status"].as_str().unwrap_or("");
        if metric==format!("{status}:{tool}"){matching+=1;}
        if tool=="workspace_read_range" && status=="completed"{
            grounded=true;read+=1;pending=false;
        }
        if editable(tool){
            edit_attempts+=1;
            if status=="completed"&&op["result"]["written"]==true
                && op["toolReceipt"]["verified"]==true {
                verified_write=true;pending=false;
            } else if status=="failed"||op["result"]["error"].is_string()
                || op["result"]["written"]==false {pending=true;}
        }
    }
    Some(match metric {
        "grounded"=>u64::from(grounded),"calls"=>calls,
        "read_calls"=>read,"edit_attempts"=>edit_attempts,
        "pending_failure"=>u64::from(pending),
        "verified_write"=>u64::from(verified_write),
        _ if metric.starts_with("completed:")||metric.starts_with("failed:")=>matching,
        _=>return None,
    })
}
fn matches_guard(guard:&Value,ops:&[Value],calls:u64,grounded:bool,depth:usize)->bool{
    if depth>3{return false;}
    if let Some(all)=guard.get("all").and_then(Value::as_array){
        return all.iter().all(|g|matches_guard(g,ops,calls,grounded,depth+1));
    }
    if let Some(any)=guard.get("any").and_then(Value::as_array){
        return any.iter().any(|g|matches_guard(g,ops,calls,grounded,depth+1));
    }
    if let Some(seq)=guard.get("sequence").and_then(Value::as_array){
        let mut pos=0usize;
        for op in ops {
            if pos<seq.len() && op["tool"]==seq[pos]["tool"]
                && op["status"]==seq[pos]["status"]{pos+=1;}
        }
        return pos==seq.len();
    }
    let Some(metric)=guard["metric"].as_str() else{return false;};
    let Some(n)=count(metric,ops,calls,grounded) else{return false;};
    let Some(t)=guard["value"].as_u64() else{return false;};
    match guard["cmp"].as_str().unwrap_or(""){
        "eq"=>n==t,"ge"=>n>=t,"lt"=>n<t,_=>false,
    }
}
pub fn choose(policy:&Value,operations:&[Value],calls:u64,
    focused:bool,source_ready:bool)->Option<Value>{
    if !focused || count("verified_write",operations,calls,source_ready)==Some(1){
        return None;
    }
    let grounded=count("grounded",operations,calls,source_ready)==Some(1);
    let failed=count("pending_failure",operations,calls,source_ready)==Some(1);
    for rule in normalize(&policy["rules"]){
        if !matches_guard(&rule["guard"],operations,calls,source_ready,0){continue;}
        let names=rule["allow"].as_array()?;
        if names.iter().any(|name|name.as_str().is_some_and(editable))
            && (!grounded||failed){continue;}
        // All guards are finite and derived from witnessed runtime events.
        return Some(json!({"ruleId":rule["id"],"allow":names,
            "revision":policy["revision"],"reason":"learned_boolean_predicate"}));
    }
    None
}
fn cond(metric:&str,cmp:&str,value:u64)->Value{
    json!({"metric":metric,"cmp":cmp,"value":value})
}
fn rule(guard:Value,allow:&str,source:&Value,parent:&Value)->Value{
    json!({"guard":guard,"allow":[allow],
        "sourceJobId":source,"parentRuleId":parent})
}
fn evidence_rows(rows:&[Value],objective:&str,domain:&str)->Vec<Value>{
    rows.iter().filter(|row|
        row["objectiveId"].as_str()==Some(objective)
        && row["domain"].as_str()==Some(domain)
        && row["jobId"].as_str().is_some_and(|id|!id.is_empty())
    ).cloned().collect()
}
/// Mutate the decision-making program after actual failures and accepted
/// improvements. Each mutation starts as an unproven candidate.
pub fn evolve(rows:&[Value],objective:&str,domain:&str)->Value{
    let same=evidence_rows(rows,objective,domain);
    let previous=same.iter().rev().find(|row|
        row["rsiJudgementTrial"]["policy"]["rules"].is_array());
    let parent=previous.map(|row|row["rsiJudgementTrial"]["policy"]["rules"].clone())
        .unwrap_or(json!([]));
    // Transfer a measured contrast between accepted and rejected descendants
    // into the actual executable runtime grammar. A two-sided cohort is
    // required; the induced rule remains an unproven, isolated trial.
    let contrasted=crate::engine::rsi_rule_induction::induce(
        &same.iter().collect::<Vec<_>>());
    if !contrasted.is_empty() {
        let mut inherited=normalize(&parent);
        let mut discovered=contrasted;
        discovered.append(&mut inherited);
        let normalized=normalize(&json!(discovered));
        let revision=memory::stable_id(&[
            "champion.contrastive-judgement.v1",&json!(normalized).to_string()]);
        return json!({"kind":"selfimprove.generated-judgement-program",
            "revision":revision,"rules":normalized,
            "sourceJobId":"independent_adopted_vs_rejected_descendant_cohorts",
            "qualityProven":false,"status":"requires_independent_real_execution",
            "operator":"contrastive_counterexample_guard_synthesis"});
    }
    let mut rules=normalize(&parent);
    let mut generated=None::<Value>;
    let mut evidence_job=Value::Null;
    for row in same.iter().rev().take(12){
        let trace=row["actionTrace"].as_array().cloned().unwrap_or_default();
        let reads=trace.iter().filter(|x|x["tool"]=="workspace_read_range"
            && x["executed"]==true).count() as u64;
        let edits=trace.iter().filter(|x|x["tool"].as_str().is_some_and(editable))
            .count();
        let errors=row["editOperations"].as_array().cloned().unwrap_or_default();
        let failure=errors.iter().find(|x|x["status"]=="failed"
            || x["error"].is_string() || x["staleVersion"]==true)
            .and_then(|e|e["tool"].as_str());
        let prior_used=row["rsiJudgementEffects"].as_array().is_some_and(|effects|
            effects.iter().any(|effect|effect["selected"]==true
                && effect["performed"]==true));
        if let Some(failed)=failure {
            let mut recovered=false;
            for t in &trace {
                if t["tool"]==failed && t["failure"]==true {recovered=false;}
                if t["tool"]=="workspace_read_range" && t["executed"]==true {
                    recovered=true;
                }
            }
            let alternate=if failed=="workspace_replace"{
                "workspace_replace_range"
            }else{"workspace_replace"};
            generated=Some(if recovered {
                rule(json!({"all":[
                    {"sequence":[{"tool":failed,"status":"failed"},
                        {"tool":"workspace_read_range","status":"completed"}]},
                    cond("pending_failure","eq",0),
                    cond("verified_write","eq",0)]}),
                    alternate,&row["jobId"],&Value::Null)
            }else {
                rule(json!({"all":[
                    cond(&format!("failed:{failed}"),"ge",1),
                    cond("pending_failure","eq",1)]}),
                    "workspace_read_range",&row["jobId"],&Value::Null)
            });
        } else if prior_used && row["accepted"]!=true && edits==0 {
            // An executed, ineffective guard is rewritten rather than kept
            // forever. A fresh source observation is required before editing.
            if let Some(old)=rules.first(){
                let parent_id=old["id"].clone();
                generated=Some(rule(json!({"all":[
                    cond("grounded","eq",1),
                    cond("read_calls","ge",1),
                    cond("edit_attempts","eq",0)]}),
                    "workspace_replace_range",&row["jobId"],&parent_id));
                rules.remove(0);
            }
        } else if reads>=2 && edits==0 && row["tokens"].as_u64().unwrap_or(0)>0 {
            let threshold=reads.min(4);
            let chosen=row["improvementDirective"]["policy"]["preferredEditTool"]
                .as_str().filter(|s|editable(s))
                .unwrap_or("workspace_replace_range");
            generated=Some(rule(json!({"all":[
                cond("grounded","eq",1),
                cond("read_calls","ge",threshold),
                cond("edit_attempts","eq",0)]}),
                chosen,&row["jobId"],&Value::Null));
        }
        if generated.is_some(){evidence_job=row["jobId"].clone();break;}
    }
    if let Some(rule)=generated{
        rules.insert(0,rule);
    }
    let normalized=normalize(&json!(rules));
    let revision=memory::stable_id(&[
        "champion.judgement-generator.v1",&json!(normalized).to_string()]);
    json!({"kind":"selfimprove.generated-judgement-program",
        "revision":revision,"rules":normalized,
        "sourceJobId":evidence_job,"qualityProven":false,
        "status":"requires_independent_real_execution",
        "operator":"condition_synthesis_from_observed_events"})
}
/// Matched sequential cohorts; only candidate rule executions with a
/// post-selection verified source effect can receive candidate gain credit.
pub fn trial(rows:&[Value],policy:Value,head:&str,objective:&str,
    model:&str,provider:&str,domain:&str)->Value{
    if policy["rules"].as_array().is_none_or(Vec::is_empty) {
        return json!({"arm":"baseline","status":"no_generated_rules","policy":policy});
    }
    let revision=policy["revision"].as_str().unwrap_or("");
    let mut candidate=0u64;
    let mut baseline=0u64;
    let mut candidate_gain=0u64;
    let mut baseline_gain=0u64;
    let mut observed=0u64;
    for row in rows{
        if row["rsiJudgementTrial"]["revision"]!=revision
            || row["generatorCommit"]!=head
            || row["objectiveId"]!=objective
            || row["generatorModel"]!=model
            || row["generatorProviderId"]!=provider
            || row["domain"]!=domain {continue;}
        let gain=row["accepted"]==true
            && row["evaluation"]["metrics"]["eligible"]==true
            && row["evaluation"]["metrics"]["regressions"].as_u64()==Some(0)
            && row["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0)>0;
        if row["rsiJudgementTrial"]["arm"]=="candidate"{
            candidate+=1;
            let performed=row["rsiJudgementEffects"].as_array().is_some_and(|effects|
                effects.iter().any(|e|e["revision"]==revision && e["performed"]==true));
            let changed=row["rsiJudgementEffects"].as_array().is_some_and(|effects|
                effects.iter().any(|e|e["revision"]==revision
                    && e["verifiedSourceWrite"]==true));
            observed+=u64::from(performed);
            candidate_gain+=u64::from(gain&&changed);
        }else if row["rsiJudgementTrial"]["arm"]=="baseline"{
            baseline+=1;
            baseline_gain+=u64::from(gain);
        }
    }
    let paired=candidate>=2&&baseline>=2;
    let status=if !paired{"collecting_frozen_judgement_cohorts"}
        else if observed==0{"judgement_not_executed"}
        else if candidate_gain>baseline_gain{"judgement_holdout_candidate_preferred"}
        else{"judgement_no_measured_gain"};
    let arm=if status=="judgement_holdout_candidate_preferred" {"candidate"}
        else if matches!(status,"judgement_not_executed"|"judgement_no_measured_gain") {
            "baseline"
        }else if candidate<=baseline{"candidate"}else{"baseline"};
    json!({"arm":arm,"status":status,"revision":revision,"policy":policy,
        "frozen":{"sourceCommit":head,"objectiveId":objective,
            "model":model,"provider":provider,"domain":domain},
        "comparison":{"candidate":candidate,"baseline":baseline,
            "candidateHoldoutGains":candidate_gain,"baselineHoldoutGains":baseline_gain,
            "executedCandidateRules":observed,
            "qualityProven":status=="judgement_holdout_candidate_preferred",
            "causalGain":"not_estimated_by_sequential_cohorts"}})
}
pub fn pinned(rows:&[Value],head:&str,objective:&str,model:&str,
    provider:&str,domain:&str)->Option<Value>{
    let prior=rows.iter().rev().find(|row|{
        let frozen=&row["rsiJudgementTrial"]["frozen"];
        frozen["sourceCommit"]==head && frozen["objectiveId"]==objective
            && frozen["model"]==model && frozen["provider"]==provider
            && frozen["domain"]==domain
            && row["rsiJudgementTrial"]["policy"]["rules"].as_array()
                .is_some_and(|r|!r.is_empty())
    })?;
    let policy=prior["rsiJudgementTrial"]["policy"].clone();
    let current=trial(rows,policy.clone(),head,objective,model,provider,domain);
    (current["status"]=="collecting_frozen_judgement_cohorts").then_some(policy)
}
pub fn effects(state:&AppState,session_id:&str)->Vec<Value>{
    let operations=state.store.all("execution_operation");
    let focus=state.store.get("session",session_id)
        .and_then(|s|s["rsiFocusFiles"].as_array().cloned())
        .unwrap_or_default();
    let mut effects=Vec::new();
    for run in state.store.all("execution").into_iter()
        .filter(|row|row["sessionId"].as_str()==Some(session_id)){
        let Some(id)=run["id"].as_str() else{continue;};
        for choice in run["rsiJudgementChoices"].as_array().into_iter().flatten(){
            let Some(tool)=choice["selectedTool"].as_str() else{continue;};
            let after=choice["afterOpSeq"].as_u64();
            let matching=operations.iter().filter(|op|
                op["runId"].as_str()==Some(id)
                && op["tool"].as_str()==Some(tool)
                && op["opSeq"].as_u64().is_some_and(|seq|
                    after.is_none_or(|n|seq>n))
                && op["status"]=="completed"
                && op["result"]["error"].is_null()).collect::<Vec<_>>();
            effects.push(json!({"revision":choice["revision"],
                "ruleId":choice["ruleId"],"selected":true,
                "performed":!matching.is_empty(),
                "verifiedSourceWrite":matching.iter().any(|op|
                    editable(tool) && op["result"]["written"]==true
                    && op["toolReceipt"]["verified"]==true
                    && (focus.is_empty()||focus.contains(&op["arguments"]["path"]))),
                "runId":id,"selectedTool":tool,"afterOpSeq":after,
                "operationIds":matching.iter().map(|op|op["id"].clone()).collect::<Vec<_>>()
            }));
        }
    }
    effects
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn cold_read_only_history_generates_live_edit_predicate(){
        let obj="objective";let domain="tools";
        let history=vec![json!({"jobId":"first","objectiveId":obj,"domain":domain,
            "tokens":1000,"actionTrace":[{"tool":"workspace_read_range",
                "executed":true},{"tool":"workspace_read_range","executed":true}]})];
        let policy=evolve(&history,obj,domain);
        let ops=vec![json!({"tool":"workspace_read_range","status":"completed",
            "result":{"sourceSha256":"a".repeat(64)}});2];
        let choice=choose(&policy,&ops,2,true,false).unwrap();
        assert_eq!(choice["allow"],json!(["workspace_replace_range"]));
        assert!(choose(&policy,&[],0,true,false).is_none());
    }
    #[test]
    fn failed_edit_synthesizes_recovery_then_changes_operator(){
        let outcome=json!({"jobId":"failed","objectiveId":"goal","domain":"tools",
            "editOperations":[{"tool":"workspace_replace","status":"failed",
                "error":"staleVersion"}],
            "actionTrace":[{"tool":"workspace_replace","failure":true,
                "runId":"run"},{"tool":"workspace_read_range","executed":true,
                "runId":"run"}]});
        let program=evolve(&[outcome],"goal","tools");
        let ops=vec![json!({"tool":"workspace_replace","status":"failed"}),
            json!({"tool":"workspace_read_range","status":"completed"})];
        assert_eq!(choose(&program,&ops,2,true,false).unwrap()["allow"],
            json!(["workspace_replace_range"]));
    }
    #[test]
    fn unrelated_status_or_unexecuted_choice_never_promotes_a_rule(){
        let policy=json!({"revision":"r","rules":[{"guard":{"metric":"read_calls",
            "cmp":"ge","value":1},"allow":["workspace_replace"]}]});
        let no=trial(&[],policy.clone(),"h","o","m","p","tools");
        assert_ne!(no["comparison"]["qualityProven"],true);
        let mut invalid=policy.clone();
        invalid["rules"][0]["allow"]=json!(["shell_exec"]);
        assert!(normalize(&invalid["rules"]).is_empty());
    }
}