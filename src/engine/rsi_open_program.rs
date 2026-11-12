//! Model-authored, source-bound graph of future intelligence experiments.
//!
//! The *model* writes nodes, mechanism descriptions and conditional edges.
//! An independently adopted design makes the graph eligible. A REBUILT child
//! chooses a node using actual paired outcomes, then executes its real source
//! edit. This interpreter itself belongs to the modifiable source graph.
//! No graph plan is itself evidence of a capability improvement.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn symbol(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 96
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
}
fn actual_source_role(g: &Value) -> bool {
    g["accepted"] == true && g["pairedRevalidated"] == true
        && g["candidateOrigin"] == "model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"] == true
        && g["systemDesign"]["source"] == "model_authored_system_design"
        && g["systemDesign"]["generatorCommit"] == g["generatorCommit"]
        && g["systemDesign"]["generationJobId"] == g["jobId"]
        && crate::engine::rsi_system::design_implemented(
            &g["systemDesign"],
            &g["acceptedSourceHashes"].as_object()
                .map(|m| m.keys().map(|p| json!(p)).collect::<Vec<_>>())
                .unwrap_or_default())
}
fn condition(g: &Value) -> &'static str {
    if g["accepted"] == true && g["pairedRevalidated"] == true {
        if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0) > 0 {
            "regressed"
        } else if g["evaluation"]["metrics"]["improvedHoldouts"]
            .as_u64().unwrap_or(0) > 0
            || g["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0) > 0 {
            "improved"
        } else { "adopted" }
    } else if g["evaluation"]["metrics"]["regressions"]
        .as_u64().unwrap_or(0) > 0 {
        "regressed"
    } else if g["candidateId"].as_str().is_some_and(|s| !s.is_empty()) {
        "rejected"
    } else { "no_candidate" }
}
fn guard(when: &str, previous: &Value) -> bool {
    match when {
        "always" => true,
        "adopted" => previous["accepted"] == true,
        "not_adopted" => previous["accepted"] != true,
        "improved" => condition(previous) == "improved",
        "regressed" => condition(previous) == "regressed",
        "rejected" => condition(previous) == "rejected",
        "no_candidate" => condition(previous) == "no_candidate",
        "neutral" => condition(previous) == "adopted",
        _ => false,
    }
}
fn known_guard(s: &str) -> bool {
    matches!(s, "always"|"adopted"|"not_adopted"|"improved"
        |"regressed"|"rejected"|"no_candidate"|"neutral")
}
fn node<'a>(program: &'a Value, id: &str) -> Option<&'a Value> {
    program["nodes"].as_array()?.iter().find(|n| n["id"] == id)
}
fn authored_valid(program: &Value) -> bool {
    if program["source"] != "model_authored_open_experiment_graph_v1" {return false;}
    let Some(nodes) = program["nodes"].as_array().filter(|n|
        !n.is_empty() && n.len() <= 96) else {return false;};
    let Some(edges) = program["edges"].as_array().filter(|e|
        e.len() <= 384) else {return false;};
    let mut ids=BTreeSet::new();
    for n in nodes {
        if !symbol(&n["id"])
            || !symbol(&n["role"])
            || n["domain"].as_str().is_none_or(|s|
                !crate::engine::rsi_evolve::DOMAINS.contains(&s))
            || n["path"].as_str().is_none_or(|s|
                !crate::engine::rsi::safe_path(s))
            || n["objective"].as_str().is_none_or(|s|
                !(20..=2000).contains(&s.chars().count()))
            || n["mechanism"].as_str().is_none_or(|s|
                !(20..=2000).contains(&s.chars().count()))
            || !ids.insert(n["id"].as_str().unwrap().to_owned()) {
            return false;
        }
    }
    if program["entry"].as_str().is_none_or(|id| !ids.contains(id)) {
        return false;
    }
    let mut seen=BTreeSet::new();
    for edge in edges {
        let (Some(from),Some(to),Some(when))=(
            edge["from"].as_str(),edge["to"].as_str(),edge["when"].as_str())
        else{return false};
        if !ids.contains(from) || !ids.contains(to) || !known_guard(when)
            || !seen.insert((from.to_owned(),to.to_owned(),when.to_owned())) {
            return false;
        }
    }
    let mut unsigned=program.clone();
    unsigned.as_object_mut().unwrap().remove("revision");
    program["revision"].as_str() == Some(hash(&unsigned).as_str())
}
/// Validate the model's proposed graph without dictating the mechanisms,
/// science-domain order, or branching topology. All runtime targets still
/// have to bind to real, current, source-hashed compiled components.
pub(crate) fn normalize(input: &Value) -> Result<Value, String> {
    if input.is_null() { return Ok(Value::Null); }
    let mut p=json!({
        "source":"model_authored_open_experiment_graph_v1",
        "entry":input["entry"],"nodes":input["nodes"],"edges":input["edges"],
        "qualityProven":false,"executableBy":"rebuilt_descendant"
    });
    p["revision"]=json!(hash(&p));
    if !authored_valid(&p) {
        return Err("모델이 작성한 개선 실험 그래프의 노드·분기·소스 계약 오류".into());
    }
    Ok(p)
}
fn frontier_target<'a>(frontier: &'a [Value], n: &Value) -> Option<&'a Value> {
    frontier.iter().find(|f|
        f["role"] == n["role"] && f["domain"] == n["domain"]
        && f["path"] == n["path"]
        && f["sourceSha256"].as_str().is_some_and(|h|
            h.len()==64 && h.bytes().all(|c|c.is_ascii_hexdigit())))
}
fn executed(g: &Value, revision: &Value) -> bool {
    g["openProgramExecuted"] == true
        && g["openProgramPlan"]["source"] == "compiled_descendant_open_experiment_graph_v1"
        && g["openProgramPlan"]["program"]["revision"] == *revision
        && g["openProgramPlan"]["generatorCommit"] == g["generatorCommit"]
        && g["openProgramPlan"]["target"]["path"].as_str().is_some_and(|p|
            crate::engine::rsi::safe_path(p))
}
fn score(history: &[Value], program: &Value, n: &Value) -> f64 {
    let mut trials=0_f64;
    let mut successes=0_f64;
    let mut gains=0_f64;
    let mut regressions=0_f64;
    for g in history {
        if g["openProgramPlan"]["program"]["revision"]!=program["revision"]
            || g["openProgramPlan"]["nodeId"]!=n["id"]
            || !executed(g,&program["revision"]) { continue; }
        trials+=1.0;
        if actual_source_role(g) { successes+=1.0; }
        if g["pairedRevalidated"]==true {
            gains+=g["evaluation"]["metrics"]["improvedHoldouts"]
                .as_u64().unwrap_or(0).min(10) as f64;
            regressions+=g["evaluation"]["metrics"]["regressions"]
                .as_u64().unwrap_or(0).min(10) as f64;
        }
    }
    // This compiled *learner* can itself be rewritten by a successor. Its
    // measurements never include hidden holdout identities or answers.
    (successes+1.0)/(trials+2.0)
        + 0.35*(gains-regressions)/(gains+regressions+2.0)
        + 0.20/(trials+1.0).sqrt()
}
/// The parent may invent a NEW experiment graph or execute the current one.
/// A child reads actual failed attempts and measured transitions; editing
/// this module replaces the selector without changing the independent oracle.
pub(crate) fn synthesize(history: &[Value], frontier: &[Value], commit: &str)
    -> Value
{
    let Some(parent)=history.iter().rev().find(|g|
        actual_source_role(g) && g["acceptedCommit"]==commit)
    else {return Value::Null};
    let fresh=&parent["systemDesign"]["openProgram"];
    let (program, origin, is_new)=if authored_valid(fresh) {
        (fresh, parent["id"].clone(),true)
    }else{
        let inherited=&parent["openProgramPlan"];
        if !executed(parent,&inherited["program"]["revision"])
            || !authored_valid(&inherited["program"]) {
            return Value::Null;
        }
        (&inherited["program"],inherited["originGenerationId"].clone(),false)
    };
    // Failures and adopted children after the parent are experimental data,
    // not executable plans supplied by an old coordinator.
    let recent_attempt=history.iter().rev().find(|g|
        g["generatorCommit"]==commit
        && executed(g,&program["revision"]));
    let previous=recent_attempt.or_else(||(!is_new).then_some(parent));
    let previous_id=previous.map(|g|g["openProgramPlan"]["nodeId"].clone())
        .unwrap_or(Value::Null);
    let mut candidates=Vec::<(f64,Value,Value)>::new();
    let mut enqueue=|n:&Value,priority:f64|{
        if let Some(f)=frontier_target(frontier,n) {
            candidates.push((score(history,program,n)+priority,n.clone(),f.clone()));
        }
    };
    if let Some(g)=previous {
        for edge in program["edges"].as_array().into_iter().flatten() {
            if edge["from"]==previous_id &&
                edge["when"].as_str().is_some_and(|when|guard(when,g)) {
                if let Some(n)=node(program,edge["to"].as_str().unwrap_or("")) {
                    enqueue(n,0.12);
                }
            }
        }
    }
    if candidates.is_empty() {
        if let Some(n)=node(program,program["entry"].as_str().unwrap_or("")) {
            enqueue(n,0.0);
        }
    }
    // An edited component may no longer exist. Allow the compiled child to
    // choose another source-grounded, model-authored graph node rather than
    // inventing an untrusted path or becoming permanently stuck.
    if candidates.is_empty() {
        for n in program["nodes"].as_array().into_iter().flatten(){
            enqueue(n,0.0);
        }
    }
    candidates.sort_by(|a,b|b.0.total_cmp(&a.0)
        .then_with(||a.1["id"].as_str().cmp(&b.1["id"].as_str())));
    let Some((priority,selected,target))=candidates.into_iter().next()
        else{return Value::Null};
    let stage=json!({
        "source":"model_authored_open_program_stage",
        "role":target["role"],"domain":target["domain"],
        "path":target["path"],"sourceSha256":target["sourceSha256"],
        "objective":selected["objective"],
        "mechanism":selected["mechanism"],
        "sourceDesignId":parent["systemDesign"]["id"],
        "predecessorCommit":parent["acceptedCommit"],
        "programmeOriginDesignId":origin,
        "programmeNodeId":selected["id"],
        "learningPolicy":"avoid_failed",
        "toolPolicy":"allow_new_modules",
        "executionPolicy":"grounded_edit",
        "qualityProven":false
    });
    let mut plan=json!({
        "source":"compiled_descendant_open_experiment_graph_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "originGenerationId":origin,
        "parentGenerationId":parent["id"],"parentAcceptedCommit":parent["acceptedCommit"],
        "newlyAuthored":is_new,"previousNodeId":previous_id,
        "program":program,"programRevision":program["revision"],
        "nodeId":selected["id"],"node":selected,"target":target,"stage":stage,
        "estimatedPriority":priority,
        "qualityProven":false,"decisionOwner":"rebuilt_descendant_binary"
    });
    plan["revision"]=json!(hash(&plan));
    plan
}
/// Independent structural validation checks authoring and exact source
/// rebinding but does not freeze an older ranker's numerical formula.
pub(crate) fn validate(
    plan: &Value, history: &[Value], frontier: &[Value], commit: &str,
) -> bool {
    if plan.is_null(){return true;}
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut() else{return false};
    obj.remove("revision");
    if plan["source"]!="compiled_descendant_open_experiment_graph_v1"
        || plan["revision"].as_str()!=Some(hash(&unsigned).as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["decisionOwner"]!="rebuilt_descendant_binary"
        || plan["qualityProven"]!=false
        || !authored_valid(&plan["program"])
        || plan["programRevision"]!=plan["program"]["revision"]
        || !frontier.iter().any(|f|f==&plan["target"])
        || plan["target"]["role"]!=plan["node"]["role"]
        || plan["target"]["domain"]!=plan["node"]["domain"]
        || plan["target"]["path"]!=plan["node"]["path"]
        || node(&plan["program"],plan["nodeId"].as_str().unwrap_or(""))
            !=Some(&plan["node"])
        || plan["stage"]["source"]!="model_authored_open_program_stage"
        || plan["stage"]["path"]!=plan["target"]["path"]
        || plan["stage"]["sourceSha256"]!=plan["target"]["sourceSha256"]
        || plan["stage"]["programmeNodeId"]!=plan["nodeId"]{
        return false;
    }
    let Some(parent)=history.iter().rev().find(|g|
        actual_source_role(g) && g["acceptedCommit"]==commit)
    else{return false};
    if plan["parentGenerationId"]!=parent["id"]
        || plan["parentAcceptedCommit"]!=parent["acceptedCommit"] {
        return false;
    }
    if plan["newlyAuthored"]==true {
        parent["systemDesign"]["openProgram"]==plan["program"]
            && plan["originGenerationId"]==parent["id"]
    } else {
        let past=&parent["openProgramPlan"];
        executed(parent,&past["program"]["revision"])
            && past["program"]==plan["program"]
            && plan["originGenerationId"]==past["originGenerationId"]
    }
}
pub(crate) fn instruction(plan: &Value) -> String {
    if !plan.is_object(){return String::new();}
    format!(
        "[COMPILED OPEN EXPERIMENT GRAPH] program={} node={} learnedRank={} \
        role={} domain={} currentSource={} sourceSha256={} mechanism={} \
        objective={}. The accepted MODEL authored the graph, not the frozen \
        coordinator. Observe independent task outcomes and transition along \
        authored conditional edges. Author an ENTIRELY NEW openProgram in \
        rsi_system_plan if the graph is no longer useful. A new method may \
        invent new intelligence roles and rewrite this very graph interpreter \
        (src/engine/rsi_open_program.rs). The current instruction is a SOURCE \
        EDIT TARGET, NOT evidence of quality or a claim of RSI completion.",
        plan["programRevision"],plan["nodeId"],plan["estimatedPriority"],
        plan["target"]["role"],plan["target"]["domain"],
        plan["target"]["path"],plan["target"]["sourceSha256"],
        plan["node"]["mechanism"],plan["node"]["objective"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unbound_and_bad_branch_targets() {
        let raw=json!({"entry":"a","nodes":[{"id":"a","role":"learning",
            "domain":"learning","path":"src/engine/learning.rs",
            "objective":"Learn a genuinely new memory update algorithm.",
            "mechanism":"Update real model and memory source using evaluated task feedback."}],
            "edges":[{"from":"a","to":"missing","when":"regressed"}]});
        assert!(normalize(&raw).is_err());
    }
    #[test]
    fn false_adoption_does_not_create_executable_graph() {
        let g=json!({"accepted":false,"systemDesign":{"openProgram":{
            "source":"model_authored_open_experiment_graph_v1"}}});
        assert!(synthesize(&[g],&[],"abc").is_null());
    }
    #[test]
    fn graph_revisions_bind_to_content() {
        let raw=json!({"entry":"a","nodes":[{"id":"a","role":"learning",
            "domain":"learning","path":"src/engine/learning.rs",
            "objective":"Replace the model memory update mechanism in the next generation.",
            "mechanism":"Compare independent outcomes and train a new source bound learner."}],
            "edges":[{"from":"a","to":"a","when":"not_adopted"}]});
        let p=normalize(&raw).unwrap();
        assert!(authored_valid(&p));
        let mut altered=p.clone();
        altered["edges"]=json!([]);
        assert!(!authored_valid(&altered));
    }
}
