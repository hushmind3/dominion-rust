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

/// Independently measured source edits may expose negative transfer: a change
/// to one subsystem makes another subsystem's previously passing holdout fail.
/// Never infer a causal edge from a multi-file change, an unmatched comparison
/// profile, a model narrative, or duplicate cases from one candidate.
/// These are source-grounded *intervention hypotheses*, not demonstrated causes.
/// The next compiled agent can rewrite this selection algorithm itself.
fn cross_component_interference(
    history:&[Value],sources:&BTreeMap<String,Value>,current_commit:&str
)->Vec<Value>{
    // One cohort is one real generator source revision and one frozen
    // comparison profile. A past source failure remains useful historical
    // evidence, but MUST NOT force a repair on a later, changed descendant.
    type Cohort=(String,String,String,String,String);
    type Witnesses=(std::collections::BTreeSet<String>,
        std::collections::BTreeSet<String>);
    let mut cohorts=BTreeMap::<Cohort,Witnesses>::new();
    for g in history.iter().filter(|g|assessed(g)){
        if g["candidateOrigin"]!="model_driven_detached_worktree"
            ||g["systemDesign"]["source"]!="model_authored_system_design"
            ||g["systemDesign"]["generatorCommit"]!=g["generatorCommit"]
            ||g["systemDesign"]["generationJobId"]!=g["jobId"] {
            continue;
        }
        let Some(base)=g["generatorCommit"].as_str().filter(|s|
            s.len()==40&&s.bytes().all(|b|b.is_ascii_hexdigit()))else{continue};
        let Some(candidate)=g["candidateId"].as_str().filter(|s|!s.is_empty())
            else{continue};
        let Some(job)=g["jobId"].as_str().filter(|s|!s.is_empty())else{continue};
        let Some(profile)=g["comparisonProfileSha256"].as_str().filter(|s|
            s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))else{continue};
        let Some(changed)=g["changedSourcePaths"].as_array().filter(|rows|
            rows.len()==1).and_then(|rows|rows[0].as_str())
            .filter(|path|crate::engine::rsi::safe_path(path))else{continue};
        if !g["systemDesign"]["targetFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(changed))) {continue;}
        let mut source_roles=sources.iter().filter(|(role,source)|
            source["path"].as_str()==Some(changed)
                &&g["systemDesign"]["componentFiles"][role.as_str()]
                    .as_array().is_some_and(|files|
                        files.iter().any(|f|f.as_str()==Some(changed))));
        let Some((source_role,_))=source_roles.next()else{continue};
        let source_role=source_role.clone();
        if source_roles.next().is_some(){continue;}
        // This is the exact pre-edit source snapshot independently frozen
        // in the original generation input. A current source with the same
        // path does NOT prove that a different old revision is still damaged.
        let Some(snapshot)=g["systemAgenda"]["frontier"].as_array()else{continue};
        let origin=snapshot.iter().filter(|row|row["path"]==changed
            &&row["role"]==source_role.as_str()).collect::<Vec<_>>();
        if origin.len()!=1 {continue;}
        let Some(source_sha)=origin[0]["sourceSha256"].as_str().filter(|s|
            s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))else{continue};
        let mut damaged=BTreeMap::<String,std::collections::BTreeSet<String>>::new();
        for case in g["evaluation"]["metrics"]["holdoutEvidence"]
            .as_array().into_iter().flatten(){
            let (Some(id),Some(domain))=(
                case["id"].as_str().filter(|s|!s.is_empty()),
                case["domain"].as_str().filter(|s|!s.is_empty()))else{continue};
            let victim=mapped_role(domain);
            if victim==source_role.as_str()||!sources.contains_key(victim)
                ||case["baselinePass"]!=true
                ||(case["candidatePass"]!=false
                    &&case["metricRegressions"].as_u64().unwrap_or(0)==0) {
                continue;
            }
            damaged.entry(victim.to_string()).or_default()
                .insert(format!("{domain}:{id}"));
        }
        for (victim,cases) in damaged{
            let entry=cohorts.entry((base.to_string(),profile.to_string(),
                source_sha.to_string(),source_role.clone(),victim))
                .or_default();
            entry.0.insert(format!("{job}:{candidate}"));
            entry.1.extend(cases);
        }
    }
    let mut rows=cohorts.into_iter().filter_map(
        |((base,profile,source_sha,from,to),(trials,cases))|{
        let source=sources.get(&from)?;
        let victim=sources.get(&to)?;
        let current=base==current_commit
            &&source["sourceSha256"].as_str()==Some(source_sha.as_str());
        Some(json!({
            "generatorSourceCommit":base,
            "sourceAnchorSha256":source_sha,
            "sourceRole":from,"victimRole":to,
            "source":source,"victim":victim,
            "comparisonProfileSha256":profile,
            "independentCounterexamples":trials.len(),
            "distinctDamagedHoldouts":cases.len(),
            "damagedHoldoutIds":cases.into_iter().take(12).collect::<Vec<_>>(),
            "requiredIndependentCounterexamples":2,
            "requiredDistinctDamagedHoldouts":2,
            "currentSourceRevision":current,
            "isolatedSourceChange":true,
            "sameFrozenComparisonProfile":true,
            "causalAttributionProven":false,
            "candidateInterferenceNotInheritedRegression":true
        }))
    }).collect::<Vec<_>>();
    rows.sort_by(|a,b|
        (b["currentSourceRevision"]==true).cmp(&(a["currentSourceRevision"]==true))
        .then_with(||b["independentCounterexamples"].as_u64()
            .cmp(&a["independentCounterexamples"].as_u64()))
        .then_with(||b["distinctDamagedHoldouts"].as_u64()
            .cmp(&a["distinctDamagedHoldouts"].as_u64()))
        .then_with(||a["sourceRole"].as_str().cmp(&b["sourceRole"].as_str()))
        .then_with(||a["victimRole"].as_str().cmp(&b["victimRole"].as_str()))
        .then_with(||a["comparisonProfileSha256"].as_str()
            .cmp(&b["comparisonProfileSha256"].as_str())));
    rows.truncate(16);
    rows
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
        let accepted=adopted(generation);
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
    let mut best=ranked.first().map(|row|row.2.clone()).unwrap_or(Value::Null);
    let interference=cross_component_interference(history,&sources,commit);
    // Two independent, same-profile, single-source counterexamples are
    // sufficient to prioritize a *new paired experiment*, never adoption.
    // Pair the damaged capability with the source suspected of interfering.
    let paired_risk=interference.iter().find(|row|
        row["independentCounterexamples"].as_u64().unwrap_or(0)>=2
            &&row["distinctDamagedHoldouts"].as_u64().unwrap_or(0)>=2
            &&row["currentSourceRevision"]==true);
    if let Some(pair)=paired_risk{
        best=json!({"role":pair["victimRole"],"target":pair["victim"],
            "measuredHoldouts":0,"unresolvedBehavior":0,
            "observedRegressions":0,"acceptedImprovements":0});
    }
    let selected_interference=paired_risk.cloned().unwrap_or(Value::Null);
    // If independent experiments repeatedly fail on a real capability,
    // the next DOMINION must invent a different improvement MECHANISM and
    // rewrite the stalled capability at the same time. More retries of the
    // unchanged planner do not constitute recursive self-improvement.
    let stalled=best["unresolvedBehavior"].as_u64().unwrap_or(0)>=3
        &&best["acceptedImprovements"].as_u64().unwrap_or(0)==0;
    let meta=if stalled && best["role"]!="improvement_methods" {
        sources.get("improvement_methods").cloned().unwrap_or(Value::Null)
    }else{Value::Null};
    let phase=if paired_risk.is_some(){
        "investigate_repeated_cross_component_negative_transfer"
    }else if stalled && meta.is_object(){
        "invent_better_improvement_method_for_failed_intelligence"
    }else if best["unresolvedBehavior"].as_u64().unwrap_or(0)>0
        ||best["observedRegressions"].as_u64().unwrap_or(0)>0{
        "repair_measured_whole_agent_weakness"
    }else if best["measuredHoldouts"].as_u64().unwrap_or(0)==0{
        "learn_unmeasured_intelligence_capability"
    }else{"extend_verified_whole_agent_capability"};
    let mut focus=if best["target"].is_object(){
        vec![best["target"].clone()]
    }else{Vec::new()};
    if let Some(pair)=paired_risk{
        if pair["source"].is_object()
            &&pair["source"]["path"]!=best["target"]["path"]{
            focus.push(pair["source"].clone());
        }
    }else if meta.is_object()&&meta["path"]!=best["target"]["path"]{
        focus.push(meta.clone());
    }
    let mut ledger=json!({
        "source":"descendant_whole_dominion_growth_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "independentlyMeasuredGenerations":assessed_generations,
        "phase":phase,
        "target":best["target"],
        "focus":focus,
        "improvementMethodSource":meta,
        "failedMethodReinventionRequired":stalled,
        "capabilityLedger":ranked.into_iter().map(|row|row.2)
            .take(32).collect::<Vec<_>>(),
        "crossComponentInterference":interference,
        "selectedInterferenceExperiment":selected_interference,
        "interferenceCausalityProven":false,
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
            (1..=2).contains(&items.len()) &&items[0]==p["target"]
                &&items.iter().all(|item|pinned(item)&&frontier.contains(item)))
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
        capability ledger={}. Cross-component counterfactual={} (unproven; only current source SHA and two distinct frozen holdouts can activate the paired repair). \
        When this counterfactual selects TWO focused roles, inspect and actually \
        rewrite both pinned source files. Contrast source-only, victim-only and \
        joint edits against the same frozen cases to distinguish repair from \
        interference. Preserve previously passing capabilities; do not promote \
        any hypothesis without independent paired evaluation. Your task is to \
        make the next compiled DOMINION BETTER at solving real work AND at \
        improving its next descendants. \
        If the cause is a failed improvement method, rewrite that method too, \
        and test both with independent heldout tasks. A patch that is adopted \
        without later capability transfer is not recursive intelligence gain. \
        Propose source-changing new intelligence when existing capabilities \
        cannot solve the weakness. Never substitute an API model ranking for complete-agent self-improvement.",
        p["phase"],p["revision"],
        p["target"]["role"],p["target"]["path"],p["capabilityLedger"],
        p["selectedInterferenceExperiment"])
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
    fn repeated_memory_failure_forces_a_new_method_and_a_memory_rewrite(){
        let frontier=vec![
            role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs"),
            role("improvement_methods","src/engine/rsi_whole_agent.rs")];
        let history=vec![
            evaluation("memory",false,false),
            evaluation("memory",false,false),
            evaluation("memory",false,false)];
        let p=plan(&history,&frontier,"c");
        assert_eq!(p["phase"],
            "invent_better_improvement_method_for_failed_intelligence");
        assert_eq!(p["target"]["role"],"memory");
        assert_eq!(p["focus"][0]["role"],"memory");
        assert_eq!(p["focus"][1]["role"],"improvement_methods");
        assert!(valid(&p,&frontier,"c",3));
    }

    fn interference_trial(name:&str,profile:&str,verified:bool)->Value{
        let mut g=evaluation("tools",true,false);
        g["jobId"]=json!(format!("job-{name}"));
        g["candidateId"]=json!(format!("candidate-{name}"));
        g["candidateOrigin"]=json!("model_driven_detached_worktree");
        g["comparisonProfileSha256"]=json!(profile);
        g["pairedRevalidated"]=json!(verified);
        g["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        g["evaluation"]["metrics"]["holdoutEvidence"][0]["id"]=
            json!(format!("case-{name}"));
        g["systemAgenda"]=json!({"frontier":[
            role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")]});
        g["systemDesign"]=json!({
            "source":"model_authored_system_design",
            "generationJobId":g["jobId"],
            "generatorCommit":g["generatorCommit"],
            "componentFiles":{
                "memory":["src/engine/memory.rs"],
                "tools":["src/engine/tools.rs"]
            },
            "targetFiles":["src/engine/memory.rs"]
        });
        g
    }
    #[test]
    fn repeated_measured_negative_transfer_makes_next_child_edit_both_sources(){
        let frontier=vec![
            role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs"),
            role("improvement_methods","src/engine/rsi_whole_agent.rs")];
        let profile="c".repeat(64);
        let history=vec![
            interference_trial("first",&profile,true),
            interference_trial("second",&profile,true)];
        let commit="b".repeat(40);
        let result=plan(&history,&frontier,&commit);
        assert_eq!(result["phase"],
            "investigate_repeated_cross_component_negative_transfer");
        assert_eq!(result["target"]["role"],"tools");
        assert_eq!(result["focus"][0]["role"],"tools");
        assert_eq!(result["focus"][1]["role"],"memory");
        assert_eq!(result["selectedInterferenceExperiment"]["independentCounterexamples"],2);
        assert_eq!(result["selectedInterferenceExperiment"]["distinctDamagedHoldouts"],2);
        assert_eq!(result["selectedInterferenceExperiment"]["currentSourceRevision"],true);
        assert_eq!(result["interferenceCausalityProven"],false);
        assert!(valid(&result,&frontier,&commit,2));
        let mut agenda=json!({"frontier":frontier,"target":Value::Null,
            "focus":[],"inheritedStage":null});
        apply(&mut agenda,&result);
        assert_eq!(agenda["focus"][1]["role"],"memory");
    }
    #[test]
    fn missing_receipts_different_cohorts_and_multifile_edits_cannot_invent_an_edge(){
        let frontier=vec![role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")];
        let a="c".repeat(64);
        let b="d".repeat(64);
        let different=vec![
            interference_trial("one",&a,true),
            interference_trial("two",&b,true)];
        let first=plan(&different,&frontier,"d");
        assert!(first["selectedInterferenceExperiment"].is_null());
        let mut multi=interference_trial("three",&a,true);
        multi["changedSourcePaths"]=json!([
            "src/engine/memory.rs","src/engine/tools.rs"]);
        let mut forgery=interference_trial("four",&a,false);
        forgery["evaluation"]["evidenceSha256"]=Value::Null;
        let result=plan(&[different[0].clone(),multi,forgery],&frontier,"d");
        assert!(result["selectedInterferenceExperiment"].is_null());
        assert_ne!(result["phase"],
            "investigate_repeated_cross_component_negative_transfer");
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
    #[test]
    fn old_generations_remain_learning_evidence_but_cannot_redirect_a_new_source(){
        let frontier=vec![role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")];
        let profile="c".repeat(64);
        let history=vec![interference_trial("one",&profile,true),
            interference_trial("two",&profile,true)];
        let new_commit="d".repeat(40);
        let p=plan(&history,&frontier,&new_commit);
        assert!(p["selectedInterferenceExperiment"].is_null());
        assert_eq!(p["crossComponentInterference"][0]["independentCounterexamples"],2);
        assert_eq!(p["crossComponentInterference"][0]["currentSourceRevision"],false);
    }
    #[test]
    fn duplicated_holdout_cases_cannot_masquerade_as_distinct_damage(){
        let frontier=vec![role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")];
        let profile="c".repeat(64);
        let first=interference_trial("one",&profile,true);
        let mut same=interference_trial("two",&profile,true);
        same["evaluation"]["metrics"]["holdoutEvidence"][0]["id"]=json!("case-one");
        let commit="b".repeat(40);
        let p=plan(&[first,same],&frontier,&commit);
        assert!(p["selectedInterferenceExperiment"].is_null());
        assert_eq!(p["crossComponentInterference"][0]["independentCounterexamples"],2);
        assert_eq!(p["crossComponentInterference"][0]["distinctDamagedHoldouts"],1);
    }
    #[test]
    fn original_source_hash_is_required_even_when_path_and_commit_match(){
        let frontier=vec![role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")];
        let profile="c".repeat(64);
        let mut first=interference_trial("one",&profile,true);
        let mut second=interference_trial("two",&profile,true);
        for trial in [&mut first,&mut second] {
            trial["systemAgenda"]["frontier"][0]["sourceSha256"]=
                json!("1".repeat(64));
        }
        let p=plan(&[first,second],&frontier,&"b".repeat(40));
        assert!(p["selectedInterferenceExperiment"].is_null());
        assert_eq!(p["crossComponentInterference"][0]["currentSourceRevision"],false);
    }
    #[test]
    fn missing_frozen_snapshot_cannot_generate_source_causal_evidence(){
        let frontier=vec![role("memory","src/engine/memory.rs"),
            role("tools","src/engine/tools.rs")];
        let profile="c".repeat(64);
        let mut first=interference_trial("one",&profile,true);
        first["systemAgenda"]["frontier"]=Value::Null;
        let p=plan(&[first,interference_trial("two",&profile,true)],
            &frontier,&"b".repeat(40));
        assert!(p["selectedInterferenceExperiment"].is_null());
        assert_eq!(p["crossComponentInterference"][0]["independentCounterexamples"],1);
    }

}
