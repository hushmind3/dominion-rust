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

pub fn plan_generation_from_file(path:&str)->Result<(),String> {
    let snapshot:Value=serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?)
        .map_err(|e|e.to_string())?;
    let commit=snapshot["generatorCommit"].as_str().ok_or("계획 생성기 버전이 없습니다.")?;
    if option_env!("DOMINION_BUILD_COMMIT")!=Some(commit) {
        return Err("계획 생성기 바이너리와 소스 커밋이 다릅니다.".into());
    }
    let input=&snapshot["input"];
    if input["rsiSystemEvolution"]!=true {
        return Err("전체 시스템 진화 작업만 후손 계획을 호출할 수 있습니다.".into());
    }
    let campaign=input["rsiCampaignId"].as_str().ok_or("계획 캠페인 ID가 없습니다.")?;
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]==campaign).collect::<Vec<_>>();
    history.sort_by_key(|row|row["attempt"].as_u64().unwrap_or(0));
    let history=crate::engine::rsi_system::revalidated_history(&state,&history);
    let source=input["rsiSourceDir"].as_str()
        .ok_or("후손 시스템 소스 위치가 없습니다.")?;
    let source_root=Path::new(source);
    let system_blueprint=crate::engine::rsi_system::blueprint(
        source_root,commit,&history);
    // Planning intelligence belongs to the newly compiled descendant.
    // The former coordinator never recomputes this source-grounded ranking.
    let frontier=crate::engine::rsi_system::architecture_frontier(
        &state,&system_blueprint,&history,
        input["rsiIndexId"].as_str().unwrap_or(""),source_root);
    let owner_focus=matches!(
        input["rsiFocusSelection"]["source"].as_str(),
        Some("owner_selected"|"user_selected"));
    let paired=paired_descendant_target(input);
    let peers=crate::engine::rsi_meta_controller::peer_lineages(
        &state,campaign,input,None);
    let interaction_graph=crate::engine::rsi_interaction::influence_graph(
        &history,&frontier);
    let negative_transfer=crate::engine::rsi_negative_transfer::plan(
        &state,&history,&frontier,commit);
    let authored_algebra=crate::engine::rsi_improvement_algebra::compile(
        &history,&frontier,commit);
    let method_learning=crate::engine::rsi_method_generalization::learn(
        &state,&history,&frontier,&authored_algebra,commit);
    let algebra=crate::engine::rsi_method_generalization::adapt(
        authored_algebra,&method_learning);
    let model_evolution=crate::engine::rsi_model_lineage::plan(
        &state,&history,input,commit)?;
    let science_active=input["rsiAutonomous"]==true && !owner_focus && !paired;
    // A measured B1 control or a newly rewritten A2 method reserves the
    // immediate successor for the next source-grounded intervention. Other
    // diagnostic programs remain available once this trial has completed.
    let controlled_transition=science_active && matches!(
        algebra["selected"]["intent"].as_str(),
        Some("evolve_measured_method_after_controlled_downstream_gain"
            |"run_revised_method_in_controlled_descendant"));
    let predesign=crate::engine::rsi_evolve::meta_experiment_policy::synthesize(
        &history,&frontier,&algebra,input,commit);
    let redesign=crate::engine::rsi_evolve::meta_experiment_policy::design_target(
        &predesign,commit,input);
    let meta_redesign_target=redesign.as_ref().and_then(|target|
        frontier.iter().find(|source|source["path"]==target["path"]
            && source["sourceSha256"]==target["sourceSha256"])).cloned();
    let meta_redesign_active=science_active && !controlled_transition
        && meta_redesign_target.is_some();
    let mut negative_transfer_active=science_active
        && !controlled_transition && !meta_redesign_active
        && negative_transfer["selected"]["victim"].is_object()
        && negative_transfer["selected"]["source"].is_object();
    let mut capability_active=science_active && !meta_redesign_active
        && !negative_transfer_active
        && crate::engine::rsi_method_generalization::prefer_capability_target(
            &algebra,&method_learning);
    let mut algebra_active=science_active && !meta_redesign_active
        && !negative_transfer_active
        && !capability_active && algebra["selected"]["target"].is_object();
    let raw_meta=crate::engine::rsi_meta_controller::synthesize(
        &history,&frontier,DOMAINS,
        crate::engine::rsi_system::inherited_domain(&history).as_deref());
    // Genuine isolated cross-role effects reshape the editable meta
    // algorithm BEFORE peer references or reciprocal curricula are ranked.
    let holdout_meta=crate::engine::rsi_interaction::reinforce_meta_with_holdout(
        raw_meta,&interaction_graph);
    let peer_meta=crate::engine::rsi_meta_controller::integrate_peer_evidence(
        holdout_meta,&peers);
    let reciprocal_graph=crate::engine::rsi_symbiosis::transition_graph(&history);
    let meta=crate::engine::rsi_symbiosis::apply_reciprocal_feedback(
        peer_meta,&reciprocal_graph);
    let suggested_stage=if input["rsiAutonomous"]==true && !owner_focus
        && !paired && !meta_redesign_active && !negative_transfer_active
        && !algebra_active && !capability_active {
        crate::engine::rsi_system::inherited_stage(&state,&history,&frontier)
    }else{Value::Null};
    let meta_policy=crate::engine::rsi_meta_controller::apply_successor_stage(
        meta,&suggested_stage);
    let successor_stage=if suggested_stage.is_object()
        && meta_policy["successorStage"]==suggested_stage {
        suggested_stage
    }else{Value::Null};
    // On sustained verified prediction drift, the compiled child must
    // actually try revising its prediction implementation before unrelated
    // exploratory curricula overwrite the chosen source.
    let predictor_repair_active=science_active && !controlled_transition
        && !meta_redesign_active
        && meta_policy["predictionRuleRepair"]["repairRuleSourceSelected"]==true
        && meta_policy["target"]["path"]=="src/engine/rsi_prequential.rs";
    if predictor_repair_active {
        negative_transfer_active=false;
        capability_active=false;
        algebra_active=false;
    }
    let mut architecture_target=if input["rsiAutonomous"]==true
        && !owner_focus && !paired && meta_policy["target"].is_object() {
        meta_policy["target"].clone()
    }else{frontier.first().cloned().unwrap_or(Value::Null)};
    // Frozen holdout measurements and actual candidate paths provide a
    // complementary graph to the adopted-generation A→B→A lineage. Joint
    // edits remain confounded, never evidence of a single role's causal gain.
    let coupled_portfolio=if input["rsiAutonomous"]==true
        && !owner_focus && !paired && architecture_target.is_object() {
        crate::engine::rsi_interaction::coupled_portfolio(
            &architecture_target,&frontier,&interaction_graph,DOMAINS)
    }else{Value::Null};
    // The recursive scientist does not merely collect observations.
    // It decides whether to isolate a missing contrast, invent a new
    // executable intelligence role, return an invented capability to its
    // origin, or rewrite the scientific improvement policy itself.
    let factorial=crate::engine::rsi_factorial::design(
        &history,&frontier,&interaction_graph,DOMAINS,commit);
    let science=crate::engine::rsi_recursive_scientist::plan(
        &history,&frontier,&factorial,&coupled_portfolio,commit);
    // Independently adopted model-authored improvement operators can compose
    // across any number of roles; no fixed scientist stage is needed.
    if meta_redesign_active {
        architecture_target=meta_redesign_target.clone().unwrap_or(Value::Null);
    }else if predictor_repair_active {
        architecture_target=meta_policy["target"].clone();
    }else if negative_transfer_active {
        architecture_target=negative_transfer["selected"]["victim"].clone();
    }else if algebra_active {
        architecture_target=algebra["selected"]["target"].clone();
    }else if capability_active {
        architecture_target=method_learning["suggestedCapabilityTarget"].clone();
    }else if science_active && !predictor_repair_active
        && science["phase"]!="discover_new_mechanism" {
        if let Some(first)=science["focus"].as_array().and_then(|f|f.first()){
            architecture_target=first.clone();
        }
    }
    // Cross-lineage methods influence actual source search ONLY when the
    // scientific program or predictor repair has not reserved a different
    // descendant experiment. Exactly one donor can be active.
    let mut meta_policy=meta_policy;
    if science_active && science["phase"]=="discover_new_mechanism"
        && !negative_transfer_active && !algebra_active && !capability_active
        && successor_stage.is_null() && architecture_target==meta_policy["target"] {
        meta_policy=crate::engine::rsi_peer_transfer::learn(
            meta_policy,&peers,&history);
        architecture_target=meta_policy["target"].clone();
    }
    let symbiosis=crate::engine::rsi_symbiosis::curriculum(
        &history,&frontier,&json!({"target":architecture_target}),DOMAINS);
    let inherited=if input["rsiAutonomous"]==true {
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    let frontier_hint=if input["rsiAutonomous"]==true && !paired && !owner_focus
 {
        architecture_target["domain"].as_str().filter(|domain|DOMAINS.contains(domain))
            .map(str::to_string)
    }else{None};
    let successor_hint=if paired || owner_focus {
        input["rsiSelectedDomain"].as_str().map(str::to_string)
    }else{
        (if negative_transfer_active || algebra_active || capability_active
            || (science_active && science["phase"]!="discover_new_mechanism") {
            architecture_target["domain"].as_str()
        }else{None}).map(str::to_string)
            .or_else(||successor_stage["domain"].as_str().map(str::to_string))
            .or(frontier_hint).or(inherited.clone())
            .or_else(||system_evolution_target_hint(
                input,snapshot["attempt"].as_u64().unwrap_or(0)))
    };
    let objective=selection_objective(input);
    let trial=if paired || negative_transfer_active || algebra_active
        || capability_active || (science_active
        && science["phase"]!="discover_new_mechanism") {None} else {
        input["rsiTrialAlgorithmId"].as_str().map(str::to_string)
    };
    let (domain,selection,strategy)=select_generation_strategy(
        &state,&history,&objective,None,trial,successor_hint,commit);
    let mut source_input=input.clone();
    source_input["rsiTargetDomain"]=json!(domain);
    source_input["rsiSelectedDomain"]=json!(domain);
    let policy=strategy["id"].as_str()
        .and_then(|id|state.store.get("algorithm",id))
        .or_else(||crate::engine::algorithms::active(&state,"selfimprove.search"));
    let mut ranked=choose_focus_files(&state,&source_input,&history,policy.as_ref());
    // A source-bound architecture gap is a descendant-selected candidate,
    // never an independently proven gain.
    if input["rsiAutonomous"]==true && !owner_focus && !paired
        && architecture_target["domain"]==domain {
        if let Some(path)=architecture_target["path"].as_str() {
            if crate::engine::rsi::safe_path(path) && source_root.join(path).is_file() {
                ranked.retain(|row|row["path"]!=path);
                ranked.insert(0,json!({
                    "path":path,"sourceHash":architecture_target["sourceSha256"],
                    "source":"compiled_descendant_architecture_frontier",
                    "role":architecture_target["role"],
                    "domain":domain,"sourceIndexId":input["rsiIndexId"]
                }));
            }
        }
    }
    // The compiled descendant may propose improving TWO separate intelligence
    // subsystems, not merely report an A→B edge after the fact. Current SHA
    // identity is verified by the existing source index and frozen validator.
    if input["rsiAutonomous"]==true && !owner_focus && !paired
        && successor_stage.is_null() && architecture_target["domain"]==domain
        && symbiosis["complement"]["fromPath"]==architecture_target["path"] {
        let partner=&symbiosis["complement"];
        if let Some(path)=partner["toPath"].as_str()
            .filter(|path|crate::engine::rsi::safe_path(path)
                && source_root.join(path).is_file()) {
            ranked.retain(|row|row["path"]!=path);
            ranked.insert(1,json!({
                "path":path,"sourceHash":partner["toSha256"],
                "source":"compiled_reciprocal_intelligence_curriculum",
                "role":partner["toRole"],"domain":domain,
                "sourceIndexId":input["rsiIndexId"]
            }));
        }
    }
    // The descendant can explore cross-DOMAIN causal hypotheses instead
    // of only selecting two files under the same domain heuristic. Each
    // candidate remains source-indexed and the owner-selected focus wins.
    if coupled_portfolio.is_object()
        && architecture_target["domain"]==domain && !owner_focus && !paired {
        let mut insertion=1usize;
        for member in coupled_portfolio["focus"].as_array()
            .into_iter().flatten().skip(1).take(2) {
            let Some(path)=member["path"].as_str().filter(|p|
                crate::engine::rsi::safe_path(p) && source_root.join(p).is_file())
                else{continue};
            let Some(sha)=member["sourceSha256"].as_str() else{continue};
            if crate::engine::code_index::indexed_source_hash(
                &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                .as_deref()!=Some(sha){continue;}
            ranked.retain(|row|row["path"]!=path);
            ranked.insert(insertion.min(ranked.len()),json!({
                "path":path,"sourceHash":sha,
                "source":"compiled_holdout_cross_role_portfolio",
                "role":member["role"],"domain":member["domain"],
                "sourceIndexId":input["rsiIndexId"]
            }));
            insertion+=1;
        }
    }
    // An accepted source-written operator chooses the actual next edit,
    // regardless of whether it was called A, B, C, or invented this week.
    // Its method code becomes the target when previous executions regress.
    if algebra_active {
        let row=&algebra["selected"]["target"];
        if let (Some(path),Some(sha))=(row["path"].as_str(),row["sourceSha256"].as_str()) {
            if crate::engine::rsi::safe_path(path)
                && source_root.join(path).is_file()
                && crate::engine::code_index::indexed_source_hash(
                    &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()==Some(sha) {
                ranked.retain(|entry|entry["path"]!=path);
                ranked.insert(0,json!({
                    "path":path,"sourceHash":sha,"role":row["role"],
                    "domain":row["domain"],"sourceIndexId":input["rsiIndexId"],
                    "source":"compiled_model_authored_improvement_algebra"
                }));
            }
        }
    }
    // The generated experiment is an actual editing decision, not just
    // appended prompt text. Isolated experiments MUST focus only one role;
    // accepted-source recursive stages take priority over older guesses.
    if science_active && !predictor_repair_active
        && !algebra_active && !capability_active
        && science["phase"]!="discover_new_mechanism" {
        let members=science["focus"].as_array().cloned().unwrap_or_default();
        for member in members.into_iter().rev(){
            let Some(path)=member["path"].as_str()
                .filter(|path|crate::engine::rsi::safe_path(path)
                    && source_root.join(path).is_file()) else{continue};
            let Some(sha)=member["sourceSha256"].as_str() else{continue};
            if crate::engine::code_index::indexed_source_hash(
                &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                .as_deref()!=Some(sha){continue;}
            ranked.retain(|row|row["path"]!=path);
            ranked.insert(0,json!({
                "path":path,"sourceHash":sha,"role":member["role"],
                "domain":member["domain"],
                "source":"compiled_recursive_scientific_program",
                "sourceIndexId":input["rsiIndexId"]
            }));
        }
    }
    // Repeated source-isolated holdout regressions demand a real TWO-file
    // experiment in the next compiled descendant, not another planner note.
    // Source SHA and index identity are checked again before each source edit.
    if negative_transfer_active && architecture_target["domain"]==domain {
        for (position,key) in [(0usize,"victim"),(1usize,"source")] {
            let row=&negative_transfer["selected"][key];
            let (Some(path),Some(sha))=(
                row["path"].as_str().filter(|p|crate::engine::rsi::safe_path(p)),
                row["sourceSha256"].as_str())else{continue};
            if !source_root.join(path).is_file()
                || crate::engine::code_index::indexed_source_hash(
                    &state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()!=Some(sha){continue;}
            ranked.retain(|entry|entry["path"]!=path);
            ranked.insert(position.min(ranked.len()),json!({
                "path":path,"sourceHash":sha,"role":row["role"],
                "domain":row["domain"],"sourceIndexId":input["rsiIndexId"],
                "source":"compiled_independent_negative_transfer_pair"
            }));
        }
    }
    let focus=if negative_transfer_active {
        ranked.iter().take(2).filter_map(|row|row["path"].as_str())
            .collect::<Vec<_>>()
    }else if algebra_active || capability_active
        || (science_active && !predictor_repair_active
            && matches!(science["phase"].as_str(),
        Some("isolate_left"|"isolate_right"))) {
        ranked.iter().take(1).filter_map(|row|row["path"].as_str())
            .collect::<Vec<_>>()
    }else {
        ranked.iter().filter_map(|row|row["path"].as_str())
            .filter(|p|crate::engine::rsi::safe_path(p))
            .take(8).collect::<Vec<_>>()
    };
    let directive=crate::engine::rsi_system::improvement_directive(&history);
    let archive=crate::engine::rsi_archive::select(
        &state,&history,commit,&input["rsiFrozenCases"]);
    let mut edit_policy=learned_edit_method(&state,input,&history);
    if successor_stage["source"]=="rebound_verified_model_successor_program_v1" {
        let previous=edit_policy["revision"].clone();
        if successor_stage["executionPolicy"]=="grounded_edit" {
            edit_policy["checkpointAfter"]=json!(2);
            edit_policy["preferredTool"]=json!("workspace_replace_range");
        }else{
            edit_policy["checkpointAfter"]=json!(4);
        }
        edit_policy["selectedBySuccessorStage"]=successor_stage["stageIndex"].clone();
        edit_policy["successorDesignId"]=successor_stage["designId"].clone();
        edit_policy["revision"]=json!(format!("{:x}",Sha256::digest(
            serde_json::to_vec(&json!([previous,successor_stage,commit]))
                .unwrap_or_default())));
    }
    let inherited_program=crate::engine::rsi_system::inherited_program(&state,&history);
    if let Some(program)=inherited_program.as_ref(){
        // The existing runtime obeys rsiEditPolicy.program as an actual
        // per-round tool menu, so this affects execution rather than prose.
        let source_revision=edit_policy["revision"].clone();
        edit_policy["program"]=program["stages"].clone();
        edit_policy["inheritedProgram"]=program.clone();
        edit_policy["parentGenomeRevision"]=source_revision.clone();
        edit_policy["revision"]=json!(format!("{:x}",Sha256::digest(
            serde_json::to_vec(&json!([
                source_revision,program["revision"],commit
            ])).unwrap_or_default())));
    }
    // Decisions about the EXPERIMENT are now generated from the source-
    // compiled policy. The closure proof only observes consequences and
    // never chooses the next experiment or target.
    let mut policy_input=input.clone();
    policy_input["rsiMetaOperatorActive"]=json!(algebra_active);
    policy_input["rsiFocusFiles"]=json!(&focus);
    policy_input["rsiSelectedDomain"]=json!(domain);
    policy_input["rsiArchitectureTarget"]=architecture_target.clone();
    let experiment_policy=crate::engine::rsi_evolve::meta_experiment_policy::synthesize(
        &history,&frontier,&algebra,&policy_input,commit);
    let comparative_mode=if peers["references"].as_array()
        .is_some_and(|rows|!rows.is_empty()) {"cross_lineage_hybrid_design"}
        else if meta_policy["counterexamples"].as_array()
            .is_some_and(|rows|rows.len()>=2) {"multi_trajectory_design"}
        else {"local_source_redesign"};
    println!("{}",json!({
        "source":"compiled_descendant_system_controller_v2",
        "generatorCommit":commit,"historyCount":history.len(),
        "domain":domain,"selection":selection,"strategy":strategy,
        "focusFiles":focus,"ranked":ranked,
        "systemBlueprint":system_blueprint,
        "sourceTreeSha256":system_blueprint["sourceTreeSha256"],
        "architectureFrontier":frontier,
        "architectureTarget":architecture_target,
        "metaPolicy":meta_policy,
        "symbioticCurriculum":symbiosis,
        "interactionGraph":interaction_graph,
        "negativeTransferPlan":negative_transfer,
        "coupledPortfolio":coupled_portfolio,
        "factorialExperiment":factorial,
        "recursiveScientificProgram":science,
        "improvementAlgebra":algebra,
        "methodGeneralization":method_learning,
        "experimentPolicy":experiment_policy,
        "metaOperatorActive":algebra_active,
        "metaRedesignTarget":meta_redesign_target,
        "metaRedesignActive":meta_redesign_active,
        "modelEvolutionPlan":model_evolution,
        "successorStage":successor_stage,
        "peerLineages":peers,
        "comparativeMode":comparative_mode,
        "decisionOwner":"rebuilt_descendant_binary",
        "improvementDirective":directive,
        "archiveSelection":archive,
        "inheritedProgram":inherited_program,
        "editPolicy":edit_policy
    }));
    Ok(())
}

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

