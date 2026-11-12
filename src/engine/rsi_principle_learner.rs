//! Editable science of improvement: learns which *model-authored improvement
//! principles* increase NEXT-GENERATION intelligence, not just parent adoption.
//!
//! Every evidence unit is independently paired, has a changed-source receipt,
//! and (for transfer) a descendant compiled directly from the accepted parent.
//! This learner updates a capability x method predictor from those outcomes.
//! Unknown/new roles and methods are discovered from model-written source,
//! rather than closed enums. It is itself an editable intelligence component.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn commit(v:&Value)->bool{
    v.as_str().is_some_and(|x|x.len()==40
        && x.bytes().all(|byte|byte.is_ascii_hexdigit()))
}
fn source_hash(v:&Value)->bool{
    v.as_str().is_some_and(|x|x.len()==64
        && x.bytes().all(|byte|byte.is_ascii_hexdigit()))
}
fn paired(g:&Value)->bool{
    g["pairedRevalidated"]==true && g["selfControllerVerified"]==true
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && commit(&g["generatorCommit"])
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
        && g["changedSourcePaths"].as_array().is_some_and(|r|!r.is_empty())
        && g["evaluation"]["metrics"]["holdoutEvidence"].as_array()
            .is_some_and(|rows|!rows.is_empty())
}
fn adopted(g:&Value)->bool{
    paired(g) && g["accepted"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && commit(&g["acceptedCommit"])
}
fn changed(g:&Value,role:&str)->bool{
    if !paired(g){return false}
    let Some(files)=g["systemDesign"]["componentFiles"][role].as_array()
        .filter(|rows|!rows.is_empty())else{return false};
    g["changedSourcePaths"].as_array().is_some_and(|actual|
        files.iter().any(|path|actual.contains(path)
            && path.as_str().is_some_and(crate::engine::rsi::safe_path)
            && (!adopted(g)||source_hash(&g["acceptedSourceHashes"]
                [path.as_str().unwrap_or("")]))))
}
fn current<'a>(frontier:&'a [Value],role:&str)->Option<&'a Value>{
    frontier.iter().find(|x|x["role"]==role
        && x["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && source_hash(&x["sourceSha256"]))
}
fn method_source<'a>(frontier:&'a [Value])->Option<&'a Value>{
    frontier.iter().find(|x|x["path"]=="src/engine/rsi_principle_learner.rs"
        && x["role"]=="improvement_methods"&&source_hash(&x["sourceSha256"]))
        .or_else(||current(frontier,"improvement_methods"))
}

/// The ability of an improved agent to improve OTHER tasks is a measured
/// outcome, not an author's guess. Counting a model-designed method as adopted
/// requires its method source to be in the independently accepted patch.
fn witnessed_principle(g:&Value,p:&Value,need_adoption:bool)->bool{
    if !paired(g)||(need_adoption&&!adopted(g)){return false;}
    let Some(name)=p["name"].as_str().filter(|s|
        !s.is_empty()&&s.len()<=80)else{return false};
    if !name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'
        ||b==b'-'){return false;}
    let Some(role)=p["methodRole"].as_str()else{return false};
    let Some(files)=p["methodSourcePaths"].as_array()
        .filter(|files|!files.is_empty()&&files.len()<=8)else{return false};
    let Some(targets)=p["improvesRoles"].as_array()
        .filter(|roles|!roles.is_empty()&&roles.len()<=12)else{return false};
    if !targets.iter().all(|r|r.as_str().is_some_and(|s|
        !s.is_empty()&&s.len()<=64)){return false;}
    if !changed(g,role){return false;}
    files.iter().all(|file|file.as_str().is_some_and(|path|
        crate::engine::rsi::safe_path(path)
            && g["systemDesign"]["componentFiles"][role].as_array()
                .is_some_and(|paths|paths.iter().any(|p|p==path))
            && g["changedSourcePaths"].as_array()
                .is_some_and(|paths|paths.iter().any(|p|p==path))
            && (!need_adoption||source_hash(&g["acceptedSourceHashes"][path]))))
}

