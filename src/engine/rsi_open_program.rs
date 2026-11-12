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
fn known_guard(s: &str) -> bool {
    matches!(s, "always"|"adopted"|"not_adopted"|"improved"
        |"regressed"|"rejected"|"no_candidate"|"neutral")
}
fn valid_guard(when: &Value, depth: usize) -> bool {
    if depth > 8 {return false;}
    if let Some(name)=when.as_str() {return known_guard(name);}
    let Some(map)=when.as_object() else{return false};
    if map.len()!=1 && !(map.len()==3
        && map.contains_key("metric") && map.contains_key("op")
        && map.contains_key("value")) {return false;}
    if let Some(all)=map.get("all").and_then(Value::as_array) {
        return !all.is_empty() && all.len()<=16
            && all.iter().all(|sub|valid_guard(sub,depth+1));
    }
    if let Some(any)=map.get("any").and_then(Value::as_array) {
        return !any.is_empty() && any.len()<=16
            && any.iter().all(|sub|valid_guard(sub,depth+1));
    }
    if let Some(not)=map.get("not") {
        return valid_guard(not,depth+1);
    }
    let Some(metric)=when["metric"].as_str() else{return false};
    let Some(op)=when["op"].as_str() else{return false};
    metric.len()<=80 && !metric.is_empty()
        && metric.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_')
        && matches!(op,">"|">="|"<"|"<="|"=="|"!=")
        && when["value"].as_f64().is_some_and(|v|v.is_finite())
}
fn guard(when: &Value, previous: &Value) -> bool {
    if let Some(s)=when.as_str() {
        return match s {
            "always"=>true,
            "adopted"=>previous["accepted"]==true,
            "not_adopted"=>previous["accepted"]!=true,
            "improved"=>condition(previous)=="improved",
            "regressed"=>condition(previous)=="regressed",
            "rejected"=>condition(previous)=="rejected",
            "no_candidate"=>condition(previous)=="no_candidate",
            "neutral"=>condition(previous)=="adopted",
            _=>false
        };
    }
    if let Some(all)=when["all"].as_array() {
        return all.iter().all(|sub|guard(sub,previous));
    }
    if let Some(any)=when["any"].as_array() {
        return any.iter().any(|sub|guard(sub,previous));
    }
    if let Some(not)=when.get("not") {
        return !guard(not,previous);
    }
    // Numeric predicates can refer only to aggregate metrics supplied by
    // independent evaluation. Raw holdout rows, IDs or answers are not
    // traversable by this interpreter.
    let Some(name)=when["metric"].as_str() else{return false};
    let Some(actual)=previous["evaluation"]["metrics"][name].as_f64()
        else{return false};
    let Some(expected)=when["value"].as_f64() else{return false};
    match when["op"].as_str() {
        Some(">")=>actual>expected,
        Some(">=")=>actual>=expected,
        Some("<")=>actual<expected,
        Some("<=")=>actual<=expected,
        Some("==")=>(actual-expected).abs()<f64::EPSILON,
        Some("!=")=>(actual-expected).abs()>=f64::EPSILON,
        _=>false
    }
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
        let (Some(from),Some(to))=(
            edge["from"].as_str(),edge["to"].as_str())
        else{return false};
        let when=&edge["when"];
        if !ids.contains(from) || !ids.contains(to) || !valid_guard(when,0)
            || !seen.insert((from.to_owned(),to.to_owned(),hash(when))) {
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
fn carried(g: &Value, revision: &Value) -> bool {
    if !actual_source_role(g) || !executed(g,revision) {return false;}
    let Some(path)=g["openProgramPlan"]["target"]["path"].as_str()
        else{return false};
    g["acceptedSourceHashes"][path].as_str().is_some_and(|h|
        h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
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
        // A graph cannot earn improvement credit from an unrelated
        // independently adopted patch: its selected source must be in the
        // actual accepted candidate's source-hash ledger.
        let target_path=n["path"].as_str().unwrap_or("");
        if actual_source_role(g)
            && g["acceptedSourceHashes"][target_path].as_str().is_some_and(|h|
                h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit())) {
            successes+=1.0;
        }
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
        if !carried(parent,&inherited["program"]["revision"])
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
    fn enqueue(
        candidates:&mut Vec<(f64,Value,Value)>,history:&[Value],
        program:&Value,frontier:&[Value],n:&Value,priority:f64,
    ) {
        if let Some(f)=frontier_target(frontier,n) {
            candidates.push((score(history,program,n)+priority,n.clone(),f.clone()));
        }
    }
    if let Some(g)=previous {
        for edge in program["edges"].as_array().into_iter().flatten() {
            if edge["from"]==previous_id && guard(&edge["when"],g) {
                if let Some(n)=node(program,edge["to"].as_str().unwrap_or("")) {
                    enqueue(&mut candidates,history,program,frontier,n,0.12);
                }
            }
        }
    }
    if candidates.is_empty() {
        if let Some(n)=node(program,program["entry"].as_str().unwrap_or("")) {
            enqueue(&mut candidates,history,program,frontier,n,0.0);
        }
    }
    // An edited component may no longer exist. Allow the compiled child to
    // choose another source-grounded, model-authored graph node rather than
    // inventing an untrusted path or becoming permanently stuck.
    if candidates.is_empty() {
        for n in program["nodes"].as_array().into_iter().flatten(){
            enqueue(&mut candidates,history,program,frontier,n,0.0);
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
        carried(parent,&past["program"]["revision"])
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
    fn model_authored_numeric_guards_use_only_aggregate_metrics() {
        let expression=json!({"all":[
            {"metric":"improvedHoldouts","op":">=","value":2},
            {"not":"regressed"}
        ]});
        assert!(valid_guard(&expression,0));
        let observed=json!({"evaluation":{"metrics":{
            "improvedHoldouts":3,"regressions":0}},"accepted":true,
            "pairedRevalidated":true});
        assert!(guard(&expression,&observed));
        let no_eval=json!({"evaluation":{"metrics":{
            "measurements":[{"id":"hidden","candidatePass":true}]}}});
        assert!(!guard(&expression,&no_eval));
    }
    #[test]
    fn failed_real_descendant_selects_model_authored_recovery_branch() {
        let graph=normalize(&json!({
            "entry":"train","nodes":[
                {"id":"train","role":"learning","domain":"learning",
                 "path":"src/engine/learning.rs",
                 "objective":"Rewrite the learning source to improve repeated task failures.",
                 "mechanism":"Use aggregate feedback to learn a new task improvement method."},
                {"id":"repair","role":"memory","domain":"memory",
                 "path":"src/engine/memory.rs",
                 "objective":"Recover the failing memory subsystem from observed failures.",
                 "mechanism":"Transfer learning errors into the memory revision algorithm."}
            ],"edges":[{"from":"train","to":"repair","when":"not_adopted"}]
        })).unwrap();
        let parent_commit="1".repeat(40);
        let parent=json!({
            "id":"g1","jobId":"j1","accepted":true,
            "generatorCommit":"0".repeat(40),
            "acceptedCommit":parent_commit,
            "pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true,"improvedHoldouts":1}},
            "systemDesign":{
                "id":"d1","source":"model_authored_system_design",
                "generatorCommit":"0".repeat(40),
                "generationJobId":"j1","component":"learning",
                "targetFiles":["src/engine/learning.rs"],
                "openProgram":graph
            },
            "acceptedSourceHashes":{"src/engine/learning.rs":"a".repeat(64)}
        });
        let frontier=vec![
            json!({"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceSha256":"a".repeat(64)}),
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceSha256":"b".repeat(64)})
        ];
        let first=synthesize(&[parent.clone()],&frontier,
            parent_commit.as_str());
        assert_eq!(first["nodeId"],"train");
        assert!(validate(&first,&[parent.clone()],&frontier,
            parent_commit.as_str()));
        let rejected=json!({"id":"g2","jobId":"j2",
            "generatorCommit":parent_commit,
            "openProgramPlan":first,"openProgramExecuted":true,
            "candidateId":"rejected-candidate","accepted":false});
        let history=vec![parent,rejected];
        let next=synthesize(&history,&frontier,
            history[1]["generatorCommit"].as_str().unwrap());
        assert_eq!(next["nodeId"],"repair");
        assert_eq!(next["target"]["role"],"memory");
        assert!(validate(&next,&history,&frontier,
            history[1]["generatorCommit"].as_str().unwrap()));
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
