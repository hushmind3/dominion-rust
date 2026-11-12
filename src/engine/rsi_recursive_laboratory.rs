//! Self-rewriting discovery laboratory for whole-intelligence RSI.
//! The compiled child designs interventions, invents new wired intelligence,
//! develops it, returns capability to its source, and rewrites the discovery
//! method. Frozen paired evaluation alone controls adoption.
//!
//! This program's source is part of the child, not a supervisor-side ranking
//! rule. An accepted rewrite changes how the next binary invents improvements.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn sha(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn hash(value:&Value)->bool {
    value.as_str().is_some_and(|s|s.len()==64
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn valid_source(row:&Value)->bool {
    row["role"].as_str().is_some_and(|r|!r.is_empty()&&r.len()<=64)
        && row["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && hash(&row["sourceSha256"])
}
fn adopted(g:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["selfControllerVerified"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["acceptedCommit"].as_str().is_some_and(|s|
            s.len()==40&&s.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["generatorCommit"].as_str().is_some_and(|s|
            s.len()==40&&s.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
}
fn changed(g:&Value,role:&str)->bool {
    if !adopted(g){return false;}
    let Some(files)=g["systemDesign"]["componentFiles"][role].as_array()
        .filter(|v|!v.is_empty()) else{return false};
    let Some(actual)=g["changedSourcePaths"].as_array() else{return false};
    files.iter().any(|file|file.as_str().is_some_and(|path|
        actual.iter().any(|x|x==path)
            && g["acceptedSourceHashes"][path].as_str().is_some_and(|s|
                s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))))
}
fn direct_child<'a>(history:&'a [Value],parent:&str,role:&str)->Option<&'a Value>{
    history.iter().rev().find(|g|
        g["generatorCommit"]==parent && changed(g,role))
}
/// The origin and the scientific method must be rewritten at the same
/// source path selected by the running descendant, not elsewhere in a
/// similarly named component. This prevents false cross-generation closure.
fn direct_child_path<'a>(
    history:&'a [Value],parent:&str,role:&str,path:&str
)->Option<&'a Value>{
    history.iter().rev().find(|g|{
        g["generatorCommit"]==parent && changed(g,role)
            && g["changedSourcePaths"].as_array().is_some_and(|files|
                files.iter().any(|file|file==path))
            && g["acceptedSourceHashes"][path].as_str().is_some_and(|hash|
                hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()))
    })
}
fn current<'a>(frontier:&'a [Value],role:&str)->Option<&'a Value>{
    frontier.iter().find(|row|row["role"]==role&&valid_source(row))
}
fn method_source<'a>(frontier:&'a [Value])->Option<&'a Value>{
    frontier.iter().find(|row|row["path"]=="src/engine/rsi_recursive_laboratory.rs"
        && row["role"]=="improvement_methods"&&valid_source(row))
        .or_else(||frontier.iter().find(|row|
            row["path"]=="src/engine/rsi_causal_ecology.rs"&&valid_source(row)))
        .or_else(||current(frontier,"improvement_methods"))
}
fn new_role_name(left:&str,right:&str,roles:&BTreeSet<String>)->String {
    let stem=|v:&str|v.bytes().filter(|b|b.is_ascii_lowercase()
        ||b.is_ascii_digit()||*b==b'_').take(16).map(char::from)
        .collect::<String>();
    let prefix=format!("symbiotic_{}_{}",stem(left),stem(right));
    for n in 0..100 {
        let role=if n==0{prefix.clone()}else{format!("{prefix}_{n}")};
        if role.len()<=64&&!roles.contains(&role){return role;}
    }
    format!("symbiotic_{}",roles.len())
}

