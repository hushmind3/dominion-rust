//! Adaptive search population for the native self-improvement worker.
//! Only measured descendant outcomes affect fitness. Exploration is not a gain.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const GENES: [&str; 8] = [
    "objective", "exploration", "holdout", "efficiency",
    "barren", "token", "regressions", "unresolved",
];
const DEFAULT: [f64; 8] = [2.0, 1.0, 1.5, 1.0, 1.0, 0.7, 1.2, 0.8];

fn number(row: &Value, key: &str, default: f64) -> f64 {
    row[key].as_f64().filter(|n| n.is_finite() && (0.0..=5.0).contains(n))
        .unwrap_or(default)
}

fn valid_commit(value: &Value) -> bool {
    value.as_str().is_some_and(|sha| sha.len() == 40
        && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Count genuine frozen A/B gains. A claimed acceptance flag, candidate record,
/// or successful compiler check does not count as a fitness observation.
fn measured_gain(row: &Value) -> Option<u64> {
    let proof = &row["acceptedProof"];
    let evaluation = &row["evaluation"];
    let frozen = &proof["evaluation"];
    let metrics = &frozen["metrics"];
    // The engine stores a *pair comparison summary* in the generation,
    // while acceptedProof contains the evaluator's original immutable record.
    // They have different shapes; connect them by their frozen receipt hash
    // and exact metric payload, never by whole-object equality.
    let receipt = frozen["evidenceSha256"].as_str()
        .filter(|hash| hash.len() == 64
            && hash.bytes().all(|byte|byte.is_ascii_hexdigit()));
    if row["accepted"] != true
        || proof["status"] != "verified"
        || receipt.is_none()
        || frozen["evidenceSha256"] != evaluation["evidenceSha256"]
        || frozen["metrics"] != evaluation["metrics"]
        || frozen["source"] != "server_invoked_local_paired_git_worktrees"
        || evaluation["candidateId"] != row["candidateId"]
        || metrics["eligible"] != true
        || metrics["regressions"].as_u64() != Some(0)
        || proof["baseCommit"] != row["generatorCommit"]
        || proof["adoptedCommit"] != row["acceptedCommit"]
        || !valid_commit(&row["generatorCommit"])
        || !valid_commit(&row["acceptedCommit"])
        || proof["candidateId"] != row["candidateId"]
        || row["candidateId"].as_str().is_none_or(str::is_empty)
    {
        return None;
    }
    metrics["improvedHoldouts"].as_u64().filter(|n| *n > 0)
}

fn source_attempt(row: &Value) -> bool {
    row["jobId"].as_str().is_some_and(|id| !id.is_empty())
        && row["domain"].as_str().is_some_and(|s| !s.is_empty())
        && row["sourceIntegrity"]["sourceClean"] == true
}

/// Select an executable search-scoring genome from a quality-diverse archive.
/// Parents are saved algorithm records, not unexecuted LLM suggestions.
/// A rejected policy is mutated; a trial-supported policy can reproduce.
/// Its offspring is still a trial hypothesis, never automatically promoted.
pub fn breed(history: &[Value], parents: &[Value], objective: &str) -> Option<Value> {
    let observed: Vec<&Value> = history.iter().rev()
        .filter(|row| source_attempt(row)).take(24).collect();
    // Autonomous campaigns intentionally use an empty selection objective
    // to keep all capability domains eligible. It is a valid stable key.
    if observed.len() < 2 {
        return None;
    }
    let objective_hash = crate::engine::memory::stable_id(&[objective]);
    let mut parent: Option<&Value> = None;
    let mut parent_rank = f64::NEG_INFINITY;
    let mut supported_parents = Vec::<&Value>::new();
    for item in parents {
        if item["kind"] != "selfimprove.search"
            || item["implementation"] != "expression"
            || item["source"] != "rsi-generated"
            || item["parameters"]["genome"].as_object().is_none()
            || item["rsiTrial"]["objectiveHash"] != objective_hash
        {
            continue;
        }
        let trials = item["rsiTrial"]["trials"].as_array();
        if trials.is_none_or(Vec::is_empty) {
            continue;
        }
        let (mut supported, mut rejected, mut inconclusive) = (0usize, 0usize, 0usize);
        let mut used_pairs = BTreeSet::new();
        for trial in trials.into_iter().flatten() {
            match trial["verdict"].as_str().unwrap_or("") {
                "supported" => {
                    let pair = (trial["jobId"].as_str(), trial["comparison"]["baselineJobId"].as_str());
                    // An unverified trial must not reserve its identifiers
                    // and exclude a subsequently verified independent pair.
                    let verified_receipt = trial["comparison"]["matchedHoldoutReceipts"] == true
                        && trial["comparison"]["holdoutLossesAgainstControl"].as_u64() == Some(0);
                    let unique_verified_pair = match pair {
                        (Some(job), Some(baseline))
                            if verified_receipt && !job.is_empty() && !baseline.is_empty() =>
                        {
                            used_pairs.insert((job.to_string(), baseline.to_string()))
                        }
                        _ => false,
                    };
                    if unique_verified_pair {
                        supported += 1;
                    } else {
                        inconclusive += 1;
                    }
                }
                "rejected" => rejected += 1,
                "inconclusive" => inconclusive += 1,
                _ => {}
            }
        }
        if supported >= 2 && item["rsiTrial"]["status"] == "adopted" {
            supported_parents.push(item);
        }
        // A measured failure remains eligible as a mutation parent but
        // cannot outrank an independently supported solution.
        let rank = supported as f64 * 6.0 - rejected as f64 * 5.0
            - inconclusive as f64 * 1.5;
        let id = item["id"].as_str().unwrap_or("");
        if rank > parent_rank || (rank == parent_rank
            && id < parent.and_then(|p| p["id"].as_str()).unwrap_or("~"))
        {
            parent_rank = rank;
            parent = Some(item);
        }
    }
    let mut weights = DEFAULT;
    if let Some(ancestor) = parent {
        for (i, key) in GENES.iter().enumerate() {
            weights[i] = number(&ancestor["parameters"]["genome"], key, DEFAULT[i]);
        }
    }
    // Preserve diversity: combine alternate gene axes from a different
    // independently measured island elite, never from an untested proposal.
    let mate = supported_parents.into_iter()
        .filter(|candidate| parent.is_none_or(|p| candidate["id"] != p["id"]))
        .filter(|candidate| candidate["parameters"]["genome"]
            != parent.map(|p|p["parameters"]["genome"].clone()).unwrap_or(Value::Null))
        .min_by(|a,b| a["id"].as_str().unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or("")));
    let crossed = if let Some(second) = mate {
        for (i, key) in GENES.iter().enumerate() {
            if i % 2 == 1 {
                weights[i] = (weights[i] + number(
                    &second["parameters"]["genome"],key,DEFAULT[i])) / 2.0;
            }
        }
        true
    } else {
        false
    };
    let mut islands = BTreeMap::<String, (usize, usize, usize)>::new();
    let mut verified = 0u64;
    let mut barren = 0usize;
    let mut compiler_failed = 0usize;
    let mut tokens = 0u64;
    let mut unique_revisions = BTreeSet::new();
    for row in &observed {
        let domain = row["domain"].as_str().unwrap_or("unknown");
        let entry = islands.entry(domain.to_string()).or_default();
        entry.0 += 1;
        if let Some(gain) = measured_gain(row) {
            verified += gain.min(16);
            entry.1 += 1;
        } else if row["sourceIntegrity"]["diffBytes"].as_u64().unwrap_or(0) == 0 {
            barren += 1;
            entry.2 += 1;
        } else if row["sourceIntegrity"]["checksPassed"] != true {
            compiler_failed += 1;
            entry.2 += 1;
        }
        tokens += row["tokens"].as_u64().unwrap_or(0).min(250_000);
        if let Some(rev) = row["generatorCommit"].as_str() {
            unique_revisions.insert(rev);
        }
    }
    let failures = barren + compiler_failed;
    let n = observed.len() as f64;
    let barren_rate = barren as f64 / n;
    let failure_rate = failures as f64 / n;
    let avg_cost = tokens as f64 / n;
    // Mutation is conditioned on the *observable* failure distribution, not
    // on the model's belief about its own performance. Each dimension alters
    // an executable scoring formula used by select_domain().
    let mut mutation = Vec::new();
    if crossed {
        mutation.push("measured_island_crossover");
    }
    if barren_rate >= 0.4 {
        weights[1] += 0.65;
        weights[4] += 0.5;
        weights[7] += 0.35;
        mutation.push("search_diversification");
    }
    if compiler_failed > 0 {
        weights[6] += 0.35;
        weights[7] += 0.45;
        mutation.push("integrity_feedback");
    }
    if verified > 0 {
        weights[2] += (verified as f64 / n).min(0.8);
        weights[3] += 0.45;
        weights[0] += 0.15;
        mutation.push("measured_holdout_gain");
    }
    if avg_cost > 10_000.0 {
        weights[5] += 0.55;
        weights[3] += 0.25;
        mutation.push("token_cost_pressure");
    }
    if islands.len() < 2 && observed.len() >= 3 {
        weights[1] += 0.4;
        mutation.push("single_domain_escape");
    }
    if unique_revisions.len() < 2 && observed.len() >= 4 {
        weights[7] += 0.3;
        mutation.push("unproductive_lineage");
    }
    if mutation.is_empty() {
        // New executable variant even when all outcomes are incomplete:
        // diversify the population rather than repeating a static formula.
        weights[1] += 0.3;
        weights[7] += 0.2;
        mutation.push("uncertainty_exploration");
    }
    // Rotate the mutation axis after a rejected trial. This is meta-learning
    // about *how* to search, not a new claim about task quality.
    if parent.is_some_and(|p| p["rsiTrial"]["status"] == "trial_rejected") {
        weights[1] += 0.4;
        weights[5] += 0.2;
        mutation.push("rejected_parent_variation");
    }
    for w in &mut weights {
        *w = w.clamp(0.1, 5.0);
    }
    let expression = format!(
        "objectiveMatch * {:.3} + exploration * {:.3} + holdoutYield * {:.3} \
         + gainPer10kTokens * {:.3} - barrenRate * {:.3} - tokenCost * {:.3} \
         - regressions * {:.3} + unresolved * {:.3}",
        weights[0], weights[1], weights[2], weights[3],
        weights[4], weights[5], weights[6], weights[7],
    );
    let genome: serde_json::Map<String, Value> = GENES.iter().enumerate()
        .map(|(i, key)| (key.to_string(), json!(weights[i]))).collect();
    Some(json!({
        "expression": expression,
        "genome": genome,
        "operator": mutation,
        "parentId": parent.map(|p|p["id"].clone()).unwrap_or(Value::Null),
        "parentRevision": parent.map(|p|p["revision"].clone()).unwrap_or(Value::Null),
        "mateId": mate.map(|p|p["id"].clone()).unwrap_or(Value::Null),
        "evidence": {
            "observedGenerations": observed.len(),
            "domains": islands.iter().map(|(domain, (attempts, wins, failures))| json!({
                "domain": domain, "attempts": attempts,
                "verifiedGains": wins, "failures": failures
            })).collect::<Vec<_>>(),
            "independentHoldoutGains": verified,
            "barrenRate": barren_rate,
            "failureRate": failure_rate,
            "meanTokens": avg_cost,
            "distinctGeneratorCommits": unique_revisions.len(),
            "verifiedFitnessOnly": true
        }
    }))
}

/// Choose an execution-level mutation from actual lineage rather than from
/// a stale prompt. Different source domains share *operators*, not assumed
/// effects. No adoption is credited until the native verifier approves it.
pub fn action(history: &[Value], domain: &str, focus: Option<&str>) -> Value {
    let local = history.iter().rev().find(|row|
        row["domain"].as_str() == Some(domain) && source_attempt(row));
    let best = history.iter().rev().find(|row|
        row["domain"].as_str() != Some(domain) && measured_gain(row).is_some());
    let recent_failures = history.iter().rev().take(6)
        .filter(|row| row["domain"].as_str() == Some(domain)
            && measured_gain(row).is_none()).count();
    let kind = if let Some(last) = local {
        if last["sourceIntegrity"]["diffBytes"].as_u64().unwrap_or(0) > 0
            && last["sourceIntegrity"]["checksPassed"] != true {
            "repair_real_source_failure"
        } else if last["accepted"] == true && measured_gain(last).is_some() {
            "extend_measured_capability"
        } else if last["newFindings"].as_u64().unwrap_or(0) > 0
            && last["candidateId"].is_null() {
            "implement_observed_research"
        } else if recent_failures >= 2 {
            "mutate_failed_approach"
        } else {
            "implement_targeted_change"
        }
    } else if best.is_some() {
        "transfer_proven_operator"
    } else {
        "implement_targeted_change"
    };
    json!({
        "kind": kind, "domain": domain, "focus": focus,
        "priorLocalJobId": local.map(|row|row["jobId"].clone()).unwrap_or(Value::Null),
        "priorVerifiedTransferJobId": best.map(|row|row["jobId"].clone()).unwrap_or(Value::Null),
        "failedLocalGenerations": recent_failures,
        "hasMeasuredTransferParent": best.is_some(),
        "claim": "candidate_strategy_not_improvement_proof",
    })
}

pub fn instruction(plan: &Value) -> String {
    let focus = plan["focus"].as_str().unwrap_or("");
    let task = match plan["kind"].as_str().unwrap_or("") {
        "repair_real_source_failure" =>
            "진짜 컴파일 실패가 남았습니다. 실제 오류 줄을 다시 읽고 기존 수정안의 결함을 고쳐 재검증하세요.",
        "extend_measured_capability" =>
            "독립 holdout에서 이익이 측정된 구현을 기반으로 다음 결함을 고치세요. 같은 코드를 반복하지 마세요.",
        "implement_observed_research" =>
            "이전 세대가 관찰한 근거를 실제 실행 가능한 최소 Rust 변경으로 전환하세요.",
        "mutate_failed_approach" =>
            "동일 분야 접근이 반복 실패했습니다. 기존 도구 순서와 구현 가설을 수정해 다른 방법으로 코드를 작성하세요.",
        "transfer_proven_operator" =>
            "다른 분야에서 독립 검증된 성공의 일반적 해결 원리를 적용하세요. 이전 분야의 코드를 그대로 복사하거나 성능을 추정하지 마세요.",
        _ =>
            "실제 소스 근거에 연결되는 최소 Rust 개선을 구현하고 독립 평가를 받으세요.",
    };
    format!("\n\n[DOMINION 내부 진화 실행 결정]\n\
        선택된 대상: {focus}\n\
        새 실행 방법: {}\n\
        {task}\n\
        방법 선택 기록은 성공 보상이 아니며 실제 소스 변경·컴파일·동결 holdout이 후속 검증합니다.\n",
        plan["kind"].as_str().unwrap_or("implement_targeted_change"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observed(id: &str, domain: &str, diff: u64) -> Value {
        json!({"jobId": id, "domain": domain, "accepted": false,
            "candidateId": null, "newFindings": 0, "tokens": 15000,
            "sourceIntegrity": {"sourceClean": true, "diffBytes": diff, "checksPassed": diff > 0},
            "generatorCommit": "a".repeat(40)})
    }
    fn winner(id: &str, domain: &str) -> Value {
        let mut row = observed(id, domain, 16);
        let metrics=json!({"eligible":true,"regressions":0,"improvedHoldouts":2});
        let sha="c".repeat(64);
        let frozen=json!({"source":"server_invoked_local_paired_git_worktrees",
            "evidenceSha256":sha,"metrics":metrics});
        row["accepted"] = json!(true);
        row["candidateId"] = json!(format!("candidate-{id}"));
        row["acceptedCommit"] = json!("b".repeat(40));
        row["evaluation"] = json!({"candidateId":row["candidateId"],
            "evidenceSha256":sha,"metrics":metrics,"verdict":"improved"});
        row["acceptedProof"] = json!({"status":"verified","baseCommit":"a".repeat(40),
            "adoptedCommit":"b".repeat(40),"candidateId":row["candidateId"],
            "evaluation":frozen});
        row
    }
    #[test]
    fn repeated_real_no_patch_runs_create_an_executable_exploratory_hypothesis() {
        let history = vec![observed("one","memory",0),observed("two","memory",0)];
        let result = breed(&history,&[],"").unwrap();
        assert!(breed(&history,&[],"improve memory").is_some(),
            "explicit goals and unrestricted autonomous campaigns must both evolve");
        assert_eq!(result["evidence"]["independentHoldoutGains"],0);
        assert!(result["operator"].as_array().unwrap().contains(&json!("search_diversification")));
        assert!(result["expression"].as_str().unwrap().contains("barrenRate"));
        let empty: Vec<Value> = vec![];
        assert!(breed(&empty,&[],"improve memory").is_none());
    }
    #[test]
    fn only_verifiable_paired_evidence_is_fitness() {
        let mut counterfeit=winner("fake","memory");
        counterfeit["acceptedProof"]["evaluation"]["metrics"]["improvedHoldouts"]=json!(999);
        assert_eq!(measured_gain(&counterfeit),None);
        let authentic=winner("real","tools");
        assert_eq!(measured_gain(&authentic),Some(2));
        let result=breed(&[counterfeit,authentic],&[],"evolve tools").unwrap();
        assert_eq!(result["evidence"]["independentHoldoutGains"],2);
        assert!(result["operator"].as_array().unwrap().contains(&json!("measured_holdout_gain")));
    }
    #[test]
    fn measured_parent_is_selected_and_mutated_without_automatic_promotion() {
        let objective="improve execution";
        let goal=crate::engine::memory::stable_id(&[objective]);
        let genome=json!({"objective":2.0,"exploration":1.1,"holdout":1.8,
            "efficiency":1.3,"barren":0.7,"token":0.5,"regressions":1.0,"unresolved":1.0});
        let good=json!({"id":"a","revision":"rev-a","kind":"selfimprove.search",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"genome":genome},"rsiTrial":{
                "objectiveHash":goal,"status":"adopted",
                "trials":[
                    {"jobId":"trial-1","verdict":"supported",
                     "comparison":{"baselineJobId":"control-1",
                        "matchedHoldoutReceipts":true,"holdoutLossesAgainstControl":0}},
                    {"jobId":"trial-2","verdict":"supported",
                     "comparison":{"baselineJobId":"control-2",
                        "matchedHoldoutReceipts":true,"holdoutLossesAgainstControl":0}}
                ]}});
        let bad=json!({"id":"z","revision":"rev-z","kind":"selfimprove.search",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"genome":genome},"rsiTrial":{
                "objectiveHash":goal,"status":"trial_rejected",
                "trials":[{"verdict":"rejected"}]}});
        let result=breed(&[observed("one","memory",0),observed("two","tools",0)],
            &[bad,good],objective).unwrap();
        assert_eq!(result["parentId"],"a");
        assert_eq!(result["parentRevision"],"rev-a");
        assert_ne!(result["genome"],genome);
        assert!(result["expression"].as_str().unwrap().contains("gainPer10kTokens"));
    }
    #[test]
    fn distinct_verified_island_elites_combine_genes_but_stay_unverified() {
        let objective="";
        let key=crate::engine::memory::stable_id(&[objective]);
        let parent=|id:&str,explore:f64| {
            let genome=json!({"objective":2.0,"exploration":explore,"holdout":1.0,
                "efficiency":1.0,"barren":1.0,"token":1.0,
                "regressions":1.0,"unresolved":1.0});
            json!({"id":id,"kind":"selfimprove.search",
                "implementation":"expression","source":"rsi-generated",
                "parameters":{"genome":genome},"rsiTrial":{
                    "objectiveHash":key,"status":"adopted",
                    "trials":[
                        {"jobId":format!("{id}-t1"),"verdict":"supported",
                         "comparison":{"baselineJobId":format!("{id}-b1"),
                            "matchedHoldoutReceipts":true,"holdoutLossesAgainstControl":0}},
                        {"jobId":format!("{id}-t2"),"verdict":"supported",
                         "comparison":{"baselineJobId":format!("{id}-b2"),
                            "matchedHoldoutReceipts":true,"holdoutLossesAgainstControl":0}}
                    ]}})
        };
        let children=breed(
            &[observed("a","memory",0),observed("b","tools",0)],
            &[parent("a",1.0),parent("b",3.0)],objective).unwrap();
        assert_eq!(children["parentId"],"a");
        assert_eq!(children["mateId"],"b");
        assert!(children["operator"].as_array().unwrap()
            .contains(&json!("measured_island_crossover")));
        assert!(children["genome"]["exploration"].as_f64().unwrap()>2.0);
        assert_eq!(children["evidence"]["independentHoldoutGains"],0,
            "crossover is not proof of a new independently measured win");
    }

    #[test]
    fn action_transfers_only_a_verified_operator_without_claiming_new_gain() {
        let rows=vec![winner("old","memory")];
        let plan=action(&rows,"tools",Some("src/engine/tools.rs"));
        assert_eq!(plan["kind"],"transfer_proven_operator");
        assert_eq!(plan["claim"],"candidate_strategy_not_improvement_proof");
        assert!(instruction(&plan).contains("src/engine/tools.rs"));
        let failed=action(&[observed("a","tools",0),observed("b","tools",0)],
            "tools",None);
        assert_eq!(failed["kind"],"mutate_failed_approach");
    }
}
