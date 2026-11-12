//! Whole-DOMINION self-improvement, not model swapping or optimizing only RSI.
//!
//! A *rebuilt DOMINION* inherits the actual code, memory, learning, tools,
//! model routing and an editable source-level improvement policy. This module
//! learns from the independent frozen paired evaluator where the complete
//! agent still FAILS, and changes which intelligence implementation the NEXT
//! compiled descendant actually edits. Candidate gains and descendant
//! productivity are separate, observable quantities. No self-reported gain.
//! This algorithm is itself part of the next editable descendant.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;

fn hash(v:&Value)->String{
    format!("{:x}",Sha256::digest(
        serde_json::to_vec(v).unwrap_or_default()))
}
fn pinned(v:&Value)->bool{
    v["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && v["role"].as_str().is_some_and(|s|!s.is_empty())
        && v["sourceSha256"].as_str().is_some_and(|s|
            s.len()==64&&s.bytes().all(|c|c.is_ascii_hexdigit()))
}
fn assessed(g:&Value)->bool{
    g["pairedRevalidated"]==true
        && g["selfControllerVerified"]==true
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && g["evaluation"]["evidenceSha256"].as_str().is_some_and(|sha|
            sha.len()==64&&sha.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["comparisonProfileSha256"].as_str().is_some_and(|sha|
            sha.len()==64&&sha.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["evaluation"]["metrics"]["holdoutEvidence"].as_array()
            .is_some_and(|items|!items.is_empty())
}
fn adopted(g:&Value)->bool{
    assessed(g)&&g["accepted"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["acceptedCommit"].as_str().is_some_and(|s|
            s.len()==40&&s.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["candidateOrigin"]=="model_driven_detached_worktree"
}
fn mapped_role(domain:&str)->&str{
    match domain{
        "model"=>"models","context"|"planning"=>"harness",
        "search"=>"algorithms","ui"=>"frontend",
        "improver"=>"improvement_methods",
        other=>other
    }
}
#[derive(Default)]
struct Observed{
    tested:u64,
    unresolved:u64,
    regressions:u64,
    improved:u64,
    verified_adopted_gains:u64,
    rebuilt_child_trials:u64,
    rebuilt_child_adoptions:u64,
}
/// Only the external oracle's case-level observations can change the
/// empirical state; an untested ability remains UNKNOWN, not "improved".
pub(crate) fn plan(history:&[Value],frontier:&[Value],commit:&str)->Value{
    let mut sources=BTreeMap::<String,Value>::new();
    for source in frontier.iter().filter(|source|pinned(source)){
        if let Some(role)=source["role"].as_str(){
            // The complete intelligence architecture may include descendants'
            // newly invented roles. Never require a closed list of targets.
            sources.entry(role.to_owned()).or_insert_with(||source.clone());
        }
    }
    let mut observations=BTreeMap::<String,Observed>::new();
    for role in sources.keys(){observations.entry(role.clone()).or_default();}
    let mut assessed_generations=0usize;
    for generation in history.iter().filter(|g|assessed(g)){
        assessed_generations+=1;
        let accepted=adopted(g);
        for case in generation["evaluation"]["metrics"]["holdoutEvidence"]
            .as_array().into_iter().flatten(){
            let Some(domain)=case["domain"].as_str()else{continue};
            let role=mapped_role(domain);
            let Some(row)=observations.get_mut(role)else{continue};
            let (Some(before),Some(after))=(
                case["baselinePass"].as_bool(),
                case["candidatePass"].as_bool())else{continue};
            row.tested+=1;
            if !before&&!after{row.unresolved+=1;}
            if before&&!after||case["metricRegressions"].as_u64()
                .unwrap_or(0)>0{row.regressions+=1;}
            if !before&&after||case["metricGains"].as_u64()
                .unwrap_or(0)>0{
                row.improved+=1;
                if accepted{row.verified_adopted_gains+=1;}
            }
        }
        // A parent rewrite is not a stronger DOMINION until its *compiled*
        // child can itself independently improve. Count the real next child
        // separately; an API model's weights were never modified here.
        if accepted{
            let Some(rebuilt)=generation["acceptedCommit"].as_str()else{continue};
            for child in history.iter().filter(|child|
                child["generatorCommit"]==rebuilt
                    && assessed(child)
                    && child["generatorBinarySha256"].as_str().is_some_and(|s|
                        s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))
                    &&child["systemBlueprintVerified"]==true){
                let role=mapped_role(child["domain"].as_str().unwrap_or(""));
                if let Some(row)=observations.get_mut(role){
                    row.rebuilt_child_trials+=1;
                    if adopted(child){row.rebuilt_child_adoptions+=1;}
                }
            }
        }
    }
    let mut ranked=observations.into_iter().filter_map(|(role,stats)|{
        let source=sources.get(&role)?;
        // A measurable weakness in real DOMINION behavior outranks
        // improving an RSI controller simply because it is easy to patch.
        // Unknown capability coverage is a first-class growth opportunity;
        // it never masquerades as an already proved gain.
        let unmet=2.8*stats.unresolved as f64
            +3.2*stats.regressions as f64;
        let exploration=4.5/(stats.tested as f64+1.0).sqrt();
        let improvement=0.2*stats.verified_adopted_gains as f64;
        let productivity_gap=if stats.rebuilt_child_trials>0 {
            1.5*(1.0-stats.rebuilt_child_adoptions as f64
                /stats.rebuilt_child_trials as f64)
        }else{0.0};
        let score=unmet+exploration+productivity_gap-improvement;
        // Among equally uncertain targets, prefer actual intelligence
        // capabilities to the infrastructure that proposes edits.
        let nonmeta=role!="improvement_methods";
        Some((score,nonmeta,json!({
            "role":role,"target":source,"priority":score,
            "measuredHoldouts":stats.tested,
            "unresolvedBehavior":stats.unresolved,
            "observedRegressions":stats.regressions,
            "observedImprovements":stats.improved,
            "acceptedImprovements":stats.verified_adopted_gains,
            "verifiedRebuiltChildTrials":stats.rebuilt_child_trials,
            "verifiedRebuiltChildAdoptions":stats.rebuilt_child_adoptions,
            "growthProven":false
        })))
    }).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b.0.total_cmp(&a.0)
        .then_with(||b.1.cmp(&a.1))
        .then_with(||a.2["role"].as_str().cmp(&b.2["role"].as_str())));
    let best=ranked.first().map(|row|row.2.clone()).unwrap_or(Value::Null);
    let phase=if best["unresolvedBehavior"].as_u64().unwrap_or(0)>0
        ||best["observedRegressions"].as_u64().unwrap_or(0)>0{
        "repair_measured_whole_agent_weakness"
    }else if best["measuredHoldouts"].as_u64().unwrap_or(0)==0{
        "learn_unmeasured_intelligence_capability"
    }else{"extend_verified_whole_agent_capability"};
    let mut ledger=json!({
        "source":"descendant_whole_dominion_growth_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "independentlyMeasuredGenerations":assessed_generations,
        "phase":phase,
        "target":best["target"],
        "focus":if best["target"].is_object(){vec![best["target"].clone()]}
            else{Vec::new()},
        "capabilityLedger":ranked.into_iter().map(|row|row.2)
            .take(32).collect::<Vec<_>>(),
        "nextGenerationMustRewriteIntelligence":true,
        "newMethodMayRewriteThisPolicy":true,
        "externalOracleUnchanged":true,
        "wholeAgentImprovementProven":false,
        "modelWeightsImproved":false,
        "experimentalPolicyEditableSource":"src/engine/rsi_whole_agent.rs"
    });
    ledger["revision"]=json!(hash(&ledger));ledger
}
pub(crate) fn valid(p:&Value,frontier:&[Value],commit:&str,len:usize)->bool{
    let mut without=p.clone();
    let Some(data)=without.as_object_mut()else{return false};
    data.remove("revision");
    p["source"]=="descendant_whole_dominion_growth_v1"
        &&p["revision"].as_str()==Some(hash(&without).as_str())
        &&p["generatorCommit"]==commit
        &&p["historyCount"].as_u64()==Some(len as u64)
        &&p["wholeAgentImprovementProven"]==false
        &&p["externalOracleUnchanged"]==true
        &&p["target"].as_object().is_some_and(|target|
            pinned(&Value::Object(target.clone()))
                &&frontier.iter().any(|row|row==&p["target"]))
        &&p["focus"].as_array().is_some_and(|items|
            items.len()==1&&items[0]==p["target"])
}
pub(crate) fn apply(agenda:&mut Value,ledger:&Value){
    if ledger["source"]!="descendant_whole_dominion_growth_v1"
        ||agenda["inheritedStage"].is_object()
        ||agenda["factorialExperiment"]["descendantContinuation"]==true
        ||agenda["recursiveLaboratory"]["phase"].as_str().is_some_and(|phase|
            phase!="factorial_science") {return;}
    let Some(frontier)=agenda["frontier"].as_array()else{return};
    if !frontier.contains(&ledger["target"])||!pinned(&ledger["target"]){
        return;
    }
    agenda["target"]=ledger["target"].clone();
    agenda["focus"]=ledger["focus"].clone();
    // These proposals remain visible in their frozen input records, but
    // cannot simultaneously claim that a DIFFERENT source was executed.
    agenda["principleCurriculum"]=Value::Null;
    agenda["factorialExperiment"]=Value::Null;
    agenda["wholeDominionGrowth"]=ledger.clone();
}
pub(crate) fn instruction(p:&Value)->String{
    if p["source"]!="descendant_whole_dominion_growth_v1"{
        return String::new();
    }
    format!(
        "[EVOLVE THE WHOLE DOMINION, NOT JUST ITS MODEL OR RSI PLANNER]\n\
        Actual successor growth plan: {} revision={}. \
        Edit the ACTUAL intelligence source role={} path={}. \
        Frozen behavioral deficits and cases are observable in the source-grounded \
        capability ledger={}. Your task is to make the next recompiled DOMINION \
        BETTER at solving real work AND at improving its next descendants. \
        If the cause is a failed improvement method, rewrite that method too, \
        and test both with independent heldout tasks. A patch that is adopted \
        without later capability transfer is not recursive intelligence gain. \
        Propose source-changing new intelligence when existing capabilities \
        cannot solve the weakness. Never substitute an API model ranking or \
        LoRA training for complete-agent self-improvement.",
        p["phase"],p["revision"],
        p["target"]["role"],p["target"]["path"],p["capabilityLedger"])
}
#[cfg(test)]
mod tests{
    use super::*;
    fn role(role:&str,path:&str)->Value{
        json!({"role":role,"path":path,"domain":"learning",
            "sourceSha256":"a".repeat(64)})
    }
    fn evaluation(domain:&str,before:bool,after:bool)->Value{
        json!({"generatorCommit":"b".repeat(40),
            "pairedRevalidated":true,"selfControllerVerified":true,
            "comparisonProfileSha256":"e".repeat(64),
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"f".repeat(64),
                "metrics":{"holdoutEvidence":[{
                    "id":"case","domain":domain,
                    "baselinePass":before,"candidatePass":after,
                    "metricGains":0,"metricRegressions":0}]}}})
    }
    #[test]
    fn the_next_dominion_rewrites_real_unresolved_memory_not_its_meta_planner(){
        let frontier=vec![
            role("improvement_methods","src/engine/rsi_meta_controller.rs"),
            role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")];
        let result=plan(&[evaluation("memory",false,false)],&frontier,"b");
        assert_eq!(result["target"]["role"],"memory");
        assert_eq!(result["phase"],"repair_measured_whole_agent_weakness");
        assert!(valid(&result,&frontier,"b",1));
        let mut agenda=json!({"frontier":frontier,
            "target":role("improvement_methods",
                "src/engine/rsi_meta_controller.rs"),
            "focus":[],"inheritedStage":null});
        apply(&mut agenda,&result);
        assert_eq!(agenda["target"]["role"],"memory");
        assert_eq!(agenda["wholeDominionGrowth"]["revision"],
            result["revision"]);
    }
    #[test]
    fn unverified_fake_gain_does_not_change_whole_agent_ability_ledger(){
        let frontier=vec![role("models","src/engine/models.rs"),
            role("tools","src/engine/tools.rs")];
        let mut fake=evaluation("tools",false,true);
        fake["pairedRevalidated"]=json!(false);
        let studied=plan(&[fake],&frontier,"g");
        assert_eq!(studied["independentlyMeasuredGenerations"],0);
        assert_eq!(studied["capabilityLedger"][0]["acceptedImprovements"],0);
        assert_eq!(studied["wholeAgentImprovementProven"],false);
    }
}
