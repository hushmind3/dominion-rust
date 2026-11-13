//! Receipt-derived RSI decision-rule synthesis.
//!
//! Stage names are a bootstrap. These rules are executable conditional nodes:
//! a bounded Boolean expression over measured tool events and a generated tool
//! action. Children can invent new combinations of predicates, thresholds,
//! operator choices, and transitions without adding Rust stage names.
//! This synthesizer does not claim a source-quality improvement: only the
//! independent paired holdout and adopted descendant lineage can do that.
use serde_json::{json, Value};
use std::collections::BTreeSet;

const SOURCE_TOOLS: &[&str] = &[
    "code_search", "workspace_read_range", "workspace_replace_range",
    "workspace_replace", "workspace_write",
];
const EDIT_TOOLS: &[&str] = &[
    "workspace_replace_range", "workspace_replace", "workspace_write",
];
const MAX_RULES: usize = 6;

#[derive(Debug, Clone)]
pub struct Decision {
    pub id: String,
    pub allow: Vec<String>,
}
impl Decision {
    pub fn allows(&self, tool: &str) -> bool {
        self.allow.iter().any(|name| name == tool)
    }
    pub fn edit_only(&self) -> bool {
        self.allow.iter().all(|name| EDIT_TOOLS.contains(&name.as_str()))
    }
}

fn metric_valid(metric: &str) -> bool {
    if matches!(metric, "grounded" | "tool_calls" | "edit_attempts"
        | "unrecovered_edit_failure" | "successful_write" | "search_after_ground") {
        return true;
    }
    if let Some((status, tool)) = metric.split_once(':') {
        return matches!(status, "completed" | "failed")
            && SOURCE_TOOLS.contains(&tool);
    }
    false
}

fn guard_valid(value: &Value, depth: usize) -> bool {
    if depth > 3 {return false;}
    // A learned temporal predicate is an ordered sequence of real tool
    // events, not an extra named state in a hardcoded stage catalog.
    if let Some(events)=value.get("sequence").and_then(Value::as_array) {
        return events.len()>=2 && events.len()<=4 && events.iter().all(|event|
            event["tool"].as_str().is_some_and(|tool|SOURCE_TOOLS.contains(&tool))
            && matches!(event["status"].as_str(),Some("completed"|"failed")));
    }
    if let Some(all)=value.get("all").and_then(Value::as_array) {
        return !all.is_empty() && all.len()<=4
            && all.iter().all(|child|guard_valid(child,depth+1));
    }
    if let Some(any)=value.get("any").and_then(Value::as_array) {
        return !any.is_empty() && any.len()<=4
            && any.iter().all(|child|guard_valid(child,depth+1));
    }
    if let Some(not)=value.get("not") {
        return guard_valid(not,depth+1);
    }
    let metric=value["metric"].as_str().unwrap_or("");
    let op=value["cmp"].as_str().unwrap_or("");
    metric_valid(metric) && matches!(op,"eq"|"ge"|"lt")
        && value["value"].as_u64().is_some_and(|n|n<=48)
}

pub fn normalize(raw: &Value) -> Vec<Value> {
    let mut result=Vec::new();
    let mut seen=BTreeSet::new();
    for rule in raw.as_array().into_iter().flatten().take(MAX_RULES * 3) {
        if !guard_valid(&rule["guard"],0) {continue;}
        let Some(allowed)=rule["allow"].as_array() else {continue;};
        if allowed.is_empty() || allowed.len()>3 {continue;}
        let Some(names)=allowed.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {continue;};
        if names.iter().any(|name|!SOURCE_TOOLS.contains(name)) {continue;}
        let mut tools=names.into_iter().map(str::to_string).collect::<Vec<_>>();
        tools.sort();
        tools.dedup();
        let identifier=crate::engine::memory::stable_id(&[
            "rsi.synthesized-guard.v1",
            &rule["guard"].to_string(),&tools.join("|"),
        ]);
        if seen.insert(identifier.clone()) {
            result.push(json!({
                "id":identifier,
                "guard":rule["guard"],
                "allow":tools,
                "sourceJobId":rule["sourceJobId"],
                "parentRuleId":rule["parentRuleId"],
            }));
        }
        if result.len()>=MAX_RULES {break;}
    }
    result
}

