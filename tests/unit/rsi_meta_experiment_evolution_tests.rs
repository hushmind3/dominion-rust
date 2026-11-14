use super::*;
use serde_json::json;

#[test]
fn contract_is_typed_and_executes_only_available_arms(){
    let p=propose(&[],"a".repeat(40).as_str(),false,false);
    assert_eq!(p["arm"],"explore");
    assert_eq!(p["generatorCommit"],"a".repeat(40));
    assert_eq!(p["source"],"compiled_descendant_experiment_strategy_v1");
    assert_eq!(p["observedTrials"],0);
    assert!(validate(&p,&"a".repeat(40)).is_ok());
    assert!(validate(&p,&"b".repeat(40)).is_err());
    let p=propose(&[],"a".repeat(40).as_str(),true,true);
    assert_eq!(p["arm"],"repair");
    assert_eq!(p["effectProven"],false);
}
fn run(id:&str,method:&Value,accepted:bool,gains:u64,regressions:u64)->Value{
    json!({
        "jobId":id,"generatorCommit":method["generatorCommit"],
        "candidateOrigin":"model_driven_detached_worktree",
        "pairedMeasured":true,"pairedRevalidated":accepted,
        "accepted":accepted,"tokens":1000,
        "evaluation":{"metrics":{"improvedHoldouts":gains,"regressions":regressions}},
        "experimentStrategy":method,
        "experimentStrategyApplied":{
            "source":"running_compiled_descendant_experiment_strategy_v1",
            "generatorCommit":method["generatorCommit"],
            "revision":method["revision"],"arm":method["arm"],"workerApplied":true
        }
    })
}
#[test]
fn real_accepted_outcome_mutates_and_inherits_experiment_genome(){
    let first=propose(&[],"a".repeat(40).as_str(),false,false);
    let history=vec![run("j1",&first,true,2,0)];
    let next=propose(&history,&"b".repeat(40),false,false);
    assert_ne!(next["revision"],first["revision"]);
    assert_eq!(next["parentRevision"],first["revision"]);
    assert_eq!(next["evidenceJobIds"],json!(["j1"]));
    assert!(next["genome"]["payoff_weight"].as_u64().unwrap()>=
        first["genome"]["payoff_weight"].as_u64().unwrap());
    assert!(validate(&next,&"b".repeat(40)).is_ok());
}
#[test]
fn asserted_policy_without_real_worker_execution_never_trains_meta_search(){
    let first=propose(&[],"a".repeat(40).as_str(),false,false);
    let mut fake=run("not-run",&first,true,5,0);
    fake["experimentStrategyApplied"]["workerApplied"]=json!(false);
    let next=propose(&[fake],&"b".repeat(40),false,false);
    assert_eq!(next["observedTrials"],0);
    assert!(next["parentRevision"].is_null());
    assert_eq!(next["genome"],first["genome"]);
}
#[test]
fn source_boundary_rejects_injected_genome_weights(){
    let mut p=propose(&[],"a".repeat(40).as_str(),false,false);
    p["genome"]["cost_weight"]=json!(999);
    assert!(validate(&p,&"a".repeat(40)).is_err());
    p["genome"]["cost_weight"]=json!(2);
    p["effectProven"]=json!(true);
    assert!(validate(&p,&"a".repeat(40)).is_err());
}

#[test]
fn experiment_genome_must_match_real_runnable_intervention(){
    let open=propose(&[],&"a".repeat(40),false,false);
    assert!(implementation_matches(&open,&Value::Null,&Value::Null));
    assert!(!implementation_matches(&open,&json!({"selected":{}}),&Value::Null));
    let factorial=propose(&[],&"a".repeat(40),true,false);
    let mut payload=factorial.clone();
    payload["arm"]=json!("factorial");
    assert!(implementation_matches(&payload,&json!({"selected":{"id":"test"}}),&Value::Null));
    assert!(!implementation_matches(&payload,&Value::Null,&Value::Null));
    let repaired=propose(&[],&"a".repeat(40),true,true);
    assert_eq!(repaired["arm"],"repair");
    assert!(implementation_matches(&repaired,&Value::Null,&json!({"selected":{"id":"test"}})));
}
#[test]
fn manually_altered_policy_revision_cannot_be_inherited(){
    let mut p=propose(&[],&"a".repeat(40),false,false);
    p["genome"]["novelty_weight"]=json!(7);
    assert!(validate(&p,&"a".repeat(40)).is_err());
    p["genome"]["novelty_weight"]=json!(4);
    p["revision"]=json!("0".repeat(64));
    assert!(validate(&p,&"a".repeat(40)).is_err());
}

#[test]
fn genuine_rejected_paired_experiment_changes_next_genome(){
    let initial=propose(&[],&"a".repeat(40),false,false);
    let row=run("measured-neutral",&initial,false,0,0);
    let next=propose(&[row],&"b".repeat(40),false,false);
    assert_eq!(next["observedTrials"],1);
    assert_eq!(next["parentRevision"],initial["revision"]);
    assert!(next["genome"]["novelty_weight"].as_u64().unwrap()>=
        initial["genome"]["novelty_weight"].as_u64().unwrap());
    let mut rejected=run("not-measured",&initial,false,0,0);
    rejected["pairedMeasured"]=json!(false);
    assert_eq!(propose(&[rejected],&"b".repeat(40),false,false)["observedTrials"],0);
}


#[test]
fn duplicate_persisted_job_receipts_never_create_extra_experimental_evidence() {
    let initial=propose(&[],&"a".repeat(40),true,true);
    let once=run("same-job",&initial,true,2,0);
    let a=propose(&[once.clone()],&"b".repeat(40),true,true);
    let b=propose(&[once.clone(),once.clone(),once.clone()],&"b".repeat(40),true,true);
    assert_eq!(a,b,"replayed worker receipts are not independent trials");
    assert_eq!(b["observedTrials"],1);
    assert_eq!(b["evidenceJobIds"],json!(["same-job"]));
}

#[test]
fn early_weak_results_do_not_lock_the_evolutionary_experiment_arm() {
    let first=propose(&[],&"a".repeat(40),true,true);
    assert_eq!(first["arm"],"repair");
    // A measured but unsuccessful repair must reopen exploration/factorial
    // rather than rewarding that arm for a single noisy run.
    let weak=run("real-failure",&first,false,0,0);
    let next=propose(&[weak],&"b".repeat(40),true,true);
    assert_ne!(next["arm"],"repair");
    assert_eq!(next["observedTrials"],1);
    assert!(validate(&next,&"b".repeat(40)).is_ok());
}

#[test]
fn unmeasured_or_missing_job_identity_cannot_train_rsi_strategy() {
    let first=propose(&[],&"a".repeat(40),true,true);
    let mut invalid=run("job-1",&first,true,4,0);
    invalid["jobId"]=json!("");
    let p=propose(&[invalid],&"b".repeat(40),true,true);
    assert_eq!(p["observedTrials"],0);
    assert!(p["parentRevision"].is_null());
    assert_eq!(p["genome"],first["genome"]);
}

#[test]
fn independently_measured_jobs_still_evolve_and_preserve_lineage() {
    let first=propose(&[],&"a".repeat(40),true,true);
    let second=propose(&[run("j-1",&first,false,0,1)],&"b".repeat(40),true,true);
    let third=propose(&[
        run("j-1",&first,false,0,1),
        run("j-2",&second,true,3,0),
    ],&"c".repeat(40),true,true);
    assert_eq!(third["observedTrials"],2);
    assert_eq!(third["parentRevision"],second["revision"]);
    assert!(validate(&third,&"c".repeat(40)).is_ok());
}
