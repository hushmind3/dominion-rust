//! Compositional improvement-method evolution. The model is a tool; DOMINION
//! generates and executes a new improvement controller from previous worker
//! outcomes. This module does not generate code patches or claim quality gains.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
struct Genome {
    checkpoint_after: u64,
    preferred_tool: &'static str,
    repair_limit: u64,
    program: Vec<String>,
    rules: Vec<Value>,
}

impl Genome {
    fn baseline() -> Self {
        Self { checkpoint_after: 3, preferred_tool: "workspace_replace_range", repair_limit: 2,
            program: crate::engine::rsi_edit_program::baseline(), rules:Vec::new() }
    }

    fn from_policy(policy: &Value) -> Self {
        let mut result = Self::baseline();
        result.checkpoint_after = policy["checkpointAfter"].as_u64().unwrap_or(3).clamp(2, 5);
        result.preferred_tool = match policy["preferredTool"].as_str() {
            Some("workspace_replace") => "workspace_replace",
            _ => "workspace_replace_range",
        };
        result.repair_limit = policy["repairLimit"].as_u64().unwrap_or(2).min(3);
        result.program = crate::engine::rsi_edit_program::normalize(&policy["program"]);
        result.rules = crate::engine::rsi_decision_rules::normalize(&policy["rules"]);
        result
    }

    fn revision(&self) -> String {
        crate::engine::memory::stable_id(&[
            "rsi.composed-edit-method.v2",
            &self.checkpoint_after.to_string(),
            self.preferred_tool,
            &self.repair_limit.to_string(),
            &self.program.join(">"),
            &serde_json::to_string(&self.rules).unwrap_or_default(),
        ])
    }
}

fn observed_worker(row: &Value) -> bool {
    row["jobId"].as_str().is_some_and(|id| !id.is_empty())
        && row["tokens"].as_u64().is_some_and(|n| n > 0)
        && row["editPolicy"]["method"].as_str().is_some_and(|m| !m.is_empty())
}

/// A controller now contains an executable stage program: discovery,
/// grounding, optional counterexample search, editing, optional re-grounding
/// after a failed edit and external verification. Stage transitions are driven
/// by tool receipts; the numeric thresholds and repair budgets also evolve.
/// Mutations are selected from real failure causes and accumulated payoff.
fn mutate(mut program: Genome, last: &Value) -> (Genome, &'static str) {
    let error = last["error"].as_str().unwrap_or("").to_lowercase();
    let compile_failed = last["captureError"].as_str().unwrap_or("").contains("compile")
        || error.contains("build_or_typecheck")
        || error.contains("compiler")
        || error.contains("syntax");
    let failed_ops = last["editEvidence"]["recentFailures"].as_array()
        .into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(" ").to_lowercase();
    let verified = last["editEvidence"]["verifiedWrites"].as_u64().unwrap_or(0);
    let candidate = last["candidateId"].is_string();
    let neutral = candidate && last["evaluation"]["verdict"] != "improved"
        && last["accepted"] != true;
    let attempted = last["editEvidence"]["attempts"].as_u64().unwrap_or(0);
    let mutation;
    if error.contains("token_budget") || error.contains("토큰 예산") {
        program.checkpoint_after = program.checkpoint_after.saturating_sub(1).max(2);
        program.repair_limit = program.repair_limit.saturating_sub(1);
        mutation = "shorten_discovery_and_repair_from_token_exhaustion";
    } else if compile_failed || (verified > 0 && !candidate) {
        program.repair_limit = (program.repair_limit + 1).min(3);
        // An actual patch that fails validation needs a repair budget, not
        // another speculative read-only investigation.
        mutation = "increase_verifier_guided_repair_after_real_patch";
    } else if failed_ops.contains("json") || failed_ops.contains("argument")
        || failed_ops.contains("인수") || failed_ops.contains("unchanged") {
        program.checkpoint_after = program.checkpoint_after.saturating_sub(1).max(2);
        program.preferred_tool = "workspace_replace_range";
        mutation = "compact_hash_grounded_edit_after_tool_failure";
    } else if attempted > 0 && verified == 0 {
        program.preferred_tool = if program.preferred_tool == "workspace_replace_range" {
            "workspace_replace"
        } else { "workspace_replace_range" };
        program.checkpoint_after = program.checkpoint_after.saturating_sub(1).max(2);
        mutation = "switch_edit_operator_after_failed_execution";
    } else if neutral {
        // An independently built but ineffective patch calls for more
        // grounded source context before the next behavioral hypothesis.
        program.checkpoint_after = (program.checkpoint_after + 1).min(5);
        mutation = "expand_source_evidence_after_neutral_holdout";
    } else {
        program.checkpoint_after = program.checkpoint_after.saturating_sub(1).max(2);
        program.preferred_tool = if program.preferred_tool == "workspace_replace_range" {
            "workspace_replace"
        } else { "workspace_replace_range" };
        mutation = "explore_alternative_edit_composition";
    }
    program.program = crate::engine::rsi_edit_program::mutate(&program.program, last);
    program.rules = crate::engine::rsi_decision_rules::evolve(&program.rules, last);
    (program, mutation)
}

