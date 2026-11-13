//! Typed, model-authored improvement-of-improvement protocol.
//!
//! This is compiled by a descendant, not interpreted as an instruction from
//! the resident API. Planning is pure; only the independent supervisor may
//! adopt candidate source or change the evaluation profile. A protocol may
//! target the selector, experiment designer, learner, harness, or another
//! source-backed intelligence subsystem, including this compiler.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

const AUTHOR: &str = "model_authored_recursive_evolution_protocol_v1";
const COMPILED: &str = "compiled_descendant_recursive_evolution_protocol_v1";

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Operator {
    id:String,
    role:String,
    domain:String,
    path:String,
    objective:String,
    mechanism:String,
    experiment:String,
}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Transition {
    from:String,
    to:String,
    when:String,
    // The accepted model can evolve thresholded experiment transitions.
    #[serde(default,skip_serializing_if="Option::is_none")]
    guard:Option<super::meta_transition_policy::Guard>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Protocol {
    source:String,
    entry:String,
    operators:Vec<Operator>,
    transitions:Vec<Transition>,
    quality_proven:bool,
    revision:String,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Compiled {
    source:String,
    generator_commit:String,
    history_count:usize,
    parent_generation_id:String,
    parent_accepted_commit:String,
    origin_generation_id:String,
    previous_operator_id:Option<String>,
    // A rejected yet independently measured attempt can change the NEXT
    // experiment without claiming an accepted source revision.
    #[serde(default,skip_serializing_if="Option::is_none")]
    feedback_generation_id:Option<String>,
    operator_id:String,
    operator:Operator,
    target:Value,
    protocol:Protocol,
    evaluation_owner:String,
    quality_proven:bool,
    revision:String,
}

fn digest<T:Serialize>(v:&T)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn identity(id:&str)->bool {
    (1..=64).contains(&id.len())
        && id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
fn sha(s:&str,n:usize)->bool {
    s.len()==n && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn experiments(s:&str)->bool {
    matches!(s,"paired_source"|"isolated_source"|"factorial_source"
        |"method_control"|"cross_component"|"meta_policy_rewrite")
}
fn outcomes(s:&str)->bool {
    matches!(s,"adopted"|"quality_gain"|"regressed"|"rejected"
        |"inconclusive"|"meta_gain"|"method_gain"|"measured")
}
fn valid_protocol(p:&Protocol)->bool {
    if p.source!=AUTHOR || p.quality_proven
        || !(1..=32).contains(&p.operators.len()) || p.transitions.len()>96
        || !sha(&p.revision,64) {return false;}
    let mut unsigned=serde_json::to_value(p).unwrap_or(Value::Null);
    if let Some(m)=unsigned.as_object_mut(){m.remove("revision");}
    if digest(&unsigned)!=p.revision{return false;}
    let mut ids=BTreeSet::new();
    for op in &p.operators {
        if !identity(&op.id) || !ids.insert(op.id.as_str())
            || !crate::engine::rsi_component_birth::valid_role(&op.role)
            || !crate::engine::rsi_evolve::DOMAINS.contains(&op.domain.as_str())
            || !crate::engine::rsi::safe_path(&op.path)
            || !(24..=1800).contains(&op.objective.chars().count())
            || !(24..=1800).contains(&op.mechanism.chars().count())
            || !experiments(&op.experiment) {return false;}
    }
    if !ids.contains(p.entry.as_str()){return false;}
    let mut transitions=BTreeSet::new();
    for edge in &p.transitions {
        if !ids.contains(edge.from.as_str()) || !ids.contains(edge.to.as_str())
            || !outcomes(&edge.when)
            || (edge.when=="measured" && edge.guard.is_none())
            || edge.guard.as_ref().is_some_and(|guard|
                !super::meta_transition_policy::valid(guard))
            || !transitions.insert((&edge.from,&edge.to,&edge.when)) {return false;}
    }
    true
}

/// A proposal is a program, not a success receipt. No AppState is exposed to
/// the language grammar or its validation. The SHA seals executable policy.
pub(crate) fn normalize(input:&Value)->Result<Value,String>{
    if input.is_null(){return Ok(Value::Null);}
    let operators:Vec<Operator>=serde_json::from_value(input["operators"].clone())
        .map_err(|e|format!("개선 연산자 형식 오류: {e}"))?;
    let transitions:Vec<Transition>=serde_json::from_value(input["transitions"].clone())
        .map_err(|e|format!("개선 전이 형식 오류: {e}"))?;
    let mut p=Protocol{source:AUTHOR.into(),
        entry:input["entry"].as_str().ok_or("진화 프로토콜 시작점 없음")?.into(),
        operators,transitions,quality_proven:false,revision:String::new()};
    let mut value=serde_json::to_value(&p).map_err(|e|e.to_string())?;
    value.as_object_mut().ok_or("프로토콜 객체가 아닙니다")?.remove("revision");
    p.revision=digest(&value);
    if !valid_protocol(&p){return Err("진화 연산자·전이·소스 경로가 유효하지 않습니다.".into());}
    serde_json::to_value(p).map_err(|e|e.to_string())
}
pub(crate) fn valid_authored(value:&Value)->bool {
    serde_json::from_value::<Protocol>(value.clone())
        .is_ok_and(|p|valid_protocol(&p))
}
fn parent<'a>(history:&'a [Value],commit:&str)->Option<&'a Value>{
    history.iter().rev().find(|g|
        crate::engine::rsi_mutual_program::adopted_source(g)
            && g["acceptedCommit"]==commit)
}
fn outcome(g:&Value)->&'static str {
    if g["methodAblation"]["qualityProven"]==true
        && g["methodAblation"]["methodAssociationObserved"]==true {"method_gain"}
    else if g["evaluation"]["metrics"]["metaHoldoutGains"]
        .as_u64().unwrap_or(0)>0 {"meta_gain"}
    else if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0 {"regressed"}
    else if g["accepted"]==true
        && g["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0)>0 {"quality_gain"}
    else if g["accepted"]==true {"adopted"}
    else if g["pairedMeasured"]==true {"rejected"}
    else {"inconclusive"}
}
/// A typed metric predicate can drive a transition with no hard-coded
/// A/B stage or named qualitative success state. The 'measured' trigger
/// requires a real paired episode and a model-authored, validated guard.
fn transition_matches(edge:&Transition,episode:&Value)->bool {
    let relevant=if edge.when=="measured" {
        edge.guard.is_some()
            && (episode["pairedRevalidated"]==true || episode["pairedMeasured"]==true)
    }else{edge.when==outcome(episode)};
    relevant && edge.guard.as_ref().is_none_or(|guard|
        super::meta_transition_policy::evaluate(guard,episode))
}
/// This is an episode FEEDBACK selector, not an adoption selector. It may
/// inspect only the attempted operator under the same compiled generator and
/// identical adopted protocol. The adopted ancestor remains the source owner.
fn measured_retry<'a>(history:&'a [Value],commit:&str,protocol:&Protocol)->Option<&'a Value>{
    history.iter().rev().find(|g|
        g["accepted"]!=true && g["generatorCommit"]==commit
        && (g["pairedRevalidated"]==true || g["pairedMeasured"]==true)
        && g["candidateId"].as_str().is_some_and(|id|!id.is_empty())
        && g["evaluation"]["metrics"].is_object()
        && g["controllerPlan"]["metaEvolutionProgram"]["source"]==COMPILED
        && g["controllerPlan"]["metaEvolutionProgram"]["generatorCommit"]==commit
        && g["controllerPlan"]["metaEvolutionProgram"]["protocol"]["revision"]==protocol.revision
        && g["controllerPlan"]["metaEvolutionProgram"]["operatorId"].as_str()
            .is_some_and(|id|protocol.operators.iter().any(|op|op.id==id)))
}
fn target(frontier:&[Value],operator:&Operator,allowed:&[&str])->Option<Value>{
    if !allowed.contains(&operator.domain.as_str()){return None;}
    frontier.iter().find(|row|
        row["role"]==operator.role
        && row["domain"]==operator.domain
        && row["path"]==operator.path
        && row["sourceSha256"].as_str().is_some_and(|h|sha(h,64)))
        .cloned()
}

