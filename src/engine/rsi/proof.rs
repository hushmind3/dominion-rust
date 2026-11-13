//! Evidence-only RSI closure evaluator, separate from the mutable child planner.
//! The API must revalidate stored paired receipts before calling these functions.
use super::*;

#[path = "proof/method_lineage.rs"]
mod method_lineage;
#[path = "proof/method_contrast.rs"]
mod method_contrast;
#[path = "proof/full_cycle.rs"]
mod full_cycle;

pub fn full_cycle_report(state:&AppState,history:&[Value])->Value {
    full_cycle::report(state,history)
}


pub(crate) fn contrast_evidence(state:&AppState,generation:&Value)->Value {
    method_contrast::evidence(state,generation)
}
pub(crate) use method_lineage::{
    method_snapshot as inherited_method_snapshot,
    downstream_feedback as verified_downstream_feedback,
};


/// Empty strings are not model errors; a research-only generation sets the
/// field to "" and must not abort the whole campaign.
/// A verified generation is an adopted one whose paired comparison was
/// eligible, used at least two holdout cases and produced no regression.
pub fn generation_is_verified(generation: &Value) -> bool {
    if generation["accepted"] != true {
        return false;
    }
    let metrics = &generation["evaluation"]["metrics"];
    generation["pairedRevalidated"]==true
        && generation["evaluation"]["candidateStatus"]=="verified"
        && generation["evaluation"]["measurementScope"]=="frozen_native_cases_only"
        && metrics["eligible"] == true
        && metrics["holdoutCases"].as_u64().unwrap_or(0) >= 2
        && metrics["regressions"].as_u64().unwrap_or(0) == 0
}

/// Read the original paired-run receipts again before displaying or
/// persisting a claimed recursive capability. A mutable generation flag is
/// only a cache; it must never be the final proof.
pub fn revalidated_history(state:&AppState, history:&[Value]) -> Vec<Value> {
    let mut checked=history.iter().map(|generation| {
        let mut checked=generation.clone();
        let valid=if generation_is_verified(generation) {
            generation["candidateId"].as_str()
                .and_then(|id|state.store.get("rsi_candidate",id))
                .is_some_and(|candidate| {
                    candidate["status"]=="verified"
                        && candidate["evaluation"]["status"]=="verified"
                        && candidate["evaluation"]["metrics"]==generation["evaluation"]["metrics"]
                        && candidate["evaluation"]["evidenceSha256"]==generation["evaluation"]["evidenceSha256"]
                        && candidate["evaluation"]["id"].is_string()
                        && candidate["experimentAdoption"]["commit"]==generation["acceptedCommit"]
                        && candidate["experimentAdoption"]["generatorCommit"]==generation["generatorCommit"]
                        && candidate["experimentAdoption"]["campaignId"]==generation["campaignId"]
                        && candidate["target"]==generation["candidateTarget"]
                        && candidate["paths"]==generation["changedSourcePaths"]
                        && candidate["origin"]==generation["candidateOrigin"]
                        && source_change_recorded(generation)
                        && crate::engine::rsi::verify_paired_receipt(state,&candidate).is_ok()
                })
        } else {
            false
        };
        checked["pairedRevalidated"]=json!(valid);
        // A serialized success flag is not evidence: only replayed jobs and
        // exact original A/B/C receipts can set this later in this function.
        checked["recursiveFeedbackVerified"]=json!(false);
        checked
    }).collect::<Vec<_>>();
    for row in &mut checked {
        let contrast=method_contrast::evidence(state,row);
        row["methodContrast"]=contrast.clone();
        row["methodContrastVerified"]=json!(contrast["qualityProven"]==true);
    }
    for index in 2..checked.len(){
        let valid=verified_feedback_reuse(state,&checked[index-2..=index]);
        checked[index]["recursiveFeedbackVerified"]=json!(valid);
    }
    checked
}

