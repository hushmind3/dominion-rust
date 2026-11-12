//! Descendant-editable bridge between independently promoted local model
//! weights and independently measured source-improvement productivity.
//!
//! Token-level loss improvement is not the same as an agent that can produce
//! better descendants.  This experiment designer groups actual PEFT-powered
//! source revisions and no-adapter controls by the SAME frozen source/profile,
//! objective and task focus, and refuses to call an observational contrast
//! causal.  A repeated shortfall may change the *source* next edited by the
//! compiled successor; the parent evaluator remains the sole adoption gate.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn hex(s:&str,n:usize)->bool{
    s.len()==n&&s.bytes().all(|c|c.is_ascii_hexdigit())
}
fn pinned(v:&Value)->bool{
    v["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        &&v["role"].as_str().is_some_and(|s|!s.is_empty()&&s.len()<=64)
        &&v["sourceSha256"].as_str().is_some_and(|s|hex(s,64))
}
fn assessed(g:&Value)->bool{
    g["pairedRevalidated"]==true && g["selfControllerVerified"]==true
        && g["systemBlueprintVerified"]==true
        &&g["candidateOrigin"]=="model_driven_detached_worktree"
        &&g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        &&g["generatorCommit"].as_str().is_some_and(|s|hex(s,40))
        &&g["comparisonProfileSha256"].as_str().is_some_and(|s|hex(s,64))
        &&g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        &&g["evaluation"]["evidenceSha256"].as_str().is_some_and(|s|hex(s,64))
        &&g["evaluation"]["metrics"]["eligible"].is_boolean()
        &&g["evaluation"]["metrics"]["holdoutEvidence"].is_array()
        &&g["jobId"].as_str().is_some_and(|s|!s.is_empty())
        &&pinned(&g["systemAgenda"]["target"])
        &&g["systemAgenda"]["generatorCommit"]==g["generatorCommit"]
        &&g["systemAgenda"]["frontier"].as_array()
            .is_some_and(|rows|rows.contains(&g["systemAgenda"]["target"]))
}
fn isolated_cases(g:&Value)->BTreeSet<String>{
    g["evaluation"]["metrics"]["holdoutEvidence"].as_array()
        .into_iter().flatten().filter_map(|case|{
            if !case["baselinePass"].is_boolean()
                ||!case["candidatePass"].is_boolean(){return None;}
            let id=case["id"].as_str().filter(|s|!s.is_empty())?;
            let domain=case["domain"].as_str().filter(|s|!s.is_empty())?;
            Some(format!("{domain}:{id}"))
        }).collect()
}
fn actual_adapter(g:&Value)->Option<String>{
    let selection=&g["sourceModelSelection"];
    let used=&g["adaptedModelActuallyUsed"];
    if selection["source"]!="independent_parent_model_adapter_pair_v1"
        || selection["status"]!="promoted"
        || selection["pairedModelImprovementMeasured"]!=true
        || selection["adoptionScope"]!="isolated_descendant_inference_only"
        || selection["productionDeployed"]!=false
        || selection["campaignId"]!=g["campaignId"]
        || !selection["adapterSha256"].as_str().is_some_and(|s|hex(s,64))
        || !selection["adapterConfigSha256"].as_str().is_some_and(|s|hex(s,64))
        || used["source"]!="running_adapted_descendant_inference_v1"
        || used["promotionId"]!=selection["id"]
        || used["adapterSha256"]!=selection["adapterSha256"]
        || used["campaignId"]!=g["campaignId"]
        || used["modelWeightsVerifiedAtLaunch"]!=true
        || used["actuallyUsedInGeneration"]!=true
        || used["actualModelCalls"].as_u64().unwrap_or(0)==0
        || used["actualModelTokens"].as_u64().unwrap_or(0)==0
        || used["providerId"].as_str().is_none_or(|p|!p.starts_with("rsi-adapter-"))
        || g["generatorProviderId"]!=used["providerId"]{
        return None;
    }
    selection["adapterSha256"].as_str().map(str::to_string)
}
fn no_adapter_control(g:&Value)->bool{
    g["sourceModelSelection"].is_null()
        &&g["adaptedModelActuallyUsed"].is_null()
        &&g["generatorProviderId"].as_str()
            .is_some_and(|p|!p.starts_with("rsi-adapter-"))
}
fn actually_adopted(g:&Value)->bool{
    g["accepted"]==true &&g["evaluation"]["metrics"]["eligible"]==true
        &&g["acceptedCommit"].as_str().is_some_and(|s|hex(s,40))
        &&g["acceptedSourceHashes"].as_object().is_some_and(|m|!m.is_empty())
}
type Cohort=(String,String,String,String,String,String);
#[derive(Default)]
struct Trials {
    adapters:BTreeMap<String,(bool,BTreeSet<String>)>,
    controls:BTreeMap<String,(bool,BTreeSet<String>)>,
}
pub(crate) fn plan(history:&[Value],frontier:&[Value],commit:&str)->Value{
    let method=frontier.iter().find(|r|
        r["path"]=="src/engine/rsi_model_weights.rs"&&pinned(r));
    let router=frontier.iter().find(|r|
        r["path"]=="src/engine/rsi_model_learning.rs"&&pinned(r));
    let mut groups=BTreeMap::<Cohort,Trials>::new();
    for g in history.iter().filter(|g|assessed(g)){
        let target=&g["systemAgenda"]["target"];
        let Some(base)=g["generatorCommit"].as_str()else{continue};
        let Some(profile)=g["comparisonProfileSha256"].as_str()else{continue};
        let Some(source_path)=target["path"].as_str()else{continue};
        let Some(source_hash)=target["sourceSha256"].as_str()else{continue};
        let Some(objective)=g["objectiveId"].as_str().filter(|s|!s.is_empty())else{continue};
        let Some(candidate)=g["candidateId"].as_str()else{continue};
        let cases=isolated_cases(g);
        if cases.len()<2 {continue;}
        let key=(base.to_string(),profile.to_string(),objective.to_string(),
            source_path.to_string(),source_hash.to_string(),g["campaignId"].as_str()
                .unwrap_or("").to_string());
        let adopted=actually_adopted(g);
        let group=groups.entry(key).or_default();
        if let Some(sha)=actual_adapter(g){
            let unique=format!("{}:{candidate}",g["jobId"].as_str().unwrap_or(""));
            // One candidate contributes at most once to one model cohort.
            group.adapters.entry(format!("{sha}:{unique}")).or_insert((adopted,cases));
        } else if no_adapter_control(g) {
            let unique=format!("{}:{candidate}",g["jobId"].as_str().unwrap_or(""));
            group.controls.entry(unique).or_insert((adopted,cases));
        }
    }
    let mut evidence=Vec::<Value>::new();
    let mut selected=Value::Null;
    for ((base,profile,objective,path,source_hash,campaign),trials) in groups{
        // Different promoted adapters never pool to manufacture a loss.
        let mut adapter_groups=BTreeMap::<String,Vec<&(bool,BTreeSet<String>)>>::new();
        for (key,trial) in &trials.adapters {
            if let Some((sha,_))=key.split_once(':'){
                adapter_groups.entry(sha.to_string()).or_default().push(trial);
            }
        }
        for (sha,adapter_trials) in adapter_groups{
            let baseline=trials.controls.values().collect::<Vec<_>>();
            let mut adapter_cases=BTreeSet::new();
            let mut control_cases=BTreeSet::new();
            for (_,cases) in &adapter_trials{adapter_cases.extend(cases.iter().cloned());}
            for (_,cases) in &baseline{control_cases.extend(cases.iter().cloned());}
            let comparable=adapter_cases.intersection(&control_cases).count();
            let adapted_wins=adapter_trials.iter().filter(|(won,_)|*won).count();
            let control_wins=baseline.iter().filter(|(won,_)|*won).count();
            let both=adapter_trials.len()>=2 && baseline.len()>=2&&comparable>=2;
            let stale=method.is_none_or(|m|{
                // The feedback may remain in history after a method rewrite.
                // It cannot force the same rewrite again on stale source.
                let Some(snapshot)=history.iter().find(|g|
                    g["generatorCommit"]==base
                    &&g["comparisonProfileSha256"]==profile
                    &&g["objectiveId"]==objective
                    &&g["systemAgenda"]["target"]["path"]==path
                    &&g["systemAgenda"]["target"]["sourceSha256"]==source_hash
                    &&g["campaignId"]==campaign)else{return true;};
                !snapshot["systemAgenda"]["frontier"].as_array().is_some_and(|rows|
                    rows.iter().any(|r|r["path"]==m["path"]
                        &&r["sourceSha256"]==m["sourceSha256"]))
            });
            let negative=both && control_wins>=2 && adapted_wins==0;
            let active=negative&&!stale;
            let row=json!({
                "generatorCommit":base,"comparisonProfileSha256":profile,
                "objectiveId":objective,"campaignId":campaign,
                "originalTaskSource":path,"originalTaskSourceSha256":source_hash,
                "adapterSha256":sha,
                "adaptedSourceTrials":adapter_trials.len(),
                "adaptedSourceAdoptions":adapted_wins,
                "nonAdaptedSourceTrials":baseline.len(),
                "nonAdaptedSourceAdoptions":control_wins,
                "matchedHoldoutCaseCount":comparable,
                "sameFrozenTaskAndSource":true,
                "sameBaseModelWeightsProven":false,
                "causalityProven":false,
                "methodSourceStillUnchanged":!stale,
                "prioritizeMethodRedesign":active,
                "negativeProductivityHypothesis":negative
            });
            if active &&selected.is_null(){selected=row.clone();}
            evidence.push(row);
        }
    }
    evidence.sort_by(|a,b|
        (b["prioritizeMethodRedesign"]==true)
            .cmp(&(a["prioritizeMethodRedesign"]==true))
        .then_with(||b["matchedHoldoutCaseCount"].as_u64()
            .cmp(&a["matchedHoldoutCaseCount"].as_u64()))
        .then_with(||a["adapterSha256"].as_str().cmp(&b["adapterSha256"].as_str())));
    evidence.truncate(12);
    if selected.is_null(){
        selected=evidence.iter().find(|row|row["prioritizeMethodRedesign"]==true)
            .cloned().unwrap_or(Value::Null);
    }
    let focus=if selected.is_object(){
        method.iter().chain(router).cloned().collect::<Vec<_>>()
    }else{Vec::new()};
    let mut result=json!({
        "source":"compiled_weight_to_method_productivity_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "evidence":evidence,"selected":selected,
        "target":focus.first().cloned().unwrap_or(Value::Null),
        "focus":focus,
        "redesignTrainingMethod":!selected.is_null(),
        "learningAndRoutingCoupled":true,
        "frozenSourceOracleUnchanged":true,
        "causalityProven":false,
        "newWeightsOrAbilityProven":false
    });
    let mut digest=result.clone();
    digest.as_object_mut().unwrap().remove("revision");
    result["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&digest).unwrap_or_default())));
    result
}
pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,len:usize)->bool{
    let mut digest=plan.clone();
    let Some(obj)=digest.as_object_mut()else{return false};
    obj.remove("revision");
    plan["source"]=="compiled_weight_to_method_productivity_v1"
        && plan["generatorCommit"]==commit
        && plan["historyCount"].as_u64()==Some(len as u64)
        && plan["revision"].as_str()==Some(format!("{:x}",
            Sha256::digest(serde_json::to_vec(&digest).unwrap_or_default())).as_str())
        && plan["frozenSourceOracleUnchanged"]==true
        && plan["causalityProven"]==false
        && plan["newWeightsOrAbilityProven"]==false
        && (plan["redesignTrainingMethod"]==false
            || plan["selected"]["prioritizeMethodRedesign"]==true
                &&plan["target"]["path"]=="src/engine/rsi_model_weights.rs"
                &&plan["focus"].as_array().is_some_and(|rows|
                    !rows.is_empty()&&rows.len()<=2&&rows[0]==plan["target"]
                        &&rows.iter().all(|row|pinned(row)&&frontier.contains(row))))
}
pub(crate) fn apply(agenda:&mut Value,plan:&Value){
    if plan["source"]!="compiled_weight_to_method_productivity_v1"
        ||plan["redesignTrainingMethod"]!=true
        ||agenda["inheritedStage"].is_object()
        ||agenda["factorialExperiment"]["descendantContinuation"]==true
        ||agenda["recursiveLaboratory"]["phase"].as_str().is_some_and(|v|
            v!="factorial_science")
        ||agenda["wholeDominionGrowth"]["phase"]==
            "investigate_repeated_cross_component_negative_transfer" {
        return;
    }
    let Some(frontier)=agenda["frontier"].as_array()else{return};
    let Some(focus)=plan["focus"].as_array()else{return};
    if focus.is_empty()||focus.len()>2||focus.iter()
        .any(|row|!pinned(row)||!frontier.contains(row)){return;}
    agenda["target"]=plan["target"].clone();
    agenda["focus"]=json!(focus);
    agenda["weightMethodLearning"]=plan.clone();
}
pub(crate) fn instruction(plan:&Value)->String{
    if plan["redesignTrainingMethod"]!=true{return String::new();}
    format!(
        "[SOURCE-EXECUTED WEIGHT-TO-METHOD RSI] Independent frozen source-edit \
         outcomes after ACTUAL promoted LoRA inference failed against comparable \
         no-adapter source-edit trials. Original matched evidence={}. \
         This is an OBSERVATIONAL productivity shortfall, NOT proof of poor \
         weights or causal model harm: baseline model weights may differ. \
         REDESIGN the executable model-training selection, private evaluation \
         gate and successor inference decision in exact source={} and paired \
         route={} when present. Invent new training data choice/optimizer/credit \
         assignment if evidence supports it; keep source hash grounding and the \
         independent private parent oracle. A revised model method must be \
         compiled into the next child, then evaluated for real cross-domain \
         improvements and descendant source-productivity. Do not simply repeat \
         the same unproductive training or claim improved weights.",
        plan["selected"],plan["target"]["path"],
        plan["focus"].as_array().and_then(|v|v.get(1))
            .map(|v|v["path"].to_string()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn role(path:&str)->Value{
        json!({"role":"improvement_methods","path":path,
            "sourceSha256":"a".repeat(64),"domain":"improver"})
    }
    fn case(id:&str)->Value{
        json!({"id":id,"domain":"learning","baselinePass":false,
            "candidatePass":true})
    }
    fn trial(id:&str,adapted:bool,adopted:bool)->Value{
        let method=role("src/engine/rsi_model_weights.rs");
        let target=role("src/engine/learning.rs");
        let mut g=json!({
            "jobId":id,"campaignId":"campaign","objectiveId":"objective",
            "candidateId":format!("candidate-{id}"),
            "generatorCommit":"b".repeat(40),
            "comparisonProfileSha256":"c".repeat(64),
            "pairedRevalidated":true,"selfControllerVerified":true,
            "systemBlueprintVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "systemAgenda":{"generatorCommit":"b".repeat(40),
                "target":target,"frontier":[target,method]},
            "generatorProviderId":if adapted{"rsi-adapter-one"}else{"plain-provider"},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"e".repeat(64),
                "metrics":{"eligible":adopted,
                    "holdoutEvidence":[case("a"),case("b")]}},
            "accepted":adopted,
            "acceptedCommit":if adopted{"d".repeat(40)}else{String::new()},
            "acceptedSourceHashes":if adopted {
                json!({"src/engine/learning.rs":"f".repeat(64)})
            }else{json!({})}
        });
        if adapted{
            g["sourceModelSelection"]=json!({
                "source":"independent_parent_model_adapter_pair_v1",
                "status":"promoted",
                "pairedModelImprovementMeasured":true,
                "adoptionScope":"isolated_descendant_inference_only",
                "productionDeployed":false,"campaignId":"campaign",
                "adapterSha256":"1".repeat(64),
                "adapterConfigSha256":"2".repeat(64),"id":"promoted"});
            g["adaptedModelActuallyUsed"]=json!({
                "source":"running_adapted_descendant_inference_v1",
                "promotionId":"promoted","campaignId":"campaign",
                "adapterSha256":"1".repeat(64),
                "actuallyUsedInGeneration":true,
                "modelWeightsVerifiedAtLaunch":true,
                "providerId":"rsi-adapter-one",
                "actualModelCalls":3,"actualModelTokens":512});
        }
        g
    }
    #[test]
    fn measured_adapted_productivity_loss_rewrites_own_weight_method(){
        let frontier=vec![role("src/engine/rsi_model_weights.rs"),
            role("src/engine/rsi_model_learning.rs")];
        let history=vec![trial("a",true,false),trial("b",true,false),
            trial("c",false,true),trial("d",false,true)];
        let p=plan(&history,&frontier,&"f".repeat(40));
        assert_eq!(p["redesignTrainingMethod"],true);
        assert_eq!(p["selected"]["adaptedSourceTrials"],2);
        assert_eq!(p["selected"]["nonAdaptedSourceAdoptions"],2);
        assert!(valid(&p,&frontier,&"f".repeat(40),4));
        let mut agenda=json!({"frontier":frontier,
            "target":role("src/engine/learning.rs"),"focus":[],
            "inheritedStage":null,"factorialExperiment":null,
            "recursiveLaboratory":null,"wholeDominionGrowth":null});
        apply(&mut agenda,&p);
        assert_eq!(agenda["target"]["path"],"src/engine/rsi_model_weights.rs");
        assert_eq!(agenda["focus"].as_array().unwrap().len(),2);
    }
    #[test]
    fn ready_server_and_unverified_usage_cannot_train_model_method(){
        let frontier=vec![role("src/engine/rsi_model_weights.rs")];
        let mut a=trial("a",true,false);
        a["adaptedModelActuallyUsed"]["actualModelTokens"]=json!(0);
        let p=plan(&[a,trial("b",true,false),trial("c",false,true),
            trial("d",false,true)],&frontier,&"f".repeat(40));
        assert_eq!(p["redesignTrainingMethod"],false);
    }
    #[test]
    fn changed_method_source_revokes_stale_productivity_pressure(){
        let mut new_method=role("src/engine/rsi_model_weights.rs");
        new_method["sourceSha256"]=json!("9".repeat(64));
        let frontier=vec![new_method];
        let p=plan(&[trial("a",true,false),trial("b",true,false),
            trial("c",false,true),trial("d",false,true)],
            &frontier,&"f".repeat(40));
        assert_eq!(p["redesignTrainingMethod"],false);
        assert_eq!(p["evidence"][0]["methodSourceStillUnchanged"],false);
    }
    #[test]
    fn different_profile_objective_or_source_cannot_masquerade_as_control(){
        let frontier=vec![role("src/engine/rsi_model_weights.rs")];
        let mut control=trial("c",false,true);
        control["comparisonProfileSha256"]=json!("f".repeat(64));
        let mut other=trial("d",false,true);
        other["objectiveId"]=json!("unrelated-objective");
        let p=plan(&[trial("a",true,false),trial("b",true,false),
            control,other],&frontier,&"f".repeat(40));
        assert_eq!(p["redesignTrainingMethod"],false);
    }
    #[test]
    fn explicit_or_inherited_science_is_not_overridden(){
        let frontier=vec![role("src/engine/rsi_model_weights.rs")];
        let p=plan(&[trial("a",true,false),trial("b",true,false),
            trial("c",false,true),trial("d",false,true)],
            &frontier,&"f".repeat(40));
        let mut agenda=json!({"frontier":frontier,
            "target":role("src/engine/learning.rs"),"focus":[],
            "inheritedStage":{"source":"model_authored_stage"}});
        apply(&mut agenda,&p);
        assert!(agenda["weightMethodLearning"].is_null());
    }
}