/// Exact accepted, rebuilt source descendants determine progress. A model's
/// intent cannot masquerade as capability transfer or method improvement.
fn pending_invention(history:&[Value],frontier:&[Value],commit:&str)->Option<Value>{
    for birth in history.iter().rev().filter(|g|adopted(g)) {
        for invention in birth["systemDesign"]["newComponents"]
            .as_array().into_iter().flatten(){
            let Some(role)=invention["role"].as_str() else{continue};
            if !changed(birth,role){continue;}
            let Some(novel)=current(frontier,role) else{continue};
            let old=invention["improvesRole"].as_str().or_else(||
                birth["systemDesign"]["interactionEdges"].as_array()
                    .into_iter().flatten().find(|edge|edge["fromRole"]==role)
                    .and_then(|edge|edge["toRole"].as_str()))
                .unwrap_or("improvement_methods");
            let Some(origin)=current(frontier,old) else{continue};
            if origin["role"]==novel["role"]{continue;}
            let first=birth["acceptedCommit"].as_str()?;
            let child=direct_child(history,first,role);
            if child.is_none() && commit==first{
                return Some(json!({"phase":"develop_invented_intelligence",
                    "target":novel,"origin":origin,"inventionRole":role,
                    "birthCommit":first,"birthJob":birth["jobId"],
                    "lineage":[]}));
            }
            let Some(child)=child else{continue};
            let second=child["acceptedCommit"].as_str()?;
            let returned=direct_child_path(history,second,old,
                origin["path"].as_str()?);
            if returned.is_none() && commit==second {
                return Some(json!({"phase":"return_to_origin",
                    "target":origin,"origin":novel,"inventionRole":role,
                    "birthCommit":first,"birthJob":birth["jobId"],
                    "lineage":[child["acceptedCommit"]]}));
            }
            let Some(returned)=returned else{continue};
            let third=returned["acceptedCommit"].as_str()?;
            let Some(method)=method_source(frontier) else{continue};
            let method_role=method["role"].as_str()?;
            if direct_child_path(history,third,method_role,
                method["path"].as_str()?).is_none() && commit==third {
                return Some(json!({"phase":"rewrite_scientific_method",
                    "target":method,"origin":origin,"inventionRole":role,
                    "birthCommit":first,"birthJob":birth["jobId"],
                    "lineage":[child["acceptedCommit"],returned["acceptedCommit"]]}));
            }
        }
    }
    None
}

/// Observed scientific-method trials must be both executed and source-
/// changing. Planning a study, naming a role or claiming success without
/// a paired candidate's actual changed files contributes ZERO evidence.
fn witnessed_science_trial(g:&Value,method:&str)->bool {
    let lab=&g["recursiveLaboratory"];
    if lab["source"]!="descendant_editable_recursive_laboratory_v1"
        || lab["method"]["name"]!=method
        || g["systemAgenda"]["recursiveLaboratory"]!=*lab
        || lab["generatorCommit"]!=g["generatorCommit"]
        || g["selfControllerVerified"]!=true
        || g["pairedRevalidated"]!=true
        || !g["candidateId"].is_string()
        || !g["evaluation"]["metrics"]["holdoutEvidence"].is_array(){
        return false;
    }
    let Some(changed)=g["changedSourcePaths"].as_array() else{return false;};
    if lab["phase"]=="invent_new_intelligence" {
        let name=&lab["invention"]["role"];
        return g["systemDesign"]["newComponents"].as_array()
            .is_some_and(|rows|rows.iter().any(|item|&item["role"]==name
                && item["sourcePaths"].as_array().is_some_and(|paths|
                    !paths.is_empty()&&paths.iter().all(|path|
                        changed.contains(path)))));
    }
    lab["focus"].as_array().is_some_and(|focus|focus.iter().any(|row|
        changed.iter().any(|path|path==&row["path"])))
}

/// Evidence-based control policy is ordinary editable source. A method's
/// posterior counts verified, measured source generations, not self-reports.
fn choose_method(history:&[Value])->Value {
    let names=["experiment_isolation","coupled_architecture",
        "invent_compiled_subsystem","counterexample_repair",
        "rewrite_discovery_method"];
    let mut candidates=Vec::new();
    for name in names{
        let trials=history.iter()
            .filter(|g|witnessed_science_trial(g,name)).collect::<Vec<_>>();
        let wins=trials.iter().filter(|g|adopted(g)).count();
        let inspected=trials.len();
        let posterior=(wins as f64+1.0)/(inspected as f64+2.0);
        let uncertainty=1.0/((inspected+1) as f64).sqrt();
        let score=posterior+0.16*uncertainty;
        candidates.push((score,json!({"name":name,"observedTrials":inspected,
            "independentAdoptions":wins,"posteriorAdoption":posterior,
            "uncertainty":uncertainty,"score":score,"qualityProven":false})));
    }
    candidates.sort_by(|a,b|b.0.total_cmp(&a.0)
        .then_with(||a.1["name"].as_str().cmp(&b.1["name"].as_str())));
    candidates[0].1.clone()
}

