//! strategy: source-grounded compiled descendant planning responsibility.
use super::*;

pub(crate) fn research_continuation(history: &[Value]) -> Option<String> {
    // Research is useful only when it leads to an experiment. After three
    // source-reading generations without an executable patch, force the
    // next generation to change its strategy instead of endlessly reading.
    let research_only=history.iter().rev().take(3)
        .take_while(|row|row["accepted"]!=true && row["candidateId"].is_null()
            && row["newFindings"].as_u64().unwrap_or(0)>0)
        .count();
    if research_only>=3 {return None;}
    history.last()
        .filter(|row| row["candidateId"].is_null()
            && row["newFindings"].as_u64().unwrap_or(0) > 0)
        .and_then(|row| row["domain"].as_str())
        .filter(|domain| DOMAINS.contains(domain))
        .map(str::to_string)
}

pub(crate) fn generation_target_plan(domain: &str, selection: &str, strategy: &Value, commit: &str) -> Value {
    json!({"domain":domain,"selection":selection,"strategy":strategy,"generatorCommit":commit})
}

/// A synthesized method is a next-generation executable trial, not a
/// permanent override of the search algorithm. Require that its ID is in
/// the concrete reuse decision, and preserve higher-priority policy trials.
/// A real compiler-failed parent candidate is concrete executable work.
/// The child must select its source domain before the ordinary search policy
/// can replan toward unrelated files. Match campaign/base/patch identity;
/// only a saved failed source candidate can pin a descendant's work.
pub(crate) fn pending_failed_patch_domain(
    state:&AppState,input:&Value,generator_commit:&str
)->Option<String> {
    let recovery=&input["rsiFailedPatchRecovery"];
    let id=recovery["candidateId"].as_str()?;
    let candidate=state.store.get("rsi_candidate",id)?;
    if recovery["baseCommit"]!=generator_commit
        || candidate["baseCommit"]!=generator_commit
        || candidate["experimentCampaignId"]!=input["rsiCampaignId"]
        || candidate["patchSha256"].as_str().is_none_or(str::is_empty)
        || recovery["checks"].as_array().is_none_or(Vec::is_empty) {
        return None;
    }
    let paths=candidate["paths"].as_array()?;
    let focus=recovery["focusPath"].as_str().filter(|path|
        crate::engine::rsi::safe_path(path)
        && paths.iter().any(|value|value.as_str()==Some(*path)));
    let path=focus.or_else(||paths.iter().filter_map(Value::as_str)
        .find(|path|crate::engine::rsi::safe_path(path)))?;
    Some(focus_domain_hint(path).to_string())
}

pub(crate) fn pending_method_recovery_domain(input:&Value)->Option<String> {
    let id=input["rsiDecisionInput"]["methodRecoveryCandidateId"].as_str()?;
    let selected=input["rsiDecisionInput"]["reuseDecision"]["methodIds"].as_array()?;
    if !selected.iter().any(|item|item.as_str()==Some(id)) {return None}
    input["rsiSelectedDomain"].as_str().filter(|domain|
        DOMAINS.contains(domain)).map(str::to_string)
}

