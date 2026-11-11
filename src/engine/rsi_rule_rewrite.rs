//! Executable meta-programs that rewrite the rule synthesizer's output.
//!
//! The child no longer inherits only a condition AST: it inherits a bounded
//! transformation program which edits how *future* condition ASTs are made.
//! A real failed tool, a rule that fired without producing a patch, or an
//! independently evaluated candidate is required before a rewrite is learned.
//! Learning the meta-program never implies a measured software improvement.
use serde_json::{json, Value};
use std::collections::BTreeSet;

const EDIT_TOOLS:&[&str]=&["workspace_replace_range","workspace_replace","workspace_write"];
const MAX_OPS:usize=4;

pub fn normalize(raw:&Value)->Vec<Value>{
    let mut result=Vec::new();
    let mut seen=BTreeSet::new();
    for op in raw.as_array().into_iter().flatten().take(MAX_OPS*2) {
        let Some(kind)=op["kind"].as_str() else {continue;};
        let normalized=match kind {
            "switch_edit"=>{
                let Some(from)=op["from"].as_str().filter(|s|EDIT_TOOLS.contains(s)) else {continue;};
                let Some(to)=op["to"].as_str().filter(|s|EDIT_TOOLS.contains(s)&&*s!=from) else {continue;};
                json!({"kind":kind,"from":from,"to":to})
            },
            "tighten"|"widen"=>{
                let Some(metric)=op["metric"].as_str().filter(|s|matches!(*s,
                    "tool_calls"|"edit_attempts"|"search_after_ground"|
                    "completed:code_search"|"completed:workspace_read_range")) else {continue;};
                let Some(value)=op["value"].as_u64().filter(|v|*v<=24) else {continue;};
                json!({"kind":kind,"metric":metric,"value":value})
            },
            "prune"=>{
                let Some(metric)=op["metric"].as_str().filter(|s|matches!(*s,
                    "tool_calls"|"completed:code_search"|"search_after_ground")) else {continue;};
                json!({"kind":kind,"metric":metric})
            },
            _=>continue,
        };
        let identity=normalized.to_string();
        if seen.insert(identity){result.push(normalized);}
        if result.len()>=MAX_OPS{break;}
    }
    result
}

fn stage_rule(guard:&Value,op:&Value)->Value {
    let condition=json!({"metric":op["metric"],"cmp":"ge","value":op["value"]});
    match op["kind"].as_str().unwrap_or("") {
        "tighten"=>{
            let mut clauses=guard["all"].as_array().cloned()
                .unwrap_or_else(||vec![guard.clone()]);
            // Same metric, newer observed threshold: replace, not stack two
            // contradictory thresholds or grow an unbounded AST.
            clauses.retain(|g|g["metric"]!=op["metric"]);
            if clauses.len()>=4 {clauses.remove(clauses.len()-1);}
            clauses.push(condition);
            json!({"all":clauses})
        },
        "widen"=>{
            // Do not generalize a write past the runtime's real-source-read
            // gate; this only broadens the condition *within* that gate.
            let mut clauses=guard["any"].as_array().cloned()
                .unwrap_or_else(||vec![guard.clone()]);
            if clauses.len()>=4 {clauses.remove(clauses.len()-1);}
            clauses.push(condition);
            json!({"any":clauses})
        },
        "prune"=>{
            if let Some(parts)=guard["all"].as_array() {
                let keep=parts.iter().filter(|g|g["metric"]!=op["metric"])
                    .cloned().collect::<Vec<_>>();
                if keep.is_empty(){guard.clone()}
                else if keep.len()==1 {keep[0].clone()}
                else{json!({"all":keep})}
            } else {guard.clone()}
        },
        _=>guard.clone(),
    }
}