/// A negative cross-subsystem effect creates a concrete hypothesis for a
/// NEW module that repairs the interaction and learns better interventions.
fn invention_proposal(frontier:&[Value],pair:&Value,meta:&Value)->Value{
    let mut left=pair["left"].clone();
    let mut right=pair["right"].clone();
    let mut strongest=0u64;
    for edge in meta["interactionDynamics"]["edges"].as_array()
        .into_iter().flatten(){
        let count=edge["isolatedPairedRegressions"].as_u64().unwrap_or(0);
        if count<=strongest{continue;}
        let Some(a)=edge["sourceRole"].as_str() else{continue};
        let Some(b)=edge["targetRole"].as_str() else{continue};
        if a==b{continue;}
        if let (Some(source),Some(target))=(current(frontier,a),current(frontier,b)){
            left=source.clone();right=target.clone();strongest=count;
        }
    }
    if !valid_source(&left)||!valid_source(&right)
        || left["role"]==right["role"] {return Value::Null;}
    let roles=frontier.iter().filter_map(|row|row["role"].as_str())
        .map(str::to_string).collect::<BTreeSet<_>>();
    let role=new_role_name(left["role"].as_str().unwrap_or(""),
        right["role"].as_str().unwrap_or(""),&roles);
    json!({"role":role,"domain":"improver",
        "purpose":"Executable co-improvement mechanism that uses measured cross-system errors to redesign both intelligence generation and task improvement.",
        "improvesRole":left["role"],
        "from":left,"to":right,
        "sourcePaths":[format!("src/engine/rsi_{role}.rs")],
        "integrationPaths":["src/engine/mod.rs",
            "src/engine/rsi_recursive_laboratory.rs"],
        "diagnosis":if strongest>0{"measured_cross_role_regression"}
            else{"open_world_cold_start_hypothesis"},
        "observedRegressions":strongest,
        "requiresExecutableIntegration":true,
        "requiresIndependentAdoption":true,
        "qualityProven":false})
}

