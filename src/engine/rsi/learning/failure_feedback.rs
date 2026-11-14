//! Evidence-grounded RSI error gradients, inspired by GEPA's actionable
//! evaluator feedback. This is NOT model-weight learning or a claim that
//! scalar evaluation alone identifies a causal defect.
//! The input must already be replayed by rsi_system::revalidated_history.
use serde_json::{json,Value};
use std::collections::BTreeMap;

fn measured(g:&Value)->bool{
    // replay.rs revalidates the original paired candidate receipt even when
    // the candidate was rejected. pairedRevalidated means ADOPTED, not
    // independently evaluated; requiring it would erase failed experiments.
    g["pairedMeasured"]==true
        && g["pairedReceiptReplayed"]==true
        && g["evaluation"]["metrics"]["measurements"].is_array()
        && g["generatorCommit"].as_str().is_some_and(|x|
            x.len()==40 && x.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn signal(metric:&Value)->Option<(&'static str,f64)>{
    let baseline=metric["baseline"].as_f64()?;
    let candidate=metric["candidate"].as_f64()?;
    if !baseline.is_finite()||!candidate.is_finite(){return None;}
    let direction=metric["direction"].as_str()?;
    let signed=match direction{
        "higher"=>candidate-baseline,
        "lower"=>baseline-candidate,
        _=>return None,
    };
    if metric["regressed"]==true && signed < -1e-9{
        return Some(("regression",(-signed).min(1000.0)));
    }
    if metric["improved"]==true && signed>1e-9 {return None;}
    // An unchanged, already solved test is not a useful mutation target.
    if signed.abs()<=1e-9 && metric["key"]=="quality"
        && direction=="higher" && candidate>=1.0-1e-9 {return None;}
    if signed.abs()<=1e-9 && candidate>=0.0 {
        return Some(("stagnation",if direction=="higher"{
            (1.0-candidate).clamp(0.0,1.0)
        }else{0.5}));
    }
    None
}
fn hint(kind:&str)->&'static str {
    match kind{
        "regression"=>
            "Identify the changed source path causing this measured regression; isolate the smallest repair and rerun the frozen pair.",
        _=>
            "The measured baseline and candidate are unchanged; change the implementation hypothesis, not the grading code or example text.",
    }
}

/// Choose at most four nonduplicate per-case counterexamples across recent
/// independently revalidated generations. A regression outranks a stagnant
/// already-known case. Never call this evidence of a root cause or completion.
pub(crate) fn reflective_feedback(history:&[Value],domain:&str)->Value{
    let mut distinct=BTreeMap::<(String,String,String),Value>::new();
    for generation in history.iter().rev().filter(|g|measured(g)).take(20){
        for case in generation["evaluation"]["metrics"]["measurements"]
            .as_array().into_iter().flatten(){
            let Some(id)=case["id"].as_str().filter(|s|!s.is_empty())else{continue};
            let case_domain=case["domain"].as_str()
                .or_else(||generation["domain"].as_str()).unwrap_or("");
            if case_domain!=domain{continue;}
            for metric in case["metrics"].as_array().into_iter().flatten(){
                let Some(key)=metric["key"].as_str().filter(|s|!s.is_empty())else{continue};
                let Some((kind,severity))=signal(metric)else{continue};
                let identity=(case_domain.to_string(),id.to_string(),key.to_string());
                distinct.entry(identity).or_insert_with(||json!({
                    "generationId":generation["id"],
                    "generatorCommit":generation["generatorCommit"],
                    "candidateId":generation["candidateId"],
                    "caseId":id,"domain":case_domain,"metric":key,
                    "classification":kind,
                    "baseline":metric["baseline"],
                    "candidate":metric["candidate"],
                    "direction":metric["direction"],
                    "severity":severity,
                    "changedSourcePaths":generation["changedSourcePaths"]
                        .as_array().map(|paths|paths.iter().take(4).cloned().collect::<Vec<_>>()),
                    "hypothesis":hint(kind)
                }));
            }
        }
    }
    let mut signals=distinct.into_values().collect::<Vec<_>>();
    signals.sort_by(|a,b|{
        (b["classification"]=="regression").cmp(&(a["classification"]=="regression"))
            .then_with(||b["severity"].as_f64().unwrap_or(0.0)
                .total_cmp(&a["severity"].as_f64().unwrap_or(0.0)))
            .then_with(||a["caseId"].as_str().cmp(&b["caseId"].as_str()))
    });
    signals.truncate(4);
    // Successful *adopted* modifications are useful counterfactual context:
    // retrieve only separately revalidated, eligible improvements in this
    // domain. Never teach from an unadopted patch, model assertion or fixture.
    // This is bounded case evidence, not a claim of transferable causality.
    let mut successful_cases=std::collections::BTreeSet::new();
    let mut prior_successes=Vec::new();
    for generation in history.iter().rev().filter(|g|measured(g)).take(20) {
        if generation["accepted"]!=true
            || generation["pairedRevalidated"]!=true
            || generation["evaluation"]["metrics"]["eligible"]!=true
            || !generation["acceptedCommit"].as_str()
                .is_some_and(|hash|hash.len()==40
                    && hash.bytes().all(|byte|byte.is_ascii_hexdigit())) {
            continue;
        }
        for case in generation["evaluation"]["metrics"]["measurements"]
            .as_array().into_iter().flatten() {
            let Some(case_id)=case["id"].as_str().filter(|s|!s.is_empty()) else {continue};
            let case_domain=case["domain"].as_str()
                .or_else(||generation["domain"].as_str()).unwrap_or("");
            if case_domain!=domain || successful_cases.contains(case_id) {continue;}
            let gain=case["metrics"].as_array().into_iter().flatten().find(|metric|{
                let Some(baseline)=metric["baseline"].as_f64() else{return false};
                let Some(candidate)=metric["candidate"].as_f64() else{return false};
                if !baseline.is_finite() || !candidate.is_finite()
                    || metric["improved"]!=true {return false;}
                match metric["direction"].as_str() {
                    Some("higher")=>candidate>baseline+1e-9,
                    Some("lower")=>candidate<baseline-1e-9,
                    _=>false
                }
            });
            let Some(metric)=gain else {continue};
            successful_cases.insert(case_id.to_string());
            prior_successes.push(json!({
                "caseId":case_id,"domain":domain,"metric":metric["key"],
                "baseline":metric["baseline"],"candidate":metric["candidate"],
                "direction":metric["direction"],
                "acceptedCommit":generation["acceptedCommit"],
                "sourcePaths":generation["changedSourcePaths"].as_array()
                    .map(|paths|paths.iter().take(4).cloned().collect::<Vec<_>>()),
                "transferVerified":false
            }));
            if prior_successes.len()>=2 {break}
        }
        if prior_successes.len()>=2 {break}
    }
    json!({
        "source":"independent_paired_counterexample_gradient_v1",
        "targetDomain":domain,
        "signals":signals,
        "priorSuccesses":prior_successes,
        "origin":"replayed_oracle_metrics_only",
        "rootCauseProven":false,
        "qualityImprovementClaimed":false
    })
}

#[cfg(test)]
mod tests{
 use super::*;
 fn gen(id:&str,commit:&str,case:&str,key:&str,a:f64,b:f64,regressed:bool)->Value{
    json!({"id":id,"generatorCommit":commit,"domain":"tools",
        "pairedMeasured":true,"pairedRevalidated":true,
        "pairedReceiptReplayed":true,
        "evaluation":{"metrics":{"measurements":[{"id":case,"domain":"tools",
            "metrics":[{"key":key,"direction":"higher",
                "baseline":a,"candidate":b,"improved":b>a,"regressed":regressed}]}]}}})
 }
 #[test]
 fn measured_regression_outvotes_zero_gain_and_is_bounded(){
    let hash="a".repeat(40);
    let a=gen("early",&hash,"a","quality",0.6,0.6,false);
    let b=gen("later",&hash,"b","quality",0.9,0.5,true);
    let report=reflective_feedback(&[a,b],"tools");
    assert_eq!(report["signals"][0]["caseId"],"b");
    assert_eq!(report["signals"][0]["classification"],"regression");
    assert_eq!(report["rootCauseProven"],false);
 }
 #[test]
 fn forged_and_satisfied_cases_do_not_influence_next_generation(){
    let hash="b".repeat(40);
    let mut forged=gen("forged",&hash,"fake","quality",0.2,0.0,true);
    forged["pairedReceiptReplayed"]=json!(false);
    let solved=gen("solved",&hash,"done","quality",1.0,1.0,false);
    let report=reflective_feedback(&[forged,solved],"tools");
    assert_eq!(report["signals"].as_array().unwrap().len(),0);
 }
 #[test]
 fn newest_evidence_deduplicates_old_trials_and_wrong_domains(){
    let hash="c".repeat(40);
    let old=gen("g1",&hash,"x","quality",0.3,0.3,false);
    let new=gen("g2",&hash,"x","quality",0.8,0.8,false);
    let report=reflective_feedback(&[old,new],"tools");
    assert_eq!(report["signals"].as_array().unwrap().len(),1);
    assert_eq!(report["signals"][0]["candidate"],0.8);
    assert_eq!(reflective_feedback(&[gen("g3",&hash,"x","quality",0.5,0.1,true)],
        "memory")["signals"].as_array().unwrap().len(),0);
 }
 
 #[test]
 fn noisy_unmeasured_tail_does_not_hide_older_verified_regression(){
    let hash="d".repeat(40);
    let meaningful=gen("measured",&hash,"case","quality",0.8,0.4,true);
    let noise=(0..25).map(|_|json!({"pairedMeasured":false}))
        .collect::<Vec<_>>();
    let mut history=vec![meaningful];history.extend(noise);
    let result=reflective_feedback(&history,"tools");
    assert_eq!(result["signals"][0]["caseId"],"case");
 }
 #[test]
 fn rejected_but_independently_replayed_regression_teaches_next_descendant(){
    let hash="f".repeat(40);
    let mut rejected=gen("rejected",&hash,"broken","quality",0.9,0.2,true);
    rejected["pairedRevalidated"]=json!(false);
    rejected["accepted"]=json!(false);
    let report=reflective_feedback(&[rejected],"tools");
    assert_eq!(report["signals"][0]["caseId"],"broken");
    assert_eq!(report["signals"][0]["classification"],"regression");
    assert_eq!(report["priorSuccesses"].as_array().unwrap().len(),0);
 }
 #[test]
 fn retrieves_only_independently_validated_adopted_successes(){
    let hash="e".repeat(40);
    let mut accepted=gen("success",&hash,"passed","quality",0.2,0.7,false);
    accepted["accepted"]=json!(true);
    accepted["acceptedCommit"]=json!("f".repeat(40));
    accepted["evaluation"]["metrics"]["eligible"]=json!(true);
    accepted["changedSourcePaths"]=json!(["src/engine/algorithms.rs"]);
    let mut unadopted=gen("draft",&hash,"fake","quality",0.1,0.9,false);
    unadopted["accepted"]=json!(false);
    unadopted["acceptedCommit"]=json!("a".repeat(40));
    unadopted["evaluation"]["metrics"]["eligible"]=json!(true);
    let result=reflective_feedback(&[accepted.clone(),unadopted],"tools");
    assert_eq!(result["priorSuccesses"].as_array().unwrap().len(),1);
    assert_eq!(result["priorSuccesses"][0]["caseId"],"passed");
    assert_eq!(result["priorSuccesses"][0]["transferVerified"],false);
    accepted["pairedRevalidated"]=json!(false);
    let empty=reflective_feedback(&[accepted],"tools");
    assert_eq!(empty["priorSuccesses"].as_array().unwrap().len(),0);
 }

}