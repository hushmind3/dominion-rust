//! planner descendant: compiled descendant RSI responsibility.
use super::*;

/// The coordinator's guess is auditable; a rebuilt descendant's policy
/// may select a different domain and must then recompute its source evidence.
pub(super) fn generation_target_plan(domain:&str,selection:&str,strategy:&Value,commit:&str)->Value {
    json!({"domain":domain,"selection":selection,"strategy":strategy,"generatorCommit":commit})
}

pub(super) fn reconcile_descendant_target(
    state:&AppState,job:&mut Value,history:&[Value],
    domain:&str,selection:&str,strategy:&Value,commit:&str
)->Result<bool,String>{
    let proposed=generation_target_plan(domain,selection,strategy,commit);
    let old=job["input"]["rsiTargetPlan"].clone();
    if old==proposed {return Ok(false);}
    if !old.is_object() {return Err("차세대 대상 재계획의 기존 근거가 없습니다.".into());}
    let base=job["input"]["rsiPromptBase"].as_str()
        .ok_or("차세대 재계획에 필요한 원본 지시문이 없습니다.")?.to_string();
    let inherited=job["input"]["rsiPromptEvidence"].as_str().unwrap_or("").to_string();
    let research=job["input"]["rsiPromptResearch"].as_str().unwrap_or("").to_string();
    job["input"]["rsiTargetDomain"]=json!(domain);
    if job["input"]["rsiAutoFocus"]==true {
        let path=job["input"]["rsiSourceDir"].as_str()
            .ok_or("차세대 소스 경로 없음")?;
        let source=PathBuf::from(path);
        let index=crate::engine::code_index::refresh(state,&source)?;
        job["input"]["rsiIndexId"]=index["id"].clone();
        let policy=strategy["id"].as_str()
            .and_then(|id|state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(state,"selfimprove.search"));
        let ranked=choose_focus_files(state,&job["input"],history,policy.as_ref());
        let selected=ranked.iter().filter_map(|row|row["path"].as_str())
            .map(str::to_string).collect::<Vec<_>>();
        job["input"]["rsiFocusFiles"]=json!(selected);
        job["input"]["rsiFocusSelection"]=json!({
            "source":"descendant_indexed_evidence","domain":domain,
            "candidates":ranked,
            "policyRevision":policy.as_ref().map(|a|a["revision"].clone()),
            "sourceIndexId":index["id"]
        });
    }
    let path=job["input"]["rsiSourceDir"].as_str()
        .ok_or("차세대 소스 경로 없음")?;
    let focus=focus_file_context(state,&json!({"labDir":path}),&job["input"]);
    let bound=job["input"]["model"].as_str().is_some_and(|s|!s.is_empty())
        && job["input"]["providerId"].as_str().is_some_and(|s|!s.is_empty());
    let variants=[
        ("full",format!("{base}\n\n{inherited}{research}{focus}"),false,false),
        ("source_on_demand",format!("{base}\n\n{inherited}{research}"),true,false),
        ("research_on_demand",format!("{base}\n\n{inherited}"),true,true),
    ];
    let remaining=job["input"]["rsiRemainingCampaignTokens"].as_u64();
    let Some((mode,prompt,source_deferred,research_deferred))=variants.into_iter()
        .find(|(_,prompt,_,_)| !bound || remaining.is_none_or(|available|
            crate::engine::chat::estimate_text_tokens(prompt).saturating_add(256)<=available))
    else {return Err("rsi_token_budget: 차세대 재계획 입력 예산 부족".into())};
    job["input"]["prompt"]=json!(prompt);
    job["input"]["rsiPromptContextChars"]=json!(prompt.chars().count());
    job["input"]["rsiFocusContextChars"]=json!(
        if source_deferred {0}else{focus.chars().count()});
    job["input"]["rsiContextPlan"]=json!({
        "mode":mode,"sourceExcerptDeferred":source_deferred && !focus.is_empty(),
        "researchReadLedgerDeferred":research_deferred && !research.is_empty(),
        "evidencePreserved":true
    });
    job["input"]["rsiTargetPlan"]=proposed.clone();
    if job["input"]["rsiDecisionInput"].is_object() {
        job["input"]["rsiDecisionInput"]["nextDomain"]=json!(domain);
        job["input"]["rsiDecisionInput"]["decisionSource"]=json!("descendant_worker");
    }
    let receipt=json!({
        "coordinatorPlan":old,"descendantPlan":proposed,
        "sourceIndexId":job["input"]["rsiIndexId"],
        "reason":"descendant_authoritative_selection"
    });
    job["rsiTargetReplan"]=receipt.clone();
    job["input"]["rsiTargetReplan"]=receipt;
    Ok(true)
}