/// Outcome evidence is counted separately for each target capability. The
/// child must actually change the claimed role; changing an improver and
/// reporting a "memory" gain does not become a memory transfer.
fn measured_child(g:&Value,role:&str)->bool{
    paired(g)&&changed(g,role)
}
fn holdout_summary(g:&Value)->(usize,usize,BTreeSet<String>){
    let mut gains=0usize;let mut regressions=0usize;
    let mut domains=BTreeSet::new();
    for case in g["evaluation"]["metrics"]["holdoutEvidence"]
        .as_array().into_iter().flatten(){
        let Some(domain)=case["domain"].as_str()else{continue};
        domains.insert(domain.to_owned());
        let positive=case["baselinePass"]==false && case["candidatePass"]==true
            || case["metricGains"].as_u64().unwrap_or(0)>0;
        let negative=case["baselinePass"]==true && case["candidatePass"]==false
            || case["metricRegressions"].as_u64().unwrap_or(0)>0;
        if positive{gains+=1;}
        if negative{regressions+=1;}
    }
    (gains,regressions,domains)
}

#[derive(Default)]
struct Evidence{
    parent_adoptions:u64,
    direct_child_trials:u64,
    direct_child_adoptions:u64,
    direct_child_regressions:u64,
    parent_commits:BTreeSet<String>,
    child_commits:BTreeSet<String>,
    holdout_domains:BTreeSet<String>,
    child_holdout_gains:u64,
    child_holdout_regressions:u64
}
/// A learned relationship between any accepted, model-authored improvement
/// principle and future descendant changes across arbitrary intelligence
/// components. Parent -> child comparisons are observational, not causal.
pub(crate) fn learn(history:&[Value],frontier:&[Value])->Value{
    let mut learned=BTreeMap::<(String,String,String),Evidence>::new();
    let mut attempted=BTreeMap::<String,u64>::new();
    for g in history.iter().filter(|g|paired(g)){
        for item in g["systemDesign"]["improvementPrinciples"]
            .as_array().into_iter().flatten(){
            if !witnessed_principle(g,item,false){continue;}
            let name=item["name"].as_str().unwrap_or("").to_owned();
            *attempted.entry(name.clone()).or_default()+=1;
            if !adopted(g)||!witnessed_principle(g,item,true){continue;}
            let Some(role)=item["methodRole"].as_str()else{continue};
            let Some(parent_commit)=g["acceptedCommit"].as_str()else{continue};
            for recipient in item["improvesRoles"].as_array()
                .into_iter().flatten().filter_map(Value::as_str){
                let record=learned.entry((
                    name.clone(),role.to_owned(),recipient.to_owned()
                )).or_default();
                if record.parent_commits.insert(parent_commit.to_owned()){
                    record.parent_adoptions+=1;
                }
                for child in history.iter().filter(|child|
                    child["generatorCommit"]==parent_commit
                        && measured_child(child,recipient)){
                    let Some(candidate)=child["candidateId"].as_str() else{continue};
                    let key=format!("{parent_commit}:{candidate}");
                    if !record.child_commits.insert(key){continue;}
                    record.direct_child_trials+=1;
                    let (gains,regressions,domains)=holdout_summary(child);
                    record.child_holdout_gains+=gains as u64;
                    record.child_holdout_regressions+=regressions as u64;
                    record.holdout_domains.extend(domains);
                    if adopted(child){
                        record.direct_child_adoptions+=1;
                    }else if regressions>0{
                        record.direct_child_regressions+=1;
                    }
                }
            }
        }
    }
    let roles=frontier.iter().filter_map(|row|row["role"].as_str())
        .collect::<BTreeSet<_>>();
    let mut rows=learned.into_iter().filter(|((_,_,target),_)|
        roles.contains(target.as_str())).map(|((name,method,target),item)|{
        let trials=item.direct_child_trials as f64;
        let wins=item.direct_child_adoptions as f64;
        // Beta posterior is a tunable PREDICTIVE model, not a causal proof.
        let posterior=(wins+1.0)/(trials+2.0);
        let diversity=item.holdout_domains.len();
        // A more general principle must help new domains, not just replay a
        // single benchmark. This score is source-editable by successors.
        let predictive_value=posterior*2.0
            +(diversity.min(5) as f64)*0.2
            +(item.parent_adoptions.min(4) as f64)*0.04
            -(item.direct_child_regressions as f64)*0.2;
        json!({"name":name,"methodRole":method,
            "improvesRole":target,
            "parentSourceAdoptions":item.parent_adoptions,
            "verifiedDirectChildTrials":item.direct_child_trials,
            "independentlyAdoptedChildren":item.direct_child_adoptions,
            "measuredChildRegressions":item.direct_child_regressions,
            "childHoldoutGains":item.child_holdout_gains,
            "childHoldoutRegressions":item.child_holdout_regressions,
            "observedChildHoldoutDomains":item.holdout_domains
                .into_iter().collect::<Vec<_>>(),
            "posteriorChildAdoption":posterior,
            "predictionScore":predictive_value,
            "claim":"observed_compiled_descendant_transfer_not_causal_attribution",
            "unseenTaskGeneralizationProven":false})
    }).collect::<Vec<_>>();
    rows.sort_by(|a,b|{
        b["predictionScore"].as_f64().unwrap_or(0.0)
            .total_cmp(&a["predictionScore"].as_f64().unwrap_or(0.0))
            .then_with(||a["name"].as_str().cmp(&b["name"].as_str()))
            .then_with(||a["improvesRole"].as_str().cmp(&b["improvesRole"].as_str()))
    });
    let mut model=json!({
        "source":"compiled_descendant_principle_transfer_predictor_v1",
        "edges":rows.into_iter().take(96).collect::<Vec<_>>(),
        "attemptedSourcePrinciples":attempted,
        "historyCount":history.len(),
        "crossRolePredictionsOnly":true,
        "learnedFrom":"independent_paired_source_adoption_and_direct_compiled_child_outcomes",
        "predictorIsEditable":"src/engine/rsi_principle_learner.rs",
        "causalTransferProven":false
    });
    model["sha256"]=json!(digest(&model));model
}

