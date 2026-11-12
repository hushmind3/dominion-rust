//! Learn an execution-time model-routing policy from independent RSI outcomes.
//!
//! A model is never called "better" because it authored a successful-sounding
//! proposal. Only distinct, paired source-changing candidates evaluated from
//! the SAME generator revision, SAME frozen profile and SAME capability
//! domain can compare model choices. Such evidence updates the actual request
//! router, while explicit user model selection and free/light policy remain.
//! This is learned routing, NOT finetuning of foundation model weights.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn sha(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn identity(g:&Value)->Option<(String,String)>{
    Some((
        g["generatorProviderId"].as_str()?.to_string(),
        g["generatorModel"].as_str()?.to_string()
    ))
}
fn eligible(g:&Value)->bool{
    g["pairedRevalidated"]==true
        && g["selfControllerVerified"]==true
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            !paths.is_empty()&&paths.iter().all(|p|
                p.as_str().is_some_and(crate::engine::rsi::safe_path)))
        && g["generatorCommit"].as_str().is_some_and(|v|
            v.len()==40 && v.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["comparisonProfileSha256"].as_str().is_some_and(|v|
            v.len()==64 && v.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["domain"].as_str().is_some_and(|s|!s.is_empty())
        && identity(g).is_some()
        && g["evaluation"]["metrics"]["holdoutEvidence"].as_array()
            .is_some_and(|rows|!rows.is_empty())
}
type Cohort=(String,String,String);
fn cohort(g:&Value)->Option<Cohort>{
    if !eligible(g){return None;}
    Some((
        g["generatorCommit"].as_str()?.to_string(),
        g["comparisonProfileSha256"].as_str()?.to_string(),
        g["domain"].as_str()?.to_string()
    ))
}
#[derive(Default)]
struct Stats {
    paired_cohorts:BTreeSet<Cohort>,
    independent_jobs:BTreeSet<String>,
    verified_gains:u64,
    regressions:u64,
    adopted:u64,
    domains:BTreeSet<String>
}
/// Train an empirical capability-selection model on *comparable* trials.
/// No comparisons between different models unless their runs share an exact
/// frozen baseline and evaluation profile. A single lucky match is untrusted.
pub(crate) fn learn(history:&[Value])->Value{
    let mut cohorts=BTreeMap::<Cohort,Vec<&Value>>::new();
    for g in history{
        if let Some(key)=cohort(g){cohorts.entry(key).or_default().push(g);}
    }
    let mut stats=BTreeMap::<(String,String),Stats>::new();
    let mut comparable=0u64;
    for (cohort,rows) in &cohorts{
        let models=rows.iter().filter_map(|g|identity(g))
            .collect::<BTreeSet<_>>();
        if models.len()<2{continue;}
        comparable+=1;
        for g in rows{
            let Some(model)=identity(g)else{continue};
            let Some(job)=g["jobId"].as_str()else{continue};
            let item=stats.entry(model).or_default();
            if !item.independent_jobs.insert(job.to_string()){continue;}
            item.paired_cohorts.insert(cohort.clone());
            item.domains.insert(cohort.2.clone());
            for case in g["evaluation"]["metrics"]["holdoutEvidence"]
                .as_array().into_iter().flatten(){
                let pass=case["baselinePass"]==false&&case["candidatePass"]==true
                    ||case["metricGains"].as_u64().unwrap_or(0)>0;
                let regress=case["baselinePass"]==true&&case["candidatePass"]==false
                    ||case["metricRegressions"].as_u64().unwrap_or(0)>0;
                if pass{item.verified_gains+=1;}
                if regress{item.regressions+=1;}
            }
            if g["accepted"]==true
                && g["evaluation"]["metrics"]["eligible"]==true
                && g["acceptedCommit"].as_str().is_some_and(|v|v.len()==40){
                item.adopted+=1;
            }
        }
    }
    let rows=stats.into_iter().map(|((provider,model),s)|{
        let independent=s.independent_jobs.len();
        let cohorts=s.paired_cohorts.len();
        let domains=s.domains.len();
        // The learned posterior needs at least two independent baselines
        // AND two domains before it can alter general-purpose routing.
        let trusted=cohorts>=2&&domains>=2&&independent>=2;
        let gains=s.verified_gains as f64;
        let losses=s.regressions as f64;
        let score=(gains+1.0)/(gains+losses+2.0)
            +0.15*(s.adopted as f64)/(independent.max(1) as f64);
        json!({
            "providerId":provider,"model":model,
            "comparableCohorts":cohorts,"observedDomains":domains,
            "independentTrials":independent,
            "holdoutGains":s.verified_gains,"holdoutRegressions":s.regressions,
            "independentlyAdoptedSourceCandidates":s.adopted,
            "posteriorMeasuredImprovement":score,
            "trustedForDeepAutoRoute":trusted,
            "attribution":"observational_model_plus_harness_not_weight_causal_proof"
        })
    }).collect::<Vec<_>>();
    let mut model=json!({
        "source":"compiled_trainable_model_routing_v1",
        "comparableBaselineProfiles":comparable,
        "models":rows,
        "modelWeightsUpdated":false,
        "learnerEditableSource":"src/engine/rsi_model_learning.rs",
        "trainingOrigin":"independent_frozen_paired_harness_trials",
        "holdoutContentsNotReusedForTraining":true,
        "qualityProvenForUnseenUserTasks":false
    });
    model["sha256"]=json!(sha(&model));model
}
/// Only route matches with sufficient, shared independent evidence may
/// overrule static capability heuristics. User-selected model never calls
/// this function.
pub(crate) fn compared_preference(model:&Value,a:&Value,b:&Value)->Option<std::cmp::Ordering>{
    let find=|row:&Value|model["models"].as_array().and_then(|models|
        models.iter().find(|candidate|
            candidate["providerId"]==row["id"]
                && candidate["model"]==row["model"]
                && candidate["trustedForDeepAutoRoute"]==true));
    let a=find(a)?;let b=find(b)?;
    let pa=a["posteriorMeasuredImprovement"].as_f64()?;
    let pb=b["posteriorMeasuredImprovement"].as_f64()?;
    if (pa-pb).abs()<0.12{return None;}
    Some(pb.total_cmp(&pa))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(job:&str,base:&str,domain:&str,provider:&str,good:bool)->Value{
        json!({"jobId":job,"generatorCommit":base,
            "generatorProviderId":provider,"generatorModel":"model",
            "domain":domain,"comparisonProfileSha256":"f".repeat(64),
            "changedSourcePaths":["src/engine/router.rs"],
            "pairedRevalidated":true,"selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":job,"accepted":good,
            "acceptedCommit":if good{json!("c".repeat(40))}else{Value::Null},
            "evaluation":{"metrics":{"eligible":good,"holdoutEvidence":[{
                "id":"frozen","baselinePass":!good,"candidatePass":good,
                "metricGains":0,"metricRegressions":0
            }]}}})
    }
    #[test]
    fn comparable_cross_domain_model_learning_changes_route_preference(){
        let a="a".repeat(40);let b="b".repeat(40);
        let evidence=vec![
            sample("a1",&a,"memory","strong",true),
            sample("b1",&a,"memory","weak",false),
            sample("a2",&b,"tools","strong",true),
            sample("b2",&b,"tools","weak",false)
        ];
        let model=learn(&evidence);
        let strong=json!({"id":"strong","model":"model"});
        let weak=json!({"id":"weak","model":"model"});
        assert_eq!(compared_preference(&model,&strong,&weak),
            Some(std::cmp::Ordering::Less));
        let mut not_measured=evidence;
        not_measured[0]["pairedRevalidated"]=json!(false);
        let untrusted=learn(&not_measured);
        assert!(compared_preference(&untrusted,&strong,&weak).is_none());
    }
    #[test]
    fn fabricated_no_edit_candidate_does_not_train_model_routing(){
        let a="a".repeat(40);let b="b".repeat(40);
        let mut fabricated=sample("a1",&a,"memory","strong",true);
        fabricated["changedSourcePaths"]=json!([]);
        let model=learn(&[
            fabricated,sample("b1",&a,"memory","weak",false),
            sample("a2",&b,"tools","strong",true),
            sample("b2",&b,"tools","weak",false)
        ]);
        let strong=json!({"id":"strong","model":"model"});
        let weak=json!({"id":"weak","model":"model"});
        assert!(compared_preference(&model,&strong,&weak).is_none());
    }
    #[test]
    fn no_model_comparison_without_same_frozen_baseline(){
        let a="a".repeat(40);let b="b".repeat(40);
        let model=learn(&[
            sample("solo",&a,"memory","model-a",true),
            sample("other",&b,"memory","model-b",false)
        ]);
        assert_eq!(model["comparableBaselineProfiles"],0);
    }
}
