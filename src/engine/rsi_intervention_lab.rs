//! Source-grounded, successor-editable, resource-aware factorial experiment
//! designer for an OPEN RSI topology (including entirely invented roles).
//! The immutable evaluator owns paired measurements and promotions.
//! No observational contrast is reported as randomized causal proof.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

type Block=(String,String,bool);
type Arms=[Vec<f64>;3];
fn digest(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn hash_valid(s:&str)->bool{
    s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit())
}
fn grounded(row:&Value)->bool{
    row["role"].as_str().is_some_and(crate::engine::rsi_open_topology::valid_role)
        && row["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && row["sourceSha256"].as_str().is_some_and(hash_valid)
}
fn count_variant(g:&Value,a:&str,b:&str)->Option<usize>{
    if g["factorialMeasured"]!=true
        || g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["factorialCompliance"]["eligibleForFactorial"]!=true {
        return None;
    }
    let edited=g["changedSourcePaths"].as_array()
        .filter(|paths|!paths.is_empty()&&paths.len()<=24)?;
    let previous=g["descendantPlan"]["architectureFrontier"].as_array()?;
    let declared=&g["systemDesign"]["componentFiles"];
    let mut roles=BTreeSet::<String>::new();
    for entry in edited{
        let path=entry.as_str().filter(|p|crate::engine::rsi::safe_path(p))?;
        let mut matching=previous.iter().filter(|row|row["path"]==path)
            .filter_map(|row|row["role"].as_str())
            .filter(|role|*role==a||*role==b)
            .collect::<BTreeSet<_>>();
        // A newly invented module may be absent from the PRE-EDIT frontier.
        // It can count only when the source-bound model design assigns it
        // to exactly one tested role, and no unrelated file was changed.
        for role in [a,b]{
            if declared[role].as_array().is_some_and(|files|
                files.iter().any(|f|f.as_str()==Some(path))){
                matching.insert(role);
            }
        }
        if matching.len()!=1{return None;}
        roles.insert((*matching.iter().next()?).to_string());
    }
    if roles.len()==1 && roles.contains(a){Some(0)}
    else if roles.len()==1 && roles.contains(b){Some(1)}
    else if roles.len()==2 && roles.contains(a)&&roles.contains(b){Some(2)}
    else{None}
}
fn mean(v:&[f64])->f64{
    v.iter().sum::<f64>()/v.len().max(1) as f64
}

/// Only independently measured candidates from the identical generator SHA,
/// frozen-case profile, case id and baseline outcome form comparable blocks.
/// Rejected candidates are legitimate NEGATIVE experiments, not adoptions.
fn observation(history:&[Value],a:&str,b:&str,commit:&str,profile:&str)->Value{
    let mut counts=[0usize;3];
    let mut blocks=BTreeMap::<Block,Arms>::new();
    let mut provenance=BTreeSet::<String>::new();
    for g in history.iter().filter(|g|
        g["generatorCommit"]==commit
        && g["comparisonProfileSha256"]==profile
        && g["factorialMeasured"]==true) {
        let Some(arm)=count_variant(g,a,b) else{continue};
        let Some(id)=g["candidateId"].as_str() else{continue};
        if !provenance.insert(id.to_string()){continue;}
        let mut contributed=false;
        for case in g["evaluation"]["metrics"]["measurements"].as_array()
            .into_iter().flatten(){
            if case["holdout"]!=true {continue;}
            let (Some(name),Some(before),Some(after))=(
                case["id"].as_str().filter(|s|!s.is_empty()),
                case["baselinePass"].as_bool(),case["candidatePass"].as_bool()
            )else{continue};
            blocks.entry((name.to_string(),profile.to_string(),before))
                .or_insert_with(||[Vec::new(),Vec::new(),Vec::new()])
                [arm].push((after as i32-before as i32) as f64);
            contributed=true;
        }
        if contributed{counts[arm]+=1;}
    }
    let mut matched=Vec::new();
    for ((name,_,baseline),arms) in blocks.iter(){
        if arms.iter().any(Vec::is_empty){continue;}
        let a=mean(&arms[0]);let b=mean(&arms[1]);let both=mean(&arms[2]);
        matched.push(json!({"holdout":name,"baselinePass":baseline,
            "aOnlyDelta":a,"bOnlyDelta":b,"jointDelta":both,
            "interaction":both-a-b,
            "replicates":arms.iter().map(Vec::len).collect::<Vec<_>>() }));
    }
    let average=(!matched.is_empty()).then(||mean(&matched.iter()
        .filter_map(|r|r["interaction"].as_f64()).collect::<Vec<_>>()));
    let rows=matched.len();
    let mut evidence=json!({
        "source":"independent_matched_frozen_case_factorial_evidence_v1",
        "generatorCommit":commit,"profileSha256":profile,
        "roles":[a,b],
        "observedArms":{"aOnly":counts[0],"bOnly":counts[1],"joint":counts[2]},
        "completeFrozenCaseBlocks":rows,
        "meanInteractionContrast":average,
        "contrasts":matched.into_iter().take(16).collect::<Vec<_>>(),
        "observationalOnly":true,"causalityProven":false,
        "candidateCount":counts.iter().sum::<usize>()
    });
    evidence["sha256"]=json!(digest(&evidence));
    evidence
}

/// A general pair search over an open descendant source graph. Missing
/// counterfactuals have information value, not fake reward. The compiled
/// descendant controls this design and can rewrite this file next generation.
pub(crate) fn design(history:&[Value],frontier:&[Value],meta:&Value,
    symbiosis:&Value,allowed:&[&str],commit:&str,profile:Option<&str>)->Value{
    let profile=profile.filter(|p|hash_valid(p)).unwrap_or("");
    // No frozen profile means no independently comparable intervention.
    if profile.is_empty(){return Value::Null;}
    let primary=&meta["target"];
    let mut targets=BTreeMap::<String,Value>::new();
    for row in frontier.iter().filter(|row|grounded(row)
        && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))){
        let name=row["role"].as_str().unwrap_or("");
        targets.entry(name.to_string()).or_insert_with(||row.clone());
    }
    let mut pairs=Vec::<(Value,Value)>::new();
    if grounded(primary) && targets.contains_key(primary["role"].as_str().unwrap_or("")){
        let role=primary["role"].as_str().unwrap_or("");
        if let Some(partner)=symbiosis["complement"]["toRole"].as_str()
            .and_then(|r|targets.get(r)) {
            if role!=partner["role"]{pairs.push((primary.clone(),partner.clone()));}
        }
        for row in targets.values().filter(|row|row["role"]!=role){
            if !pairs.iter().any(|(_,b)|b["role"]==row["role"]){
                pairs.push((primary.clone(),row.clone()));
            }
        }
    }
    let mut ranked=Vec::<(f64,Value)>::new();
    for (a,b) in pairs.into_iter().take(40){
        let roles=(a["role"].as_str().unwrap_or(""),
            b["role"].as_str().unwrap_or(""));
        let evidence=observation(history,roles.0,roles.1,commit,profile);
        let counts=&evidence["observedArms"];
        let trial=[counts["aOnly"].as_u64().unwrap_or(0),
            counts["bOnly"].as_u64().unwrap_or(0),
            counts["joint"].as_u64().unwrap_or(0)];
        // The most informative missing arm is selected automatically.
        // Joint code changes are attempted only after both isolated edits
        // have actual independently measured baselines.
        let missing=trial.iter().position(|n|*n==0);
        let arm=missing.unwrap_or_else(||{
            trial.iter().enumerate().min_by_key(|(_,v)|*v)
                .map(|(i,_)|i).unwrap_or(0)
        });
        let phase=["isolate_a","isolate_b","test_joint"][arm];
        let target=if arm==1{b.clone()}else{a.clone()};
        let informative=trial.iter().filter(|n|**n>0).count() as f64;
        let balanced=1.0/(1.0+trial[arm] as f64);
        let coverage=evidence["completeFrozenCaseBlocks"].as_u64().unwrap_or(0);
        let score=0.7*informative+0.45*balanced
            +0.20*(coverage.min(8) as f64/8.0)
            +if roles.0=="improvement_methods"||roles.1=="improvement_methods"{
                0.15
            }else{0.0};
        ranked.push((score,json!({
            "a":a,"b":b,"target":target,"armIndex":arm,
            "phase":phase,"sourceIsolationRequired":arm!=2,
            "activeRoles":if arm==2{json!([roles.0,roles.1])}
                else if arm==0{json!([roles.0])}else{json!([roles.1])},
            "evidence":evidence,
            "expectedInformationGainPriority":score,
            "qualityProven":false,"causalityProven":false
        })));
    }
    ranked.sort_by(|(a,l),(b,r)|b.total_cmp(a)
        .then_with(||l["a"]["role"].as_str().cmp(&r["a"]["role"].as_str()))
        .then_with(||l["b"]["role"].as_str().cmp(&r["b"]["role"].as_str())));
    let selected=ranked.first().map(|(_,row)|row.clone()).unwrap_or(Value::Null);
    let mut result=json!({
        "source":"compiled_open_topology_counterfactual_lab_v1",
        "generatorCommit":commit,"comparisonProfileSha256":profile,
        "historyCount":history.len(),
        "selected":selected,
        "pairAlternatives":ranked.into_iter().take(6)
            .map(|(_,v)|v).collect::<Vec<_>>(),
        "independentEvaluatorOnly":true,
        "claim":"matched_observational_factorial_not_randomized_causality",
        "qualityProven":false,
        "modelMayEvolveExperimentalMethod":true
    });
    result["sha256"]=json!(digest(&result));
    result
}

