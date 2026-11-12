//! Evidence-conditioned model ecology for real, rebuilt RSI generations.
//!
//! Configured API, local and future model providers are treated as *distinct*
//! inference endpoints. This does NOT fine-tune an API's proprietary weights.
//! The mutable descendant controller learns which actual model should author
//! its next source patch; the ordinary frozen paired evaluator, not an LLM
//! claim or local LoRA's self-test, supplies the outcome.
//!
//! This source can itself be rewritten by an independently adopted descendant.
//! No owner model choice is overridden without opt-in; only already configured
//! and enabled endpoints may receive inference requests.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn sha(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn identifier(s:&str)->bool{
    !s.is_empty()&&s.len()<=192
        && !s.chars().any(|c|c.is_control()||c=='\n'||c=='\r')
}
fn endpoint(v:&Value)->Option<(String,String)>{
    let id=v["providerId"].as_str().filter(|s|identifier(s))?;
    let model=v["model"].as_str().filter(|s|identifier(s))?;
    Some((id.to_owned(),model.to_owned()))
}
fn endpoint_value(key:&(String,String))->Value{
    json!({"providerId":key.0,"model":key.1})
}
fn source_bound_candidate(g:&Value)->bool{
    g["modelAttributionConsistent"]==true
        && g["modelAttribution"]["verifiedHomogeneous"]==true
        && g["modelAttribution"]["providerId"]==g["generatorProviderId"]
        && g["modelAttribution"]["model"]==g["generatorModel"]
        && g["modelAttribution"]["calls"]==g["tokenUsage"]["calls"]
        && g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["acceptedSourceHashes"].as_object().is_some_and(|m|
            !m.is_empty()&&m.values().all(|h|h.as_str().is_some_and(|s|
                s.len()==64&&s.bytes().all(|c|c.is_ascii_hexdigit()))))
        && g["generatorBinarySha256"].as_str().is_some_and(|s|!s.is_empty())
}
fn comparable(g:&Value, commit:&str, profile:&str, domain:&str, objective:&str)->bool{
    g["modelAttributionConsistent"]==true
        && g["modelAttribution"]["verifiedHomogeneous"]==true
        && g["modelAttribution"]["providerId"]==g["generatorProviderId"]
        && g["modelAttribution"]["model"]==g["generatorModel"]
        && g["modelAttribution"]["calls"]==g["tokenUsage"]["calls"]
        && g["generatorCommit"]==commit
        && g["comparisonProfileSha256"]==profile
        && g["objectiveId"]==objective
        && g["domain"]==domain
        && g["systemEvolution"]==true
        && g["generatorBinarySha256"].as_str().is_some_and(|s|!s.is_empty())
        && g["nativeMutation"].is_null()
        && g["generatorProviderId"].as_str().is_some_and(identifier)
        && g["generatorModel"].as_str().is_some_and(identifier)
        && (!g["modelRoute"].is_object()
            || (g["modelRouteExecuted"]==true
                && g["modelRoute"]["selected"]["providerId"]==g["generatorProviderId"]
                && g["modelRoute"]["selected"]["model"]==g["generatorModel"]))
}
#[derive(Default)]
struct Trial {
    attempts:u64,
    paired:u64,
    accepted:u64,
    gains:u64,
    regressions:u64,
    tokens:u64,
    observedChildren:u64,
    productiveChildren:u64,
}
impl Trial{
    fn update(&mut self,g:&Value,history:&[Value]){
        self.attempts+=1;
        self.tokens+=g["tokens"].as_u64().unwrap_or(0);
        if g["pairedRevalidated"]==true{
            self.paired+=1;
            self.gains+=g["evaluation"]["metrics"]["improvedHoldouts"]
                .as_u64().unwrap_or(0).min(128);
            self.regressions+=g["evaluation"]["metrics"]["regressions"]
                .as_u64().unwrap_or(0).min(128);
        }
        if source_bound_candidate(g){
            self.accepted+=1;
            // Real downstream productivity is a second, separate learning
            // signal. Only a rebuilt binary's independent, adopted task
            // improvement can credit the parent model selection.
            if let Some(head)=g["acceptedCommit"].as_str() {
                for child in history.iter().filter(|child|
                    child["generatorCommit"].as_str()==Some(head)
                    && child["generatorBinarySha256"].as_str().is_some_and(|h|
                        !h.is_empty())
                    && child["generatorBinarySha256"]!=g["generatorBinarySha256"]
                    && child["jobId"]!=g["jobId"]) {
                    self.observedChildren+=1;
                    if source_bound_candidate(child)
                        && child["evaluation"]["metrics"]["improvedHoldouts"]
                            .as_u64().unwrap_or(0)>0 {
                        self.productiveChildren+=1;
                    }
                }
            }
        }
    }
    fn quality(&self)->f64{
        // Penalize build/no-patch failures and verified regressions, not
        // untested prose. This is *source generation efficacy*, never
        // direct evidence that a local model outperforms an external API.
        ((self.accepted as f64)+0.5*self.gains as f64
            -0.9*self.regressions as f64
            -0.20*(self.attempts-self.accepted) as f64)
            /(self.attempts+2) as f64
            +0.35*(self.productiveChildren as f64)
                /(self.observedChildren as f64+2.0)
    }
    fn evidence(&self)->Value{
        json!({"attempts":self.attempts,"independentPairedRuns":self.paired,
            "sourceAdoptions":self.accepted,"aggregateHoldoutGains":self.gains,
            "aggregateRegressions":self.regressions,"tokensSpent":self.tokens,
            "observedRebuiltDescendants":self.observedChildren,
            "productiveRebuiltDescendants":self.productiveChildren,
            "observedCodeGenerationUtility":self.quality()})
    }
}
/// Pure selection law, compiled into the new descendant. The complete
/// comparison cohort is pinned to one source revision, objective, task domain
/// and frozen evaluation suite. A new provider first enters *exploration*;
/// it cannot silently acquire a "better model" verdict from unpaired runs.
pub(crate) fn decide(
    history:&[Value],baseline:&Value,options:&[Value],
    commit:&str,profile:&str,domain:&str,objective:&str,
)->Value{
    let Some(original)=endpoint(baseline) else{return Value::Null};
    let mut pool=BTreeMap::<(String,String),Trial>::new();
    pool.insert(original.clone(),Trial::default());
    for choice in options{
        if let Some(key)=endpoint(choice){pool.entry(key).or_default();}
    }
    if pool.len()<2{return Value::Null;}
    let mut raw=0_u64;
    for g in history{
        if !comparable(g,commit,profile,domain,objective){continue;}
        raw+=1;
        let key=(g["generatorProviderId"].as_str().unwrap().to_owned(),
            g["generatorModel"].as_str().unwrap().to_owned());
        if let Some(trial)=pool.get_mut(&key){trial.update(g,history);}
    }
    // Historical same-task/same-frozen-suite generations from OTHER real
    // source revisions supply transferable hypotheses, not "matched"
    // proof. Learning can survive a successful source commit changing HEAD.
    let mut transfer=BTreeMap::<(String,String),Trial>::new();
    for key in pool.keys(){transfer.insert((*key).clone(),Trial::default());}
    for g in history {
        if g["generatorCommit"]==commit
            || g["comparisonProfileSha256"]!=profile
            || g["objectiveId"]!=objective
            || g["domain"]!=domain
            || g["systemEvolution"]!=true
            || g["generatorCommit"].as_str().is_none_or(str::is_empty)
            || g["generatorBinarySha256"].as_str().is_none_or(str::is_empty)
            || !g["nativeMutation"].is_null() {continue;}
        let (Some(id),Some(model))=(g["generatorProviderId"].as_str(),
            g["generatorModel"].as_str()) else{continue};
        if let Some(t)=transfer.get_mut(&(id.to_string(),model.to_string())){
            t.update(g,history);
        }
    }
    let baseline_score=pool.get(&original).map(Trial::quality).unwrap_or(0.0);
    let baseline_evidence=pool.get(&original).map(Trial::evidence)
        .unwrap_or(Value::Null);
    let baseline_runs=pool.get(&original).map(|t|t.paired).unwrap_or(0);
    let mut preferred=original.clone();
    let mut confirmed=false;
    // "Promotion" is justified only by matched *source generation* trials
    // in a shared frozen profile. No weight-training claim follows.
    for (key,trial) in &pool {
        if *key==original || trial.paired<2 || baseline_runs<2 {continue;}
        if trial.accepted < 1 || trial.gains <= trial.regressions {continue;}
        if trial.quality() <= baseline_score+0.25 {continue;}
        if !confirmed || trial.quality()>
            pool.get(&preferred).map(Trial::quality).unwrap_or(-1e10) {
            preferred=(*key).clone();
            confirmed=true;
        }
    }
    let mut mode="evidence_based_model_routing";
    if !confirmed{
        // When a newly accepted descendant changes source HEAD, do not
        // reset learned model choice to a frozen baseline forever. Prior
        // independent outcomes over another code revision can nominate a
        // *provisional* challenger; a new matched A/B cohort must still be
        // measured before calling it a better source-generation model.
        if raw==0{
            let inherited=transfer.iter().filter(|(_,trial)|
                trial.paired>=2 && trial.accepted>=1
                && trial.gains>trial.regressions)
                .max_by(|a,b|a.1.quality().total_cmp(&b.1.quality())
                    .then_with(||b.0.cmp(a.0)));
            if let Some((key,_))=inherited{
                preferred=(*key).clone();
                mode="cross_revision_transfer_hypothesis_not_proven";
            }
        }
        // Actively measure the existing baseline before testing alternatives,
        // then fill the least-observed arm. The next generation will see
        // the REAL result, not a router's invented forecast.
        let key=pool.iter().min_by(|a,b|
            a.1.attempts.cmp(&b.1.attempts)
                .then_with(||{
                    if *a.0==original {std::cmp::Ordering::Less}
                    else if *b.0==original {std::cmp::Ordering::Greater}
                    else{a.0.cmp(b.0)}
                })).map(|(key,_)|(*key).clone()).unwrap_or(original.clone());
        if mode!="cross_revision_transfer_hypothesis_not_proven"{
            preferred=key;
            mode="paired_exploration_not_quality_proof";
        }
    }
    let selected_evidence=pool.get(&preferred)
        .map(Trial::evidence).unwrap_or(Value::Null);
    let list=pool.iter().map(|(key,trial)|{
        json!({"endpoint":endpoint_value(key),"evidence":trial.evidence(),
            "historicalTransferPrior":transfer.get(key)
                .map(Trial::evidence).unwrap_or(Value::Null),
            "matchedFrozenCohort":true})
    }).collect::<Vec<_>>();
    let mut receipt=json!({
        "source":"compiled_descendant_evidence_model_ecology_v1",
        "generatorCommit":commit,"comparisonProfileSha256":profile,
        "objectiveId":objective,"domain":domain,
        "baseline":endpoint_value(&original),
        "selected":endpoint_value(&preferred),
        "selectionMode":mode,
        "sourceGenerationQualityPromoted":confirmed,
        "foundationWeightsModified":false,
        "externalApiWeightsModified":false,
        "providerPool":list,
        "matchedAttemptCount":raw,
        "baselineEvidence":baseline_evidence,
        "selectedEvidence":selected_evidence,
        "observableOutcome":"independently_evaluated_source_generation",
        "evaluationAuthority":"frozen_independent_worktree_evaluator"
    });
    receipt["revision"]=json!(sha(&receipt));
    receipt
}
/// Consume already configured endpoints only. A *new* model/provider may
/// enter the pool as soon as the owner configures it in the live provider
/// registry; it does not require adding a hard-coded Rust vendor rule.
pub(crate) fn choose(
    state:&AppState,input:&Value,history:&[Value],commit:&str,
    domain:&str,objective:&str
)->Value{
    if input["rsiAdaptiveModelSelection"]!=true
        || input["rsiSystemEvolution"]!=true{return Value::Null;}
    let baseline=json!({
        "providerId":input["rsiOriginalProviderId"]
            .as_str().or_else(||input["providerId"].as_str()),
        "model":input["rsiOriginalModel"]
            .as_str().or_else(||input["model"].as_str())
    });
    let Some(original)=endpoint(&baseline) else{return Value::Null};
    let mut pool=Vec::<Value>::new();
    let mut seen=BTreeSet::new();
    // Explicit candidate list is a strict allowlist when supplied. If
    // absent, the user opted in to the configured enabled provider pool.
    let listed=input["rsiModelCandidates"].as_array();
    let candidates=if let Some(list)=listed{
        list.to_vec()
    }else{
        state.store.all("provider").into_iter()
            .filter(|p|p["enabled"]!=false)
            .map(|p|json!({"providerId":p["id"],"model":p["model"]}))
            .collect()
    };
    for candidate in candidates{
        let Some(key)=endpoint(&candidate) else{continue};
        if !seen.insert(key.clone()){continue;}
        if key==original{
            pool.push(endpoint_value(&key));continue;
        }
        let Some(provider)=state.store.get("provider",&key.0)else{continue};
        if provider["enabled"]==false || provider["model"].as_str()!=Some(key.1.as_str()){
            continue;
        }
        pool.push(endpoint_value(&key));
    }
    let Some(profile)=crate::engine::rsi_meta_controller::comparison_profile(input)
        else{return Value::Null};
    decide(history,&baseline,&pool,commit,&profile,domain,objective)
}
/// Post-evaluation evidence available to the next rebuilt model ecology.
/// The model's training or endpoint quality is not claimed from a source
/// improvement; independent outcomes are retained as separate observations.
pub(crate) fn observation(g:&Value)->Value{
    let route=&g["modelRoute"];
    let actual=route.is_object()&&g["modelRouteExecuted"]==true
        && route["selected"]["providerId"]==g["generatorProviderId"]
        && route["selected"]["model"]==g["generatorModel"];
    json!({
        "source":"executed_model_ecology_observation_v1",
        "routeRevision":route["revision"],
        "routeActuallyUsedForInference":actual,
        "generatorProviderId":g["generatorProviderId"],
        "generatorModel":g["generatorModel"],
        "pairedRevalidated":g["pairedRevalidated"]==true,
        "sourceCandidateAdopted":actual&&source_bound_candidate(g),
        "improvedHoldouts":if actual&&g["pairedRevalidated"]==true{
            g["evaluation"]["metrics"]["improvedHoldouts"].clone()
        }else{Value::Null},
        "regressions":if actual&&g["pairedRevalidated"]==true{
            g["evaluation"]["metrics"]["regressions"].clone()
        }else{Value::Null},
        "foundationWeightsChanged":false,
        "externalApiWeightsChanged":false,
        "fullIntelligenceModelSuperiorityProven":false
    })
}
/// An older supervisor may check only IMMUTABLE facts. It must not
/// recompute or impose its own outdated model-ranking algorithm on a newer
/// compiled descendant. Dynamic route policy is selected by the new binary.
pub(crate) fn valid_facts(
    state:&AppState,input:&Value,route:&Value,commit:&str,
    domain:&str,objective:&str,
)->bool{
    if route.is_null(){return true;}
    if input["rsiAdaptiveModelSelection"]!=true
        || input["rsiSystemEvolution"]!=true{return false;}
    let Some(original)=endpoint(&json!({
        "providerId":input["rsiOriginalProviderId"]
            .as_str().or_else(||input["providerId"].as_str()),
        "model":input["rsiOriginalModel"]
            .as_str().or_else(||input["model"].as_str())
    })) else{return false};
    let Some(selected)=endpoint(&route["selected"]) else{return false};
    if route["source"].as_str().is_none_or(|s|
        !s.starts_with("compiled_descendant_"))
        || route["generatorCommit"]!=commit
        || route["domain"]!=domain
        || route["objectiveId"]!=objective
        || route["baseline"]!=endpoint_value(&original)
        || route["comparisonProfileSha256"].as_str() !=
            crate::engine::rsi_meta_controller::comparison_profile(input).as_deref()
        || route["evaluationAuthority"]!="frozen_independent_worktree_evaluator" {
        return false;
    }
    let mut unsigned=route.clone();
    let Some(object)=unsigned.as_object_mut() else{return false};
    object.remove("revision");
    if route["revision"].as_str()!=Some(sha(&unsigned).as_str()) {
        return false;
    }
    if selected==original{return true;}
    let Some(provider)=state.store.get("provider",&selected.0) else{return false};
    if provider["enabled"]==false
        || provider["model"].as_str()!=Some(selected.1.as_str()){
        return false;
    }
    if let Some(allowlist)=input["rsiModelCandidates"].as_array(){
        allowlist.iter().any(|candidate|endpoint(candidate).as_ref()==Some(&selected))
    }else{true}
}
/// Verify that the route consumed by the model-call worker was actually
/// selected by the current compiled ecology and source-bound evidence.
pub(crate) fn valid_receipt(
    state:&AppState,input:&Value,history:&[Value],commit:&str,
    domain:&str,objective:&str
)->bool{
    let route=&input["rsiModelRoute"];
    if route.is_null(){
        return input["rsiAdaptiveModelSelection"]!=true
            || choose(state,input,history,commit,domain,objective).is_null();
    }
    let expected=choose(state,input,history,commit,domain,objective);
    expected==*route && route["generatorCommit"]==commit
        && route["selected"]["providerId"]==input["providerId"]
        && route["selected"]["model"]==input["model"]
}
#[cfg(test)]
mod tests{
    use super::*;
    fn ep(id:&str)->Value{json!({"providerId":id,"model":id})}
    fn sample(provider:&str,accepted:bool,gains:u64)->Value{
        json!({"generatorCommit":"c","comparisonProfileSha256":"p",
            "objectiveId":"o","domain":"learning","systemEvolution":true,
            "generatorBinarySha256":"binary","generatorProviderId":provider,
            "generatorModel":provider,"nativeMutation":null,
            "tokenUsage":{"calls":1},"modelAttributionConsistent":true,
            "modelAttribution":{"verifiedHomogeneous":true,
                "providerId":provider,"model":provider,"calls":1},
            "pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "accepted":accepted,
            "acceptedSourceHashes":if accepted{
                json!({"src/engine/learning.rs":"f".repeat(64)})
            }else{json!({})},
            "evaluation":{"metrics":{"eligible":true,
                "improvedHoldouts":gains,"regressions":0}}})
    }
    #[test]
    fn initially_probe_baseline_then_registered_challenger(){
        let a=ep("api");
        let b=ep("native");
        let first=decide(&[],&a,&[b.clone()],"c","p","learning","o");
        assert_eq!(first["selected"],a);
        assert_eq!(first["sourceGenerationQualityPromoted"],false);
        let second=decide(&[sample("api",false,0)],&a,&[b],
            "c","p","learning","o");
        assert_eq!(second["selected"],ep("native"));
        assert_eq!(second["selectionMode"],"paired_exploration_not_quality_proof");
    }
    #[test]
    fn independently_paired_source_edits_change_real_selected_model(){
        let a=ep("api");
        let b=ep("new_model");
        let history=vec![sample("api",false,0),
            sample("new_model",true,4),sample("api",false,0),
            sample("new_model",true,4)];
        let plan=decide(&history,&a,&[b.clone()],"c","p","learning","o");
        assert_eq!(plan["selected"],b);
        assert_eq!(plan["sourceGenerationQualityPromoted"],true);
        assert_eq!(plan["externalApiWeightsModified"],false);
    }
    #[test]
    fn a_rebuilt_descendant_inherits_candidate_prior_without_fake_proof(){
        let a=ep("api");let b=ep("local");
        let mut history=vec![sample("api",false,0),
            sample("local",true,5),sample("local",true,5)];
        for row in &mut history{row["generatorCommit"]=json!("old");}
        let candidate=decide(&history,&a,&[b.clone()],"new","p","learning","o");
        assert_eq!(candidate["selected"],b);
        assert_eq!(candidate["selectionMode"],
            "cross_revision_transfer_hypothesis_not_proven");
        assert_eq!(candidate["sourceGenerationQualityPromoted"],false);
    }
    #[test]
    fn different_revision_or_suite_never_promotes_a_model(){
        let a=ep("api");
        let b=ep("new_model");
        let mut history=vec![sample("new_model",true,5),
            sample("new_model",true,5),sample("api",false,0),
            sample("api",false,0)];
        history[0]["generatorCommit"]=json!("other");
        history[1]["comparisonProfileSha256"]=json!("other");
        let plan=decide(&history,&a,&[b],"c","p","learning","o");
        assert_eq!(plan["sourceGenerationQualityPromoted"],false);
    }
    #[test]
    fn proposed_unpaired_fine_tuning_does_not_count_as_a_better_model(){
        let a=ep("api");let b=ep("lora");
        let mut history=vec![sample("api",false,0),sample("api",false,0)];
        for _ in 0..4{
            let mut row=sample("lora",true,10);
            row["pairedRevalidated"]=json!(false);
            history.push(row);
        }
        let plan=decide(&history,&a,&[b],"c","p","learning","o");
        assert_eq!(plan["sourceGenerationQualityPromoted"],false);
        assert_eq!(plan["externalApiWeightsModified"],false);
    }
}
