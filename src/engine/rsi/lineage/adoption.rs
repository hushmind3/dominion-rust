//! lineage adoption — extracted RSI responsibility, sharing only parent interfaces.
use super::*;

pub(super) fn proof_goal_reached(state:&AppState,input:&Value,record:&Value)->bool {
    input["rsiStopAfterProof"]==true
        && record["maxAcceptedStreak"].as_u64().unwrap_or(0)>=3
        && crate::engine::rsi::campaign_capability_proven(state,record)
}

/// Empty strings are not model errors; a research-only generation sets the
/// field to "" and must not abort the whole campaign.
pub(super) fn generation_model_error(current: &Value) -> Option<&str> {
    current["result"]["rsi"]["modelError"]
        .as_str()
        .filter(|error| !error.trim().is_empty())
}

pub(crate) async fn adopt(
    context: &JobContext,
    lab: &Path,
    candidate: &Value,
    base: &str,
) -> Result<String, String> {
    if candidate["status"] != "verified"
        || candidate["evaluation"]["metrics"]["eligible"] != true
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || candidate["baseCommit"] != base
    {
        return Err("독립 평가를 통과한 현재 세대 후보만 실험 계보에 채택합니다.".into());
    }
    if git(context, lab, &["rev-parse", "HEAD"]).await? != base
        || !git(context, lab, &["status", "--porcelain"])
            .await?
            .is_empty()
    {
        return Err("실험 기준 소스가 변경됐습니다.".into());
    }
    let patch = candidate["patch"].as_str().ok_or("패치 없음")?;
    if format!("{:x}", Sha256::digest(patch.as_bytes()))
        != candidate["patchSha256"].as_str().unwrap_or("")
    {
        return Err("채택 패치 해시 불일치".into());
    }
    let file = lab
        .parent()
        .ok_or("실험 경로 오류")?
        .join(format!("{}.patch", uuid::Uuid::new_v4()));
    std::fs::write(&file, patch).map_err(|e| e.to_string())?;
    let applied = git(
        context,
        lab,
        &["apply", file.to_str().ok_or("패치 경로 오류")?],
    )
    .await;
    let _ = std::fs::remove_file(file);
    applied?;
    git(context, lab, &["add", "--all"]).await?;
    git(
        context,
        lab,
        &[
            "-c",
            "user.name=DOMINION experiment",
            "-c",
            "user.email=rsi@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--no-verify",
            "-m",
            "RSI: adopt independently measured experimental candidate",
        ],
    )
    .await?;
    git(context, lab, &["rev-parse", "HEAD"]).await
}

/// The coordinator's guess is auditable; a rebuilt descendant's policy
/// may select a different domain and must then recompute its source evidence.
pub(super) fn generation_target_plan(domain:&str,selection:&str,strategy:&Value,commit:&str)->Value {
    json!({"domain":domain,"selection":selection,"strategy":strategy,"generatorCommit":commit})
}


/**
 * A compiled successor's plan is an executable command, not a suggestion
 * that the predecessor may silently replace in a second selection pass.
 * Reject stale/mismatched plans rather than falling back to parent policy.
 * The frozen evaluator remains independent of this decision.
 */
pub(super) fn compiled_system_target(
    job: &Value,
    commit: &str,
    history_len: usize,
) -> Result<Option<(String, &'static str, Value)>, String> {
    let input = &job["input"];
    if input["rsiSystemEvolution"] != true {
        return Ok(None);
    }
    let plan = &input["rsiDescendantPlan"];
    let domain = plan["domain"].as_str()
        .filter(|value| DOMAINS.contains(value))
        .ok_or("후손 시스템 계획에 실행 가능한 분야가 없습니다.")?;
    let expected = generation_target_plan(
        domain, "compiled_descendant_system_plan", &plan["strategy"], commit,
    );
    if plan["source"] != "compiled_descendant_system_plan_v1"
        || plan["generatorCommit"] != commit
        || plan["campaignId"] != input["rsiCampaignId"]
        || plan["indexId"] != input["rsiIndexId"]
        || plan["historyCount"].as_u64() != Some(history_len as u64)
        || plan["attempt"].as_u64() != Some(history_len as u64 + 1)
        || plan["systemBlueprint"] != input["rsiSystemBlueprint"]
        || plan["metaPolicy"] != input["rsiMetaPolicy"]
        || plan["peerLineages"] != input["rsiPeerLineages"]
        || plan["comparativeMode"] != input["rsiComparativeMode"]
        || plan["successorStage"] != input["rsiSuccessorStage"]
        || plan["symbioticCurriculum"] != input["rsiSymbioticCurriculum"]
        || plan["factorialExperiment"] != input["rsiFactorialExperiment"]
        || plan["negativeTransfer"] != input["rsiNegativeTransfer"]
        || plan["editPolicy"] != input["rsiEditPolicy"]
        || plan["systemBlueprint"]["generatorCommit"] != commit
        || input["rsiTargetPlan"] != expected
        || input["rsiTargetDomain"] != domain
        || plan["rankedFiles"].as_array().is_none()
    {
        return Err("컴파일된 후손 계획의 세대·색인·전략·시스템 모형이 실행 작업과 다릅니다.".into());
    }
    if input["rsiFocusSelection"]["source"] == "compiled_descendant_system_plan" {
        let ranked = plan["rankedFiles"].as_array().expect("validated ranked files");
        let listed: Vec<&str> = ranked.iter()
            .filter_map(|entry| entry["path"].as_str()).collect();
        let selected: Vec<&str> = input["rsiFocusFiles"].as_array()
            .into_iter().flatten().filter_map(Value::as_str).collect();
        if listed != selected
            || listed.iter().any(|path| !crate::engine::rsi::safe_path(path))
        {
            return Err("후손이 선택한 실제 소스 파일을 기존 코디네이터가 변경했습니다.".into());
        }
    }
    Ok(Some((domain.to_string(), "compiled_descendant_system_plan",
        plan["strategy"].clone())))
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
    job["input"]["rsiFocusGrounded"]=json!(!source_deferred
        && focus.contains("sourceSha256: ") && focus.contains("~~~"));
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