pub(super) fn select_generation_strategy(
    state: &AppState,
    history: &[Value],
    objective: &str,
    continuation: Option<String>,
    trial_id: Option<String>,
    selected_domain: Option<String>,
    generator_commit: &str,
) -> (String, &'static str, Value) {
    // Use the same candidate-scoped raw evaluation evidence when the rebuilt
    // child worker chooses a domain. Do not let stale claim fields steer it.
    let history_enriched=history_with_verified_payoffs(state,history);
    let history=&history_enriched;
    let active = crate::engine::algorithms::active(state, "selfimprove.search");
    // Research is inherited in the prompt even when a policy trial takes
    // priority. Otherwise an endlessly productive reading loop starves the
    // executable candidate forever.
    let pending_trial=trial_id.as_deref()
        .and_then(|id|state.store.get("algorithm",id))
        .is_some_and(|algorithm|matches!(
            algorithm["rsiTrial"]["status"].as_str(),
            Some("pending_trial"|"pending_second_trial")
        ));
    if let Some(domain) = continuation.filter(|_|!pending_trial) {
        return (domain, "research_continuation", Value::Null);
    }
    if let Some(id) = trial_id {
        if let Some(algorithm) = state.store.get("algorithm", &id).filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["status"] != "trial_rejected"
        }) {
            if scored_domains(&algorithm,history,objective).is_none() {
                let baseline=active.unwrap_or_else(||json!({
                    "implementation":"failure-clusters","parameters":{}
                }));
                let mut strategy=search_strategy(&baseline,"active");
                strategy["trialFallback"]=json!("candidate_expression_invalid_for_runtime_features");
                return (select_domain(&baseline,history,objective),"algorithm_active",strategy);
            }
            // Pin the candidate's target to the domain used by its previous
            // active-policy control. Recomputing the adaptive score *after*
            // control generation can switch domains and make the controlled
            // experiment permanently incomparable.
            let pinned=history.iter().rev().find(|row|{
                row["generatorCommit"]==generator_commit
                    && row["domainSelection"]=="algorithm_active"
                    && row["searchStrategy"]["controlForCandidateId"].as_str()==Some(id.as_str())
                    && row["searchStrategy"]["controlForCandidateRevision"]==algorithm["revision"]
                    && row["domain"].as_str().is_some_and(|d|
                        candidate_domains(objective).contains(&d))
            }).and_then(|row|row["domain"].as_str());
            let domain=pinned.map(str::to_string)
                .unwrap_or_else(||select_domain(&algorithm, history, objective));
            let generation = json!({"domain":domain,"generatorCommit":generator_commit});
            if comparable_trial_baseline(history, &generation, &algorithm).is_some() {
                return (domain, "algorithm_trial", search_strategy(&algorithm, "trial"));
            }
            // A barren control cannot supply paired quality evidence, even
            // after repeated attempts. After two real no-patch controls in the
            // candidate's chosen domain, run the candidate ONCE to explore a
            // different path. Its trial remains inconclusive for promotion
            // unless a separately measured active control exists.
            let barren_controls=history.iter().filter(|row|{
                row["generatorCommit"]==generator_commit
                    && row["domain"]==domain
                    && row["domainSelection"]=="algorithm_active"
                    && row["candidateId"].is_null()
                    && row["accepted"]==false
                    && row["jobId"].as_str().is_some_and(|id|!id.is_empty())
            }).count();
            if barren_controls>=2
                && algorithm["rsiTrial"]["trials"].as_array().is_none_or(Vec::is_empty) {
                let mut strategy=search_strategy(&algorithm,"trial");
                strategy["trialFallback"]=json!("exploratory_after_barren_controls");
                strategy["evidenceLevel"]=json!("unpaired_exploration_never_promotion");
                return (domain,"algorithm_trial",strategy);
            }
            // Bootstrap a comparable control in the same domain. Different
            // domains cannot prove improved targeting.
            let active = active
                .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
            let mut selected = search_strategy(&active, "active");
            selected["trialFallback"] = json!("unused_comparable_baseline_missing");
            selected["controlForCandidateId"] = algorithm["id"].clone();
            selected["controlForCandidateRevision"] = algorithm["revision"].clone();
            selected["controlDomainPinned"] = json!(domain);
            return (domain, "algorithm_active", selected);
        }
        let algorithm = active
            .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
        let mut selected = search_strategy(&algorithm, "active");
        selected["trialFallback"] = json!("candidate_missing_or_rejected");
        return (
            select_domain(&algorithm, history, objective),
            "algorithm_active",
            selected,
        );
    }
    let algorithm =
        active.unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
    if let Some(domain) = selected_domain.filter(|domain| DOMAINS.contains(&domain.as_str())) {
        let mut strategy = search_strategy(&algorithm, "active");
        strategy["decisionSource"] = json!("parent_next_decision");
        return (domain, "algorithm_active", strategy);
    }
    (
        select_domain(&algorithm, history, objective),
        "algorithm_active",
        search_strategy(&algorithm, "active"),
    )
}

