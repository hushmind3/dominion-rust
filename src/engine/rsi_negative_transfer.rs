//! Source-grounded, independently replayed NEGATIVE transfer between
//! intelligence capabilities. A candidate changing A may damage capability B.
//! This is an experimental hypothesis, never a claim that the accepted
//! intelligence regressed; rejected candidates are still useful counterexamples.
//! The compiled descendant may rewrite this learner in ordinary Rust source.
use crate::{engine::rsi,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn hash(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn sha(s:&str,n:usize)->bool{
    s.len()==n && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn measured_role(domain:&str)->&str{
    match domain{
        "model"|"models"|"model_architecture"=>"models",
        "recall"|"memory"=>"memory",
        "training"|"learning"=>"learning",
        "tool-use"|"tools"=>"tools",
        "context"|"planning"|"harness"=>"harness",
        "improver"|"algorithm"|"search"|"scientific_method"
            |"improvement_methods"=>"improvement_methods",
        "ui"|"frontend"=>"frontend",
        other=>other
    }
}
fn regressed(m:&Value)->bool {
    if m["holdout"]!=true || m["baselinePass"]!=true {return false;}
    if m["candidatePass"]==false {return true;}
    m["metrics"].as_array().is_some_and(|rows|
        rows.iter().any(|row|row["regressed"]==true))
}
type CohortKey=(String,String,String,String);
#[derive(Default)]
struct Cases{
    candidate_ids:BTreeSet<String>,
    case_ids:BTreeSet<String>,
    observations:usize,
}
fn record(cohorts:&mut BTreeMap<CohortKey,Cases>,key:CohortKey,
    candidate:&str,case:&str) {
    let row=cohorts.entry(key).or_default();
    if row.candidate_ids.insert(candidate.to_string()){
        row.observations+=1;
    }
    row.case_ids.insert(case.to_string());
}
/// Confirm frozen paired receipts *again*, not the generation's editable
/// claimed metrics. Repeated single-source failures on the SAME generator
/// revision/profile expose a source-victim experiment worth running next.
pub(crate) fn plan(state:&AppState,history:&[Value],frontier:&[Value],
    commit:&str)->Value {
    let mut cohorts=BTreeMap::<CohortKey,Cases>::new();
    for g in history {
        let (Some(id),Some(profile),Some(generator))=(
            g["candidateId"].as_str(),
            g["evaluationProfileSha256"].as_str().filter(|s|sha(s,64)),
            g["generatorCommit"].as_str().filter(|s|sha(s,40)))
        else{continue};
        if g["pairedMeasured"]!=true {continue;}
        let Some(candidate)=state.store.get("rsi_candidate",id)else{continue};
        if candidate["baseCommit"]!=generator
            || candidate["evaluation"]["source"]!=
                "server_invoked_local_paired_git_worktrees"
            || candidate["paths"]!=g["changedSourcePaths"]{continue;}
        let Some(metrics)=rsi::replayed_pair_metrics(state,&candidate)
            else{continue};
        if metrics!=g["evaluation"]["metrics"]{continue;}
        let Some(changed)=candidate["paths"].as_array()
            .filter(|rows|rows.len()==1)
            .and_then(|rows|rows[0].as_str())
            .filter(|path|rsi::safe_path(path))else{continue};
        let sources=frontier.iter().filter(|row|
            row["path"]==changed
            && row["sourceSha256"].as_str().is_some_and(|s|sha(s,64)))
            .collect::<Vec<_>>();
        // If the changed source is classified as multiple roles, no unique
        // candidate source can be blamed. Reject this confounded attribution.
        if sources.len()!=1 {continue;}
        let Some(from)=sources[0]["role"].as_str()else{continue};
        for m in metrics["measurements"].as_array().into_iter().flatten(){
            if !regressed(m){continue;}
            let (Some(domain),Some(case))=(m["domain"].as_str(),
                m["id"].as_str().filter(|s|!s.is_empty()))
                else{continue};
            let to=measured_role(domain);
            if to==from || !frontier.iter().any(|r|
                r["role"]==to && r["path"]!=changed
                && r["sourceSha256"].as_str().is_some_and(|s|sha(s,64))) {
                continue;
            }
            record(&mut cohorts,(generator.to_string(),
                profile.to_string(),from.to_string(),to.to_string()),id,case);
        }
    }
    let mut evidence=Vec::<Value>::new();
    for ((generator,profile,from,to),case) in cohorts {
        let Some(source)=frontier.iter().find(|r|
            r["role"]==from && r["path"].as_str()
                .is_some_and(rsi::safe_path))else{continue};
        let Some(victim)=frontier.iter().find(|r|
            r["role"]==to && r["path"]!=source["path"]
                && r["sourceSha256"].as_str().is_some_and(|s|sha(s,64)))
                else{continue};
        let supported=case.candidate_ids.len()>=2 && case.case_ids.len()>=2;
        evidence.push(json!({
            "sourceRole":from,"victimRole":to,
            "source":source,"victim":victim,
            "generatorSourceCommit":generator,"frozenProfileSha256":profile,
            "independentCandidateCounterexamples":case.candidate_ids.len(),
            "distinctDamagedHoldoutCases":case.case_ids.len(),
            "candidateIds":case.candidate_ids.iter().take(8).collect::<Vec<_>>().collect::<Vec<_>>(),
            "caseIds":case.case_ids.iter().take(12).collect::<Vec<_>>().collect::<Vec<_>>(),
            "supportedForPairedInvestigation":supported,
            "candidateDamageNotInheritedRegression":true,
            "causalAttributionProven":false
        }));
    }
    evidence.sort_by(|a,b|{
        b["independentCandidateCounterexamples"].as_u64().cmp(
            &a["independentCandidateCounterexamples"].as_u64())
            .then_with(||b["distinctDamagedHoldoutCases"].as_u64().cmp(
                &a["distinctDamagedHoldoutCases"].as_u64()))
            .then_with(||a["sourceRole"].as_str().cmp(&b["sourceRole"].as_str()))
            .then_with(||a["victimRole"].as_str().cmp(&b["victimRole"].as_str()))
    });
    evidence.truncate(32);
    let selected=evidence.iter().find(|row|
        row["supportedForPairedInvestigation"]==true)
        .cloned().unwrap_or(Value::Null);
    let mut plan=json!({
        "source":"compiled_replayed_negative_transfer_repair_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "selected":selected,"crossCapabilityCounterexamples":evidence,
        "requiredIndependentCandidates":2,
        "requiredDistinctHoldoutCases":2,
        "experiment":"source_only_vs_victim_only_vs_joint",
        "noClaimOfAcceptedIntelligenceRegression":true,
        "causalityProven":false,"qualityProven":false
    });
    plan["sha256"]=json!(hash(&plan));
    plan
}
pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,len:usize)->bool{
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut()else{return false};
    obj.remove("sha256");
    if plan["source"]!="compiled_replayed_negative_transfer_repair_v1"
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(len as u64)
        || plan["sha256"].as_str()!=Some(hash(&unsigned).as_str())
        || plan["noClaimOfAcceptedIntelligenceRegression"]!=true
        || plan["causalityProven"]!=false || plan["qualityProven"]!=false {
        return false;
    }
    let good=|row:&Value| row["supportedForPairedInvestigation"]==true
        && row["independentCandidateCounterexamples"].as_u64().unwrap_or(0)>=2
        && row["distinctDamagedHoldoutCases"].as_u64().unwrap_or(0)>=2
        && frontier.contains(&row["source"])
        && frontier.contains(&row["victim"])
        && row["source"]["role"]!=row["victim"]["role"]
        && row["source"]["path"]!=row["victim"]["path"];
    plan["selected"].is_null() || good(&plan["selected"])
}
pub(crate) fn instruction(plan:&Value)->String{
    let pair=&plan["selected"];
    if pair.is_null(){return String::new()}
    format!(
        "INDEPENDENT WHOLE-INTELLIGENCE NEGATIVE TRANSFER: a source-only \
         change role={} file={} repeatedly damaged another previously \
         passing capability role={} file={} under SAME frozen source/profile; \
         distinct candidates={}, damaged holdout cases={}. \
         Rewrite or experimentally contrast BOTH source files. \
         Test source-only, victim-only, and joint repairs on independently \
         frozen heldouts; never treat the rejected candidate's damage as \
         a regression of the adopted intelligence. Experimental cause is NOT \
         proven. This learning algorithm is descendant-editable.",
         pair["sourceRole"],pair["source"]["path"],pair["victimRole"],
         pair["victim"]["path"],pair["independentCandidateCounterexamples"],
         pair["distinctDamagedHoldoutCases"])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counterexamples_require_distinct_trials_and_cases_on_one_cohort(){
        let mut counts=BTreeMap::new();
        let k=("a".repeat(40),"b".repeat(64),
            "models".to_string(),"memory".to_string());
        record(&mut counts,k.clone(),"candidate-1","holdout-1");
        record(&mut counts,k.clone(),"candidate-1","holdout-1");
        assert_eq!(counts[&k].candidate_ids.len(),1);
        assert_eq!(counts[&k].case_ids.len(),1);
        record(&mut counts,k.clone(),"candidate-2","holdout-2");
        assert_eq!(counts[&k].candidate_ids.len(),2);
        assert_eq!(counts[&k].case_ids.len(),2);
        let other=("a".repeat(40),"c".repeat(64),
            "models".to_string(),"memory".to_string());
        record(&mut counts,other.clone(),"candidate-3","holdout-3");
        assert_eq!(counts[&other].candidate_ids.len(),1,
            "a changed frozen profile may not inflate the original cohort");
    }
    #[test]
    fn negative_transfer_requires_a_previously_passing_holdout(){
        assert!(!regressed(&json!({"holdout":true,"baselinePass":false,
            "candidatePass":false})));
        assert!(!regressed(&json!({"holdout":false,"baselinePass":true,
            "candidatePass":false})));
        assert!(regressed(&json!({"holdout":true,"baselinePass":true,
            "candidatePass":false})));
        assert!(regressed(&json!({"holdout":true,"baselinePass":true,
            "candidatePass":true,"metrics":[{"regressed":true}]})));
    }
}