/// The non-editable supervisor verifies source identity and the supplied
/// decision's receipt without rerunning an old copy of the child's planner.
pub(crate) fn validate(plan:&Value,frontier:&[Value],commit:&str,
    history_count:usize,profile:Option<&str>)->Result<(),String>{
    if plan.is_null(){return Ok(());}
    let mut clean=plan.clone();
    let map=clean.as_object_mut().ok_or("요인별 실험 계획의 형식 오류")?;
    map.remove("sha256");
    if plan["source"]!="compiled_open_topology_counterfactual_lab_v1"
        || plan["sha256"]!=digest(&clean)
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history_count as u64)
        || plan["comparisonProfileSha256"]!=profile.unwrap_or("")
        || plan["independentEvaluatorOnly"]!=true
        || plan["qualityProven"]!=false
        || plan["pairAlternatives"].as_array().is_none_or(|rows|rows.len()>6){
        return Err("후손 실험 정책의 소스·평가 조건이 올바르지 않습니다.".into());
    }
    let check=|trial:&Value|{
        let a=&trial["a"];let b=&trial["b"];
        let target=&trial["target"];
        let arm=trial["armIndex"].as_u64();
        grounded(a)&&grounded(b)&&a["role"]!=b["role"]
            && frontier.iter().any(|r|r==a) && frontier.iter().any(|r|r==b)
            && matches!(arm,Some(0|1|2))
            && target==if arm==Some(1){b}else{a}
            && trial["sourceIsolationRequired"]==(arm!=Some(2))
            && trial["qualityProven"]==false && trial["causalityProven"]==false
            && trial["evidence"]["source"]=="independent_matched_frozen_case_factorial_evidence_v1"
            && trial["evidence"]["generatorCommit"]==commit
            && trial["evidence"]["profileSha256"]==profile.unwrap_or("")
            && {
                let mut evidence=trial["evidence"].clone();
                let ev=evidence.as_object_mut();
                if let Some(ev)=ev{ev.remove("sha256");}
                trial["evidence"]["sha256"]==digest(&evidence)
            }
    };
    if !plan["pairAlternatives"].as_array().is_some_and(|rows|
        rows.iter().all(check)
        && (plan["selected"].is_null()
            || (check(&plan["selected"])&&rows.contains(&plan["selected"])))) {
        return Err("후손의 요인별 실험 대상이 실제 소스 구조와 다릅니다.".into());
    }
    Ok(())
}