fn fact(metric:&str, ops:&[Value], calls:u64, prompt_grounded:bool) -> Option<u64> {
    let mut grounded=prompt_grounded;
    let mut failures_open=false;
    let mut search_after_ground=0u64;
    let mut edited=0u64;
    let mut write_ok=false;
    let mut matches=0u64;
    for op in ops {
        let tool=op["tool"].as_str().unwrap_or("");
        let status=op["status"].as_str().unwrap_or("");
        let completed=status=="completed";
        if metric==format!("{status}:{tool}") {matches+=1;}
        if completed && tool=="code_search" && grounded {search_after_ground+=1;}
        if completed && tool=="workspace_read_range" {
            grounded=true;
            failures_open=false;
        }
        if EDIT_TOOLS.contains(&tool) {
            edited+=1;
            if completed && op["result"]["written"]==true
                && op["toolReceipt"]["verified"]==true {write_ok=true;}
            else if status=="failed" || (completed && op["result"]["written"]==false) {
                failures_open=true;
            }
        }
    }
    match metric {
        "grounded" => Some(u64::from(grounded)),
        "tool_calls" => Some(calls),
        "edit_attempts" => Some(edited),
        "unrecovered_edit_failure" => Some(u64::from(failures_open)),
        "successful_write" => Some(u64::from(write_ok)),
        "search_after_ground" => Some(search_after_ground),
        _ if metric_valid(metric) => Some(matches),
        _ => None,
    }
}

fn matches_guard(guard:&Value, ops:&[Value], calls:u64, grounded:bool, depth:usize)->bool {
    if depth>3 {return false;}
    if let Some(sequence)=guard.get("sequence").and_then(Value::as_array) {
        // Keep temporal evidence within one actual model execution. A read
        // by worker A and search by worker B are not a learned transition.
        // Try every possible start, since an earlier unmatched execution
        // must not hide a later genuine sequence in the current worker.
        for (start,op) in ops.iter().enumerate() {
            if op["tool"]!=sequence[0]["tool"]
                || op["status"]!=sequence[0]["status"] {continue;}
            let run=op["runId"].as_str();
            let mut at=1usize;
            for next in ops.iter().skip(start+1) {
                if run.is_some() && next["runId"].as_str()!=run {continue;}
                if next["tool"]==sequence[at]["tool"]
                    && next["status"]==sequence[at]["status"] {
                    at+=1;
                    if at==sequence.len() {return true;}
                }
            }
        }
        return false;
    }
    if let Some(all)=guard.get("all").and_then(Value::as_array) {
        return all.iter().all(|g|matches_guard(g,ops,calls,grounded,depth+1));
    }
    if let Some(any)=guard.get("any").and_then(Value::as_array) {
        return any.iter().any(|g|matches_guard(g,ops,calls,grounded,depth+1));
    }
    if let Some(not)=guard.get("not") {
        return !matches_guard(not,ops,calls,grounded,depth+1);
    }
    let Some(metric)=guard["metric"].as_str() else {return false;};
    let Some(current)=fact(metric,ops,calls,grounded) else {return false;};
    let Some(value)=guard["value"].as_u64() else {return false;};
    match guard["cmp"].as_str().unwrap_or("") {
        "eq" => current==value,
        "ge" => current>=value,
        "lt" => current<value,
        _ => false,
    }
}