/// Apply a learned *program of AST transformations* to a proposed rule set.
/// Invalid candidates fall back to the original rule, never to arbitrary
/// text supplied by an LLM.
pub fn apply(program:&[Value],raw_rules:&[Value])->Vec<Value> {
    let mut output=Vec::new();
    for rule in crate::engine::rsi_decision_rules::normalize(&json!(raw_rules)) {
        let mut proposal=rule.clone();
        for op in normalize(&json!(program)) {
            match op["kind"].as_str().unwrap_or("") {
                "switch_edit"=>{
                    if let Some(names)=proposal["allow"].as_array_mut(){
                        for tool in names {
                            if *tool==op["from"]{*tool=op["to"].clone();}
                        }
                    }
                },
                "tighten"|"widen"|"prune"=>{
                    // Counterexample predicates should apply to synthesized
                    // editing plans, not disable the obligatory re-read
                    // required to repair a hash mismatch.
                    if proposal["allow"].as_array().is_some_and(|tools|
                        tools.iter().any(|t|t.as_str().is_some_and(|name|EDIT_TOOLS.contains(&name)))) {
                        proposal["guard"]=stage_rule(&proposal["guard"],&op);
                    }
                },
                _=>{},
            }
        }
        let normalized=crate::engine::rsi_decision_rules::normalize(&json!([proposal]));
        output.push(normalized.into_iter().next().unwrap_or(rule));
    }
    crate::engine::rsi_decision_rules::normalize(&json!(output))
}

/// Mutate the AST-generating algorithm from the actual previous execution.
/// Operator identity is purely structural; job IDs do not split equivalent
/// programs into artificial new generations.
pub fn evolve(parent:&[Value],observation:&Value)->Vec<Value>{
    let mut learned=normalize(&json!(parent));
    if observation["jobId"].as_str().is_none_or(str::is_empty)
        || observation["tokens"].as_u64().unwrap_or(0)==0 {return learned;}
    let timeline=observation["toolTimeline"].as_array();
    let mut new_op=None::<Value>;
    // Failed edits teach a different patch operator.
    if let Some(failed)=timeline.into_iter().flatten().rev().find(|event|
        event["status"]=="failed" && matches!(event["tool"].as_str(),
            Some("workspace_replace_range"|"workspace_replace"))) {
        let from=failed["tool"].as_str().unwrap_or("");
        let to=if from=="workspace_replace_range"{"workspace_replace"}
            else{"workspace_replace_range"};
        new_op=Some(json!({"kind":"switch_edit","from":from,"to":to}));
    }
    let model_edit_matches_adoption=observation["toolTimeline"].as_array().is_some_and(|events|
        events.iter().any(|event|{
            let adopted=observation["acceptedSourceHashes"].as_object()
                .and_then(|hashes|event["path"].as_str().and_then(|p|hashes.get(p)))
                .and_then(Value::as_str);
            event["status"]=="completed" && event["result"]["written"]==true
                && event["toolReceipt"]["verified"]==true
                && adopted.is_some_and(|digest|digest.len()==64
                    && event["result"]["postSha256"].as_str()==Some(digest))
        }));
    if new_op.is_none() && observation["accepted"]==true
        && observation["evidenceVerified"]==true
        && observation["evaluation"]["metrics"]["eligible"]==true
        && model_edit_matches_adoption
        && observation["toolTimeline"].as_array().is_some_and(|events|
            events.iter().any(|x|x["tool"]=="code_search" && x["status"]=="completed")) {
        // Remove an expensive search predicate *as a trial*, never as a
        // verified performance gain. The independent tournament still decides
        // whether this rewritten generator is preferable.
        new_op=Some(json!({"kind":"prune","metric":"completed:code_search"}));
    }
    if new_op.is_none() && observation["candidateId"].is_string()
        && observation["accepted"]!=true
        && observation["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && observation["evaluation"]["metrics"].is_object() {
        let searches=timeline.into_iter().flatten()
            .filter(|x|x["tool"]=="code_search"&&x["status"]=="completed")
            .count() as u64;
        new_op=Some(json!({"kind":"tighten","metric":"completed:code_search",
            "value":searches.saturating_add(1).min(24)}));
    }
    if new_op.is_none() && observation["candidateId"].is_null()
        && observation["ruleExecutions"].as_array().is_some_and(|x|!x.is_empty()) {
        let reads=timeline.into_iter().flatten().filter(|x|
            x["tool"]=="workspace_read_range" && x["status"]=="completed")
            .count() as u64;
        if reads>0 {
            new_op=Some(json!({"kind":"widen","metric":"completed:workspace_read_range",
                "value":reads.min(24)}));
        }
    }
    if let Some(op)=new_op {
        // Replace the previous experiment in the same genetic slot; repeated
        // failures must evolve the generator instead of growing a fixed list.
        learned.retain(|item|item["kind"]!=op["kind"]);
        learned.insert(0,op);
        learned.truncate(MAX_OPS);
    }
    normalize(&json!(learned))
}