pub(crate) fn select_generation_strategy(
    state: &AppState,
    history: &[Value],
    objective: &str,
    continuation: Option<String>,
    trial_id: Option<String>,
    selected_domain: Option<String>,
    generator_commit: &str,
) -> (String, &'static str, Value) {
    let active = crate::engine::algorithms::active(state, "selfimprove.search");
    // A genuine queued policy is work to execute, not another research note.
    // Continue reading in the worker prompt, but give its trial/control turn
    // priority so a four-generation budget can reach a real experiment.
    let executable_trial=trial_id.as_deref()
        .and_then(|id|state.store.get("algorithm",id))
        .is_some_and(|algorithm|matches!(algorithm["rsiTrial"]["status"].as_str(),
            Some("pending_trial"|"pending_second_trial")));
    if let Some(domain) = continuation.filter(|_|!executable_trial) {
        return (domain, "research_continuation", Value::Null);
    }
    if let Some(id) = trial_id {
        if let Some(algorithm) = state.store.get("algorithm", &id).filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["status"] != "trial_rejected"
        }) {
            if valid_domain_scores(&algorithm,history,objective).is_none() {
                let baseline=active.unwrap_or_else(||json!({
                    "implementation":"failure-clusters","parameters":{}
                }));
                let mut strategy=search_strategy(&baseline,"active");
                strategy["trialFallback"]=json!("candidate_expression_invalid_for_runtime_features");
                return (select_domain(&baseline,history,objective),"algorithm_active",strategy);
            }
            // A prior control for this exact content-addressed policy pins
            // its domain. Re-scoring after the control adds new history can
            // otherwise change the domain and prevent the policy from ever
            // reaching its comparable next-generation trial.
            let pinned=history.iter().rev().find(|row|
                row["generatorCommit"]==generator_commit
                    && row["domainSelection"]=="algorithm_active"
                    && row["searchStrategy"]["pendingCandidateId"]==algorithm["id"]
                    && row["searchStrategy"]["pendingCandidateRevision"]==algorithm["revision"]
                    && row["domain"].as_str().is_some_and(|domain|DOMAINS.contains(&domain)));
            let domain=pinned.and_then(|row|row["domain"].as_str())
                .map(str::to_string)
                .unwrap_or_else(||select_domain(&algorithm, history, objective));
            let generation = json!({"domain":domain,"generatorCommit":generator_commit});
            if comparable_trial_baseline(history, &generation, &algorithm).is_some() {
                return (domain, "algorithm_trial", search_strategy(&algorithm, "trial"));
            }
            // Repeated active-policy generations may produce no patch at all.
            // They have no frozen measurements, and can never satisfy the
            // comparable-control gate. Try the proposed policy once to let a
            // different target search actually produce a source patch. Its
            // policy trial is still INCONCLUSIVE without a verified control;
            // a separately verified patch may still improve the next binary.
            let no_patch_controls=history.iter().rev()
                .filter(|row|row["generatorCommit"]==generator_commit
                    && row["domain"]==domain
                    && row["domainSelection"]=="algorithm_active"
                    && row["searchStrategy"]["pendingCandidateId"]==algorithm["id"]
                    && row["candidateId"].is_null()
                    && row["accepted"]==false)
                .count();
            if no_patch_controls>=2 && algorithm["rsiTrial"]["trials"].as_array()
                .is_none_or(Vec::is_empty) {
                let mut exploratory=search_strategy(&algorithm,"trial");
                exploratory["trialFallback"]=json!("exploratory_after_two_no_patch_controls");
                exploratory["policyEvidence"]=json!("unpaired_exploration_not_promotion");
                return (domain,"algorithm_trial",exploratory);
            }
            // The active policy must supply a control in the domain actually
            // selected by the candidate; unrelated controls cannot test it.
            let active = active
                .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
            let mut selected = search_strategy(&active, "active");
            selected["trialFallback"] = json!("collecting_same_domain_control");
            selected["decisionSource"] = json!("candidate_target_active_control");
            selected["pendingCandidateId"] = json!(id);
            selected["pendingCandidateRevision"] = algorithm["revision"].clone();
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

/// Replan against the descendant's actual selector. A parent selected source
/// excerpt cannot be reused under a different strategy or domain; record both
/// decisions and rebuild the prompt from the original bounded ingredients.
pub(crate) fn reconcile_descendant_target(
    state:&AppState,job:&mut Value,history:&[Value],
    domain:&str,selection:&str,strategy:&Value,commit:&str,
)->Result<bool,String> {
    let actual=generation_target_plan(domain,selection,strategy,commit);
    let previous=job["input"]["rsiTargetPlan"].clone();
    if previous==actual {return Ok(false);}
    // Legacy jobs without a predeclared plan are still executable; they have
    // no frozen coordinator claim to overwrite and are not re-labelled.
    if !previous.is_object() {return Ok(false);}
    let base=job["input"]["rsiPromptBase"].as_str()
        .ok_or("rsi_target_replan: 원본 목표 프롬프트 없음")?.to_string();
    let evidence=job["input"]["rsiPromptEvidence"].as_str().unwrap_or("").to_string();
    let research=job["input"]["rsiPromptResearch"].as_str().unwrap_or("").to_string();
    job["input"]["rsiTargetDomain"]=json!(domain);
    if job["input"]["rsiAutoFocus"]==true {
        let source=PathBuf::from(job["input"]["rsiSourceDir"].as_str()
            .ok_or("rsi_target_replan: 소스 폴더 없음")?);
        let index=crate::engine::code_index::refresh(state,&source)?;
        job["input"]["rsiIndexId"]=index["id"].clone();
        let policy=strategy["id"].as_str()
            .and_then(|id|state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(state,"selfimprove.search"));
        let ranked=choose_focus_files(state,&job["input"],history,policy.as_ref());
        job["input"]["rsiFocusFiles"]=json!(ranked.iter().filter_map(|r|r["path"].as_str())
            .collect::<Vec<_>>());
        job["input"]["rsiFocusSelection"]=json!({
            "source":"descendant_indexed_evidence",
            "candidates":ranked,
            "domain":domain,
            "policyRevision":policy.as_ref().map(|p|p["revision"].clone()),
            "sourceIndexId":index["id"]
        });
        // Rebinding is mandatory after a descendant selector changes its
        // focus. Otherwise coordinator and rebuilt worker choose different
        // targets and source-edit checkpoint remains disconnected.
        bind_action_to_source(state,&mut job["input"],history);
    }
    let source=job["input"]["rsiSourceDir"].as_str()
        .ok_or("rsi_target_replan: 소스 경로 없음")?.to_string();
    let focus=focus_file_context(state,&json!({"labDir":source}),&job["input"]);
    job["input"]["rsiFocusGrounded"]=json!(focus_excerpt_has_source(&job["input"],&focus));
    let composed=format!("{base}\n\n{evidence}{research}{focus}");
    let bound=job["input"]["model"].as_str().is_some_and(|s|!s.is_empty())
        && job["input"]["providerId"].as_str().is_some_and(|s|!s.is_empty());
    let limit=job["input"]["rsiRemainingCampaignTokens"].clone();
    if bound && !generation_fits_remaining_budget(
        &json!({"limits":{"tokenBudget":limit},"tokens":0}),
        &json!({"prompt":composed})) {
        return Err("rsi_token_budget: 차세대 목표 재계획 문맥이 잔여 예산을 초과합니다.".into());
    }
    job["input"]["prompt"]=json!(composed);
    job["input"]["rsiFocusContextChars"]=json!(focus.chars().count());
    job["input"]["rsiPromptContextChars"]=json!(composed.chars().count());
    job["input"]["rsiTargetPlan"]=actual.clone();
    if job["input"]["rsiDecisionInput"].is_object() {
        job["input"]["rsiDecisionInput"]["nextDomain"]=json!(domain);
        job["input"]["rsiDecisionInput"]["decisionSource"]=json!("descendant_worker");
    }
    let receipt=json!({
        "coordinatorPlan":previous,
        "descendantPlan":actual,
        "generatorCommit":commit,
        "sourceIndexId":job["input"]["rsiIndexId"],
        "reason":"descendant_authoritative_selection"
    });
    job["rsiTargetReplan"]=receipt.clone();
    job["input"]["rsiTargetReplan"]=receipt;
    Ok(true)
}

/// A model-authored edit rule earns credit ONLY if that exact rule fired
/// before a verified source write to a file present in the independently
/// adopted post-change source ledger. A neutral paired trial is a small
/// feasibility observation, never a claimed capability gain.