fn serialize(genome: Genome, method: &str, parent: &str, mutation: &str,
    observations: usize, sample_score: f64, cause: Option<&str>) -> Value {
    let revision = genome.revision();
    json!({
        "method":method, "checkpointAfter":genome.checkpoint_after,
        "preferredTool":genome.preferred_tool, "repairLimit":genome.repair_limit,
        "program":genome.program, "programKind":"receipt_driven_source_edit_procedure",
        "rules":genome.rules,"ruleKind":"synthesized_boolean_execution_guards",
        "revision":revision, "parentRevision":parent,
        "mutation":mutation, "derivedFromJobId":cause,
        "observations":observations, "score":sample_score,
        "basis":"composed_executable_controller_from_real_worker_outcomes",
        "improvementProven":false
    })
}

/// Generate a new executable controller after unproductive outcomes, and
/// reselect a proven high-payoff controller when there is repeated verified
/// evidence. Proposals never enter the quality ledger as successes merely
/// because the controller was generated or executed.
pub fn evolve(history: &[Value], payoff: fn(&Value) -> f64) -> Value {
    let observed = history.iter().filter(|row| observed_worker(row)).collect::<Vec<_>>();
    let fallback=Genome::baseline();
    if observed.is_empty() {
        return serialize(fallback, "balanced", "", "bootstrap", 0, 0.0, None);
    }

    // Cohorts aggregate reward by executable revision, not cosmetic method
    // names, so changing the implementation's parameters creates a new trial.
    let mut cohorts=BTreeMap::<String,(Genome,u64,f64,u64)>::new();
    for row in &observed {
        let genome=Genome::from_policy(&row["editPolicy"]);
        let revision=genome.revision();
        let entry=cohorts.entry(revision).or_insert((genome,0,0.0,0));
        entry.1+=1;
        let causal_reward=payoff(row).clamp(0.0,1.0);
        entry.2+=causal_reward;
        if row["accepted"]==true && row["evidenceVerified"]==true
            && causal_reward>=0.95 {entry.3+=1;}
    }
    let total=observed.len() as f64;
    let mut best=None::<(Genome,String,f64,u64)>;
    for (revision,(genome,count,reward,proven)) in &cohorts {
        // A historical method that produced verified gains can be reused;
        // unlike raw UCB, successful outcomes are not erased by exploration.
        let score=reward / *count as f64
            + (0.15 * (total + 1.0).ln() / *count as f64).sqrt()
            + if *proven>=2 {0.2} else {0.0};
        if best.as_ref().is_none_or(|(_,_,prior,_)| score>*prior) {
            best=Some((genome.clone(),revision.clone(),score,*proven));
        }
    }
    let (best_genome,best_revision,best_score,proven)=best.expect("observed cohort");
    if proven>=2 && best_score>=0.9 {
        let selected_name=if best_genome==fallback {"balanced"} else {"validated-composition"};
        return serialize(best_genome, selected_name,
            &best_revision,"reuse_measured_successful_method",
            observed.len(),best_score,observed.last().and_then(|r|r["jobId"].as_str()));
    }

    let last=observed.last().copied().expect("nonempty observations");
    // Explore a variant of the most recent method unless there is a
    // verified high-performing parent. This means successive failures can
    // mutate the mutation path itself, rather than restart from a fixed list.
    let parent=if proven>0 {best_genome.clone()} else {Genome::from_policy(&last["editPolicy"])};
    let parent_revision=parent.revision();
    let (mut generated,mutation)=mutate(parent.clone(),last);
    let used=cohorts.keys().map(String::as_str).collect::<BTreeSet<_>>();
    // If a failure would repeat a previously tried genome, compose a novel
    // combination by rotating the bounded execution parameters. No static
    // catalog of three methods is consulted.
    if generated==parent || used.contains(generated.revision().as_str()) {
        for index in 0..32u64 {
            let candidate=Genome {
                checkpoint_after:2 + ((index + observed.len() as u64) % 4),
                preferred_tool:if (index + observed.len() as u64)%2==0 {
                    "workspace_replace_range"
                }else{"workspace_replace"},
                repair_limit:(index + parent.repair_limit) % 4,
                // Keep the failure-learned stage topology while exploring
                // alternative budgets/tools. Only after exhausting those
                // combinations may a proven older topology donate a stage.
                program: if index >= 16 && proven > 0 {
                    crate::engine::rsi_edit_program::recombine(
                        &generated.program, &best_genome.program)
                } else { generated.program.clone() },
                rules: generated.rules.clone(),
            };
            if !used.contains(candidate.revision().as_str()) {
                generated=candidate;
                break;
            }
        }
    }
    let revision=generated.revision();
    serialize(generated,&format!("composed-{}",&revision[..revision.len().min(10)]),
        &parent_revision,mutation,observed.len(),best_score,
        last["jobId"].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reward(v:&Value)->f64{
        if v["accepted"]==true && v["evidenceVerified"]==true {1.0}
        else if v["candidateId"].is_string() {0.1}
        else {0.0}
    }
    fn row(job:&str,policy:Value)->Value{
        json!({"jobId":job,"tokens":12000,"editPolicy":policy,
            "candidateId":null,"accepted":false,
            "editEvidence":{"verifiedWrites":0,"attempts":1}})
    }

    #[test]
    fn synthesis_mutates_real_failures_into_new_executable_controllers(){
        let initial=evolve(&[],reward);
        assert_eq!(initial["checkpointAfter"],3);
        let next=evolve(&[row("first",initial.clone())],reward);
        assert_ne!(next["revision"],initial["revision"]);
        assert!(next["method"].as_str().unwrap().starts_with("composed-"));
        assert!(next["checkpointAfter"].as_u64().unwrap()>=2);
        assert!(next["checkpointAfter"].as_u64().unwrap()<=5);
        assert!(next["repairLimit"].as_u64().unwrap()<=3);
        assert!(next["parentRevision"].is_string());
    }

    #[test]
    fn verified_patch_without_candidate_evolves_repair_algorithm(){
        let initial=evolve(&[],reward);
        let mut failure=row("real-write",initial);
        failure["editEvidence"]["verifiedWrites"]=json!(1);
        let next=evolve(&[failure],reward);
        assert_eq!(next["mutation"],"increase_verifier_guided_repair_after_real_patch");
        assert_eq!(next["repairLimit"],3);
    }

    #[test]
    fn exhausted_budget_shrinks_repair_and_changes_next_algorithm(){
        let initial=evolve(&[],reward);
        let mut failure=row("token-exhausted",initial);
        failure["error"]=json!("rsi_token_budget: exhausted");
        failure["editEvidence"]["verifiedWrites"]=json!(1);
        let next=evolve(&[failure],reward);
        assert_eq!(next["mutation"],"shorten_discovery_and_repair_from_token_exhaustion");
        assert_eq!(next["repairLimit"],1);
    }

    #[test]
    fn cannot_learn_from_claims_that_never_executed(){
        let initial=evolve(&[],reward);
        let fake=json!({"jobId":"fake","tokens":0,"editPolicy":initial,
            "accepted":true,"evidenceVerified":true});
        assert_eq!(evolve(&[fake],reward)["method"],"balanced");
    }

    #[test]
    fn repeated_independent_success_retains_a_proven_controller(){
        let policy=evolve(&[],reward);
        let mut rows=Vec::new();
        for id in ["one","two","three"]{
            let mut item=row(id,policy.clone());
            item["accepted"]=json!(true);
            item["evidenceVerified"]=json!(true);
            rows.push(item);
        }
        let selected=evolve(&rows,reward);
        assert_eq!(selected["revision"],policy["revision"]);
        assert_eq!(selected["mutation"],"reuse_measured_successful_method");
    }

    #[test]
    fn generated_source_program_inherits_failure_recovery_nodes() {
        let initial=evolve(&[],reward);
        assert!(initial["program"].as_array().unwrap().iter().any(|s|s=="ground"));
        let mut failure=row("hash-failure",initial.clone());
        failure["editEvidence"]["recentFailures"]=json!(["workspace_replace_range: sha mismatch"]);
        let repaired=evolve(&[failure],reward);
        assert!(repaired["program"].as_array().unwrap().iter().any(|s|s=="reground"));
        assert_ne!(initial["revision"],repaired["revision"],
            "changing executable stage topology must change revision");
        let mut neutral=row("neutral-holdout",repaired.clone());
        neutral["candidateId"]=json!("candidate");
        neutral["evaluation"]=json!({"verdict":"unchanged"});
        let again=evolve(&[row("hash-failure",initial),neutral],reward);
        assert!(again["program"].as_array().unwrap().iter().any(|s|s=="contrast"));
        assert!(again["program"].as_array().unwrap().iter().any(|s|s=="reground"));
        assert_eq!(again["improvementProven"],false);
    }

    #[test]
    fn three_descendant_generations_rewrite_their_own_execution_procedure() {
        let mut history=Vec::new();
        let base=evolve(&history,reward);
        let mut first=row("generation-1",base);
        first["editEvidence"]["recentFailures"]=json!(["source sha mismatch"]);
        history.push(first);
        let second=evolve(&history,reward);
        assert!(second["program"].as_array().unwrap().contains(&json!("reground")));

        let mut generated=row("generation-2",second.clone());
        generated["candidateId"]=json!("candidate-2");
        generated["evaluation"]=json!({"verdict":"unchanged"});
        history.push(generated);
        let third=evolve(&history,reward);
        assert!(third["program"].as_array().unwrap().contains(&json!("contrast")));
        assert!(third["program"].as_array().unwrap().contains(&json!("reground")));

        let mut stalled=row("generation-3",third);
        stalled["programStages"]=json!([
            {"phase":"ground","toolCalls":1},{"phase":"contrast","toolCalls":3}
        ]);
        history.push(stalled);
        let fourth=evolve(&history,reward);
        assert!(!fourth["program"].as_array().unwrap().contains(&json!("contrast")),
            "the real stage stall must not be overwritten by duplicate-genome exploration");
        assert!(fourth["program"].as_array().unwrap().contains(&json!("reground")));
        assert_eq!(fourth["improvementProven"],false);
    }

    #[test]
    fn descendants_generate_novel_predicates_not_only_stage_labels(){
        let baseline=evolve(&[],reward);
        let mut generation=row("g1",baseline);
        generation["toolTimeline"]=json!([
            {"tool":"workspace_read_range","status":"completed"},
            {"tool":"code_search","status":"completed"},
            {"tool":"code_search","status":"completed"},
            {"tool":"code_search","status":"completed"}
        ]);
        let evolved=evolve(&[generation],reward);
        assert_eq!(evolved["rules"][0]["guard"]["all"][2]["value"],3);
        assert_eq!(evolved["improvementProven"],false);
        assert!(evolved["revision"]!=evolved["parentRevision"]);
    }

    #[test]
    fn generated_methods_can_compose_beyond_three_fixed_choices(){
        let mut policy=evolve(&[],reward);
        let mut revisions=BTreeSet::new();
        let mut history=Vec::new();
        revisions.insert(policy["revision"].as_str().unwrap().to_owned());
        for i in 0..9 {
            let mut outcome=row(&format!("failure-{i}"),policy);
            outcome["editEvidence"]["recentFailures"]=json!(["workspace_replace_range: unchanged"]);
            history.push(outcome);
            policy=evolve(&history,reward);
            revisions.insert(policy["revision"].as_str().unwrap().to_owned());
        }
        assert!(revisions.len()>3,"an executable method generator must go beyond a fixed three-name catalog");
    }
}
