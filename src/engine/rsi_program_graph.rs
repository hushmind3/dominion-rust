//! Open-ended, model-authored recursive improvement programs.
//!
//! A frozen oracle adopts source revisions; the accepted model design may
//! independently author an arbitrary directed graph of NEXT intelligence
//! experiments. Every node can target any current or newly born subsystem,
//! including this graph compiler. A rebuilt descendant runs the graph, and
//! a later adopted design may replace its graph and learn new transitions.
//! Graph topology, mechanisms, roles and editing objectives are not fixed
//! in the resident supervisor. Validation guards source and lineage facts.
use crate::engine::{rsi,rsi_mutual_program,rsi_component_birth,rsi_evolve,rsi_principle_learning};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn sha(value:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn label(value:&Value)->bool {
    value.as_str().is_some_and(|s|!s.is_empty()&&s.len()<=64
        && s.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_'||b==b'-'))
}
fn condition(s:&str)->bool {
    matches!(s,"adopted"|"improved"|"neutral"|"regressed")
}
fn outcome(g:&Value)->&'static str {
    if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0 {
        "regressed"
    }else if g["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0)>0
        || g["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0 {
        "improved"
    }else{"neutral"}
}
pub(crate) fn valid_authored(program:&Value)->bool {
    if program["source"]!="model_authored_open_improvement_program_v1" {
        return false;
    }
    let mut unsigned=program.clone();
    let Some(body)=unsigned.as_object_mut() else{return false};
    body.remove("revision");
    if program["revision"].as_str()!=Some(sha(&unsigned).as_str()) {
        return false;
    }
    let Some(nodes)=program["nodes"].as_array().filter(|v|!v.is_empty()&&v.len()<=24)
        else{return false};
    let Some(edges)=program["edges"].as_array().filter(|v|v.len()<=64)
        else{return false};
    let mut ids=BTreeSet::new();
    for node in nodes {
        let Some(role)=node["role"].as_str().filter(|r|
            rsi_component_birth::valid_role(r)) else{return false};
        // Model-authored transferable abstraction keys are optional and
        // never earn quality credit without the independent task receipt.
        if !node["principleKeys"].is_null() {
            let Some(keys)=node["principleKeys"].as_array()
                .filter(|keys|!keys.is_empty()&&keys.len()<=8) else{return false};
            let mut unique=BTreeSet::new();
            if !keys.iter().all(|k|k.as_str().is_some_and(|key|
                (3..=48).contains(&key.len())
                && key.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_')
                && unique.insert(key.to_owned()))) {return false;}
        }
        if !label(&node["id"]) || !ids.insert(node["id"].as_str().unwrap().to_string())
            || node["domain"].as_str().is_none_or(|d|!rsi_evolve::DOMAINS.contains(&d))
            || node["path"].as_str().is_none_or(|p|!rsi::safe_path(p))
            || node["objective"].as_str().is_none_or(|s|!(24..=1600).contains(&s.chars().count()))
            || node["mechanism"].as_str().is_none_or(|s|!(24..=1600).contains(&s.chars().count()))
            || role=="system" {
            return false;
        }
    }
    if program["entry"].as_str().is_none_or(|id|!ids.contains(id)){return false;}
    let mut unique=BTreeSet::new();
    for edge in edges {
        let (Some(from),Some(to),Some(when))=(
            edge["from"].as_str(),edge["to"].as_str(),edge["when"].as_str())
        else{return false};
        if !ids.contains(from)||!ids.contains(to)||!condition(when)
            || !unique.insert((from.to_string(),to.to_string(),when.to_string())) {
            return false;
        }
    }
    true
}
/// Called when a *model-authored* system design is recorded. It is only
/// a hypothesis until that generation really changes source and its
/// independent paired experiment adopts it.
pub(crate) fn normalize(input:&Value)->Result<Value,String> {
    if input.is_null(){return Ok(Value::Null);}
    let mut program=json!({
        "source":"model_authored_open_improvement_program_v1",
        "entry":input["entry"],"nodes":input["nodes"],
        "edges":input["edges"],"qualityProven":false,
        "executableBy":"recompiled_descendant_controller",
    });
    program["revision"]=json!(sha(&program));
    if !valid_authored(&program) {
        return Err("열린 재귀 개선 프로그램의 노드·전이·소스·메커니즘이 유효하지 않습니다.".into());
    }
    Ok(program)
}
fn path_target<'a>(frontier:&'a [Value],node:&Value,allowed:&[&str])->Option<&'a Value> {
    frontier.iter().find(|row|
        row["role"]==node["role"]
        && row["domain"]==node["domain"]
        && row["path"]==node["path"]
        && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))
        && row["sourceSha256"].as_str().is_some_and(|s|
            s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())))
}
fn executed(g:&Value)->bool {
    let plan=&g["openImprovementProgram"];
    let Some(path)=plan["target"]["path"].as_str() else{return false};
    let source_verified=g["changedSourcePaths"].as_array().is_some_and(|paths|
        paths.iter().any(|row|row.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|h|
            h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()));
    source_verified && g["metaPolicy"]["target"]==plan["target"]
        && plan["source"]=="compiled_descendant_open_improvement_program_v1"
        && g["metaPolicy"]["selectionMode"]=="authored_open_improvement_program"
        && g["metaPolicy"]["openImprovementRevision"]==plan["revision"]
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["compiledPlanExecuted"]["openImprovementRevision"]==plan["revision"]
        && g["compiledPlanExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
}
fn node<'a>(program:&'a Value,id:&str)->Option<&'a Value> {
    program["nodes"].as_array()?.iter().find(|n|n["id"]==id)
}
fn edge_allowed(edge:&Value,previous:&Value)->bool {
    edge["when"]=="adopted" || edge["when"]==outcome(previous)
}
/// The mutable compiled descendant chooses which graph node to explore.
/// Fixed control-flow is *not* required: accepted model designs can invent
/// arbitrary named principles, cycles, branches and meta-program edits.
pub(crate) fn synthesize(history:&[Value],frontier:&[Value],
    allowed:&[&str],commit:&str)->Value {
    let Some(parent)=history.iter().rev().find(|g|
        rsi_mutual_program::adopted_source(g) && g["acceptedCommit"]==commit)
        else{return Value::Null};
    let new_program=&parent["systemDesign"]["openProgram"];
    let authored=valid_authored(new_program);
    let previous=&parent["openImprovementProgram"];
    let inherited=previous["source"]=="compiled_descendant_open_improvement_program_v1"
        && executed(parent) && valid_authored(&previous["program"]);
    let (program,origin,previous_id,from_authored)=if authored {
        (new_program,parent["id"].clone(),Value::Null,true)
    }else if inherited {
        (&previous["program"],previous["originGenerationId"].clone(),
            previous["nodeId"].clone(),false)
    }else{return Value::Null};
    // Learn from independent held-out task cases AND from what a later,
    // differently compiled descendant did with the accepted source.
    // This ranking rule is inside this editable module, not a frozen
    // coordinator policy; its features and weights can be rewritten.
    let learned=rsi_principle_learning::study(history);
    let mut choices=Vec::new();
    if from_authored {
        if let Some(row)=node(program,program["entry"].as_str().unwrap_or("")) {
            choices.push((0_i64,row.clone()));
        }
    }else if let Some(id)=previous_id.as_str() {
        for edge in program["edges"].as_array().into_iter().flatten()
            .filter(|e|e["from"]==id && edge_allowed(e,parent)) {
            if let Some(row)=node(program,edge["to"].as_str().unwrap_or("")) {
                // The dynamic grammar is model-authored. The descendant's
                // ranker can itself be changed by a future source patch.
                let priority=if edge["when"]==outcome(parent){2_i64}else{1_i64};
                choices.push((priority,row.clone()));
            }
        }
    }
    // Program-authored outcome edges constrain what is permitted.
    // Independently MEASURED effectiveness, not alphabetical node IDs,
    // then chooses among the valid next experiments.
    let mut ranked=choices.into_iter().filter_map(|(edge_priority,row)| {
        let target=path_target(frontier,&row,allowed)?;
        let forecast=rsi_principle_learning::estimate(&learned,&row);
        let measured_score=forecast["estimatedPriority"].as_f64().unwrap_or(0.0);
        // The outcome-specific edge is a mild tie-breaker. A real track
        // record can supersede its original static ordering.
        let score=measured_score+if edge_priority==2{0.08}else{0.0};
        Some((score,row,target.clone(),forecast))
    }).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b.0.total_cmp(&a.0)
        .then_with(||a.1["id"].as_str().cmp(&b.1["id"].as_str())));
    let Some((chosen_score,selected,target,selection_evidence))=
        ranked.into_iter().next() else{return Value::Null};
    let node_id=selected["id"].clone();
    let mut plan=json!({
        "source":"compiled_descendant_open_improvement_program_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "program":program,"programSha256":program["revision"],
        "originGenerationId":origin,
        "originatedFromNewAdoptedDesign":from_authored,
        "previousNodeId":previous_id,
        "parentGenerationId":parent["id"],
        "parentAcceptedCommit":parent["acceptedCommit"],
        "nodeId":node_id,"node":selected,"target":target,
        "principleLearningSha256":learned["sha256"],
        "principleSelectionEvidence":selection_evidence,
        "principleSelectionPriority":chosen_score,
        "currentSourceRebound":true,
        "evaluationOwner":"independent_supervisor",
        "qualityProven":false
    });
    plan["revision"]=json!(sha(&plan));
    plan
}
/// Structural supervisor validation, deliberately not re-running the old
/// supervisor's ranking policy. New descendants may rewrite that algorithm.
pub(crate) fn valid(plan:&Value,history:&[Value],frontier:&[Value],
    commit:&str)->bool {
    if plan.is_null(){return true;}
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut() else{return false};
    obj.remove("revision");
    let Some(parent)=history.iter().rev().find(|g|
        rsi_mutual_program::adopted_source(g) && g["acceptedCommit"]==commit)
        else{return false};
    if plan["source"]!="compiled_descendant_open_improvement_program_v1"
        || plan["revision"].as_str()!=Some(sha(&unsigned).as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["parentGenerationId"]!=parent["id"]
        || plan["parentAcceptedCommit"]!=parent["acceptedCommit"]
        || plan["currentSourceRebound"]!=true
        || plan["evaluationOwner"]!="independent_supervisor"
        || plan["qualityProven"]!=false
        || plan["principleLearningSha256"]
            !=rsi_principle_learning::study(history)["sha256"]
        || !valid_authored(&plan["program"])
        || plan["programSha256"]!=plan["program"]["revision"]
        || !frontier.iter().any(|f|f==&plan["target"])
        || plan["target"]["path"]!=plan["node"]["path"]
        || plan["target"]["role"]!=plan["node"]["role"]
        || plan["target"]["domain"]!=plan["node"]["domain"] {
        return false;
    }
    let Some(chosen)=plan["nodeId"].as_str() else{return false};
    if node(&plan["program"],chosen)!=Some(&plan["node"]){return false;}
    if plan["principleSelectionEvidence"]
        !=rsi_principle_learning::estimate(
            &rsi_principle_learning::study(history),&plan["node"]) {
        return false;
    }
    if plan["originatedFromNewAdoptedDesign"]==true {
        return parent["systemDesign"]["openProgram"]==plan["program"]
            && plan["originGenerationId"]==parent["id"]
            && plan["previousNodeId"].is_null()
            && plan["nodeId"]==plan["program"]["entry"];
    }
    let prior=&parent["openImprovementProgram"];
    prior["source"]=="compiled_descendant_open_improvement_program_v1"
        && executed(parent)
        && prior["program"]==plan["program"]
        && plan["originGenerationId"]==prior["originGenerationId"]
        && plan["previousNodeId"]==prior["nodeId"]
        && plan["program"]["edges"].as_array().is_some_and(|edges|
            edges.iter().any(|edge|edge["from"]==prior["nodeId"]
                && edge["to"]==plan["nodeId"] && edge_allowed(edge,parent)))
}
pub(crate) fn instruction(plan:&Value)->String {
    if !plan.is_object(){return String::new();}
    let node=&plan["node"];
    format!(
        "OPEN SELF-IMPROVEMENT PROGRAM revision={} authored={} selectedNode={} \
         role={} source={} sha={}. Measured principle estimate={} \
         from independent ledger={}. Hypothesis and transferable mechanism: {}. \
         Executable objective: {}. This is an experiment instruction, not a \
         demonstrated gain. Edit actual source, submit rsi_system_plan with \
         real changed targetFiles; you may author a COMPLETELY NEW openProgram \
         (nodes, entry, conditional edges, novel roles, cyclic dependencies, \
         optional principleKeys reusable across different model-authored ideas, \
         meta-program rewrite targets) rather than extending an old fixed phase \
         list. Only independently adopted sources create runnable successors.",
        plan["revision"],plan["originGenerationId"],plan["nodeId"],
        node["role"],node["path"],plan["target"]["sourceSha256"],
        plan["principleSelectionEvidence"],plan["principleLearningSha256"],
        node["mechanism"],node["objective"])
}
/// A proven rewrite is an adopted, different model-authored program that
/// was subsequently EXECUTED by a genuinely rebuilt descendant. The graph
/// is not credited for a self-reported proposal or a successful tool call.
// Replayable proof remains a distinct immutable responsibility.
#[path = "rsi/proof/open_handoff.rs"]
mod proof_handoff;
pub(crate) use proof_handoff::handoff;

// Heavy regression fixtures live outside the production source inventory.
#[cfg(test)]
include!("../../tests/support/engine/rsi_program_graph.tests.rs");