pub fn choose(policy:&Value, ops:&[Value], calls:u64,
    focused:bool, prompt_grounded:bool)->Option<Decision> {
    if !focused {return None;}
    if fact("successful_write",ops,calls,prompt_grounded)==Some(1) {return None;}
    let ground=fact("grounded",ops,calls,prompt_grounded)==Some(1);
    // A source excerpt alone is not a fresh editable file hash. Generated
    // rules may propose writes only after this run actually observed the
    // current source through the normal native tool.
    let verified_source_read=ops.iter().any(|op|
        op["status"]=="completed" && op["tool"]=="workspace_read_range");
    let failed=fact("unrecovered_edit_failure",ops,calls,prompt_grounded)==Some(1);
    for rule in normalize(&policy["rules"]) {
        if !matches_guard(&rule["guard"],ops,calls,prompt_grounded,0) {continue;}
        let allow=rule["allow"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
        // No generated predicate can authorize a speculative source write.
        // Failed edits must first regain a current source/hash observation.
        if allow.iter().any(|tool|EDIT_TOOLS.contains(&tool.as_str()))
            && (!ground || !verified_source_read || failed) {continue;}
        return Some(Decision{id:rule["id"].as_str().unwrap_or("").to_string(),allow});
    }
    None
}

fn condition(metric:&str,cmp:&str,value:u64)->Value {
    json!({"metric":metric,"cmp":cmp,"value":value})
}
fn conjunction(conditions:Vec<Value>)->Value {json!({"all":conditions})}

/// Mutate a learned Boolean program without growing its AST depth forever.
/// Preserve its existing predicates, then specialize its call budget.
fn specialize_call_guard(guard:&Value,limit:u64)->Value {
    // Flatten descendants' conjunctions, retaining predicates rather than
    // infinitely wrapping all(all(all(...))). Replace obsolete call limits:
    // preserving two contradictory limits means the child never evolves.
    fn flatten(guard:&Value, clauses:&mut Vec<Value>){
        if let Some(all)=guard.get("all").and_then(Value::as_array) {
            for child in all {flatten(child,clauses);}
        } else if guard["metric"]!="tool_calls" && clauses.len()<3 {
            clauses.push(guard.clone());
        }
    }
    let mut clauses=Vec::new();
    flatten(guard,&mut clauses);
    clauses.push(condition("tool_calls","lt",limit.min(48)));
    conjunction(clauses)
}

/// A verified, actually adopted source improvement can teach DOMINION to
/// eliminate the search work that preceded its successful grounded edit.
/// The compressed method is only a *proposal* until a descendant independently
/// measures and adopts its source change. Success must originate from the
/// immutable paired-evaluation receipt, not model prose or a generated test.
fn proposed_success_compression(outcome:&Value)->Option<Value> {
    if outcome["accepted"]!=true || outcome["evidenceVerified"]!=true
        || outcome["evaluation"]["metrics"]["eligible"]!=true {return None;}
    let job=outcome["jobId"].as_str().filter(|s|!s.is_empty())?;
    let timeline=outcome["toolTimeline"].as_array()?;
    // Never stitch together a read from one execution and an unrelated
    // edit in a later repair execution: the source evidence is run-local.
    let mut observations=std::collections::BTreeMap::<&str,(bool,u64)>::new();
    let mut winner=None;
    for op in timeline {
        let Some(run)=op["runId"].as_str().filter(|s|!s.is_empty()) else {continue;};
        let name=op["tool"].as_str().unwrap_or("");
        let entry=observations.entry(run).or_default();
        if name=="workspace_read_range" && op["status"]=="completed" {
            entry.0=true;
        }
        if name=="code_search" && op["status"]=="completed" && entry.0 {
            entry.1+=1;
        }
        if matches!(name,"workspace_replace_range"|"workspace_replace")
            && entry.0 && entry.1>0 && op["status"]=="completed"
            && op["result"]["written"]==true
            && op["toolReceipt"]["verified"]==true {
            let matches_adopted=outcome["acceptedSourceHashes"].as_object()
                .and_then(|files|op["path"].as_str().and_then(|path|files.get(path)))
                .and_then(Value::as_str)
                .is_some_and(|sha|sha.len()==64 &&
                    op["result"]["postSha256"].as_str()==Some(sha));
            if matches_adopted {
                winner=Some((name,run));
                break;
            }
        }
    }
    let (winner,winning_run)=winner?;
    let losing=outcome["ruleExecutions"].as_array().into_iter().flatten()
        .rev().find(|e|e["runId"].as_str()==Some(winning_run))
        .and_then(|e|e["ruleId"].as_str()).unwrap_or("");
    Some(new_rule(conjunction(vec![
        condition("completed:workspace_read_range","ge",1),
        condition("edit_attempts","eq",0),
        condition("successful_write","eq",0),
    ]),&[winner],job,losing))
}

fn timeline_counts(outcome:&Value, tool:&str, status:&str)->u64 {
    outcome["toolTimeline"].as_array().into_iter().flatten()
        .filter(|op|op["tool"]==tool && op["status"]==status)
        .count() as u64
}
fn new_rule(guard:Value,allow:&[&str],source:&str,parent:&str)->Value {
    json!({"guard":guard,"allow":allow,"sourceJobId":source,
        "parentRuleId":if parent.is_empty(){Value::Null}else{json!(parent)}})
}

/// Transform one *executed* failure into a novel conditional execution node.
/// Rules use actual tool receipts; candidate names or a model's prose alone
/// cannot become a source of an executable policy.
pub fn evolve(parent:&[Value],outcome:&Value)->Vec<Value> {
    let mut rules=normalize(&json!(parent));
    let Some(job)=outcome["jobId"].as_str().filter(|id|!id.is_empty()) else {return rules;};
    if outcome["tokens"].as_u64().unwrap_or(0)==0 {return rules;}
    let failed_range=timeline_counts(outcome,"workspace_replace_range","failed");
    let failed_exact=timeline_counts(outcome,"workspace_replace","failed");
    let searches=timeline_counts(outcome,"code_search","completed");
    let reads=timeline_counts(outcome,"workspace_read_range","completed");
    let attempts=failed_range+failed_exact
        +timeline_counts(outcome,"workspace_replace_range","completed")
        +timeline_counts(outcome,"workspace_replace","completed");
    let mut generated=proposed_success_compression(outcome);

    // A historical rule actually ran but stalled: mutate the rule's *guard*
    // instead of simply retaining its fixed numeric retry controller.
    if outcome["accepted"]!=true && generated.is_none() {
        if let Some(id)=outcome["ruleExecutions"].as_array().into_iter().flatten()
            .last().and_then(|row|row["ruleId"].as_str()) {
            if let Some(index)=rules.iter().position(|rule|rule["id"]==id) {
                let prior=rules.remove(index);
                let changed=specialize_call_guard(&prior["guard"],
                    outcome["ruleExecutions"].as_array().into_iter().flatten()
                        .last().and_then(|row|row["toolCalls"].as_u64())
                        .unwrap_or(3).saturating_add(2));
                let tools=prior["allow"].as_array().into_iter().flatten()
                    .filter_map(Value::as_str).collect::<Vec<_>>();
                generated=Some(new_rule(changed,&tools,job,id));
            }
        }
    }
    if generated.is_none() {
        // Temporal recovery learned from a *completed* failed-edit -> re-read
        // sequence. After the new hash was observed, use a different source
        // operator rather than revisiting an already disproven edit action.
        let timeline=outcome["toolTimeline"].as_array();
        let mut recovered_tool=None::<&str>;
        if let Some(operations)=timeline {
            let mut failed=None::<(&str,&str)>;
            for op in operations {
                let Some(run)=op["runId"].as_str().filter(|s|!s.is_empty()) else {continue;};
                let name=op["tool"].as_str().unwrap_or("");
                if matches!(name,"workspace_replace_range"|"workspace_replace")
                    && op["status"]=="failed" {failed=Some((name,run));}
                if name=="workspace_read_range" && op["status"]=="completed" {
                    if let Some((tool,failed_run))=failed {
                        if run==failed_run {recovered_tool=Some(tool);}
                    }
                }
            }
        }
        if let Some(tool)=recovered_tool.filter(|_|outcome["candidateId"].is_null()) {
            let alternative=if tool=="workspace_replace_range" {
                "workspace_replace"
            } else {"workspace_replace_range"};
            generated=Some(new_rule(conjunction(vec![
                json!({"sequence":[
                    {"tool":tool,"status":"failed"},
                    {"tool":"workspace_read_range","status":"completed"}
                ]}),
                condition("unrecovered_edit_failure","eq",0),
                condition("successful_write","eq",0),
            ]),&[alternative],job,""));
        } else if failed_range+failed_exact>0 {
            // Derive the failing operator from observed receipts. The child
            // invents a guard requiring a fresh read after that specific
            // failure, rather than selecting a named stage from a catalog.
            let failed_tool=if failed_range>=failed_exact {
                "workspace_replace_range"
            } else {"workspace_replace"};
            generated=Some(new_rule(conjunction(vec![
                condition("unrecovered_edit_failure","eq",1),
                condition(&format!("failed:{failed_tool}"),"ge",1),
            ]),&["workspace_read_range"],job,""));
        } else if searches>=2 && attempts==0 && outcome["candidateId"].is_null() && reads>0 {
            let cutoff=searches.clamp(2,6);
            generated=Some(new_rule(conjunction(vec![
                condition("grounded","eq",1),
                condition("edit_attempts","eq",0),
                condition("completed:code_search","ge",cutoff),
            ]),&["workspace_replace","workspace_replace_range"],job,""));
        } else if outcome["candidateId"].is_string() && outcome["accepted"]!=true
            && outcome["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees" {
            // A neutral candidate induces a new counterexample-search
            // judgment before another edit. This is not a success reward.
            generated=Some(new_rule(conjunction(vec![
                condition("grounded","eq",1),
                condition("search_after_ground","eq",0),
                condition("edit_attempts","eq",0),
            ]),&["code_search","workspace_read_range"],job,""));
        }
    }
    if let Some(rule)=generated {
        let mut combined=vec![rule];combined.extend(rules);
        normalize(&json!(combined))
    } else {rules}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn worker(id:&str,policy:&Value,events:Vec<Value>)->Value {
        json!({"jobId":id,"tokens":4000,"editPolicy":policy,
            "candidateId":null,"accepted":false,"toolTimeline":events})
    }
    fn event(tool:&str,status:&str)->Value {
        json!({"runId":"test-run","tool":tool,"status":status,"result":{}})
    }
    #[test]
    fn synthesizes_predicate_from_a_real_failed_operator_and_runs_it(){
        let parent=json!({});
        let first=worker("generation-one",&parent,vec![
            event("workspace_read_range","completed"),
            event("workspace_replace_range","failed")]);
        let rule=evolve(&[],&first);
        assert_eq!(rule.len(),1);
        let live=json!({"rules":rule});
        let operations=first["toolTimeline"].as_array().unwrap().clone();
        let decision=choose(&live,&operations,2,true,false).unwrap();
        assert!(decision.allows("workspace_read_range"));
        assert!(!decision.allows("workspace_replace_range"));
        let mut recovered=operations;
        recovered.push(event("workspace_read_range","completed"));
        assert!(choose(&live,&recovered,3,true,false).is_none());
    }
    #[test]
    fn composes_new_threshold_from_observed_search_stall() {
        let events=vec![event("workspace_read_range","completed"),
            event("code_search","completed"),event("code_search","completed"),
            event("code_search","completed")];
        let learned=evolve(&[],&worker("search-stalled",&json!({}),events.clone()));
        assert_eq!(learned[0]["guard"]["all"][2]["value"],3);
        let live=json!({"rules":learned});
        assert!(choose(&live,&events[..3],3,true,false).is_none());
        let action=choose(&live,&events,4,true,false).unwrap();
        assert!(action.edit_only());
        assert!(action.allows("workspace_replace"));
        assert!(!action.allows("code_search"));
    }
    #[test]
    fn a_failed_rule_execution_rewrites_its_own_boolean_guard(){
        let original=evolve(&[],&worker("g1",&json!({}),vec![
            event("workspace_read_range","completed"),
            event("workspace_replace_range","failed")]));
        let id=original[0]["id"].as_str().unwrap().to_owned();
        let second=json!({"jobId":"g2","tokens":4000,"accepted":false,
            "ruleExecutions":[{"ruleId":id,"toolCalls":3}]});
        let revised=evolve(&original,&second);
        assert_eq!(revised.len(),1);
        assert_ne!(revised[0]["id"],original[0]["id"]);
        let clauses=revised[0]["guard"]["all"].as_array().unwrap();
        assert!(clauses.iter().any(|clause|clause["metric"]=="tool_calls"),
            "the descendant must gain a measured call-budget guard");
        assert!(clauses.iter().any(|clause|clause["metric"]=="failed:workspace_replace_range"),
            "rewriting must preserve the observed failure predicate");
    }
    #[test]
    fn new_temporal_rule_requires_ordered_recovery_then_changes_edit_operator(){
        let timeline=vec![
            event("workspace_read_range","completed"),
            event("workspace_replace_range","failed"),
            event("workspace_read_range","completed"),
        ];
        let rules=evolve(&[],&worker("recovered-g1",&json!({}),timeline.clone()));
        assert!(rules[0]["guard"]["all"][0]["sequence"].is_array());
        let choice=choose(&json!({"rules":rules}),&timeline,3,true,false).unwrap();
        assert!(choice.allows("workspace_replace"));
        assert!(!choice.allows("workspace_replace_range"));
        let unrecovered=vec![
            event("workspace_read_range","completed"),
            event("workspace_replace_range","failed")
        ];
        assert!(choose(&json!({"rules":rules}),&unrecovered,2,true,false).is_none());
    }
    #[test]
    fn temporal_rule_does_not_join_observations_across_execution_runs(){
        let policy=json!({"rules":[{
            "guard":{"sequence":[
                {"tool":"workspace_read_range","status":"completed"},
                {"tool":"code_search","status":"completed"}
            ]},"allow":["workspace_replace_range"]
        }]});
        let mixed=vec![
            json!({"runId":"old","tool":"workspace_read_range","status":"completed"}),
            json!({"runId":"new","tool":"code_search","status":"completed"})
        ];
        assert!(choose(&policy,&mixed,2,true,false).is_none());
        let valid=vec![
            json!({"runId":"old","tool":"workspace_read_range","status":"completed"}),
            json!({"runId":"new","tool":"workspace_read_range","status":"completed"}),
            json!({"runId":"new","tool":"code_search","status":"completed"})
        ];
        assert!(choose(&policy,&valid,3,true,false).is_some(),
            "a previous unmatched execution must not mask a later real match");
    }
    #[test]
    fn repeated_rule_stalls_do_not_make_unbounded_guard_trees(){
        let mut rules=evolve(&[],&worker("g1",&json!({}),vec![
            event("workspace_read_range","completed"),
            event("workspace_replace_range","failed")]));
        for number in 0..5u64 {
            let old=rules[0]["id"].as_str().unwrap().to_owned();
            let next=json!({"jobId":format!("g{}",number+2),"tokens":3000,
                "ruleExecutions":[{"ruleId":old,"toolCalls":number+3}]});
            rules=evolve(&rules,&next);
            assert_eq!(rules.len(),1);
            assert!(guard_valid(&rules[0]["guard"],0));
        }
    }
    #[test]
    fn verified_success_compresses_source_search_into_a_new_edit_rule(){
        let mut result=worker("successful-child",&json!({}),vec![
            event("code_search","completed"),
            event("workspace_read_range","completed"),
            event("code_search","completed"),
            event("code_search","completed"),
            json!({"runId":"test-run","tool":"workspace_replace","status":"completed",
                "path":"src/engine/example.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}}),
        ]);
        result["acceptedSourceHashes"]=json!({"src/engine/example.rs":"a".repeat(64)});
        result["accepted"]=json!(true);
        result["evidenceVerified"]=json!(true);
        result["evaluation"]=json!({"metrics":{"eligible":true}});
        let compressed=evolve(&[],&result);
        assert_eq!(compressed.len(),1);
        assert_eq!(compressed[0]["sourceJobId"],"successful-child");
        assert_eq!(compressed[0]["allow"],json!(["workspace_replace"]));
        let read=vec![event("workspace_read_range","completed")];
        let next=choose(&json!({"rules":compressed}),&read,1,true,false).unwrap();
        assert!(next.edit_only());
        assert!(next.allows("workspace_replace"));
        assert!(!next.allows("code_search"));
        let mut insufficient=result.clone();
        insufficient["evidenceVerified"]=json!(false);
        assert!(evolve(&[],&insufficient).is_empty());
    }
    #[test]
    fn inherited_success_guard_only_earns_quality_if_next_child_executes(){
        let base=worker("g1",&json!({}),vec![
            event("workspace_read_range","completed"),
            event("code_search","completed"),
            json!({"runId":"test-run","tool":"workspace_replace_range","status":"completed",
                "path":"src/engine/example.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}})
        ]);
        let mut successful=base;
        successful["acceptedSourceHashes"]=json!({"src/engine/example.rs":"a".repeat(64)});
        successful["accepted"]=json!(true);
        successful["evidenceVerified"]=json!(true);
        successful["evaluation"]=json!({"metrics":{"eligible":true}});
        let child=evolve(&[],&successful);
        assert_eq!(child.len(),1);
        // A policy proposal is not evidence that the next model used it.
        let next=worker("g2",&json!({"rules":child.clone()}),vec![]);
        assert!(evolve(&child,&next).len()>=1);
    }
    #[test]
    fn source_evidence_from_different_runs_cannot_invent_a_success_transition(){
        let mut outcome=worker("adopted",&json!({}),vec![
            json!({"runId":"run-1","tool":"workspace_read_range","status":"completed"}),
            json!({"runId":"run-1","tool":"code_search","status":"completed"}),
            json!({"runId":"run-2","tool":"workspace_replace","status":"completed",
                "result":{"written":true},"toolReceipt":{"verified":true}})
        ]);
        outcome["accepted"]=json!(true);
        outcome["evidenceVerified"]=json!(true);
        outcome["evaluation"]=json!({"metrics":{"eligible":true}});
        assert!(evolve(&[],&outcome).is_empty());
    }
    #[test]
    fn learned_edit_guard_needs_native_source_read_not_only_prompt_context(){
        let policy=json!({"rules":[{"guard":
            {"metric":"grounded","cmp":"eq","value":1},
            "allow":["workspace_replace_range"]}]});
        assert!(choose(&policy,&[],0,true,true).is_none(),
            "an injected excerpt is not a current source hash");
        let read=vec![event("workspace_read_range","completed")];
        assert!(choose(&policy,&read,1,true,true).unwrap()
            .allows("workspace_replace_range"));
    }
    #[test]
    fn false_success_and_unknown_tool_cannot_become_rules(){
        let fake=json!({"jobId":"x","tokens":0,
            "toolTimeline":[event("workspace_replace_range","failed")],
            "accepted":true});
        assert!(evolve(&[],&fake).is_empty());
        let invalid=json!([{"guard":condition("sneaky","ge",0),
            "allow":["shell_exec"]}]);
        assert!(normalize(&invalid).is_empty());
    }
}