/// Actual policy transition, evaluated by the rebuilt descendant. The model
/// can author arbitrary cyclic/branching operator graphs; no A/B/A/B stage is
/// required. The parent must be independently adopted; the evaluator cannot
/// be edited through this plan.
pub(crate) fn compile(history:&[Value],frontier:&[Value],allowed:&[&str],commit:&str)->Value {
    let Some(ancestor)=parent(history,commit) else{return Value::Null;};
    let original=&ancestor["systemDesign"]["evolutionProtocol"];
    let previous=&ancestor["metaEvolutionProgram"];
    let authored=valid_authored(original);
    let inherited=!authored
        && previous["source"]==COMPILED
        && valid_authored(&previous["protocol"])
        && previous["generatorCommit"]==ancestor["generatorCommit"]
        && ancestor["compiledPlanExecuted"]["evolutionProtocolRevision"]==previous["revision"];
    let (protocol,origin,prior_id):(&Value,Value,Option<String>)=if authored {
        (original,ancestor["id"].clone(),None)
    }else if inherited {
        ( &previous["protocol"], previous["originGenerationId"].clone(),
            previous["operatorId"].as_str().map(str::to_owned))
    }else{return Value::Null};
    let Ok(p)=serde_json::from_value::<Protocol>(protocol.clone()) else{return Value::Null;};
    let retry=measured_retry(history,commit,&p);
    let prior_id=retry.and_then(|g|g["controllerPlan"]["metaEvolutionProgram"]["operatorId"]
        .as_str().map(str::to_owned)).or(prior_id);
    let observation=retry.unwrap_or(ancestor);
    let choices:Vec<&Operator>=if let Some(prior)=prior_id.as_deref(){
        p.transitions.iter().filter(|edge|
            edge.from==prior && transition_matches(edge,observation))
            .filter_map(|edge|p.operators.iter().find(|op|op.id==edge.to))
            .collect()
    }else{p.operators.iter().filter(|op|op.id==p.entry).collect()};
    // The permitted successors come from the adopted program. Scoring among
    // them uses measured downstream outcomes instead of node-ID ordering.
    let mut options=choices.into_iter().filter_map(|op|{
        let source=target(frontier,op,allowed)?;
        let relevant=history.iter().filter(|g|g["controllerPlan"]["metaEvolutionProgram"]
            ["operatorId"]==op.id
            && (g["pairedMeasured"]==true || g["pairedRevalidated"]==true))
            .collect::<Vec<_>>();
        let wins=relevant.iter().filter(|g|g["evaluation"]["metrics"]
            ["improvedHoldouts"].as_u64().unwrap_or(0)>0
            && g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)==0)
            .count();
        let estimated=(wins as f64+1.0)/(relevant.len() as f64+2.0);
        Some((estimated,op.clone(),source))
    }).collect::<Vec<_>>();
    options.sort_by(|a,b|b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    let Some((_,selected,source))=options.into_iter().next() else{return Value::Null;};
    let mut compiled=Compiled{
        source:COMPILED.into(),generator_commit:commit.into(),history_count:history.len(),
        parent_generation_id:ancestor["id"].as_str().unwrap_or("").into(),
        parent_accepted_commit:commit.into(),
        origin_generation_id:origin.as_str().unwrap_or("").into(),
        previous_operator_id:prior_id,
        feedback_generation_id:retry.and_then(|g|g["id"].as_str().map(str::to_owned)),
        operator_id:selected.id.clone(),
        operator:selected,target:source,protocol:p,
        evaluation_owner:"independent_supervisor".into(),quality_proven:false,
        revision:String::new()
    };
    let mut value=serde_json::to_value(&compiled).unwrap_or(Value::Null);
    if let Some(fields)=value.as_object_mut(){fields.remove("revision");}
    compiled.revision=digest(&value);
    serde_json::to_value(compiled).unwrap_or(Value::Null)
}