/// A target derived from independent frozen paired outcomes is scoped to
/// this exact adopted revision. A descendant can synthesize its OWN edit
/// policy, but its speculative search selector must not silently overwrite
/// the measured objective before testing the next candidate.
pub(super) fn paired_descendant_target(input:&Value)->bool {
    input["rsiSourceScopeExpanded"]==true
        && input["rsiDecisionInput"]["basis"]==
            "independently_measured_post_adoption_target"
        && input["rsiSelectedDomain"]==input["rsiDecisionInput"]["targetDomain"]
        && input["rsiSelectedDomain"].as_str()
            .is_some_and(|domain|DOMAINS.contains(&domain))
}

#[cfg(test)]
mod paired_descendant_target_tests {
    use super::*;
    #[test]
    fn immutable_paired_target_outvotes_unmeasured_descendant_exploration() {
        let evidence=json!({"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"improver",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"improver"}});
        assert!(paired_descendant_target(&evidence));
        let mut mismatched=evidence.clone();
        mismatched["rsiSelectedDomain"]=json!("memory");
        assert!(!paired_descendant_target(&mismatched));
        let mut speculative=evidence;
        speculative["rsiDecisionInput"]["basis"]=json!("parent_next_decision");
        assert!(!paired_descendant_target(&speculative));
        let only_system=json!({"rsiSystemEvolution":true,"rsiSelectedDomain":"improver"});
        assert!(!paired_descendant_target(&only_system),
            "system mode cannot masquerade as independently measured target");
    }
}

/// Ask the exact rebuilt descendant to plan its next improvement. This
/// command is deliberately read-only and never starts a server or a model.
pub(super) fn system_evolution_target_hint(input:&Value, attempt:u64)->Option<String> {
    if paired_descendant_target(input) {
        input["rsiSelectedDomain"].as_str().map(str::to_string)
    } else if attempt==1 && input["rsiAutonomous"]==true {
        Some("improver".into())
    } else {None}
}

#[path = "descendant/controller.rs"]
mod controller;
pub use controller::plan_generation_from_file;

#[cfg(test)]
mod system_evolution_handoff_tests {
    use super::*;
    #[test]
    fn measured_descendant_targets_survive_but_stale_parent_hints_do_not() {
        let first=json!({"rsiSystemEvolution":true,"rsiAutonomous":true,"rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&first,1),Some("improver".into()));
        assert_eq!(system_evolution_target_hint(&first,2),None);
        let explicit=json!({"rsiSystemEvolution":true,"rsiAutonomous":false,
            "rsiObjective":"Improve memory","rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&explicit,1),None,
            "owner-defined objectives must outrank the default improver seed");
        let measured=json!({"rsiSystemEvolution":true,"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"context",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"context"}});
        assert_eq!(system_evolution_target_hint(&measured,2),Some("context".into()));
    }
}

