//! recompiled descendant plan and target validation.
//! Only descendant_planner owns these implementations. Other RSI stages use explicit
//! names imported through the stable rsi_evolve facade.
use super::{AppState, DOMAINS, Path, PathBuf, Value, candidate_domains, choose_focus_files, comparable_trial_baseline, focus_file_context, history_with_verified_payoffs, json, learned_edit_method, scored_domains, search_strategy, select, select_domain, selection_objective};
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

// Regression tests relocated to tests/support/engine/rsi_evolve.tests.rs

/// Ask the exact rebuilt descendant to plan its next improvement. This
/// command is deliberately read-only and never starts a server or a model.
pub(super) fn system_evolution_target_hint(input:&Value, attempt:u64)->Option<String> {
    if paired_descendant_target(input) {
        input["rsiSelectedDomain"].as_str().map(str::to_string)
    } else if input["rsiAutonomous"]!=true && input["rsiAutoFocus"]==false
        && input["rsiFocusFiles"].as_array().is_some_and(|rows|!rows.is_empty()) {
        // A real user-specified source task is an immutable task constraint
        // regardless of whether it comes from an interactive UI or a
        // downstream A/B. The compiled planner still selects its algorithms.
        input["rsiSelectedDomain"].as_str()
            .filter(|domain|DOMAINS.contains(domain)).map(str::to_string)
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
    if !crate::engine::rsi_principle_learning::verify_replay(&state,&history) {
        return Err("후손 원리 학습의 후보별 독립 평가 기록을 재생하지 못했습니다.".into());
    }
    if input["rsiAdaptiveApiRouting"]==true
        && !crate::engine::rsi_api_route_learning::verify_replay(&state,&history) {
        return Err("API 경로 학습의 실제 호출·평가 영수증을 재생하지 못했습니다.".into());
    }
    let objective=selection_objective(input);
    let source=input["rsiGeneratorSourceDir"].as_str()
        .or_else(||input["rsiSourceDir"].as_str())
        .ok_or("후손 시스템 소스 위치가 없습니다.")?;
    let system_blueprint=crate::engine::rsi_system::blueprint(Path::new(source),commit,&history);
    let mut frontier=crate::engine::rsi_system::architecture_frontier(&state,&system_blueprint,&history,
        input["rsiIndexId"].as_str().unwrap_or(""),Path::new(input["rsiSourceDir"].as_str().unwrap_or(source)));
    let inherited_stage=crate::engine::rsi_system::inherited_stage(&history,&frontier);
    if let Some(stage)=inherited_stage.as_ref() {
        // Descendant meta-learning decides between a newly discovered goal
        // and an inherited executable policy. Previously adopted stages
        // compete with novel alternatives on the SAME source-backed frontier.
        for row in &mut frontier {
            if row["role"]==stage["role"] && row["domain"]==stage["domain"]
                && row["path"]==stage["path"]
                && row["sourceSha256"]==stage["sourceSha256"] {
                row["inheritedProgramStage"]=json!(true);
                row["learningPolicy"]=stage["learningPolicy"].clone();
                row["toolPolicy"]=stage["toolPolicy"].clone();
                row["executionPolicy"]=stage["executionPolicy"].clone();
            }
        }
    }
    let explicit=input["rsiAutonomous"]!=true && input["rsiAutoFocus"]==false
        && input["rsiFocusFiles"].as_array().is_some_and(|paths|!paths.is_empty());
    let inherited=if input["rsiAutonomous"]==true {
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    // A whole-system autonomous descendant may revisit ANY intelligence
    // component; keyword matching in the old fixed objective is not a cage.
    let allowed=if input["rsiSystemEvolution"]==true
        && input["rsiAutonomous"]==true && !explicit {
        DOMAINS.to_vec()
    }else{candidate_domains(&objective)};
    let lineage=crate::engine::rsi_system::lineage_capsule(&history);
    // Source-adopted META methods are actionable only if the exact method
    // is still executable for this domain and frozen held-out cases exist.
    // The recompilable descendant selects the real capability transfer task;
    // the resident supervisor never writes this target for it.
    // A frozen post-adoption *supervisor suggestion* is not an owner edit
    // constraint. Once the compiled descendant observes a usable adopted
    // method, that executable transfer outranks the old cost-only suggestion.
    let super::meta_composition::MetaComposition {
        method_transfer,
        feedback_repair,
        influence,
        principle_learning,
        api_route_policy,
        factorial,
        evolution_program,
        peers,
        mutual_program,
        reflexive_program,
        open_program,
        meta_policy,
        architecture_target,
        co_design,
        successor_stage,
    }=super::meta_composition::compose(
        &state,input,&history,&frontier,&allowed,commit,campaign,
        &lineage,inherited.as_deref(),inherited_stage.as_ref(),explicit)?;
    let successor_hint=if explicit {
        system_evolution_target_hint(input,snapshot["attempt"].as_u64().unwrap_or(0))
    }else if let Some(domain)=evolution_program["target"]["domain"].as_str(){
        Some(domain.to_owned())
    }else if let Some(domain)=method_transfer["domain"].as_str(){
        Some(domain.to_owned())
    }else if feedback_repair {
        Some("improver".to_owned())
    }else if paired_descendant_target(input) {
        system_evolution_target_hint(input,snapshot["attempt"].as_u64().unwrap_or(0))
    }else{architecture_target["domain"].as_str().map(str::to_string).or(inherited)};
    // A verified ancestor's reciprocal program takes priority over an
    // unrelated pending search trial. Otherwise the source chooser can silently
    // move to another domain and the A->B->A stage never actually executes.
    let trial=if paired_descendant_target(input) || method_transfer.is_object() || feedback_repair
        || evolution_program.is_object() || reflexive_program.is_object() || open_program.is_object()
        || (!explicit && matches!(mutual_program["phase"].as_str(),
            Some("complement"|"return"))) {None} else {
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
    let ranked=choose_focus_files(&state,&source_input,&history,policy.as_ref());
    let explicit_paths=input["rsiFocusFiles"].as_array()
        .filter(|paths|input["rsiAutonomous"]!=true
            && input["rsiAutoFocus"]==false && !paths.is_empty());
    let mut focus=if let Some(paths)=explicit_paths {
        paths.iter().filter_map(Value::as_str)
            .filter(|path|crate::engine::rsi::safe_path(path))
            .take(8).collect::<Vec<_>>()
    }else{
        ranked.iter().filter_map(|row|row["path"].as_str())
            .filter(|path|crate::engine::rsi::safe_path(path))
            .take(8).collect::<Vec<_>>()
    };
    if !explicit && architecture_target["domain"]==domain {
        let mut chosen=co_design["focus"].as_array().into_iter().flatten()
            .filter_map(|row|row["path"].as_str()).collect::<Vec<_>>();
        chosen.reverse();
        for path in chosen {
            focus.retain(|current|*current!=path);
            focus.insert(0,path);
        }
        focus.truncate(8);
    }
    // An isolated intervention must not inherit the ordinary coupled
    // redesign's extra edit targets. A joint intervention needs exactly
    // its two independently source-anchored roles in the focus window.
    if !explicit && meta_policy["selectionMode"]=="matched_factorial_experiment_design"
        && architecture_target["domain"]==domain {
        let experiment=&factorial["selected"];
        match experiment["phase"].as_str() {
            Some("isolate_left"|"isolate_right")=>{
                if let Some(path)=experiment["target"]["path"].as_str(){
                    focus=vec![path];
                }
            }
            Some("test_joint_interaction")=>{
                if let (Some(a),Some(b))=(experiment["left"]["path"].as_str(),
                    experiment["right"]["path"].as_str()){
                    if a!=b {focus=vec![a,b];}
                }
            }
            _=>{}
        }
    }
    if !explicit && reflexive_program.is_object() {
        if let Some(path)=reflexive_program["target"]["path"].as_str() {
            focus=vec![path];
        }
    }
    if !explicit && open_program.is_object() {
        if let Some(path)=open_program["target"]["path"].as_str() {
            focus=vec![path];
        }
    }
    if !explicit && meta_policy["selectionMode"]
        =="verified_calibration_driven_learner_rewrite" {
        focus=vec!["src/engine/rsi_principle_learning.rs"];
    }
    if !explicit && meta_policy["selectionMode"]
        =="prequential_task_quality_predictor_repair" {
        focus=vec!["src/engine/rsi/meta/controller_selection.rs"];
    }
    if !explicit && evolution_program.is_object() {
        if let Some(path)=evolution_program["target"]["path"].as_str(){
            focus=vec![path];
        }
    }
    let directive=crate::engine::rsi_system::improvement_directive(&history);
    let comparative_mode=if peers["references"].as_array()
        .is_some_and(|items|!items.is_empty()) {"compare_independent_peer_lineage"}
        else if lineage["failureCounterexamples"].as_array()
            .is_some_and(|items|items.len()>=2) {"multi_trajectory_reflection"}
        else {"local_source_redesign"};
    let archive=if input["rsiAutonomous"]==true && input["rsiAutoFocus"]==true {
        crate::engine::rsi_archive::select(
            &state,&history,commit,&input["rsiFrozenCases"])
    }else{Value::Null};
    let edit_policy=learned_edit_method(&state,input,&history);
    println!("{}",json!({
        "source":"compiled_descendant_system_controller_v2",
        "generatorCommit":commit,"historyCount":history.len(),
        "domain":domain,"selection":selection,"strategy":strategy,
        "focusFiles":focus,"ranked":ranked,
        "architectureFrontier":frontier,"architectureTarget":architecture_target,
        "usageEvidence":crate::engine::rsi_system::usage_evidence(&state),
        "systemBlueprint":system_blueprint,
        "improvementDirective":directive,
        "methodTransfer":method_transfer,
        "feedbackRepairSelected":feedback_repair,
        "lineageCapsule":lineage,
        "peerLineages":peers,"comparativeMode":comparative_mode,
        "metaPolicy":meta_policy,"successorStage":successor_stage,
        "interactionGraph":influence,"coDesignPortfolio":co_design,
        "factorialExperiment":factorial,
        "reflexiveProgram":reflexive_program,
        "openImprovementProgram":open_program,
        "metaEvolutionProgram":evolution_program,
        "principleLearning":principle_learning,
        "apiRoutePolicy":api_route_policy,
        "mutualProgram":mutual_program,
        "archiveSelection":archive,
        "editPolicy":edit_policy
    }));
    Ok(())
}

// Regression tests relocated to tests/support/engine/rsi_evolve.tests.rs

/// The production coordinator may pass an approved controller receipt, but
/// MUST NOT recompute and replace the compiled descendant's design. Verify the
/// exact generation and immutable source identity before any paid operation.
pub(super) fn validated_descendant_controller(
    input:&Value,history:&[Value],commit:&str,
)->Result<Option<(String,&'static str,Value)>,String>{
    if input["rsiSystemEvolution"]!=true {return Ok(None);}
    // A separately assigned downstream coding task has a pinned target and
    // a distinct, source-identified compiled generator. There is no automatic
    // next-generation plan to inherit in this mode; the real descendant
    // worker still runs the ordinary model, harness and editing policy.
    // Normal self-evolution campaigns must always provide a controller plan.
    if input["rsiControllerPlan"].is_null()
        && input["rsiGeneratorSourceDir"].as_str().is_some_and(|p|!p.is_empty())
        && input["rsiFocusFiles"].as_array().is_some_and(|p|!p.is_empty())
        && input["rsiSystemBlueprint"]["generatorCommit"]==commit
        && input["rsiSystemBlueprint"]["source"]=="compiled_descendant_system_blueprint_v1"
        && input["rsiImprovementDirective"].is_object()
        && input["rsiAutoFocus"]==false {
        return Ok(None);
    }
    let plan=&input["rsiControllerPlan"];
    let frozen=crate::engine::rsi_profile::from_input(input)?;
    if !crate::engine::rsi_evolve::experiment_contract::valid(
        plan,frozen["sha256"].as_str().unwrap_or(""),&input["rsiExperimentContract"]) {
        return Err("후손의 동결 평가·소스 커밋·메타 프로그램 계약이 불일치합니다.".into());
    }
    if plan["source"]!="compiled_descendant_system_controller_v2"
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["systemBlueprint"]["generatorCommit"]!=commit
        || plan["systemBlueprint"]["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["systemBlueprint"]["source"]!="compiled_descendant_system_blueprint_v1"
        || !plan["improvementDirective"].is_object()
        || !plan["editPolicy"].is_object()
        || plan["lineageCapsule"]["source"]!="descendant_curated_lineage_v1"
        || plan["lineageCapsule"]["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["systemBlueprint"]["lineageSha256"]!=plan["lineageCapsule"]["sha256"]
        || plan["peerLineages"]["source"]!="cross_campaign_comparative_archive_v1"
        || !crate::engine::rsi_meta_controller::valid_policy(&plan["metaPolicy"],history.len())
        || !crate::engine::rsi_interaction::valid_graph(&plan["interactionGraph"])
        || plan["metaPolicy"]["interactionGraphSha256"]!=plan["interactionGraph"]["sha256"]
        || (plan["mutualProgram"].is_object()
            && plan["metaPolicy"]["intergenerationalGraphSha256"]
                !=plan["mutualProgram"]["transferGraph"]["sha256"])
        || (matches!(plan["mutualProgram"]["phase"].as_str(),
                Some("complement"|"return"))
            && (plan["metaPolicy"]["target"]!=plan["mutualProgram"]["target"]
                || plan["metaPolicy"]["mutualProgramRevision"]
                    !=plan["mutualProgram"]["revision"]))
        || plan["architectureFrontier"].as_array().is_none_or(|frontier|
            !crate::engine::rsi_mutual_program::valid_program(
                &plan["mutualProgram"],frontier,commit,history.len(),
                &plan["mutualProgram"]["transferGraph"]))
        || (input["rsiAdaptiveApiRouting"]==true
            && (!crate::engine::rsi_api_route_learning::valid(
                &plan["apiRoutePolicy"],history,&input["rsiApprovedApiRoutes"],
                &input["rsiOriginalApiRoute"],commit)
                || input["providerId"]!=plan["apiRoutePolicy"]["chosen"]["providerId"]
                || input["model"]!=plan["apiRoutePolicy"]["chosen"]["model"]))
        || (input["rsiAdaptiveApiRouting"]!=true && plan["apiRoutePolicy"].is_object())
        || (plan["principleLearning"].is_object()
            && (!crate::engine::rsi_principle_learning::valid(
                &plan["principleLearning"],history)
                || plan["metaPolicy"]["principleLearningSha256"]
                    !=plan["principleLearning"]["sha256"]))
        || plan["architectureFrontier"].as_array().is_none_or(|frontier|
            !crate::engine::rsi_evolve::meta_evolution::valid(
                &plan["metaEvolutionProgram"],history,frontier,commit))
        || (plan["metaEvolutionProgram"].is_object()
            && (plan["metaPolicy"]["selectionMode"]!="authored_recursive_evolution_protocol"
                || plan["metaPolicy"]["target"]!=plan["metaEvolutionProgram"]["target"]
                || plan["metaPolicy"]["evolutionProtocolRevision"]
                    !=plan["metaEvolutionProgram"]["revision"]))
        || (plan["openImprovementProgram"].is_object()
            && plan["architectureFrontier"].as_array().is_none_or(|frontier|
                !crate::engine::rsi_program_graph::valid(
                    &plan["openImprovementProgram"],history,frontier,commit)
                || plan["metaPolicy"]["openImprovementRevision"]
                    !=plan["openImprovementProgram"]["revision"]
                || plan["metaPolicy"]["target"]
                    !=plan["openImprovementProgram"]["target"]))
        || (plan["reflexiveProgram"].is_object()
            && plan["architectureFrontier"].as_array().is_none_or(|frontier|
                !crate::engine::rsi_reflexive::valid(
                    &plan["reflexiveProgram"],history,frontier,commit)
                || plan["metaPolicy"]["reflexiveProgramSha256"]
                    !=plan["reflexiveProgram"]["sha256"]
                || plan["metaPolicy"]["target"]
                    !=plan["reflexiveProgram"]["target"]
                || (plan["reflexiveProgram"]["phase"]=="execute_revised_experiment"
                    && (plan["reflexiveProgram"]["experimentPlanSha256"]
                        !=plan["factorialExperiment"]["sha256"]
                        || plan["reflexiveProgram"]["target"]
                            !=plan["factorialExperiment"]["selected"]["target"]
                        || plan["metaPolicy"]["revisedExperimentPlanSha256"]
                            !=plan["factorialExperiment"]["sha256"]))))
        || (plan["factorialExperiment"].is_object()
            && plan["architectureFrontier"].as_array().is_none_or(|frontier|
                !crate::engine::rsi_factorial::valid(
                    &plan["factorialExperiment"],frontier,commit,history.len(),
                    &plan["interactionGraph"])))
        || (plan["coDesignPortfolio"].is_object()
            && plan["architectureFrontier"].as_array().is_none_or(|frontier|
                !crate::engine::rsi_interaction::valid_portfolio(
                    &plan["coDesignPortfolio"],&plan["interactionGraph"],frontier)))
        || (plan["successorStage"].is_object()
            && (plan["successorStage"]["source"]!="inherited_model_authored_improvement_program_v1"
                || plan["architectureTarget"]["path"]!=plan["successorStage"]["path"]
                || plan["architectureTarget"]["sourceSha256"]!=plan["successorStage"]["sourceSha256"]))
        || (plan["architectureTarget"].is_object()
            && plan["metaPolicy"]["target"]!=plan["architectureTarget"])
        || !matches!(plan["comparativeMode"].as_str(),
            Some("local_source_redesign"|"multi_trajectory_reflection"|"compare_independent_peer_lineage"))
    {
        return Err("후손 제어기의 출처·실행 이력·방법이 일치하지 않습니다.".into());
    }
    let domain=plan["domain"].as_str()
        .filter(|d|DOMAINS.contains(d))
        .ok_or("후손 제어기가 유효하지 않은 분야를 지정했습니다.")?;
    let selection=match plan["selection"].as_str() {
        Some("algorithm_active")=>"algorithm_active",
        Some("algorithm_trial")=>"algorithm_trial",
        Some("research_continuation")=>"research_continuation",
        _=>return Err("후손 제어기의 탐색 방법이 유효하지 않습니다.".into())
    };
    if input["rsiSelectedDomain"]!=domain
        || input["rsiTargetDomain"]!=domain
        || input["rsiTargetPlan"]["domain"]!=domain
        || input["rsiTargetPlan"]["generatorCommit"]!=commit
        || input["rsiTargetPlan"]["selection"]!=plan["selection"]
        || input["rsiTargetPlan"]["strategy"]!=plan["strategy"]
        || input["rsiEditPolicy"]!=plan["editPolicy"]
        || input["rsiSystemBlueprint"]!=plan["systemBlueprint"]
        || input["rsiImprovementDirective"]!=plan["improvementDirective"]
        || input["rsiLineageCapsule"]!=plan["lineageCapsule"]
        || input["rsiPeerLineages"]!=plan["peerLineages"]
        || input["rsiComparativeMode"]!=plan["comparativeMode"]
        || input["rsiMetaPolicy"]!=plan["metaPolicy"]
        || input["rsiSuccessorStage"]!=plan["successorStage"]
        || input["rsiInteractionGraph"]!=plan["interactionGraph"]
        || input["rsiCoDesignPortfolio"]!=plan["coDesignPortfolio"]
        || input["rsiFactorialExperiment"]!=plan["factorialExperiment"]
        || input["rsiReflexiveProgram"]!=plan["reflexiveProgram"]
        || input["rsiOpenImprovementProgram"]!=plan["openImprovementProgram"]
        || input["rsiMetaEvolutionProgram"]!=plan["metaEvolutionProgram"]
        || input["rsiPrincipleLearning"]!=plan["principleLearning"]
        || input["rsiApiRoutePolicy"]!=plan["apiRoutePolicy"]
        || input["rsiMutualProgram"]!=plan["mutualProgram"]
    {
        return Err("후손 계획과 실행 worker의 목표·전략·자기모형이 다릅니다.".into());
    }
    if input["rsiFocusSelection"]["source"]!="owner_selected"
        && input["rsiFocusFiles"]!=plan["focusFiles"] {
        return Err("후손이 선택한 소스가 worker 실행 전에 변경됐습니다.".into());
    }
    if paired_descendant_target(input) && input["rsiDecisionInput"]["targetDomain"]!=domain {
        return Err("검증된 세대 간 목표를 후손 제어기가 무시했습니다.".into());
    }
    Ok(Some((domain.to_string(),selection,plan["strategy"].clone())))
}