/// The active edit policy does not claim successful isolation; changed
/// source paths are compared AFTER execution by the independent coordinator.
/// Model-authored source edits remain its decision; the compiled descendant
/// promotes information-gain experiments into its live meta-target choice.
pub(crate) fn steer(mut meta:Value,plan:&Value)->Value{
    let selected=&plan["selected"];
    if !selected.is_object(){return meta;}
    let target=&selected["target"];
    if !grounded(target){return meta;}
    let chosen=meta["alternatives"].as_array().into_iter().flatten()
        .find(|item|item["target"]==*target).cloned();
    let previous=meta["revision"].clone();
    meta["target"]=target.clone();
    if let Some(winner)=chosen {
        meta["selectionEvidence"]=winner.clone();
        meta["forecast"]["acceptanceProbability"]=winner["predictedAdoptionProbability"].clone();
        meta["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    }
    meta["mode"]=json!("independently_matched_factorial_experiment_design");
    meta["factorialExperimentSha256"]=plan["sha256"].clone();
    meta["revision"]=json!(digest(&json!([
        previous,plan["sha256"],target,selected["phase"]
    ])));
    meta
}

pub(crate) fn compliance(plan:&Value,changed:&[Value])->Value{
    let row=&plan["selected"];
    let actual=changed.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let a=row["a"]["path"].as_str().unwrap_or("");
    let b=row["b"]["path"].as_str().unwrap_or("");
    let phase=row["phase"].as_str().unwrap_or("");
    let success=match phase {
        "isolate_a"=>!actual.is_empty()&&actual.iter().all(|p|*p==a),
        "isolate_b"=>!actual.is_empty()&&actual.iter().all(|p|*p==b),
        "test_joint"=>actual.contains(a)&&actual.contains(b)
            &&actual.iter().all(|p|*p==a||*p==b),
        _=>false
    };
    json!({"source":"observed_changed_source_factorial_classification_v1",
        "phase":phase,"eligibleForFactorial":success,
        "changedSourcePaths":changed,
        "candidateOnly":true,"qualityProven":false})
}
pub(crate) fn instruction(plan:&Value)->String{
    let s=&plan["selected"];
    if !s.is_object(){return String::new();}
    let phase=s["phase"].as_str().unwrap_or("");
    let directive=match phase{
        "isolate_a"|"isolate_b"=>
            "Edit ONLY selected target file in this trial. Companion is a read-only counterfactual reference. If other files are changed, the experiment is not an isolated observation.",
        "test_joint"=>
            "Edit BOTH the A and B source files. The independent experiment ledger will inspect actual changed files rather than your claimed roles.",
        _=>"Inspect actual source and design an independently testable modification."
    };
    format!("[SELF-EVOLVING COUNTERFACTUAL LAB] phase={phase} score={}. \
        A={} at {}, B={} at {}. Matching frozen observations={}; \
        exact original generator={} and frozen profile={}. {} \
        The compiled next-generation planner can rewrite this lab. \
        No results are causal proof without matched independent evaluation.",
        s["expectedInformationGainPriority"],s["a"]["role"],s["a"]["path"],
        s["b"]["role"],s["b"]["path"],
        s["evidence"]["completeFrozenCaseBlocks"],
        plan["generatorCommit"],plan["comparisonProfileSha256"],directive)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn dynamic_roles_can_generate_isolated_and_joint_experiment_steps(){
        let a=json!({"role":"world_model_planner","domain":"learning",
            "path":"src/engine/world.rs","sourceSha256":"a".repeat(64)});
        let b=json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_meta_controller.rs","sourceSha256":"b".repeat(64)});
        let x=design(&[],&[a.clone(),b.clone()],&json!({"target":a}),
            &Value::Null,&["learning","improver"],&"f".repeat(40),
            Some(&"1".repeat(64)));
        assert_eq!(x["selected"]["phase"],"isolate_a");
        assert_eq!(x["selected"]["b"]["role"],"improvement_methods");
        assert_eq!(compliance(&x,&[json!("src/engine/world.rs")])["eligibleForFactorial"],true);
        assert_eq!(compliance(&x,&[json!("src/engine/world.rs"),
            json!("src/engine/rsi_meta_controller.rs")])["eligibleForFactorial"],false);
    }
    #[test]
    fn factorial_sha_attestation_requires_exact_source_and_profile(){
        let a=json!({"role":"algorithms","domain":"improver",
            "path":"src/engine/algorithms.rs","sourceSha256":"a".repeat(64)});
        let b=json!({"role":"tools","domain":"tools",
            "path":"src/engine/tools.rs","sourceSha256":"b".repeat(64)});
        let commit="f".repeat(40);let profile="1".repeat(64);
        let plan=design(&[],&[a.clone(),b.clone()],&json!({"target":a.clone()}),
            &Value::Null,&["improver","tools"],&commit,Some(&profile));
        assert!(validate(&plan,&[a.clone(),b.clone()],&commit,0,Some(&profile)).is_ok());
        let mut corrupted=plan.clone();
        corrupted["selected"]["a"]["sourceSha256"]=json!("c".repeat(64));
        assert!(validate(&corrupted,&[a,b],&commit,0,Some(&profile)).is_err());
        assert!(validate(&plan,&[],&commit,0,Some(&profile)).is_err());
    }
    #[test]
    fn matched_evaluation_cannot_mix_different_generator_commits(){
        let a="algorithms";let b="tools";let profile="a".repeat(64);
        let row=json!({"id":"trial-1","candidateId":"c1","generatorCommit":"b".repeat(40),
            "comparisonProfileSha256":profile,"factorialMeasured":true,
            "factorialCompliance":{"eligibleForFactorial":true},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "changedSourcePaths":["src/engine/algorithms.rs"],
            "descendantPlan":{"architectureFrontier":[{"role":a,"path":"src/engine/algorithms.rs"}]},
            "evaluation":{"metrics":{"measurements":[{"id":"h1","holdout":true,
                "baselinePass":false,"candidatePass":true}]}}});
        let mismatched=observation(&[row],a,b,&"c".repeat(40),&profile);
        assert_eq!(mismatched["observedArms"]["aOnly"],0);
    }
}