#[cfg(test)]
mod tests{
    use super::*;
    fn rule()->Value{json!({"guard":{"all":[
        {"metric":"grounded","cmp":"eq","value":1},
        {"metric":"completed:code_search","cmp":"ge","value":2}
    ]},"allow":["workspace_replace_range"]})}
    #[test]
    fn failed_edit_changes_the_rule_generating_program_and_next_action(){
        let failure=json!({"jobId":"g1","tokens":4000,
            "toolTimeline":[{"tool":"workspace_replace_range","status":"failed"}]});
        let grammar=evolve(&[],&failure);
        assert_eq!(grammar[0]["kind"],"switch_edit");
        let child=apply(&grammar,&[rule()]);
        assert_eq!(child[0]["allow"],json!(["workspace_replace"]));
        assert_ne!(child[0]["id"],crate::engine::rsi_decision_rules::normalize(
            &json!([rule()]))[0]["id"]);
        assert!(evolve(&grammar,&json!({"jobId":"g2","tokens":0})).eq(&grammar));
    }
    #[test]
    fn evaluated_neutral_quality_changes_the_synthesis_grammar(){
        let evidence=json!({"jobId":"g1","tokens":3000,"candidateId":"p",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":false}},
            "toolTimeline":[{"tool":"code_search","status":"completed"}]});
        let new=evolve(&[],&evidence);
        assert_eq!(new[0]["kind"],"tighten");
        let modified=apply(&new,&[rule()]);
        assert_eq!(modified[0]["guard"]["all"][1]["value"],2);
    }
    #[test]
    fn accepted_source_experience_tries_eliminating_prior_search_conditions(){
        let accepted=json!({"jobId":"g2","tokens":5000,"accepted":true,
            "evidenceVerified":true,"acceptedSourceHashes":{"src/main.rs":"a".repeat(64)},
            "evaluation":{"metrics":{"eligible":true}},
            "toolTimeline":[{"tool":"code_search","status":"completed"},
                {"tool":"workspace_replace_range","status":"completed",
                    "path":"src/main.rs",
                    "result":{"written":true,"postSha256":"a".repeat(64)},
                    "toolReceipt":{"verified":true}}]});
        let grammar=evolve(&[],&accepted);
        assert_eq!(grammar[0]["kind"],"prune");
        let modified=apply(&grammar,&[rule()]);
        assert_eq!(modified[0]["guard"]["metric"],"grounded");
        assert_eq!(modified[0]["allow"],json!(["workspace_replace_range"]));
    }
    #[test]
    fn unknown_grammar_cannot_gain_tools_or_grow_unbounded(){
        let bad=json!([{"kind":"shell_exec","metric":"completed:code_search"},
            {"kind":"switch_edit","from":"code_search","to":"shell_exec"}]);
        assert!(normalize(&bad).is_empty());
        let malformed=apply(&[json!({"kind":"tighten","metric":"completed:code_search",
            "value":10000})],&[rule()]);
        assert_eq!(malformed.len(),1);
    }
}