/// Only a locally paired, evidence-hashed comparison can establish a
/// transferable gain. Reporting two tested domains is not proof that both
/// improved. Each improvement must be traced to its frozen holdout case.
fn generation_meta_transfer_proven(generation: &Value) -> bool {
    if !generation_is_verified(generation) {
        return false;
    }
    let evaluation = &generation["evaluation"];
    if evaluation["candidateStatus"] != "verified"
        || evaluation["measurementScope"] != "frozen_native_cases_only"
        || !evaluation["evidenceSha256"].as_str().is_some_and(|sha| {
            sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return false;
    }
    let metrics = &evaluation["metrics"];
    let Some(holdouts) = metrics["holdoutEvidence"].as_array() else {
        return false;
    };
    if holdouts.len() < 2 || metrics["holdoutCases"].as_u64() != Some(holdouts.len() as u64) {
        return false;
    }

    let mut ids = BTreeSet::new();
    let mut tested_domains = BTreeSet::new();
    let mut improved_domains = BTreeSet::new();
    let mut improved_count = 0u64;
    let mut improver_holdout_gain = false;
    for case in holdouts {
        let Some(id) = case["id"].as_str().filter(|id| !id.trim().is_empty()) else {
            return false;
        };
        let Some(domain) = case["domain"].as_str().filter(|domain| !domain.trim().is_empty()) else {
            return false;
        };
        if !ids.insert(id) {
            return false;
        }
        let (Some(before), Some(after), Some(metric_gains), Some(metric_regressions)) = (
            case["baselinePass"].as_bool(),
            case["candidatePass"].as_bool(),
            case["metricGains"].as_u64(),
            case["metricRegressions"].as_u64(),
        ) else {
            return false;
        };
        if metric_regressions != 0 || (before && !after)
            || (metric_gains > 0 && (!before || !after))
        {
            return false;
        }
        let improved = (!before && after) || metric_gains > 0;
        if case["improved"].as_bool() != Some(improved) {
            return false;
        }
        tested_domains.insert(domain);
        if improved {
            improved_count += 1;
            improved_domains.insert(domain);
        }
        if domain == "improver" && metric_gains > 0 {
            improver_holdout_gain = true;
        }
    }
    let Some(domain_rows) = metrics["holdoutDomains"].as_array() else {
        return false;
    };
    let reported_domains = domain_rows.iter()
        .map(|value| value.as_str().filter(|domain| !domain.trim().is_empty()))
        .collect::<Option<BTreeSet<_>>>();
    if reported_domains.as_ref() != Some(&tested_domains)
        || domain_rows.len() != tested_domains.len()
        || metrics["improvedHoldouts"].as_u64() != Some(improved_count)
    {
        return false;
    }
    let source_domain = generation["domain"].as_str().unwrap_or("");
    let cross_domain_gain = tested_domains.contains(source_domain)
        && improved_domains.iter().any(|domain| *domain != source_domain);
    let improver_gain = improver_holdout_gain
        && metrics["metaHoldoutGains"].as_u64().unwrap_or(0) > 0;
    cross_domain_gain || improver_gain
}

/// A measured transfer is meaningful only when it belongs to a verified
/// adoption. Incompatible legacy summary-only records fail closed.
pub fn meta_transfer_proven(history: &[Value]) -> bool {
    history.iter().any(generation_meta_transfer_proven)
}

/// Only an observed A -> B -> A lineage in actually compiled descendants can
/// close a recursive capability claim. This is not a causal ablation of the
/// meta-method: independent holdouts establish task gain, while exact changed
/// sources + executing child binaries establish recursive method inheritance.
fn valid_hex(value:&Value,len:usize)->bool {
    value.as_str().is_some_and(|s|s.len()==len &&
        s.bytes().all(|b|b.is_ascii_hexdigit()))
}

fn compiled_plan_executed(g:&Value)->bool {
    g["systemEvolution"]==true
        && g["systemBlueprintVerified"]==true
        && g["selfControllerVerified"]==true
        && valid_hex(&g["generatorBinarySha256"],64)
        && g["nativePlan"]["source"]=="compiled_generator_plan_v1"
        && g["nativePlan"]["generatorCommit"]==g["generatorCommit"]
        && g["nativePlan"]["systemAgenda"]==g["systemAgenda"]
        && g["nativePlan"]["metaPolicy"]==g["metaPolicy"]
        && g["metaPolicy"]["source"]=="compiled_editable_meta_controller_v1"
        && g["metaPolicy"]["revision"].is_string()
        && g["metaPolicyExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["metaPolicyExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["metaPolicyExecuted"]["actualExecutedRole"]==g["systemAgenda"]["target"]["role"]
}

fn source_change_recorded(g:&Value)->bool {
    let Some(paths)=g["changedSourcePaths"].as_array().filter(|v|!v.is_empty())
        else{return false;};
    let Some(hashes)=g["acceptedSourceHashes"].as_object()
        else{return false;};
    paths.len()==hashes.len() && paths.iter().all(|row|
        row.as_str().is_some_and(|path|
            crate::engine::rsi::safe_path(path)
            && valid_hex(&g["acceptedSourceHashes"][path],64)))
}

fn improved_domain(g:&Value,domain:&str)->bool {
    g["evaluation"]["metrics"]["holdoutEvidence"].as_array()
        .is_some_and(|cases|cases.iter().any(|case|{
            case["domain"]==domain && case["improved"]==true
                && case["candidatePass"]==true
                && case["metricRegressions"].as_u64()==Some(0)
                && (case["baselinePass"]==false
                    || case["metricGains"].as_u64().unwrap_or(0)>0)
        }))
}

/// The frozen adjudicator follows the original B/C jobs and original
/// independently paired candidate receipts. A claimed feedback field in
/// generation JSON is never enough: the rebuilt B binary must have executed
/// A's exact method bytes, and C must actually consume the B observation
/// before editing those same bytes into a new independently improved version.
fn verified_feedback_reuse(state:&AppState,chain:&[Value])->bool{
    let [a,b,c]=chain else{return false};
    if ![a,b,c].iter().all(|g|generation_is_verified(g)
        && source_change_recorded(g) && compiled_plan_executed(g))
        || a["acceptedCommit"]!=b["generatorCommit"]
        || b["acceptedCommit"]!=c["generatorCommit"]
        || c["candidateTarget"]!="improver"
        || c["candidateOrigin"]!="model_driven_detached_worktree" {
        return false;
    }
    let feedback=method_lineage::downstream_feedback(
        state,&chain[..2],c["generatorCommit"].as_str().unwrap_or(""));
    if !feedback.is_object(){return false;}
    let Some(method)=feedback["methodPath"].as_str() else{return false};
    let changed=c["changedSourcePaths"].as_array();
    if !changed.is_some_and(|paths|paths.len()==1
        && paths[0].as_str()==Some(method))
        || !valid_hex(&c["acceptedSourceHashes"][method],64)
        || c["acceptedSourceHashes"][method]==feedback["methodSourceSha256"]
        || c["downstreamFeedback"]!=feedback
        || c["nativePlan"]["downstreamFeedback"]!=feedback
        || c["systemAgenda"]["downstreamFeedbackFocus"]!=feedback
        || c["systemAgenda"]["target"]["path"]!=method
        || c["metaPolicyExecuted"]["downstreamFeedback"]!=feedback
        || !improved_domain(c,"improver")
        || c["evaluation"]["metrics"]["metaHoldoutGains"]
            .as_u64().unwrap_or(0)==0 {
        return false;
    }
    let Some(job)=c["jobId"].as_str().and_then(|id|
        state.store.get("job",id)) else{return false};
    if job["kind"]!="rsi_generation"
        || job["id"]!=c["jobId"]
        || job["input"]["rsiGeneratorCommit"]!=c["generatorCommit"]
        || job["input"]["rsiDownstreamFeedback"]!=feedback
        || job["input"]["rsiNativePlan"]["downstreamFeedback"]!=feedback
        || job["input"]["rsiSystemAgenda"]["downstreamFeedbackFocus"]!=feedback
        || job["rsiMetaPolicyExecuted"]!=c["metaPolicyExecuted"]
        || job["rsiMetaPolicyExecuted"]["downstreamFeedback"]!=feedback
        || job["rsiDomain"]!=c["domain"]
        || job["result"]["rsi"]["candidateId"]!=c["candidateId"]
        || !job["input"]["prompt"].as_str().is_some_and(|prompt|
            feedback["taskEvaluationSha256"].as_str().is_some_and(|hash|
                prompt.contains(hash)))
    {return false;}
    c["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id))
        .is_some_and(|candidate|{
            candidate["paths"]==c["changedSourcePaths"]
                && candidate["target"]=="improver"
                && candidate["experimentAdoption"]["commit"]==c["acceptedCommit"]
                && crate::engine::rsi::verify_paired_receipt(state,&candidate).is_ok()
        })
}

fn closing_triplet(history:&[Value])->Option<[&Value;3]> {
    history.windows(3).find_map(|chain|{
        let [parent,child,grandchild]=chain else{return None;};
        if !chain.iter().all(|g|generation_is_verified(g)
            && compiled_plan_executed(g) && source_change_recorded(g)
            && g["candidateOrigin"]=="model_driven_detached_worktree"
            && valid_hex(&g["acceptedCommit"],40)
            && valid_hex(&g["generatorCommit"],40))
        {return None;}
        if parent["domain"]!="improver"
            || child["domain"]=="improver"
            || grandchild["domain"]!="improver"
            || parent["candidateTarget"]!="improver"
            || grandchild["candidateTarget"]!="improver"
            || !method_lineage::recorded_method_edit(parent)
            || !method_lineage::recorded_method_edit(grandchild)
            || child["generatorCommit"]!=parent["acceptedCommit"]
            || grandchild["generatorCommit"]!=child["acceptedCommit"]
            || parent["generatorCommit"]==parent["acceptedCommit"]
            || child["generatorCommit"]==child["acceptedCommit"]
            || grandchild["generatorCommit"]==grandchild["acceptedCommit"]
            || !improved_domain(parent,"improver")
            || !improved_domain(child,child["domain"].as_str().unwrap_or(""))
            || !improved_domain(grandchild,"improver")
            || !chain.iter().any(|g|generation_meta_transfer_proven(g))
            // A→B→A means the actual descendant B used inherited method A,
            // and C consumed independently measured B feedback to rewrite A.
            // A source-labelled story without this replayed job link fails.
            || grandchild["recursiveFeedbackVerified"]!=true
        {return None;}
        let first=&parent["metaPolicyExecuted"]["actualExecutedRole"];
        let second=&child["metaPolicyExecuted"]["actualExecutedRole"];
        let third=&grandchild["metaPolicyExecuted"]["actualExecutedRole"];
        // Observed executed focus, not merely a declared source pair.
        if !first.is_string() || *first!=*third || *first==*second {
            return None;
        }
        Some([parent,child,grandchild])
    })
}

/// Durable, independently revalidated evidence to inspect in the API.
/// A good aggregate benchmark is not proof that the inherited improver ran.
pub fn recursive_closure_report(history:&[Value])->Value {
    let Some([a,b,c])=closing_triplet(history) else {
        return json!({"source":"compiled_descendant_recursive_closure_v1",
            "closedLoopObserved":false,"checkedGenerations":history.len(),
            "reason":"missing_measured_meta_to_task_to_meta_compiled_lineage",
            "causalAblationProven":false});
    };
    json!({"source":"compiled_descendant_recursive_closure_v1",
        "closedLoopObserved":true,
        "checkedGenerations":history.len(),
        "sequence":["improver","task_capability","improver"],
        "generatorCommits":[a["generatorCommit"],b["generatorCommit"],c["generatorCommit"]],
        "acceptedCommits":[a["acceptedCommit"],b["acceptedCommit"],c["acceptedCommit"]],
        "compiledBinarySha256":[a["generatorBinarySha256"],b["generatorBinarySha256"],c["generatorBinarySha256"]],
        "executedRoles":[a["metaPolicyExecuted"]["actualExecutedRole"],
            b["metaPolicyExecuted"]["actualExecutedRole"],
            c["metaPolicyExecuted"]["actualExecutedRole"]],
        "candidateIds":[a["candidateId"],b["candidateId"],c["candidateId"]],
        "independentEvaluationReceipts":[a["evaluation"]["evidenceSha256"],
            b["evaluation"]["evidenceSha256"],c["evaluation"]["evidenceSha256"]],
        "sourceEditReceipts":[a["acceptedSourceHashes"],b["acceptedSourceHashes"],
            c["acceptedSourceHashes"]],
        "crossDomainGainMeasured":true,
        "methodEvolvedTwice":true,
        "originalDescendantFeedbackReplayed":true,
        "revisedMethodSource":c["downstreamFeedback"]["methodPath"],
        "downstreamPairedReceipt":c["downstreamFeedback"]["taskEvaluationSha256"],
        "causalAblationProven":false})
}

pub fn capability_demonstrated(history:&[Value])->bool {
    recursive_closure_report(history)["closedLoopObserved"]==true
}

/// Longest run of verified adopted generations. Capability is derived from
/// measured evidence, not from the accepted counter alone.
pub fn verified_capability_streak(history: &[Value]) -> (u64, u64) {
    let mut streak = 0u64;
    let mut max = 0u64;
    for generation in history {
        if generation_is_verified(generation) {
            streak += 1;
            max = max.max(streak);
        } else {
            // An intervening research-only, failed or inconclusive generation
            // breaks an *uninterrupted* verified adoption lineage.
            streak = 0;
        }
    }
    (streak, max)
}


#[cfg(test)]
mod closure_tests {
    use super::*;

    fn measured(id:&str,domain:&str,improved:bool)->Value {
        json!({"id":id,"domain":domain,
            "baselinePass":!improved,"candidatePass":true,
            "metricGains":0,"metricRegressions":0,"improved":improved})
    }
    fn generation(from:&str,to:&str,domain:&str,role:&str,path:&str,id:&str,
        transfer:bool)->Value {
        let case2=if domain=="improver"{"context"}else{"improver"};
        let role_focus=json!({"target":{"role":role}});
        let policy=json!({"source":"compiled_editable_meta_controller_v1",
            "revision":format!("meta-{id}")});
        let mut g=json!({
            "id":id,"jobId":id,"campaignId":"campaign",
            "accepted":true,"pairedRevalidated":true,
            "recursiveFeedbackVerified":true,
            "candidateId":format!("candidate-{id}"),
            "candidateOrigin":"model_driven_detached_worktree",
            "domain":domain,"candidateTarget":if domain=="improver"{"improver"}else{"harness"},
            "generatorCommit":from,"acceptedCommit":to,
            "generatorBinarySha256":"f".repeat(64),
            "systemEvolution":true,"systemBlueprintVerified":true,
            "selfControllerVerified":true,
            "systemAgenda":role_focus,"metaPolicy":policy,
            "nativePlan":{"source":"compiled_generator_plan_v1",
                "generatorCommit":from,"systemAgenda":role_focus,
                "metaPolicy":policy},
            "metaPolicyExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":from,"metaRevision":policy["revision"],
                "actualExecutedRole":role},
            "changedSourcePaths":[path],
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":from,"generationJobId":id,
                "component":role,"targetFiles":[path]},
            "evaluation":{"candidateStatus":"verified",
                "measurementScope":"frozen_native_cases_only",
                "evidenceSha256":"e".repeat(64),
                "metrics":{"eligible":true,"holdoutCases":2,
                    "regressions":0,"holdoutDomains":[domain,case2],
                    "improvedHoldouts":if transfer{2}else{1},
                    "holdoutEvidence":[measured("case-1",domain,true),
                        measured("case-2",case2,transfer)]}}
        });
        g["acceptedSourceHashes"][path]=json!("a".repeat(64));
        g
    }

    #[test]
    fn real_meta_task_meta_compiled_lineage_is_required_for_closure() {
        let a="a".repeat(40);
        let b="b".repeat(40);
        let c="c".repeat(40);
        let d="d".repeat(40);
        let parent=generation(&a,&b,"improver","improvement_methods",
            "src/engine/rsi/meta_controller/selection.rs","p",true);
        let child=generation(&b,&c,"context","harness",
            "src/engine/jobs.rs","c",false);
        let grandchild=generation(&c,&d,"improver","improvement_methods",
            "src/engine/rsi/meta_controller/feedback.rs","g",false);
        let chain=vec![parent.clone(),child.clone(),grandchild.clone()];
        assert!(meta_transfer_proven(&chain));
        assert!(capability_demonstrated(&chain));
        let report=recursive_closure_report(&chain);
        assert_eq!(report["closedLoopObserved"],true);
        assert_eq!(report["methodEvolvedTwice"],true);
        assert_eq!(report["causalAblationProven"],false);
        let mut no_code=parent.clone();
        no_code["changedSourcePaths"]=json!(["src/engine/jobs.rs"]);
        assert!(!capability_demonstrated(&[no_code,child.clone(),grandchild.clone()]));
        let mut no_exec=child.clone();
        no_exec["metaPolicyExecuted"]=Value::Null;
        assert!(!capability_demonstrated(&[parent.clone(),no_exec,grandchild.clone()]));
        let mut wrong_binary=grandchild.clone();
        wrong_binary["nativePlan"]["generatorCommit"]=json!(b);
        assert!(!capability_demonstrated(&[parent.clone(),child.clone(),wrong_binary]));
        let mut no_feedback=grandchild.clone();
        no_feedback["recursiveFeedbackVerified"]=json!(false);
        assert!(!capability_demonstrated(&[parent.clone(),child.clone(),no_feedback]));
        let mut no_second_meta=grandchild.clone();
        no_second_meta["acceptedSourceHashes"]=json!({
            "src/engine/jobs.rs":"a".repeat(64)});
        assert!(!capability_demonstrated(&[parent.clone(),child.clone(),no_second_meta]));
        let mut no_gain=child.clone();
        no_gain["evaluation"]["metrics"]["holdoutEvidence"][0]["improved"]=json!(false);
        assert!(!capability_demonstrated(&[parent.clone(),no_gain,grandchild.clone()]));
        let mut wrong_order=grandchild.clone();
        wrong_order["metaPolicyExecuted"]["actualExecutedRole"]=json!("harness");
        assert!(!capability_demonstrated(&[parent,child,wrong_order]));
    }
}