/// Invent a new OPERATOR by recombining two independently adopted methods
/// that each enabled measured child intelligence improvements. A combination
/// is an unproven model-authored hypothesis until its OWN source and child
/// outcomes are independently evaluated. No fixed composition grammar.
fn composition_hypothesis(model:&Value,preferred_role:&str)->Value{
    let proven=model["edges"].as_array().into_iter().flatten()
        .filter(|edge|edge["verifiedDirectChildTrials"].as_u64().unwrap_or(0)>0
            && edge["independentlyAdoptedChildren"].as_u64().unwrap_or(0)>0)
        .collect::<Vec<_>>();
    let mut pairs=Vec::new();
    for (index,left) in proven.iter().enumerate(){
        for right in proven.iter().skip(index+1){
            if left["name"]==right["name"]||
                left["improvesRole"]==right["improvesRole"]{continue;}
            let relevant=left["improvesRole"]==preferred_role
                ||right["improvesRole"]==preferred_role;
            let score=left["predictionScore"].as_f64().unwrap_or(0.0)
                +right["predictionScore"].as_f64().unwrap_or(0.0)
                +if relevant{0.25}else{0.0};
            pairs.push((score,(*left).clone(),(*right).clone()));
        }
    }
    pairs.sort_by(|a,b|b.0.total_cmp(&a.0)
        .then_with(||a.1["name"].as_str().cmp(&b.1["name"].as_str()))
        .then_with(||a.2["name"].as_str().cmp(&b.2["name"].as_str())));
    let Some((_,a,b))=pairs.first()else{return Value::Null};
    let mut new=json!({
        "source":"cross_capability_principle_synthesis_v1",
        "composedFrom":[a["name"],b["name"]],
        "parentEvidence":[{"name":a["name"],
            "observedTarget":a["improvesRole"],
            "verifiedChildAdoptions":a["independentlyAdoptedChildren"]},
            {"name":b["name"],
            "observedTarget":b["improvesRole"],
            "verifiedChildAdoptions":b["independentlyAdoptedChildren"]}],
        "newTargetRole":preferred_role,
        "requireNewExecutableMethod":true,
        "qualityProven":false,"causalityProven":false
    });
    new["sha256"]=json!(digest(&new));new
}
/// Decide WHAT subsystem to improve next from measured descendant outcomes,
/// rather than only counting source rewrites of the meta-controller.
pub(crate) fn plan(
    history:&[Value],frontier:&[Value],model:&Value,commit:&str
)->Value{
    let mut scored=BTreeMap::<String,(f64,Value,Value)>::new();
    let mut role_trials=BTreeMap::<String,(u64,u64)>::new();
    for g in history.iter().filter(|g|paired(g)){
        let changed=frontier.iter().filter(|row|row["role"].as_str()
            .is_some_and(|role|changed(g,role))).map(|row|
                row["role"].as_str().unwrap_or("").to_string())
            .collect::<BTreeSet<_>>();
        for role in changed{
            let stats=role_trials.entry(role).or_default();
            stats.0+=1;
            if adopted(g){stats.1+=1;}
        }
    }
    let candidates=frontier.iter().filter(|row|
        current(frontier,row["role"].as_str().unwrap_or(""))
            .is_some() && row["sourceSha256"].as_str().is_some())
        .fold(BTreeMap::<String,Value>::new(),|mut roles,row|{
            let role=row["role"].as_str().unwrap_or("");
            roles.entry(role.to_owned()).or_insert_with(||row.clone());roles
        });
    for (role,target) in candidates {
        let (trials,wins)=role_trials.get(&role).copied().unwrap_or_default();
        let exposure=(trials+1) as f64;
        let unexplored=1.0/exposure.sqrt();
        let adoption_prior=(wins as f64+1.0)/(trials as f64+2.0);
        let best=model["edges"].as_array().into_iter().flatten()
            .filter(|edge|edge["improvesRole"]==role)
            .max_by(|a,b|a["predictionScore"].as_f64().unwrap_or(0.0)
                .total_cmp(&b["predictionScore"].as_f64().unwrap_or(0.0)))
            .cloned().unwrap_or(Value::Null);
        let observed=best["verifiedDirectChildTrials"].as_u64().unwrap_or(0)>0;
        let score=if observed {
            best["predictionScore"].as_f64().unwrap_or(0.0)
                +0.2*unexplored+0.15*adoption_prior
        } else {
            // Cold-start targets are ALL intelligence roles, including
            // models, memories, learning, tools and invented subsystems.
            0.65+0.7*unexplored+0.2*adoption_prior
        };
        scored.insert(role,(score,target,best));
    }
    // A model/learning/tool improvement is a first-class actual source edit,
    // not only a tag on an improvement-method experiment.
    let mut ranked=scored.into_iter().map(|(role,(score,target,best))|
        json!({"role":role,"predictionScore":score,
            "sourceTarget":target,"bestTransferredPrinciple":best,
            "hasMeasuredDescendantTransfer":best["verifiedDirectChildTrials"]
                .as_u64().unwrap_or(0)>0
        })).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b["predictionScore"].as_f64().unwrap_or(0.0)
        .total_cmp(&a["predictionScore"].as_f64().unwrap_or(0.0))
        .then_with(||a["role"].as_str().cmp(&b["role"].as_str())));
    let winner=ranked.first().cloned().unwrap_or(Value::Null);
    let capability=winner["sourceTarget"].clone();
    let method=method_source(frontier).cloned().unwrap_or(Value::Null);
    let composed=composition_hypothesis(model,
        capability["role"].as_str().unwrap_or(""));
    // New principle generation changes BOTH a real capability and the
    // method that creates improved capabilities, when both are available.
    let mut focus=vec![];
    if capability.is_object(){focus.push(capability.clone());}
    if method.is_object() && method["path"]!=capability["path"] {
        focus.push(method.clone());
    }
    let mut result=json!({
        "source":"compiled_adaptive_improvement_principle_curriculum_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "modelSha256":model["sha256"],
        "phase":if composed.is_object(){
            "synthesize_new_improvement_principle"
        }else if winner["hasMeasuredDescendantTransfer"]==true {
            "transfer_and_reinvent_principle"
        }else{"discover_new_cross_intelligence_principle"},
        "target":capability,
        "methodSource":method,
        "focus":focus,
        "bestObservedPrinciple":winner["bestTransferredPrinciple"],
        "compositionHypothesis":composed,
        "rankedRoles":ranked.into_iter().take(16).collect::<Vec<_>>(),
        "externalEvaluator":"frozen_paired_oracle",
        "improvedNextGenerationCapabilityNotAssumed":true,
        "requiresActualChangedSource":true,
        "editableSource":"src/engine/rsi_principle_learner.rs",
        "causalityProven":false
    });
    result["revision"]=json!(digest(&result));result
}
pub(crate) fn valid(
    policy:&Value,model:&Value,frontier:&[Value],commit:&str,len:usize
)->bool{
    let mut copy=policy.clone();
    let Some(map)=copy.as_object_mut()else{return false};map.remove("revision");
    policy["source"]=="compiled_adaptive_improvement_principle_curriculum_v1"
        && policy["revision"].as_str()==Some(digest(&copy).as_str())
        && policy["generatorCommit"]==commit
        && policy["historyCount"].as_u64()==Some(len as u64)
        && policy["modelSha256"]==model["sha256"]
        && policy["causalityProven"]==false
        && policy["requiresActualChangedSource"]==true
        && policy["focus"].as_array().is_some_and(|items|
            items.len()<=2 && items.iter().all(|row|frontier.contains(row)))
        && (policy["target"].is_null()||frontier.contains(&policy["target"]))
}
pub(crate) fn apply(agenda:&mut Value,policy:&Value){
    if policy["source"]!="compiled_adaptive_improvement_principle_curriculum_v1"
        || agenda["inheritedStage"].is_object()
        // Never suppress an independently accepted reciprocal experiment,
        // or a descendant-authored new role introduction/return.
        || agenda["factorialExperiment"]["descendantContinuation"]==true
        || !agenda["recursiveLaboratory"]["phase"].is_null()
            && agenda["recursiveLaboratory"]["phase"]!="factorial_science"{
        return;
    }
    let Some(frontier)=agenda["frontier"].as_array()else{return};
    let Some(focus)=policy["focus"].as_array()else{return};
    if focus.is_empty()||focus.len()>2
        || !focus.iter().all(|row|frontier.contains(row))
        || !frontier.contains(&policy["target"]){return;}
    agenda["target"]=policy["target"].clone();
    agenda["focus"]=policy["focus"].clone();
    agenda["principleCurriculum"]=policy.clone();
}
pub(crate) fn instruction(policy:&Value,model:&Value)->String{
    if policy["source"]!="compiled_adaptive_improvement_principle_curriculum_v1"{
        return String::new();
    }
    format!("[OPEN PRINCIPLE-TO-CAPABILITY META-LEARNING]\n\
        Phase={} targetRole={} source={} methodSource={}. \
        Transfer evidence={} new source-changing composition={}         source method predictor={}. \
        Write a NEW improvementPrinciples entry via rsi_system_plan with \
        name, methodRole, methodSourcePaths, improvesRoles, mechanism, \
        falsifiableOutcome. Actually rewrite the claimed method source and \
        the targeted subsystem source, and demonstrate that the NEXT \
        COMPILED DESCENDANT can improve other roles or domains. \
        A candidate adoption is not equal to increased self-improvement \
        ability; only separately measured offspring outcomes can train this \
        predictor. The model, learning, memory, tools and harness are \
        equally valid intelligence redesign targets. Future source revisions \
        may rewrite this learning algorithm. Frozen evaluator stays external.",
        policy["phase"],policy["target"]["role"],policy["target"]["path"],
        policy["methodSource"]["path"],policy["bestObservedPrinciple"],
        policy["compositionHypothesis"],model["sha256"])
}

