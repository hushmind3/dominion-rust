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
    rewrite_program: Vec<Value>,
}

impl Genome {
    fn baseline() -> Self {
        Self { checkpoint_after: 3, preferred_tool: "workspace_replace_range", repair_limit: 2,
            program: crate::engine::rsi_edit_program::baseline(),
            rules:Vec::new(),rewrite_program:Vec::new() }
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
        result.rewrite_program=crate::engine::rsi_rule_rewrite::normalize(&policy["rewriteProgram"]);
        result
    }

    fn revision(&self) -> String {
        // Provenance (sourceJobId, parentRuleId) is NOT executable policy.
        // Attribute rewards to identical behavior across generations rather
        // than splitting one tested controller into many cosmetic revisions.
        let rule_behavior=self.rules.iter().filter_map(|r|r["id"].as_str())
            .collect::<Vec<_>>().join(">");
        crate::engine::memory::stable_id(&[
            "rsi.composed-edit-method.v4",
            &self.checkpoint_after.to_string(),
            self.preferred_tool,
            &self.repair_limit.to_string(),
            &self.program.join(">"),
            &rule_behavior,
            &serde_json::to_string(&self.rewrite_program).unwrap_or_default(),
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
    // The executable meta-program is inherited, mutated by measured
    // experience, then APPLIED to this generation's synthesized judgments.
    // Independent source comparison still determines whether it is superior.
    program.rewrite_program=crate::engine::rsi_rule_rewrite::evolve(
        &program.rewrite_program,last);
    let generated_rules=crate::engine::rsi_decision_rules::evolve(&program.rules,last);
    program.rules=crate::engine::rsi_rule_rewrite::apply(
        &program.rewrite_program,&generated_rules);
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
        "rewriteProgram":genome.rewrite_program,
        "rewriteProgramKind":"executable_meta_algorithm_for_rule_generation",
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

    // Independently measured rival policies on the *same source/model/goal*
    // can override UCB exploration, but only after two distinct matched
    // contexts favor the challenger without a contradictory measured pair.
    // This promotes a controller, never a source patch: source adoption still
    // requires the existing frozen paired evaluator.
    let contest=crate::engine::rsi_rule_tournament::compare(&observed,payoff);
    if contest["promotion"]==true {
        if let Some(winner)=contest["winnerRevision"].as_str() {
            if let Some((genome,_,_,_))=cohorts.get(winner) {
                let last=observed.last().copied().expect("observed cohort");
                let last_revision=Genome::from_policy(&last["editPolicy"]).revision();
                if last_revision==winner {
                    // A verified winner is a stable fallback, not an eternal
                    // hard-coded policy. Its next real failure (or a successful
                    // opportunity for measured simplification) may create a
                    // new challenger without automatically promoting it.
                    let (challenger,_) = mutate(genome.clone(),last);
                    let real_failure=last["accepted"]!=true
                        && (last["editEvidence"]["attempts"].as_u64().unwrap_or(0)>0
                            || last["ruleExecutions"].as_array()
                                .is_some_and(|rows|!rows.is_empty()));
                    let measured_success=last["accepted"]==true
                        && last["evidenceVerified"]==true
                        && (challenger.rules!=genome.rules
                            || challenger.rewrite_program!=genome.rewrite_program);
                    if (real_failure || measured_success)
                        && challenger!=*genome && !cohorts.contains_key(&challenger.revision()) {
                        let mut trial=serialize(challenger,
                            "promoted-policy-descendant-trial",winner,
                            "rechallenge_independently_promoted_controller",
                            observed.len(),1.0,last["jobId"].as_str());
                        trial["competitionEvidence"]=contest;
                        return trial;
                    }
                }
                let ancestor=observed.iter()
                    .find(|row|Genome::from_policy(&row["editPolicy"]).revision()==winner)
                    .and_then(|row|row["editPolicy"]["parentRevision"].as_str())
                    .unwrap_or("");
                let mut chosen=serialize(genome.clone(),"matched-evidence-controller",
                    ancestor,"promote_independent_matched_controller",
                    observed.len(),1.0,observed.last().and_then(|r|r["jobId"].as_str()));
                chosen["competitionEvidence"]=contest;
                return chosen;
            }
        }
    }
    // Measured, causally compared controllers win priority over another
    // speculative induced guard. Otherwise a constantly changing grammar
    // can starve an independently proven policy and prevent compounding.
    // Cross-descendant counterexamples can teach a new *condition* rather
    // than merely adjusting the last worker's threshold. This method is a
    // new experimental controller, NOT an automatically promoted winner.
    // Its action must still fire and produce a source improvement under the
    // frozen paired evaluator before it earns any future positive payoff.
    let new_guards=crate::engine::rsi_rule_induction::induce(&observed);
    if !new_guards.is_empty() {
        let mut challenger=best_genome.clone();
        let mut composed=new_guards;
        composed.extend(challenger.rules);
        challenger.rules=crate::engine::rsi_decision_rules::normalize(&json!(composed));
        let revision=challenger.revision();
        if revision!=best_revision && !cohorts.contains_key(&revision) {
            let mut trial=serialize(challenger,
                "cross-descendant-counterexample-trial",
                &best_revision,"induce_new_judgment_from_success_failure_contrast",
                observed.len(),best_score,
                observed.last().and_then(|r|r["jobId"].as_str()));
            trial["contrastEvidence"]=json!({
                "source":"independent_adopted_and_paired_rejected_descendants",
                "status":"proposal_needs_actual_descendant_trial",
                "comparedGenerations":observed.len(),
            });
            return trial;
        }
    }

    if proven>=2 && best_score>=0.9 {
        let last=observed.last().copied().expect("nonempty observations");
        // Exploitation is the default, yet a verified successful procedure
        // with observed redundant source-search admits a measurable child
        // experiment. Keep the champion source method intact; only the next
        // detached worker receives this candidate controller.
        if last["accepted"]==true && last["evidenceVerified"]==true {
            let mut challenge=best_genome.clone();
            challenge.rewrite_program=crate::engine::rsi_rule_rewrite::evolve(
                &best_genome.rewrite_program,last);
            let proposed=crate::engine::rsi_decision_rules::evolve(
                &best_genome.rules,last);
            challenge.rules=crate::engine::rsi_rule_rewrite::apply(
                &challenge.rewrite_program,&proposed);
            if (challenge.rules!=best_genome.rules
                || challenge.rewrite_program!=best_genome.rewrite_program)
                && !cohorts.contains_key(&challenge.revision()) {
                return serialize(challenge,"verified-success-compression-trial",
                    &best_revision,"compress_successful_source_edit_strategy",
                    observed.len(),best_score,last["jobId"].as_str());
            }
        }
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
                rewrite_program: generated.rewrite_program.clone(),
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
#[path = "../../../../tests/unit/engine_rsi_edit_genome_tests.rs"]
mod tests;
