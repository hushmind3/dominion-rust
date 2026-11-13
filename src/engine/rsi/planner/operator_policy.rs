//! Compiled, editable experiment-selection law. The fixed coordinator owns
//! budgets, worktree isolation, evaluation and source adoption; THIS method
//! chooses which source-backed experiment-design operator runs next.
//!
//! Unlike a hard-coded A-B-A-B stage scheduler, selection credits only replayed
//! downstream measurements. Every accepted source rewrite to this law is used
//! by the NEXT compiled descendant, and can be evaluated as a meta-method.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

struct Candidate<'a>{
    id:&'a str,
    target:&'a Value,
    // Initial priors are merely a cold-start choice; measured evidence
    // dominates them. The compiled descendant may redesign this equation.
    prior:f64,
}

fn grounded(target:&Value,frontier:&[Value])->bool{
    let Some(path)=target["path"].as_str()else{return false};
    let Some(sha)=target["sourceSha256"].as_str()else{return false};
    crate::engine::rsi::safe_path(path) && sha.len()==64
        && sha.bytes().all(|b|b.is_ascii_hexdigit())
        && frontier.iter().any(|row|row["path"]==path
            && row["sourceSha256"]==sha)
}

fn returns(history:&[Value],name:&str)->(u64,u64,u64){
    let mut trials=0;
    let mut wins=0;
    let mut regressions=0;
    // The caller passes revalidated_method_history. A model-authored claim
    // without the original paired receipt earns no sample and no reward.
    for observation in history.iter().filter(|g|
        g["experimentOperator"]["id"]==name
        && g["pairedMeasured"]==true
        && g["experimentOperator"]["source"]
            =="compiled_replay_trained_experiment_operator_v1") {
        trials+=1;
        let metrics=&observation["evaluation"]["metrics"];
        if observation["accepted"]==true
            && observation["pairedRevalidated"]==true
            && observation["sourceLineageVerified"]==true
            && metrics["eligible"]==true
            && metrics["regressions"]==0
            && metrics["improvedHoldouts"].as_u64().unwrap_or(0)>0{
            wins+=1;
        }
        if metrics["regressions"].as_u64().unwrap_or(0)>0{
            regressions+=1;
        }
    }
    (trials,wins,regressions)
}

/// Independently measured native edits must also be on the chosen target
/// before history.rs attributes a result to this learning policy.
/// Five operator families share one reward/evidence interface. Their source
/// implementations (meta, authored programs, principles, frontier discovery)
/// are independently editable and may be extended with new operators in this
/// source module without adding another frozen campaign phase.
pub(crate) fn select(
    history:&[Value],frontier:&[Value],
    open_program:&Value,principle:&Value,meta:&Value,successor_stage:&Value,
)->Value {
    let Some(fallback)=frontier.iter().find(|row|grounded(row,frontier))
        else{return Value::Null};
    let candidates=[
        Candidate{id:"authored_experiment_program",
            target:&open_program["target"],prior:0.24},
        Candidate{id:"new_improvement_principle",
            target:&principle["target"],prior:0.16},
        Candidate{id:"inherited_successor_program",
            target:successor_stage,prior:0.17},
        Candidate{id:"meta_controller_rewrite",
            target:&meta["target"],prior:0.08},
        Candidate{id:"architecture_frontier_exploration",
            target:fallback,prior:0.0},
    ];
    let mut evaluated=Vec::new();
    for candidate in candidates.iter().filter(|c|grounded(c.target,frontier)){
        let (attempts,wins,regressions)=returns(history,candidate.id);
        // Quality credit, negative-transfer penalty and bounded exploration
        // define a policy that can itself be source-modified and compared.
        let posterior=(1.0+wins as f64)/(2.0+attempts as f64);
        let penalty=(regressions as f64*0.22)/(1.0+attempts as f64);
        let exploration=0.20/(1.0+attempts as f64).sqrt();
        let score=posterior-penalty+exploration+candidate.prior;
        evaluated.push((candidate.id,score,attempts,wins,regressions,
            candidate.target.clone()));
    }
    evaluated.sort_by(|a,b|b.1.total_cmp(&a.1).then_with(||a.0.cmp(b.0)));
    let Some((id,score,attempts,wins,regressions,target))=evaluated.first()
        else{return Value::Null};
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([
            id,target,attempts,wins,regressions,
            evaluated.iter().map(|e|json!([e.0,e.2,e.3,e.4]))
                .collect::<Vec<_>>()
        ])).unwrap_or_default()));
    json!({
        "source":"compiled_replay_trained_experiment_operator_v1",
        "id":id,
        "target":target,
        "revision":revision,
        "score":score,
        "observedTrials":attempts,
        "observedIndependentWins":wins,
        "observedRegressions":regressions,
        "candidateScores":evaluated.iter().map(|e|json!({
            "operatorId":e.0,"score":e.1,"trials":e.2,
            "measuredWins":e.3,"regressions":e.4
        })).collect::<Vec<_>>(),
        "rewardSource":"original_replayed_paired_measurements",
        "decisionOwner":"same_compiled_descendant_binary",
        "qualityClaim":"proposed_experiment_selection_not_independently_proven"
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn replayed_failure_changes_next_source_experiment_operator(){
        let target=json!({"path":"src/engine/rsi_meta_controller.rs",
            "sourceSha256":"a".repeat(64),"role":"improvement_methods",
            "domain":"improver"});
        let other=json!({"path":"src/engine/rsi_open_program.rs",
            "sourceSha256":"b".repeat(64),"role":"improvement_methods",
            "domain":"improver"});
        let frontier=vec![target.clone(),other.clone()];
        let initial=select(&[],&frontier,&json!({"target":other}),&Value::Null,
            &json!({"target":target}),&Value::Null);
        assert_eq!(initial["id"],"authored_experiment_program");
        let failures=(0..8).map(|_|json!({
            "experimentOperator":{"id":"authored_experiment_program",
                "source":"compiled_replay_trained_experiment_operator_v1"},
            "pairedMeasured":true,"accepted":false,
            "evaluation":{"metrics":{"regressions":1}}
        })).collect::<Vec<_>>();
        let next=select(&failures,&frontier,
            &json!({"target":other}),&Value::Null,&json!({"target":target}),
            &Value::Null);
        assert_ne!(next["id"],"authored_experiment_program");
    }
}