#[cfg(test)]
mod tests{
    use super::*;
    fn role(name:&str,path:&str)->Value{
        json!({"role":name,"path":path,"domain":"improver",
            "sourceSha256":"a".repeat(64)})
    }
    fn generation(id:&str,base:&str,accepted_commit:Option<&str>,
        role:&str,path:&str)->Value{
        let mut g=json!({"jobId":id,"generatorCommit":base,
            "acceptedCommit":accepted_commit,"candidateId":id,
            "accepted":accepted_commit.is_some(),
            "pairedRevalidated":true,"selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "changedSourcePaths":[path],"acceptedSourceHashes":{},
            "evaluation":{"metrics":{"eligible":accepted_commit.is_some(),
                "holdoutEvidence":[{"id":"holdout-a","domain":"memory",
                    "baselinePass":false,"candidatePass":true,
                    "metricGains":0,"metricRegressions":0}]}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":id,
                "component":"system","components":[role],
                "componentFiles":{},"targetFiles":[path]}});
        g["systemDesign"]["componentFiles"][role]=json!([path]);
        if accepted_commit.is_some(){g["acceptedSourceHashes"][path]=json!("f".repeat(64));}
        g
    }
    #[test]
    fn actual_child_of_adopted_method_teaches_generalization_predictor(){
        let base="a".repeat(40);let born="b".repeat(40);
        let end="c".repeat(40);
        let mut parent=generation("parent",&base,Some(&born),
            "improvement_methods","src/engine/rsi_principle_learner.rs");
        parent["systemDesign"]["improvementPrinciples"]=json!([{
            "name":"improve_memory_from_method","methodRole":"improvement_methods",
            "methodSourcePaths":["src/engine/rsi_principle_learner.rs"],
            "improvesRoles":["memory"],"mechanism":"Adapt memory retrieval",
            "falsifiableOutcome":"Higher child holdout adoption rate"}]);
        let child=generation("child",&born,Some(&end),"memory",
            "src/engine/memory.rs");
        let frontier=vec![role("memory","src/engine/memory.rs"),
            role("improvement_methods","src/engine/rsi_principle_learner.rs")];
        let model=learn(&[parent.clone(),child.clone()],&frontier);
        assert_eq!(model["edges"][0]["verifiedDirectChildTrials"],1);
        assert_eq!(model["edges"][0]["independentlyAdoptedChildren"],1);
        assert_eq!(model["edges"][0]["childHoldoutGains"],1);
        let plan=plan(&[parent.clone(),child.clone()],&frontier,&model,&end);
        assert_eq!(plan["target"]["role"],"memory");
        assert!(valid(&plan,&model,&frontier,&end,2));
        let mut fake=child;fake["generatorCommit"]=json!("d".repeat(40));
        let other=learn(&[parent,fake],&frontier);
        assert_eq!(other["edges"][0]["verifiedDirectChildTrials"],0);
    }
    #[test]
    fn two_verified_methods_from_different_intelligences_propose_new_composition(){
        let model=json!({"edges":[
            {"name":"derive_memory","improvesRole":"memory",
                "verifiedDirectChildTrials":2,"independentlyAdoptedChildren":1,
                "predictionScore":1.4},
            {"name":"revise_tools","improvesRole":"tools",
                "verifiedDirectChildTrials":1,"independentlyAdoptedChildren":1,
                "predictionScore":1.7}]});
        let composition=composition_hypothesis(&model,"memory");
        assert_eq!(composition["source"],"cross_capability_principle_synthesis_v1");
        assert_eq!(composition["composedFrom"][0],"derive_memory");
        assert_eq!(composition["composedFrom"][1],"revise_tools");
        assert_eq!(composition["qualityProven"],false);
        let insufficient=json!({"edges":[{
            "name":"unverified","improvesRole":"memory",
            "verifiedDirectChildTrials":2,"independentlyAdoptedChildren":0}]});
        assert!(composition_hypothesis(&insufficient,"memory").is_null());
    }
    #[test]
    fn rejected_source_or_method_without_real_edited_file_never_trains_model(){
        let base="a".repeat(40);let born="b".repeat(40);
        let mut g=generation("a",&base,Some(&born),
            "improvement_methods","src/engine/rsi_principle_learner.rs");
        g["systemDesign"]["improvementPrinciples"]=json!([{
            "name":"model_improver","methodRole":"improvement_methods",
            "methodSourcePaths":["src/engine/rsi_principle_learner.rs"],
            "improvesRoles":["models"]}]);
        let target=vec![role("models","src/engine/models.rs")];
        let mut falsified=g.clone();falsified["accepted"]=json!(false);
        assert_eq!(learn(&[falsified],&target)["edges"].as_array().unwrap().len(),0);
        g["changedSourcePaths"]=json!(["src/engine/other.rs"]);
        assert_eq!(learn(&[g],&target)["edges"].as_array().unwrap().len(),0);
    }
}
