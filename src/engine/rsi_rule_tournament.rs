//! Matched, independently measured policy competition for RSI.
//! A source patch only supplies a reward after the existing paired evaluator
//! validates it; competing controllers are matched on exact source and model.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

fn scope(row:&Value)->Option<String>{
    let mut values=Vec::new();
    for name in ["generatorCommit","generatorModel","generatorProviderId",
        "objectiveId","knowledgePartition","sourceBranch",
        "comparisonProfileSha256","domain"] {
        values.push(row.get(name)?.as_str().filter(|s|!s.is_empty())?);
    }
    Some(json!(values).to_string())
}
fn comparable(row:&Value)->bool {
    row["jobId"].as_str().is_some_and(|s|!s.is_empty())
        && row["tokens"].as_u64().is_some_and(|v|v>0)
        && row["editPolicy"]["revision"].as_str().is_some_and(|s|!s.is_empty())
        && row["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && row["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && row["evaluation"]["candidateId"]==row["candidateId"]
        && row["evaluation"]["evidenceSha256"].as_str()
            .is_some_and(|sha|sha.len()==64)
        && row["pairedRevalidated"]==true
        && row["modelAttributionConsistent"]==true
        && row["candidateOrigin"]=="model_driven_detached_worktree"
        && row["nativeMutation"].is_null()
        && row["tokenUsage"]["calls"].as_u64().is_some_and(|n|n>0)
        && row["evaluation"]["metrics"].is_object()
}

/// Each comparison context contributes at most one evaluation per revision;
/// retries of one policy cannot be cherry-picked into superiority evidence.
/// Two independent matching contexts must favor the challenger without a
/// contrary pair. This only promotes methods: no source adoption bypass.
pub fn compare(rows:&[&Value],reward:fn(&Value)->f64)->Value {
    let mut cohorts=BTreeMap::<String,BTreeMap<String,f64>>::new();
    let mut duplicate=BTreeSet::<String>::new();
    for row in rows {
        if !comparable(row) {continue;}
        let Some(context)=scope(row) else {continue;};
        let revision=row["editPolicy"]["revision"].as_str().unwrap();
        let entry=cohorts.entry(context.clone()).or_default();
        if entry.contains_key(revision) {
            // Multiple attempts of one method at a single fixed base would
            // let whichever run arrived first determine the winner.
            // Exclude the entire ambiguous comparison context instead.
            duplicate.insert(context);
        }else{
            entry.insert(revision.to_string(),reward(row).clamp(0.0,1.0));
        }
    }
    let mut contests=BTreeMap::<(String,String),(u64,u64,u64)>::new();
    for (context,cohort) in &cohorts {
        if duplicate.contains(context){continue;}
        let policies=cohort.iter().collect::<Vec<_>>();
        for (index,&(left,lr)) in policies.iter().enumerate() {
            for &(right,rr) in policies.iter().skip(index+1) {
                let entry=contests.entry((left.to_string(),right.to_string())).or_default();
                if *lr-*rr>0.25 {entry.0+=1;}
                else if *rr-*lr>0.25 {entry.1+=1;}
                else {entry.2+=1;}
            }
        }
    }
    let mut selected=None::<(String,String,u64)>;
    let mut comparisons=Vec::new();
    for ((a,b),(wins_a,wins_b,ties)) in contests {
        let champion=if wins_a>=2 && wins_b==0 {
            Some((a.clone(),b.clone(),wins_a))
        }else if wins_b>=2 && wins_a==0 {
            Some((b.clone(),a.clone(),wins_b))
        }else{None};
        comparisons.push(json!({"a":a,"b":b,
            "aWins":wins_a,"bWins":wins_b,"ties":ties,
            "matchedContexts":wins_a+wins_b+ties}));
        if let Some(next)=champion {
            if selected.as_ref().is_none_or(|old|next.2>old.2) {
                selected=Some(next);
            }
        }
    }
    let Some((winner,control,wins))=selected else {
        return json!({"status":"insufficient_matched_evidence",
            "promotion":false,"comparisons":comparisons});
    };
    json!({"status":"promote","promotion":true,
        "winnerRevision":winner,"controlRevision":control,
        "matchedWins":wins,"comparisons":comparisons})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reward(v:&Value)->f64{
        if v["accepted"]==true && v["evidenceVerified"]==true {1.0}
        else if v["candidateId"].is_string() {0.1}else{0.0}
    }
    fn trial(job:&str,rev:&str,commit:&str,model:&str,accepted:bool)->Value{
        json!({"jobId":job,"tokens":2300,"candidateId":job,
            "pairedRevalidated":true,"candidateOrigin":"model_driven_detached_worktree",
            "nativeMutation":null,"tokenUsage":{"calls":1},
            "modelAttributionConsistent":true,
            "sourceBranch":"dominion-sub2","domain":"learning",
            "comparisonProfileSha256":"frozen-profile",
            "accepted":accepted,"evidenceVerified":accepted,
            "generatorCommit":commit,"generatorModel":model,
            "generatorProviderId":"p","objectiveId":"goal",
            "knowledgePartition":"space",
            "editPolicy":{"revision":rev},
            "evaluation":{"candidateId":job,
                "evidenceSha256":"b".repeat(64),
                "source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":accepted}}})
    }
    #[test]
    fn two_matched_independent_wins_promote_but_one_cannot(){
        let cases=[
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","m",true),
            trial("c","base","sha2","m",false),
            trial("d","child","sha2","m",true),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs[..2],reward)["promotion"],false);
        let result=compare(&refs,reward);
        assert_eq!(result["winnerRevision"],"child");
        assert_eq!(result["matchedWins"],2);
    }
    #[test]
    fn real_flat_paired_receipts_include_neutral_controls_not_model_stories(){
        let neutral=trial("negative","base","sha1","m",false);
        assert!(comparable(&neutral),"a replayed neutral A/B trial is a valid control");
        let mut story=neutral.clone();
        story["evaluation"]["evidenceSha256"]=Value::Null;
        assert!(!comparable(&story));
        story=neutral.clone();
        story["pairedRevalidated"]=json!(false);
        assert!(!comparable(&story));
        story=neutral.clone();
        story["tokenUsage"]["calls"]=json!(0);
        assert!(!comparable(&story));
    }
    #[test]
    fn source_branch_and_frozen_profile_mismatch_never_pair_controllers() {
        let mut a=trial("a","base","sha1","m",false);
        let mut b=trial("b","rewritten","sha1","m",true);
        let c=trial("c","base","sha2","m",false);
        let d=trial("d","rewritten","sha2","m",true);
        b["comparisonProfileSha256"]=json!("different-frozen-suite");
        let rows=vec![a.clone(),b,c,d];
        let refs=rows.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
        a["sourceBranch"]=json!("main");
        assert_ne!(scope(&a),scope(&rows[0]));
    }
    #[test]
    fn repeated_same_policy_on_same_commit_does_not_cherry_pick() {
        let rows=[
            trial("a","base","sha1","m",false),
            trial("b","rewrite","sha1","m",true),
            trial("duplicated","rewrite","sha1","m",true),
            trial("c","base","sha2","m",false),
            trial("d","rewrite","sha2","m",true)
        ];
        let refs=rows.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
    }
    #[test]
    fn unmatched_model_and_contradictory_results_block_promotion(){
        let mut cases=[
            trial("a","base","sha1","m",false),
            trial("b","child","sha1","other",true),
            trial("c","base","sha2","m",true),
            trial("d","child","sha2","m",false),
        ];
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
        cases[1]["generatorModel"]=json!("m");
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
        cases[2]["evaluation"]["source"]=json!("model_story");
        let refs=cases.iter().collect::<Vec<_>>();
        assert_eq!(compare(&refs,reward)["promotion"],false);
    }
}
