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
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && g["evaluation"]["evidenceSha256"].as_str().is_some_and(|v|
            v.len()==64 && v.bytes().all(|b|b.is_ascii_hexdigit()))
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
    successor_trials:u64,
    successor_adoptions:u64,
    successor_cross_domain_wins:u64,
    cross_model_scaffold_transfer_trials:u64,
    successor_holdout_gains:u64,
    successor_holdout_regressions:u64,
    successor_jobs:BTreeSet<String>,
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
                // Whole-agent improvement productivity is measured by the
                // RECOMPILED successor generating another independently
                // evaluated source change. An adopted parent patch alone
                // does not prove stronger future self-improvement.
                if let Some(accepted)=g["acceptedCommit"].as_str(){
                    for child in history.iter().filter(|next|
                        next["generatorCommit"]==accepted && eligible(next)
                            && next["generatorBinarySha256"].as_str()
                                .is_some_and(|v|v.len()==64
                                    && v.bytes().all(|b|b.is_ascii_hexdigit()))
                            && next["systemBlueprintVerified"]==true){
                        let Some(child_id)=child["jobId"].as_str()else{continue};
                        if !item.successor_jobs.insert(child_id.to_string()){continue;}
                        if identity(child)!=identity(g){
                            // The parent agent's rewritten scaffold may have
                            // helped a DIFFERENT model. That is cross-model
                            // software transfer, not evidence that the
                            // originating API's own model improved.
                            item.cross_model_scaffold_transfer_trials+=1;
                            continue;
                        }
                        item.successor_trials+=1;
                        if child["accepted"]==true
                            && child["evaluation"]["metrics"]["eligible"]==true{
                            item.successor_adoptions+=1;
                            if child["domain"]!=g["domain"]{
                                item.successor_cross_domain_wins+=1;
                            }
                        }
                        for row in child["evaluation"]["metrics"]["holdoutEvidence"]
                            .as_array().into_iter().flatten(){
                            if row["baselinePass"]==false&&row["candidatePass"]==true
                                || row["metricGains"].as_u64().unwrap_or(0)>0{
                                item.successor_holdout_gains+=1;
                            }
                            if row["baselinePass"]==true&&row["candidatePass"]==false
                                || row["metricRegressions"].as_u64().unwrap_or(0)>0{
                                item.successor_holdout_regressions+=1;
                            }
                        }
                    }
                }
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
        let offspring_rate=(s.successor_adoptions as f64+1.0)
            /(s.successor_trials as f64+2.0);
        let offspring_gain=if s.successor_trials>0{
            0.25*offspring_rate
                +0.12*(s.successor_cross_domain_wins.min(4) as f64)
                -0.12*(s.successor_holdout_regressions.min(4) as f64)
        }else{0.0};
        let score=(gains+1.0)/(gains+losses+2.0)
            +0.15*(s.adopted as f64)/(independent.max(1) as f64)
            +offspring_gain;
        json!({
            "providerId":provider,"model":model,
            "comparableCohorts":cohorts,"observedDomains":domains,
            "independentTrials":independent,
            "holdoutGains":s.verified_gains,"holdoutRegressions":s.regressions,
            "independentlyAdoptedSourceCandidates":s.adopted,
            "verifiedCompiledSuccessorTrials":s.successor_trials,
            "verifiedCompiledSuccessorAdoptions":s.successor_adoptions,
            "crossCapabilitySuccessorAdoptions":s.successor_cross_domain_wins,
            "differentModelScaffoldTransferTrials":s.cross_model_scaffold_transfer_trials,
            "successorHoldoutGains":s.successor_holdout_gains,
            "successorHoldoutRegressions":s.successor_holdout_regressions,
            "posteriorSuccessorImprovement":offspring_rate,
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

/// Adapt the actual generation-time model, not only everyday chat routing.
/// Any owner-configured capable API, local model, or approved adapter can
/// compete on the SAME committed harness and frozen paired evaluation.
/// No third-party API weights are rewritten: the whole agent is the unit
/// whose next-generation improvement productivity is measured.
pub(crate) fn generator_competition(
    state:&crate::state::AppState,history:&[Value],
    input:&Value,commit:&str,attempt:u64,allow_autonomous:bool
)->Value{
    let current_provider=input["providerId"].as_str().unwrap_or("");
    let current_model=input["model"].as_str().unwrap_or("");
    let current=(current_provider.to_owned(),current_model.to_owned());
    let mut providers=state.store.all("provider").into_iter().filter(|p|{
        crate::engine::gateway::provider_eligible(p)
            &&p["enabled"]!=false
            &&p["id"].as_str().is_some_and(|s|!s.is_empty())
            &&p["model"].as_str().is_some_and(|s|!s.is_empty())
            &&p["baseUrl"].as_str().is_some_and(|s|!s.is_empty())
            &&(crate::engine::gateway::is_local_provider(p)
                ||p["kind"]=="cli"
                ||p["apiKey"].as_str().is_some_and(|s|!s.is_empty())
                ||p["id"].as_str().is_some_and(|id|
                    state.store.has_secret(&format!("provider:{id}"))))
    }).collect::<Vec<_>>();
    providers.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str())
        .then_with(||a["model"].as_str().cmp(&b["model"].as_str())));
    providers.dedup_by(|a,b|a["id"]==b["id"]&&a["model"]==b["model"]);
    let available=providers.iter().map(|p|(
        p["id"].as_str().unwrap_or("").to_owned(),
        p["model"].as_str().unwrap_or("").to_owned()
    )).collect::<Vec<_>>();
    let model=learn(history);
    let mut chosen=current.clone();
    let mut mode="locked_or_single_provider";
    // A selected API remains the default unless the owner opted into
    // autonomous whole-system research or explicit model comparison.
    if allow_autonomous && providers.len()>=2 && available.contains(&current){
        // A independently measured, multi-cohort comparison is strong enough
        // to favor one route (not prove that its base weights improved).
        let mut trusted=providers.iter().filter_map(|p|{
            let row=model["models"].as_array()?.iter().find(|row|
                row["providerId"]==p["id"]&&row["model"]==p["model"]
                    &&row["trustedForDeepAutoRoute"]==true)?;
            Some((
                row["posteriorMeasuredImprovement"].as_f64()?,
                (p["id"].as_str()?.to_owned(),p["model"].as_str()?.to_owned())
            ))
        }).collect::<Vec<_>>();
        trusted.sort_by(|a,b|b.0.total_cmp(&a.0).then_with(||a.1.cmp(&b.1)));
        // Source-verified measurements of a generation's returned coding
        // agent are independent of its underlying API or local weights.
        let mut trials=BTreeMap::<(String,String),usize>::new();
        for generation in history.iter().filter(|g|
            g["generatorCommit"]==commit
                && eligible(g)) {
            if let Some(pair)=identity(g){*trials.entry(pair).or_default()+=1;}
        }
        let unknown=available.iter().filter(|pair|!trials.contains_key(*pair))
            .cloned().collect::<Vec<_>>();
        if attempt>1 && !unknown.is_empty() {
            // Controlled challenger on identical code: the next attempt
            // reuses the same frozen oracle. It may be accepted only after
            // its own source-changing evaluation succeeds.
            chosen=unknown.iter().find(|pair|*pair!=&current)
                .cloned().unwrap_or_else(||unknown[0].clone());
            mode="same_source_exploration";
        }else if let Some((top,leader))=trusted.first(){
            let previous=trusted.iter().find(|(_,id)|id==&current)
                .map(|x|x.0);
            if leader!=&current && previous.is_some_and(|v|*top-v>=0.12) {
                chosen=leader.clone();
                mode="independently_measured_preference";
            }else{
                mode="measured_incumbent";
            }
        }else{
            mode="no_matched_comparison_keep_incumbent";
        }
    }
    let chosen_provider=providers.iter().find(|p|
        p["id"].as_str()==Some(chosen.0.as_str())
            &&p["model"].as_str()==Some(chosen.1.as_str()));
    let mut plan=json!({
        "source":"compiled_open_model_scaffold_competition_v1",
        "generatorCommit":commit,"attempt":attempt,
        "selectedProviderId":chosen.0,
        "selectedModel":chosen.1,
        "previousProviderId":current.0,
        "previousModel":current.1,
        "decision":mode,"autonomousResearchAuthorized":allow_autonomous,
        "availableModelRoutes":providers.iter().map(|p|json!({
            "providerId":p["id"],"model":p["model"],
            "local":crate::engine::gateway::is_local_provider(p)
        })).collect::<Vec<_>>(),
        "modelEvidenceSha256":model["sha256"],
        "selectedRouteEnabled":chosen_provider.is_some(),
        "modelParametersRewritten":false,
        "successorSelfImprovementMeasured":false,
        "apiWeightsChanged":false,
        "claim":"model_scaffold_tournament_candidate_not_model_retraining"
    });
    plan["revision"]=json!(sha(&plan));plan
}
/// A configured local backend can remain in a registry after its process
/// exits. It is NOT an available model. Validate a proposed challenger at
/// the real inference endpoint before permitting a source generation.
pub(crate) async fn live_generator_competition(
    state:&crate::state::AppState,history:&[Value],
    input:&Value,commit:&str,attempt:u64,allow_autonomous:bool
)->Value{
    let mut plan=generator_competition(
        state,history,input,commit,attempt,allow_autonomous);
    let provider_id=plan["selectedProviderId"].as_str().unwrap_or("");
    let model=plan["selectedModel"].as_str().unwrap_or("");
    if plan["selectedRouteEnabled"]!=true
        || plan["selectedProviderId"]==input["providerId"]
            &&plan["selectedModel"]==input["model"] {
        return plan;
    }
    let Some(route)=state.store.get("provider",provider_id)else{return plan};
    if !crate::engine::gateway::is_local_provider(&route){return plan;}
    let Some(base)=route["baseUrl"].as_str()
        .filter(|url|(url.starts_with("http://127.0.0.1:")
            ||url.starts_with("http://localhost:"))
            &&url.ends_with("/v1"))else{
        plan["decision"]=json!("untrusted_local_endpoint_keep_incumbent");
        plan["selectedProviderId"]=input["providerId"].clone();
        plan["selectedModel"]=input["model"].clone();
        if let Some(row)=plan.as_object_mut(){row.remove("revision");}
        plan["revision"]=json!(sha(&plan));
        return plan;
    };
    let reachable=match reqwest::Client::new()
        .get(format!("{base}/models"))
        .timeout(std::time::Duration::from_secs(2)).send().await {
        Ok(reply) if reply.status().is_success()=>reply
            .json::<Value>().await.ok()
            .and_then(|j|j["data"].as_array().cloned())
            .is_some_and(|rows|rows.iter().any(|row|row["id"]==model)),
        _=>false
    };
    if !reachable{
        plan["decision"]=json!("unavailable_local_challenger_keep_incumbent");
        plan["selectedProviderId"]=input["providerId"].clone();
        plan["selectedModel"]=input["model"].clone();
        if let Some(row)=plan.as_object_mut(){row.remove("revision");}
        plan["revision"]=json!(sha(&plan));
    }
    plan
}
/// A worker can verify the actual model call receives exactly the decision
/// the parent stored; it may then learn a better decision rule in source.
pub(crate) fn competition_matches(plan:&Value,input:&Value)->bool{
    let mut data=plan.clone();
    let Some(row)=data.as_object_mut()else{return false};row.remove("revision");
    plan["source"]=="compiled_open_model_scaffold_competition_v1"
        &&plan["revision"].as_str()==Some(sha(&data).as_str())
        &&plan["generatorCommit"]==input["rsiGeneratorCommit"]
        &&plan["selectedProviderId"]==input["providerId"]
        &&plan["selectedModel"]==input["model"]
        &&plan["apiWeightsChanged"]==false
        &&plan["modelParametersRewritten"]==false
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn test_state()->(crate::state::AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!(
            "sub1-generator-compete-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        (crate::state::AppState::new(config,store,vault),dir)
    }
    #[test]
    fn open_model_competition_really_changes_next_source_generator(){
        let (state,root)=test_state();
        state.store.put("provider",&json!({
            "id":"api","model":"paid-model","enabled":true,
            "baseUrl":"https://example.test/v1","capabilities":{"text":true}}));
        state.store.put("provider",&json!({
            "id":"local","model":"local-model","enabled":true,
            "baseUrl":"http://127.0.0.1:4433/v1","local":true}));
        state.save_secret("provider:api","connected-owner-key");
        let source="a".repeat(40);
        let current=json!({"providerId":"api","model":"paid-model",
            "rsiGeneratorCommit":source});
        let first=generator_competition(&state,&[],&current,&source,1,true);
        assert_eq!(first["selectedProviderId"],"api");
        assert!(competition_matches(&first,&current));
        let second=generator_competition(&state,&[],&current,&source,2,true);
        assert_eq!(second["selectedProviderId"],"local");
        assert_eq!(second["decision"],"same_source_exploration");
        let mut next=current.clone();
        next["providerId"]=second["selectedProviderId"].clone();
        next["model"]=second["selectedModel"].clone();
        assert!(competition_matches(&second,&next));
        assert!(!competition_matches(&second,&current));
        let locked=generator_competition(&state,&[],&current,&source,2,false);
        assert_eq!(locked["selectedProviderId"],"api");
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn offline_local_model_cannot_displace_actual_api_generator(){
        let (state,root)=test_state();
        state.store.put("provider",&json!({
            "id":"api","model":"cloud","enabled":true,
            "baseUrl":"https://example.test/v1"}));
        state.store.put("provider",&json!({
            "id":"offline","model":"unused","enabled":true,
            "baseUrl":"http://127.0.0.1:9/v1","local":true}));
        state.save_secret("provider:api","configured-token");
        let commit="d".repeat(40);
        let input=json!({"providerId":"api","model":"cloud",
            "rsiGeneratorCommit":commit});
        let planned=live_generator_competition(
            &state,&[],&input,&commit,2,true).await;
        assert_eq!(planned["selectedProviderId"],"api");
        assert_eq!(planned["decision"],
            "unavailable_local_challenger_keep_incumbent");
        assert!(competition_matches(&planned,&input));
        let _=std::fs::remove_dir_all(root);
    }
    fn sample(job:&str,base:&str,domain:&str,provider:&str,good:bool)->Value{
        json!({"jobId":job,"generatorCommit":base,
            "generatorProviderId":provider,"generatorModel":"model",
            "domain":domain,"comparisonProfileSha256":"f".repeat(64),
            "changedSourcePaths":["src/engine/router.rs"],
            "pairedRevalidated":true,"selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":job,"accepted":good,
            "acceptedCommit":if good{json!("c".repeat(40))}else{Value::Null},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"e".repeat(64),
                "metrics":{"eligible":good,"holdoutEvidence":[{
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
    fn api_model_is_credited_only_for_measured_compiled_successor_capability(){
        let a="a".repeat(40);let other="b".repeat(40);
        let parent=sample("parent",&a,"memory","api",true);
        let competitor=sample("competitor",&a,"memory","local",false);
        let unrelated_parent=sample("unrelated",&other,"tools","local",false);
        let unrelated_cmp=sample("second",&other,"tools","api",true);
        let mut offspring=sample("offspring",&"c".repeat(40),
            "tools","api",true);
        offspring["generatorBinarySha256"]=json!("a".repeat(64));
        offspring["systemBlueprintVerified"]=json!(true);
        let history=vec![parent.clone(),competitor.clone(),
            unrelated_parent.clone(),unrelated_cmp.clone(),
            offspring.clone()];
        let model=learn(&history);
        let api=model["models"].as_array().unwrap().iter()
            .find(|row|row["providerId"]=="api").unwrap();
        assert_eq!(api["verifiedCompiledSuccessorTrials"],1);
        assert_eq!(api["crossCapabilitySuccessorAdoptions"],1);
        offspring["generatorBinarySha256"]=Value::Null;
        let changed=learn(&[parent,competitor,unrelated_parent,
            unrelated_cmp,offspring]);
        let api=changed["models"].as_array().unwrap().iter()
            .find(|row|row["providerId"]=="api").unwrap();
        assert_eq!(api["verifiedCompiledSuccessorTrials"],0);
    }
    #[test]
    fn code_transfer_across_models_does_not_fake_improvement_of_api_weights(){
        let base="a".repeat(40);
        let parent=sample("parent",&base,"memory","api",true);
        let competitor=sample("competitor",&base,"memory","local",false);
        let mut offspring=sample("offline-child",&"c".repeat(40),
            "tools","local",true);
        offspring["generatorBinarySha256"]=json!("1".repeat(64));
        offspring["systemBlueprintVerified"]=json!(true);
        let model=learn(&[parent,competitor,offspring]);
        let api=model["models"].as_array().unwrap().iter()
            .find(|row|row["providerId"]=="api").unwrap();
        assert_eq!(api["differentModelScaffoldTransferTrials"],1);
        assert_eq!(api["verifiedCompiledSuccessorTrials"],0);
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
