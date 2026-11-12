//! Replay-grounded, single-donor cross-lineage RSI method trials.
//! A compiled descendant may rewrite this learner; the independent paired
//! evaluator retains sole authority over acceptance and deployment.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn is_hash(value:&Value)->bool {
    value.as_str().is_some_and(|s|s.len()==64
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn matched(g:&Value,target:&Value,profile:&Value)->bool {
    g["pairedMeasured"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["controllerPlan"]["metaPolicy"]==g["metaPolicy"]
        && g["controllerPlan"]["architectureTarget"]==g["metaPolicy"]["target"]
        && g["controllerPlan"]["generatorCommit"]==g["generatorCommit"]
        && g["comparisonProfileSha256"]==*profile
        && g["metaPolicy"]["target"]==*target
        && g["focusFiles"].as_array().is_some_and(|rows|
            rows.iter().any(|p|p==&target["path"]))
        && g["jobId"].as_str().is_some_and(|s|!s.is_empty())
}
fn adopted_target(g:&Value,target:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["changedSourcePaths"].as_array().is_some_and(|rows|
            rows.iter().any(|p|p==&target["path"]))
        && target["path"].as_str().is_some_and(|path|
            g["acceptedSourceHashes"][path].as_str().is_some_and(|s|
                s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit())
                    && s!=target["sourceSha256"].as_str().unwrap_or("")))
}
fn cohort(history:&[Value],target:&Value,profile:&Value,donor:Option<&str>)->(u64,u64) {
    let mut seen=BTreeSet::new();
    let mut attempts=0;
    let mut wins=0;
    for g in history.iter().rev().take(256) {
        if !matched(g,target,profile){continue;}
        let method=&g["metaPolicy"]["peerMethodTransfer"];
        let treated=method["active"]==true
            && method["source"]=="compiled_single_donor_peer_transfer_v1"
            && method["selectedRecipient"]==*target
            && method["comparisonProfileSha256"]==*profile
            && method["activeDonorCandidateId"].as_str()==donor;
        let control=donor.is_none() && method["active"]!=true;
        if !treated && !control {continue;}
        let Some(job)=g["jobId"].as_str()else{continue};
        if !seen.insert(job.to_owned()) {continue;}
        attempts+=1;
        wins+=u64::from(adopted_target(g,target));
    }
    (attempts,wins)
}

/// Matched donor-vs-control results change a bounded search prior, never a
/// success label. A negatively transferred method is remembered but not
/// automatically retried. Exactly one donor may be active for a generation.
pub(crate) fn learn(mut policy:Value,peers:&Value,history:&[Value])->Value {
    if policy["source"]!="compiled_editable_meta_controller_v1"
        || policy["predictionRuleRepair"]["repairRuleSourceSelected"]==true
        || policy["successorStage"].is_object()
        || peers["source"]!="paired_cross_campaign_reference_archive_v1"
        || peers["automaticPatchReuse"]!=false
        || !is_hash(&peers["comparisonProfileSha256"]){return policy;}
    let Some(donors)=peers["references"].as_array()else{return policy};
    let Some(mut alternatives)=policy["alternatives"].as_array().cloned()
        else{return policy};
    let mut evidence=Vec::new();
    let mut best:Option<(f64,usize,Value)>=None;
    for (index,entry) in alternatives.iter().enumerate() {
        let target=&entry["target"];
        if !target["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
            || !is_hash(&target["sourceSha256"]) {continue;}
        let domain=target["domain"].as_str().unwrap_or("");
        let role=target["role"].as_str().unwrap_or("");
        let (control_n,control_success)=cohort(
            history,target,&peers["comparisonProfileSha256"],None);
        let control_p=(control_success as f64+1.0)/(control_n as f64+2.0);
        for donor in donors.iter().take(4) {
            if donor["source"]!="paired_cross_campaign_design_reference_v1"
                || donor["automaticPatchReuse"]!=false
                || donor["comparisonProfileSha256"]!=peers["comparisonProfileSha256"]
                || donor["nextDomain"]!=domain
                || donor["identicalSourceFiles"].as_u64().unwrap_or(0)==0
                || !is_hash(&donor["patchSha256"])
                || donor["roles"].as_array().is_none_or(|v|
                    v.is_empty() || v.iter().any(|r|r==role))
                || donor["alignedSourceFiles"].as_array().is_none_or(Vec::is_empty) {
                continue;
            }
            let Some(id)=donor["candidateId"].as_str().filter(|s|!s.is_empty())
                else{continue};
            let (n,success)=cohort(
                history,target,&peers["comparisonProfileSha256"],Some(id));
            let p=(success as f64+1.0)/(n as f64+2.0);
            let negative=n>=3 && control_n>=3 && p+0.12<control_p;
            let bonus=if negative{0.0}else{
                0.12*(2.0*p).clamp(0.25,1.5)
                    *(1.0+(p-control_p).max(0.0)).min(1.5)
            };
            evidence.push(json!({"donorCandidateId":id,
                "recipientRole":role,"recipientPath":target["path"],
                "recipientSourceSha256":target["sourceSha256"],
                "frozenProfile":peers["comparisonProfileSha256"],
                "transferTrials":n,"transferAdoptions":success,
                "controlTrials":control_n,"controlAdoptions":control_success,
                "negativeTransferSignal":negative,"explorationBonus":bonus,
                "causalSuperiorityProven":false}));
            if bonus>0.0 && best.as_ref().is_none_or(|(other,i,d)|
                bonus>*other || (bonus==*other && (index<*i
                    || (index==*i && id<d["candidateId"].as_str().unwrap_or(""))))) {
                best=Some((bonus,index,donor.clone()));
            }
        }
    }
    policy["peerMethodLearning"]=json!({
        "source":"source_matched_single_donor_controls_v1",
        "observations":evidence,
        "comparisonProfileSha256":peers["comparisonProfileSha256"],
        "causalSuperiorityProven":false
    });
    if let Some((bonus,index,donor))=best {
        if let Some(row)=alternatives.get_mut(index){
            row["score"]=json!(row["score"].as_f64().unwrap_or(0.0)+bonus);
            row["activeTransferDonorCandidateId"]=donor["candidateId"].clone();
            row["portableMethodExplorationBonus"]=json!(bonus);
        }
        alternatives.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .then_with(||a["role"].as_str().cmp(&b["role"].as_str())));
        let winner=alternatives.first().cloned().unwrap_or(Value::Null);
        policy["target"]=winner["target"].clone();
        policy["selectionEvidence"]=winner.clone();
        policy["alternatives"]=json!(alternatives);
        policy["forecast"]["acceptanceProbability"]=winner["predictedAdoptionProbability"].clone();
        policy["forecast"]["uncertainty"]=winner["uncertainty"].clone();
        if winner["activeTransferDonorCandidateId"]==donor["candidateId"] {
            policy["peerMethodTransfer"]=json!({
                "source":"compiled_single_donor_peer_transfer_v1",
                "active":true,"activeDonorCandidateId":donor["candidateId"],
                "selectedRecipient":winner["target"],
                "comparisonProfileSha256":peers["comparisonProfileSha256"],
                "donorAcceptedCommit":donor["acceptedCommit"],
                "donorRoles":donor["roles"],
                "methodSummary":donor["designSummary"].as_str().unwrap_or("")
                    .chars().take(600).collect::<String>(),
                "hypothesis":donor["hypothesis"].as_str().unwrap_or("")
                    .chars().take(350).collect::<String>(),
                "automaticallyApplied":false,"transferProven":false
            });
        }
    }
    let previous=policy["revision"].clone();
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([previous,policy["target"],
            policy["peerMethodLearning"],policy["peerMethodTransfer"]]))
            .unwrap_or_default())));
    policy
}
/// Only the chosen method is visible to the model's actual coding turn.
pub(crate) fn instruction(policy:&Value)->String {
    let method=&policy["peerMethodTransfer"];
    if method["active"]!=true{return String::new();}
    format!("SINGLE-DONOR CROSS-LINEAGE METHOD TRIAL: donor={}, source={}, roles={}, method={}, hypothesis={}. Author your OWN code edit in {} (source SHA {}). Do not import any donor patch. The frozen paired oracle, not this instruction, determines adoption. Compare with a source-matched donor-free control.",
        method["activeDonorCandidateId"],method["donorAcceptedCommit"],
        method["donorRoles"],method["methodSummary"],method["hypothesis"],
        method["selectedRecipient"]["path"],method["selectedRecipient"]["sourceSha256"])
}
#[cfg(test)]
mod tests {
    use super::*;
    fn target()->Value{json!({"role":"memory","domain":"memory",
        "path":"src/engine/memory.rs","sourceSha256":"b".repeat(64)})}
    fn peers()->Value{json!({"source":"paired_cross_campaign_reference_archive_v1",
        "automaticPatchReuse":false,"comparisonProfileSha256":"c".repeat(64),
        "references":[{"source":"paired_cross_campaign_design_reference_v1",
            "automaticPatchReuse":false,"comparisonProfileSha256":"c".repeat(64),
            "patchSha256":"d".repeat(64),"candidateId":"donor-1","roles":["learning"],
            "nextDomain":"memory","identicalSourceFiles":1,
            "alignedSourceFiles":["src/engine/learning.rs"],
            "acceptedCommit":"e".repeat(40),
            "designSummary":"error-driven coding"}]})}
    fn policy()->Value{json!({"source":"compiled_editable_meta_controller_v1",
        "revision":"a".repeat(64),"target":target(),
        "alternatives":[{"target":target(),"role":"memory","score":0.6,
            "predictedAdoptionProbability":0.5,"uncertainty":0.2}]})}
    fn observation(id:&str,donor:Option<&str>,accepted:bool)->Value{
        let mut p=policy();
        if let Some(id)=donor {p["peerMethodTransfer"]=json!({"active":true,
            "source":"compiled_single_donor_peer_transfer_v1",
            "activeDonorCandidateId":id,
            "selectedRecipient":target(),
            "comparisonProfileSha256":"c".repeat(64)})}
        json!({"jobId":id,"generatorCommit":"f".repeat(40),
            "pairedMeasured":true,"candidateOrigin":"model_driven_detached_worktree",
            "comparisonProfileSha256":"c".repeat(64),
            "metaPolicy":p,"controllerPlan":{"metaPolicy":p,
                "architectureTarget":target(),"generatorCommit":"f".repeat(40)},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "metaPolicyExecuted":{"source":"running_compiled_descendant_binary",
                "metaRevision":p["revision"]},
            "focusFiles":["src/engine/memory.rs"],
            "accepted":accepted,"pairedRevalidated":accepted,
            "changedSourcePaths":["src/engine/memory.rs"],
            "acceptedSourceHashes":{"src/engine/memory.rs":"e".repeat(64)}})
    }
    #[test]
    fn donor_changes_trial_priority_not_adoption(){
        let mut p=policy();
        p["alternatives"].as_array_mut().unwrap().insert(0,json!({
            "target":{"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceSha256":"2".repeat(64)},
            "role":"learning","score":0.67,
            "predictedAdoptionProbability":0.5,"uncertainty":0.2}));
        let result=learn(p,&peers(),&[]);
        assert_eq!(result["target"]["role"],"memory");
        assert_eq!(result["peerMethodTransfer"]["active"],true);
        assert_eq!(result["peerMethodTransfer"]["automaticallyApplied"],false);
    }
    #[test]
    fn matched_negative_transfers_suppress_bonus(){
        let mut rows=Vec::new();
        for i in 0..3 {
            rows.push(observation(&format!("t-{i}"),Some("donor-1"),false));
            rows.push(observation(&format!("c-{i}"),None,true));
        }
        let result=learn(policy(),&peers(),&rows);
        assert_eq!(result["peerMethodLearning"]["observations"][0]["negativeTransferSignal"],true);
        assert!(result["peerMethodTransfer"].is_null());
        for row in &mut rows{row["comparisonProfileSha256"]=json!("0".repeat(64))}
        assert_eq!(learn(policy(),&peers(),&rows)
            ["peerMethodLearning"]["observations"][0]["transferTrials"],0);
    }
}