/// Supervisor and worker verify the typed contract and source lineage. Neither
/// re-executes an old controller's mutable ranking algorithm.
pub(crate) fn valid(plan:&Value,history:&[Value],frontier:&[Value],commit:&str)->bool {
    if plan.is_null(){return true;}
    let Ok(p)=serde_json::from_value::<Compiled>(plan.clone()) else{return false;};
    if p.source!=COMPILED || p.generator_commit!=commit
        || p.history_count!=history.len() || p.quality_proven
        || p.evaluation_owner!="independent_supervisor"
        || p.operator_id!=p.operator.id || !valid_protocol(&p.protocol)
        || !sha(&p.revision,64) {return false;}
    let mut unsigned=serde_json::to_value(&p).unwrap_or(Value::Null);
    if let Some(m)=unsigned.as_object_mut(){m.remove("revision");}
    if digest(&unsigned)!=p.revision{return false;}
    let Some(ancestor)=parent(history,commit) else{return false;};
    if Some(p.parent_generation_id.as_str())!=ancestor["id"].as_str()
        || Some(p.parent_accepted_commit.as_str())!=ancestor["acceptedCommit"].as_str() {return false;}
    if !frontier.iter().any(|row|row==&p.target)
        || Some(p.operator.role.as_str())!=p.target["role"].as_str()
        || Some(p.operator.domain.as_str())!=p.target["domain"].as_str()
        || Some(p.operator.path.as_str())!=p.target["path"].as_str() {return false;}
    // The supervisor/worker independently derive the source of the policy,
    // measured retry, predecessor operator and exact traversed transition.
    // Neither a failed attempt nor a mutable JSON flag can promote adoption.
    let authored=ancestor["systemDesign"]["evolutionProtocol"]==plan["protocol"]
        && valid_authored(&ancestor["systemDesign"]["evolutionProtocol"]);
    let prev=&ancestor["metaEvolutionProgram"];
    let inherited=!authored && prev["source"]==COMPILED
        && prev["protocol"]==plan["protocol"]
        && valid_authored(&prev["protocol"])
        && prev["generatorCommit"]==ancestor["generatorCommit"]
        && ancestor["compiledPlanExecuted"]["evolutionProtocolRevision"]==prev["revision"];
    if !authored && !inherited {return false;}
    let expected_origin=if authored {ancestor["id"].as_str()}
        else {prev["originGenerationId"].as_str()};
    if p.origin_generation_id.is_empty()
        || Some(p.origin_generation_id.as_str())!=expected_origin{return false;}
    let retry=measured_retry(history,commit,&p.protocol);
    if p.feedback_generation_id.as_deref()!=retry.and_then(|g|g["id"].as_str()){
        return false;
    }
    let source=retry.unwrap_or(ancestor);
    let prior=retry.and_then(|g|g["controllerPlan"]["metaEvolutionProgram"]["operatorId"].as_str())
        .or_else(||if inherited{prev["operatorId"].as_str()}else{None});
    if p.previous_operator_id.as_deref()!=prior{return false;}
    if let Some(previous)=prior {
        p.protocol.transitions.iter().any(|edge|
            edge.from==previous && edge.to==p.operator_id
                && transition_matches(edge,source))
    }else{p.operator_id==p.protocol.entry}
}
pub(crate) fn instruction(plan:&Value)->String {
    if plan["source"]!=COMPILED{return String::new();}
    format!("EVOLVABLE META PROGRAM revision={} operator={} experiment={} role={} \
        source={} sourceRevision={}. Editable operator mechanism={} and task={}. \
        This operator is selected by the accepted predecessor's executable \
        transition policy, not a fixed generational stage. Inspect its actual \
        source and implement the edit. To redesign improvement selection, the \
        next rsi_system_plan may submit evolutionProtocol with a wholly new \
        operator vocabulary, dependency graph, experiments and conditional \
        transitions. The frozen supervisor decides source adoption; this \
        compiled program can never certify its own quality.",
        plan["revision"],plan["operatorId"],plan["operator"]["experiment"],
        plan["operator"]["role"],plan["target"]["path"],
        plan["target"]["sourceSha256"],plan["operator"]["mechanism"],
        plan["operator"]["objective"])
}