/// The SCIENTIFIC METHOD itself is the object of a subsequent source patch.
/// Existing factorial design is retained; evidence and adopted lineage select
/// when the lab prioritizes a novel component or a meta-level rewrite.
pub(crate) fn plan(history:&[Value],frontier:&[Value],
    factorial:&Value,meta:&Value,commit:&str)->Value {
    let ongoing=pending_invention(history,frontier,commit);
    let method=choose_method(history);
    let mut phase="factorial_science".to_string();
    let mut focus=factorial["focus"].as_array().cloned().unwrap_or_default();
    let mut target=factorial["target"].clone();
    let mut birth=Value::Null;
    let mut ancestry=Value::Null;
    if let Some(next)=ongoing{
        phase=next["phase"].as_str().unwrap_or("factorial_science").to_string();
        target=next["target"].clone();
        focus=vec![target.clone()];
        ancestry=next;
    } else if factorial["phase"]=="invent_new_interaction"
        && factorial["descendantContinuation"]==true {
        // The independently adopted A->B->joint->A cycle now has to
        // rewrite its own experiment invention algorithm, not restart A.
        if let Some(source)=method_source(frontier){
            phase="rewrite_scientific_method".into();
            target=source.clone();
            focus=vec![target.clone()];
            ancestry=json!({"source":"adopted_reciprocal_return_to_meta",
                "priorFactorialSha256":factorial["sha256"],
                "lineageRoot":factorial["lineageRoot"]});
        }
    } else {
        let blocked=history.iter().rev().take(12).filter(|g|
            g["candidateId"].is_string()
            && g["pairedRevalidated"]==true
            && g["accepted"]!=true
            && (g["recursiveLaboratory"]["phase"]=="factorial_science"
                ||g["recursiveLaboratory"]["phase"]=="invent_new_intelligence")
        ).count();
        // A failed experiment is not evidence of a successful invention;
        // several measured dead ends motivate an entirely new mechanism.
        let diagnosed=meta["interactionDynamics"]["edges"].as_array()
            .into_iter().flatten().any(|row|
                row["isolatedPairedRegressions"].as_u64().unwrap_or(0)>0);
        let contrast=factorial["observations"]["contrasts"].as_array()
            .into_iter().flatten().any(|row|
                row["interactionContrast"].as_f64()
                    .is_some_and(|value|value<0.0));
        if (blocked>=2||diagnosed||contrast) && ongoing.is_none(){
            // The experimentally learned method is allowed to change real
            // intervention focus, not merely annotate a research prompt.
            let selected=method["name"].as_str().unwrap_or("");
            let learned=method["observedTrials"].as_u64().unwrap_or(0)>0
                && method["independentAdoptions"].as_u64().unwrap_or(0)>0;
            let repair=selected=="counterexample_repair" && learned && blocked>=2;
            let rewrite=selected=="rewrite_discovery_method" && learned;
            let failed_source=history.iter().rev().filter(|g|
                g["candidateId"].is_string() && g["accepted"]!=true
                    && g["pairedRevalidated"]==true)
                .flat_map(|g|g["focusFiles"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .find_map(|path|frontier.iter().find(|row|
                    row["path"]==path && valid_source(row)).cloned());
            if rewrite {
                if let Some(source)=method_source(frontier){
                    phase="rewrite_stalled_scientific_method".into();
                    target=source.clone();focus=vec![target.clone()];
                }
            } else if repair {
                if let Some(source)=failed_source{
                    phase="repair_measured_negative_experiment".into();
                    target=source;focus=vec![target.clone()];
                }
            } else {
                let proposal=invention_proposal(frontier,factorial,meta);
                if proposal.is_object(){
                    phase="invent_new_intelligence".into();
                    focus=vec![proposal["from"].clone(),proposal["to"].clone()];
                    target=proposal["from"].clone();
                    birth=proposal;
                }
            }
        }
    }
    focus.retain(valid_source);
    focus.truncate(2);
    // Only current indexed sources may be the immediate edit focus.
    focus.retain(|row|frontier.contains(row));
    let mut core=json!({"source":"descendant_editable_recursive_laboratory_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "phase":phase,"target":target,"focus":focus,
        "invention":birth,"recursiveAncestry":ancestry,
        "method":method,"factorialSha256":factorial["sha256"],
        "metaRevision":meta["revision"],
        "researchCodePath":"src/engine/rsi_recursive_laboratory.rs",
        "externalEvaluator":"frozen_paired_oracle",
        "actualSourceAdoptionRequired":true,
        "selfModifyScientificMethod":true,
        "qualityProven":false,"causalityProven":false});
    core["revision"]=json!(sha(&core));core
}
pub(crate) fn valid(plan:&Value,frontier:&[Value],factorial:&Value,
    meta:&Value,commit:&str,len:usize)->bool {
    if plan.is_null(){return true;}
    let mut core=plan.clone();
    let Some(obj)=core.as_object_mut() else{return false};
    obj.remove("revision");
    plan["source"]=="descendant_editable_recursive_laboratory_v1"
        && plan["revision"].as_str()==Some(sha(&core).as_str())
        && plan["generatorCommit"]==commit
        && plan["historyCount"].as_u64()==Some(len as u64)
        && plan["factorialSha256"]==factorial["sha256"]
        && plan["metaRevision"]==meta["revision"]
        && plan["actualSourceAdoptionRequired"]==true
        && plan["qualityProven"]==false&&plan["causalityProven"]==false
        && plan["focus"].as_array().is_some_and(|rows|
            rows.len()<=2 && rows.iter().all(|row|
                valid_source(row)&&frontier.contains(row)))
        && (plan["target"].is_null()
            || (valid_source(&plan["target"])&&frontier.contains(&plan["target"])))
}
pub(crate) fn apply(agenda:&mut Value,lab:&Value) {
    if lab["source"]!="descendant_editable_recursive_laboratory_v1"
        || agenda["inheritedStage"].is_object(){return;}
    let Some(frontier)=agenda["frontier"].as_array() else{return};
    let Some(focus)=lab["focus"].as_array() else{return};
    if focus.is_empty()||focus.len()>2
        || focus.iter().any(|row|!valid_source(row)||!frontier.contains(row))
        || !frontier.contains(&lab["target"]){return;}
    // The real next generation's source focus, not prompt text, changes.
    agenda["target"]=lab["target"].clone();
    agenda["focus"]=lab["focus"].clone();
    agenda["recursiveLaboratory"]=lab.clone();
}
pub(crate) fn instruction(lab:&Value)->String {
    if lab["source"]!="descendant_editable_recursive_laboratory_v1"{
        return String::new();
    }
    let action=match lab["phase"].as_str().unwrap_or(""){
        "develop_invented_intelligence"=>
            "This newly invented intelligence is independently source-adopted. Rewrite its compiled code to develop its own intelligence and verify the next generation.",
        "return_to_origin"=>
            "The invented role was developed by a rebuilt descendant. Rewrite the originating intelligence source to incorporate this new improvement method.",
        "rewrite_scientific_method"|"rewrite_stalled_scientific_method"=>
            "The originating intelligence was independently re-improved, or repeated scientific failures exposed a meta-method limitation. Now REWRITE THIS RUNNING scientific-method code and change future experimental invention, controls, ablations or hypothesis selection.",
        "repair_measured_negative_experiment"=>
            "Independent paired evaluation rejected earlier source interventions. Repair the exact observed source failure, replace the failed hypothesis or invent a better test procedure; do not replay the same patch.",
        "invent_new_intelligence"=>
            "Invent a genuinely NEW EXECUTABLE Rust intelligence subsystem and wire it into engine/mod.rs AND the living recursive laboratory. Use rsi_system_plan with components=[new role, old roles], componentFiles, newComponents containing improvesRole, sourcePaths, integrationPaths, and falsifiable interactionEdges. Create and call the real source in the same candidate; the evaluator judges adoption.",
        _=>
            "Continue source-matched factorial experimental science. Improve its ability to propose the next source intervention, not only the external task."
    };
    format!("[COMPILABLE SELF-IMPROVING RESEARCH LAB] \
        phase={} revision={} activeFocus={} inventedRole={} \
        verifiedLineage={} selectedExperimentalMethod={} \
        observedMethodTrials={}. {} \
        The next rebuilt descendant may invent a different scientific \
        experiment selection algorithm by editing this module itself. \
        Never assign causal credit to unisolated changes.",
        lab["phase"],lab["revision"],lab["focus"],
        lab["invention"]["role"],lab["recursiveAncestry"],
        lab["method"]["name"],lab["method"]["observedTrials"],action)
}

#[cfg(test)]
mod tests{
    use super::*;
    fn target(role:&str,path:&str)->Value {
        json!({"role":role,"domain":"improver","path":path,
            "sourceSha256":"a".repeat(64),"sourceBytes":1000})
    }
    fn accepted(id:&str,base:&str,head:&str,role:&str,path:&str)->Value {
        let mut g=json!({"jobId":id,"generatorCommit":base,"acceptedCommit":head,
            "accepted":true,"pairedRevalidated":true,
            "selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":id,
                "componentFiles":{},"targetFiles":[path]},
            "changedSourcePaths":[path],"acceptedSourceHashes":{}});
        g["systemDesign"]["componentFiles"][role]=json!([path]);
        g["acceptedSourceHashes"][path]=json!("b".repeat(64));
        g
    }
    #[test]
    fn only_adopted_descendant_source_changes_advance_new_role_cycle(){
        let root="a".repeat(40);let first="b".repeat(40);
        let second="c".repeat(40);let third="d".repeat(40);
        let novel=target("new_study","src/engine/rsi_new_study.rs");
        let origin=target("improvement_methods","src/engine/rsi_meta_controller.rs");
        let method=target("improvement_methods","src/engine/rsi_recursive_laboratory.rs");
        let mut birth=accepted("birth",&root,&first,"new_study",
            "src/engine/rsi_new_study.rs");
        birth["systemDesign"]["newComponents"]=json!([
            {"role":"new_study","improvesRole":"improvement_methods"}]);
        let frontier=vec![novel.clone(),origin.clone(),method.clone()];
        let step=pending_invention(&[birth.clone()],&frontier,&first).unwrap();
        assert_eq!(step["phase"],"develop_invented_intelligence");
        assert_eq!(step["target"],novel);
        let child=accepted("child",&first,&second,"new_study",
            "src/engine/rsi_new_study.rs");
        let step=pending_invention(&[birth.clone(),child.clone()],&frontier,
            &second).unwrap();
        assert_eq!(step["phase"],"return_to_origin");
        assert_eq!(step["target"]["role"],"improvement_methods");
        let back=accepted("back",&second,&third,"improvement_methods",
            "src/engine/rsi_meta_controller.rs");
        let step=pending_invention(&[birth.clone(),child,back],&frontier,
            &third).unwrap();
        assert_eq!(step["phase"],"rewrite_scientific_method");
        assert_eq!(step["target"]["path"],
            "src/engine/rsi_recursive_laboratory.rs");
        let mut false_birth=birth;
        false_birth["acceptedSourceHashes"]["src/engine/rsi_new_study.rs"]=
            Value::Null;
        assert!(pending_invention(&[false_birth],&frontier,&first).is_none());
    }
    #[test]
    fn unrelated_source_update_cannot_fake_return_or_scientific_rewrite(){
        let a="a".repeat(40);let b="b".repeat(40);let c="c".repeat(40);
        let mut birth=accepted("birth",&a,&b,"novel_agent",
            "src/engine/novel_agent.rs");
        birth["systemDesign"]["newComponents"]=json!([{
            "role":"novel_agent","improvesRole":"improvement_methods"}]);
        let child=accepted("develop",&b,&c,"novel_agent",
            "src/engine/novel_agent.rs");
        let unrelated=accepted("unrelated",&c,&"d".repeat(40),
            "improvement_methods","src/engine/rsi_evolve.rs");
        let frontier=vec![
            target("novel_agent","src/engine/novel_agent.rs"),
            target("improvement_methods","src/engine/rsi_meta_controller.rs"),
            target("improvement_methods","src/engine/rsi_recursive_laboratory.rs")
        ];
        assert!(pending_invention(&[birth,child,unrelated],&frontier,
            &"d".repeat(40)).is_none(),
            "A different method path cannot claim that the declared origin improved");
    }
    #[test]
    fn method_success_cannot_be_learned_from_an_unexecuted_or_unrelated_edit(){
        let focus=target("memory","src/engine/memory.rs");
        let mut g=accepted("real",&"a".repeat(40),&"b".repeat(40),
            "memory","src/engine/memory.rs");
        g["candidateId"]=json!("candidate");
        g["evaluation"]["metrics"]["holdoutEvidence"]=json!([
            {"id":"h","domain":"memory","baselinePass":false,"candidatePass":true}]);
        let science=json!({
            "source":"descendant_editable_recursive_laboratory_v1",
            "generatorCommit":"a".repeat(40),
            "method":{"name":"experiment_isolation"},
            "phase":"factorial_science","focus":[focus]});
        g["recursiveLaboratory"]=science.clone();
        assert!(!witnessed_science_trial(&g,"experiment_isolation"),
            "A plan absent from the executed system agenda cannot train method quality");
        g["systemAgenda"]["recursiveLaboratory"]=science;
        assert!(witnessed_science_trial(&g,"experiment_isolation"));
        g["changedSourcePaths"]=json!(["src/engine/tools.rs"]);
        assert!(!witnessed_science_trial(&g,"experiment_isolation"));
    }
    #[test]
    fn research_method_rank_only_counts_real_measured_candidate_trials(){
        let mut fake=json!({"recursiveLaboratory":{"method":{
            "name":"invent_compiled_subsystem"}},
            "candidateId":null,"accepted":true,"pairedRevalidated":true});
        let no=choose_method(&[fake.clone()]);
        assert_eq!(no["observedTrials"],0);
        fake["candidateId"]=json!("candidate");
        fake["selfControllerVerified"]=json!(true);
        let observed=choose_method(&[fake]);
        assert_eq!(observed["independentAdoptions"],0);
    }
}
