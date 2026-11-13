//! execution worker: compiled descendant RSI responsibility.
use super::*;

/// Executes only one durable generation, without starting a server, scheduler,
/// migrations or recovering another process's jobs in the shared ledger.
pub async fn worker(job_id: &str) -> Result<(), String> {
    let config = crate::config::Config::from_env();
    let store = crate::db::Store::open(&config.db_path()).map_err(|e| e.to_string())?;
    let vault = crate::vault::Vault::open(&config.key_path());
    let state = AppState::new(config, store, vault);
    let mut job = state
        .store
        .get("job", job_id)
        .ok_or("세대 작업을 찾을 수 없습니다.")?;
    if job["kind"] != "rsi_generation"
        || job["status"] != "running"
        || job["cancelRequested"] == true
        || job["input"]["rsiGeneratorCommit"].as_str() != option_env!("DOMINION_BUILD_COMMIT")
    {
        return Err("세대 작업 또는 개선기 소스 revision이 일치하지 않습니다.".into());
    }
    // Own the campaign identifier before mutating the descendant job.
    // Holding an &str into job["input"] across replan/edit-policy updates
    // prevents the Rust worker from compiling (E0502).
    let campaign = job["input"]["rsiCampaignId"]
        .as_str()
        .ok_or("자동 개선 ID 없음")?
        .to_owned();
    if state.store.get("job", &campaign).is_none_or(|p| {
        p["kind"] != "rsi_evolve" || p["status"] != "running" || p["cancelRequested"] == true
    }) {
        return Err("자동 개선이 중단되었습니다.".into());
    }
    let mut history = state
        .store
        .all("rsi_generation")
        .into_iter()
        .filter(|g| g["campaignId"] == campaign)
        .collect::<Vec<_>>();
    history.sort_by_key(|g|g["attempt"].as_u64().unwrap_or(0));
    let history=crate::engine::rsi_system::revalidated_history(&state,&history);
    if job["input"]["rsiMetaEpisode"]==true {
        bind_inherited_target(&mut job["input"],&Value::Null);
    }
    let objective = selection_objective(&job["input"]);
    // System evolution is a search mode, not a measured holdout constraint.
    // Only a genuine paired target or live failed diagnosis may pin a domain.
    let paired_target=paired_descendant_target(&job["input"])
        || job["input"]["rsiDiagnostic"]["failed"].as_u64().unwrap_or(0)>0;
    let latest=history.iter().max_by_key(|g|g["attempt"].as_u64().unwrap_or(0));
    let continuation = latest.filter(|_|!paired_target)
        .filter(|g|g["candidateId"].is_null() && g["newFindings"].as_u64().unwrap_or(0)>0)
        .and_then(|g|g["domain"].as_str()).map(str::to_string);
    let trial_id = job["input"]["rsiTrialAlgorithmId"].as_str()
        .filter(|_|!paired_target)
        .filter(|value| !value.is_empty())
        .and_then(|id|state.store.get("algorithm",id))
        .filter(|algorithm|algorithm["rsiTrial"]["campaignId"].as_str()
            .is_some_and(|owner|job["input"]["rsiAllowedTrialCampaigns"]
                .as_array().is_some_and(|ids|ids.iter().any(|id|id.as_str()==Some(owner)))))
        .and_then(|algorithm|algorithm["id"].as_str().map(str::to_string));
    let selected_domain = job["input"]["rsiSelectedDomain"]
        .as_str()
        .filter(|domain| DOMAINS.contains(domain))
        .map(str::to_string);
    let commit=job["input"]["rsiGeneratorCommit"].as_str().unwrap_or("").to_string();
    if job["input"]["rsiSystemEvolution"]==true {
        // Recheck immediately before execution: the chosen neutral source
        // parent must still refer to the compiled child's exact plan and
        // the immutable original A/B evaluator's evidence.
        let archive=&job["input"]["rsiArchiveSelection"];
        if *archive!=job["input"]["rsiControllerPlan"]["archiveSelection"]
            || job["input"]["rsiInheritedCandidateId"]!=archive["candidateId"] {
            return Err("후손 보관소 계획과 실행 작업의 상속 후보가 다릅니다.".into());
        }
        crate::engine::rsi_archive::validate_selected(
            &state,archive,&commit,&job["input"]["rsiFrozenCases"])?;
        let policy=crate::engine::rsi_model_lineage::plan(
            &state,&history,&job["input"],&commit)?;
        if job["input"]["rsiModelEvolutionPlan"]!=policy
            || job["input"]["rsiControllerPlan"]["modelEvolutionPlan"]!=policy
            || !crate::engine::rsi_model_lineage::valid(
                &policy,&job["input"],&commit,history.len()) {
            return Err("실행 모델의 선택이 원본 승인·후손 계보·독립 측정과 일치하지 않습니다.".into());
        }
        if policy["ownerOptIn"]==true &&
            (job["input"]["providerId"]!=policy["selected"]["providerId"]
                || job["input"]["model"]!=policy["selected"]["model"]) {
            return Err("세대 API 모델을 승인된 선택에서 변경할 수 없습니다.".into());
        }
        job["rsiModelEvolutionPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":commit,
            "selectionRevision":policy["revision"],
            "providerId":job["input"]["providerId"],
            "model":job["input"]["model"],
            "weightTrainingClaimed":false
        });
    }
    let (mut domain, mut domain_selection, mut strategy)=if job["input"]["rsiSystemEvolution"]==true {
        // The real compiled successor already issued an authenticated controller plan.
        // Do not silently replace its decision with the parent-era selector.
        let plan=&job["input"]["rsiControllerPlan"];
        if plan["source"]!="compiled_descendant_system_controller_v2"
            || plan["generatorCommit"]!=commit
            || plan["systemBlueprint"]!=job["input"]["rsiSystemBlueprint"]
            || plan["domain"]!=job["input"]["rsiTargetDomain"]
            || plan["focusFiles"]!=job["input"]["rsiFocusFiles"]
                && job["input"]["rsiFocusSelection"]["source"]!="owner_selected" {
            return Err("후손 제어 계획의 출처 또는 실행 대상이 달라졌습니다.".into());
        }
        let chosen=plan["domain"].as_str().filter(|d|DOMAINS.contains(d))
            .ok_or("후손 제어기가 유효하지 않은 개선 분야를 선택했습니다.")?;
        let mode=match plan["selection"].as_str() {
            Some("algorithm_active")=>"algorithm_active",
            Some("algorithm_trial")=>"algorithm_trial",
            Some("research_continuation")=>"research_continuation",
            _=>return Err("후손 제어기의 선택 정책이 유효하지 않습니다.".into())
        };
        (chosen.to_string(),mode,plan["strategy"].clone())
    }else{
        select_generation_strategy(
            &state,&history,&objective,continuation.clone(),trial_id.clone(),
            selected_domain.clone(),&commit)
    };
    // An adopted binary can revise its own targeting logic. Keep the original
    // parent decision unless the recompiled generator reaches a different
    // conclusion on the same durable history and objective.
    if job["input"]["rsiSystemEvolution"]!=true
        && job["input"]["rsiSourceAdvanced"]==true
        && job["input"]["rsiAutoFocus"]==true && continuation.is_none()
        && selected_domain.is_some() && !paired_target {
        let fresh=select_generation_strategy(&state,&history,&objective,
            None,trial_id,None,&commit);
        if fresh.0!=domain {
            (domain,domain_selection,strategy)=fresh;
            strategy["decisionSource"]=json!("descendant_algorithm");
        }
    }
    if paired_target {
        state.store.audit("rsi.independent_target_preserved",json!({
            "jobId":job_id,"generatorCommit":commit,
            "targetDomain":domain,"source":"frozen_paired_post_adoption"
        }));
    }
    if let Err(error)=reconcile_descendant_target(&state,&mut job,&history,
        &domain,domain_selection,&strategy,&commit) {
        if error.starts_with("rsi_token_budget:") {
            job["status"]=json!("failed");
            job["error"]=json!(error);
            job["result"]=json!({"rsi":{"modelError":error}});
            state.store.put("job",&job).ok_or("세대 예산 중단 기록 실패")?;
        }
        return Err(error);
    }
    // A system-evolution planner runs IN the exact rebuilt successor.
    // Its source-scoped edit policy is an executable descendant decision,
    // not a suggestion for the old coordinator to replace at runtime.
    let mut descendant_method=if job["input"]["rsiSystemEvolution"]==true {
        let plan=&job["input"]["rsiControllerPlan"];
        if plan["generatorCommit"]!=commit
            || !plan["editPolicy"].is_object()
            || job["input"]["rsiEditPolicy"]!=plan["editPolicy"] {
            return Err("후손의 실행 방법과 컴파일된 계획의 계보가 다릅니다.".into());
        }
        plan["editPolicy"].clone()
    }else{
        learned_edit_method(&state,&job["input"],&history)
    };
    if job["input"]["rsiSystemEvolution"]==true {
        let expected=crate::engine::rsi_system::inherited_program(&state,&history);
        if job["input"]["rsiControllerPlan"]["inheritedProgram"]!=json!(expected)
            || job["input"]["rsiInheritedMetaProgram"]!=json!(expected) {
            return Err("후손 실행 개선 프로그램의 독립 채택 근거가 다릅니다.".into());
        }
        let profile=crate::engine::rsi_meta_controller::comparison_profile(&job["input"]);
        crate::engine::rsi_meta_controller::validate_peer_lineages(
            &state,&job["input"]["rsiPeerLineages"],profile.as_deref(),&campaign)?;
        let frontier=job["input"]["rsiArchitectureFrontier"].as_array()
            .ok_or("후손 메타 학습을 재구성할 구조 탐색 근거 없음")?;
        let base=crate::engine::rsi_meta_controller::synthesize(
            &history,frontier,DOMAINS,
            crate::engine::rsi_system::inherited_domain(&history).as_deref());
        let observed_holdout=crate::engine::rsi_interaction::influence_graph(
            &history,frontier);
        let holdout_meta=crate::engine::rsi_interaction::reinforce_meta_with_holdout(
            base,&observed_holdout);
        let transferred=crate::engine::rsi_meta_controller::integrate_peer_evidence(
            holdout_meta,&job["input"]["rsiPeerLineages"]);
        let reciprocal=crate::engine::rsi_symbiosis::transition_graph(&history);
        let adapted=crate::engine::rsi_symbiosis::apply_reciprocal_feedback(
            transferred,&reciprocal);
        let owner_focus=matches!(
            job["input"]["rsiFocusSelection"]["source"].as_str(),
            Some("owner_selected"|"user_selected"));
        let predictor_repair_active=job["input"]["rsiAutonomous"]==true
            && !owner_focus && !paired_descendant_target(&job["input"])
            && job["input"]["rsiMetaPolicy"]["predictionRuleRepair"]["repairRuleSourceSelected"]==true
            && job["input"]["rsiMetaPolicy"]["target"]["path"]=="src/engine/rsi_prequential.rs";
        let negative_transfer_active=!predictor_repair_active
            && job["input"]["rsiAutonomous"]==true
            && !owner_focus && !paired_descendant_target(&job["input"])
            && job["input"]["rsiNegativeTransferPlan"]["selected"]["victim"].is_object()
            && job["input"]["rsiNegativeTransferPlan"]["selected"]["source"].is_object();
        if negative_transfer_active {
            let pair=&job["input"]["rsiNegativeTransferPlan"]["selected"];
            let expected=[&pair["victim"]["path"],&pair["source"]["path"]];
            let actual=job["input"]["rsiFocusFiles"].as_array()
                .ok_or("교차 지능 복구 후손의 실제 수정 대상이 없습니다.")?;
            if actual.len()!=2 || actual.iter().zip(expected.iter())
                .any(|(given,required)|given!=*required) {
                return Err("후손의 실제 편집 파일 2개가 독립 검증된 간섭 대상과 다릅니다.".into());
            }
        }
        let capability_active=!predictor_repair_active
            && job["input"]["rsiAutonomous"]==true
            && !owner_focus && !paired_descendant_target(&job["input"])
            && !negative_transfer_active
            && crate::engine::rsi_method_generalization::prefer_capability_target(
                &job["input"]["rsiImprovementAlgebra"],
                &job["input"]["rsiMethodGeneralization"]);
        let algebra_active=!predictor_repair_active
            && job["input"]["rsiAutonomous"]==true
            && !owner_focus && !paired_descendant_target(&job["input"])
            && !negative_transfer_active && !capability_active
            && job["input"]["rsiImprovementAlgebra"]["selected"]["target"].is_object();
        let suggested=if job["input"]["rsiAutonomous"]==true && !owner_focus
            && !paired_descendant_target(&job["input"])
            && !negative_transfer_active && !algebra_active && !capability_active {
            crate::engine::rsi_system::inherited_stage(&state,&history,frontier)
        }else{Value::Null};
        let mut expected=crate::engine::rsi_meta_controller::apply_successor_stage(
            adapted,&suggested);
        let expected_stage=if suggested.is_object()
            && expected["successorStage"]==suggested{suggested}
            else{Value::Null};
        // Recompute in the actual worker from the immutable source-bound
        // history; a forged model prompt cannot claim a different donor.
        let discover=job["input"]["rsiAutonomous"]==true
            && !owner_focus && !paired_descendant_target(&job["input"])
            && job["input"]["rsiRecursiveScientificProgram"]["phase"]==
                "discover_new_mechanism"
            && !negative_transfer_active && !algebra_active && !capability_active
            && expected_stage.is_null();
        if discover {
            expected=crate::engine::rsi_peer_transfer::learn(
                expected,&job["input"]["rsiPeerLineages"],&history);
        }
        if job["input"]["rsiSuccessorStage"]!=expected_stage
            || job["input"]["rsiControllerPlan"]["successorStage"]!=expected_stage
            || job["input"]["rsiControllerPlan"]["metaPolicy"]!=expected
            || job["input"]["rsiMetaPolicy"]!=expected
            || job["input"]["rsiControllerPlan"]["symbioticCurriculum"]!=
                job["input"]["rsiSymbioticCurriculum"] {
            return Err("후손의 상호개선·재귀 단계·메타 방법이 계승된 실제 기록과 다릅니다.".into());
        }
        crate::engine::rsi_symbiosis::validate_curriculum(
            &job["input"]["rsiSymbioticCurriculum"],
            &job["input"]["rsiArchitectureTarget"],frontier)?;
        let negative_transfer_expected=crate::engine::rsi_negative_transfer::plan(
            &state,&history,frontier,&commit);
        if job["input"]["rsiNegativeTransferPlan"]!=negative_transfer_expected
            || job["input"]["rsiControllerPlan"]["negativeTransferPlan"]
                !=negative_transfer_expected
            || !crate::engine::rsi_negative_transfer::valid(
                &negative_transfer_expected,frontier,&commit,history.len()) {
            return Err("실행 후손의 교차 능력 손상 기록이 독립 재검증 결과와 다릅니다.".into());
        }
        let observed_influence=observed_holdout;
        let portfolio=&job["input"]["rsiCoupledPortfolio"];
        let factor_expected=crate::engine::rsi_factorial::design(
            &history,frontier,&observed_influence,DOMAINS,&commit);
        let science_expected=crate::engine::rsi_recursive_scientist::plan(
            &history,frontier,&factor_expected,portfolio,&commit);
        let authored_algebra=crate::engine::rsi_improvement_algebra::compile(
            &history,frontier,&commit);
        let learning_expected=crate::engine::rsi_method_generalization::learn(
            &state,&history,frontier,&authored_algebra,&commit);
        let algebra_expected=crate::engine::rsi_method_generalization::adapt(
            authored_algebra,&learning_expected);
        if job["input"]["rsiFactorialExperiment"]!=factor_expected
            || job["input"]["rsiRecursiveScientificProgram"]!=science_expected
            || job["input"]["rsiControllerPlan"]["factorialExperiment"]!=factor_expected
            || job["input"]["rsiControllerPlan"]["recursiveScientificProgram"]!=science_expected
            || job["input"]["rsiControllerPlan"]["improvementAlgebra"]!=algebra_expected
            || job["input"]["rsiImprovementAlgebra"]!=algebra_expected
            || job["input"]["rsiMethodGeneralization"]!=learning_expected
            || job["input"]["rsiControllerPlan"]["methodGeneralization"]!=learning_expected
            || !crate::engine::rsi_method_generalization::valid(
                &learning_expected,&commit,history.len())
            || !crate::engine::rsi_improvement_algebra::valid(
                &algebra_expected,frontier,&commit,history.len())
            || !crate::engine::rsi_factorial::valid(
                &factor_expected,frontier,&commit,history.len(),&observed_influence)
            || !crate::engine::rsi_recursive_scientist::valid(
                &science_expected,frontier,&commit,history.len(),&factor_expected,portfolio) {
            return Err("실행 후손의 요인 실험·발명·복귀·개선기 재설계 순서가 실제 채택 계보와 다릅니다.".into());
        }
        if !crate::engine::rsi_interaction::valid_graph(&observed_influence)
            || job["input"]["rsiControllerPlan"]["interactionGraph"]!=observed_influence
            || job["input"]["rsiInteractionGraph"]!=observed_influence
            || job["input"]["rsiControllerPlan"]["coupledPortfolio"]!=*portfolio
            || (portfolio.is_object()
                && !crate::engine::rsi_interaction::valid_portfolio(
                    portfolio,&observed_influence,frontier)) {
            return Err("실행 후손의 영역별 실제 변경·독립 평가 학습 기록이 불일치합니다.".into());
        }
        if expected_stage.is_object(){
            job["rsiSuccessorStageExecuted"]=json!({
                "source":"running_compiled_descendant_binary",
                "generatorCommit":commit,
                "designId":expected_stage["designId"],
                "stageIndex":expected_stage["stageIndex"],
                "role":expected_stage["role"],
                "path":expected_stage["path"],
                "sourceSha256":expected_stage["sourceSha256"],
                "qualityProven":false
            });
        }
        // Only the actually controlling compiled operator is dispatched.
        let operator_dispatch=crate::engine::rsi_operator_provenance::dispatch(
            &state,&job,&commit,algebra_active)?;
        job["rsiOperatorDispatch"]=operator_dispatch;
        job["rsiMetaPolicyExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":commit,
            "metaRevision":expected["revision"],
            "forecast":expected["forecast"],
            "selectedRole":expected["target"]["role"]
        });
        job["rsiCompiledPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":commit,
            "metaRevision":expected["revision"]
        });
    }
    if job["input"]["rsiSystemEvolution"]==true
        && job["input"]["rsiInheritedMetaProgram"].is_object() {
        if descendant_method["program"]!=
            job["input"]["rsiInheritedMetaProgram"]["stages"] {
            return Err("계승된 실행 단계와 후손 편집 정책이 일치하지 않습니다.".into());
        }
        job["rsiInheritedMetaProgramBound"]=json!({
            "source":"compiled_successor_runtime_edit_policy",
            "generatorCommit":commit,
            "programRevision":job["input"]["rsiInheritedMetaProgram"]["revision"],
            "policyRevision":descendant_method["revision"],
            "qualityProven":false
        });
    }
    descendant_method["selectedByGeneratorCommit"]=json!(commit);
    job["input"]["rsiEditPolicy"]=descendant_method.clone();
    job["rsiEditPolicyDecision"]=json!({
        "producer":"running_descendant_binary",
        "generatorCommit":commit,
        "methodRevision":descendant_method["revision"],
        "observations":descendant_method["observations"],
    });
    state.store.audit("rsi.descendant_edit_method_selected",json!({
        "jobId":job_id,"generatorCommit":commit,"method":descendant_method,
    }));
    job["rsiDomain"] = json!(domain);
    job["rsiDomainSelection"] = json!(domain_selection);
    job["rsiSearchStrategy"] = strategy.clone();
    job["rsiSelectionObjective"] = json!(objective);
    job["rsiDecisionInput"] = job["input"]["rsiDecisionInput"].clone();
    job["input"]["rsiDomainSelection"] = json!(domain_selection);
    job["input"]["rsiSearchStrategy"] = strategy;
    let focus = job["input"]["rsiFocusFiles"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(8).collect::<Vec<_>>().join(", ");
    let capability = if focus.is_empty() { domain.clone() } else { format!("{domain} · {focus}") };
    job["rsiDomain"] = json!(domain);
    job["input"]["rsiDomain"]=json!(domain);
    let descendant_prompt=if job["input"]["rsiSystemEvolution"]==true {
        let source=job["input"]["rsiSourceDir"].as_str()
            .ok_or("후손 소스 폴더가 없습니다.")?.to_string();
        let blueprint=crate::engine::rsi_system::blueprint(
            Path::new(&source),&commit,&history);
        if blueprint!=job["input"]["rsiSystemBlueprint"] {
            return Err("후손 소스 자기모형이 컴파일된 계획과 일치하지 않습니다.".into());
        }
        let expected=crate::engine::rsi_system::improvement_directive(&history);
        if job["input"]["rsiImprovementDirective"]!=expected {
            return Err("후손 실패 학습 지시와 실제 세대 기록이 다릅니다.".into());
        }
        job["rsiSystemBlueprintVerified"]=json!(true);
        job["rsiImprovementDirectiveVerified"]=json!(true);
        state.store.audit("rsi.descendant_system_runtime",json!({
            "jobId":job_id,"generatorCommit":commit,
            "source":"compiled_successor_planning_and_context",
            "architectureComponents":blueprint["architecture"].as_array().map(Vec::len),
            "sourceTreeSha256":blueprint["sourceTreeSha256"],
            "improvementDirective":expected["mode"]
        }));
        let campaign_record=state.store.get("rsi_campaign",&campaign)
            .unwrap_or(Value::Null);
        // Recompute the context from the NEW generator's code. A previous
        // resident server can neither select the prompt nor re-inject stale
        // source excerpts as purported evidence.
        let observed=generation_context(&state,&job["input"],&campaign_record);
        let research=research_digest(&state,&job["input"],&campaign_record);
        // Neutral archived source is inherited AFTER the campaign checkout.
        // A pre-inheritance excerpt belongs to the wrong bytes and SHA.
        // Give the model a fresh-read instruction instead of stale source.
        let focus=if job["input"]["rsiInheritedCandidateId"].is_string() {
            "중립 후속 후보가 별도 작업공간에 적용됩니다. 이전 소스 발췌와 해시는 변경 전 버전입니다. workspace_read_range로 현재 원문과 sourceSha256을 확인한 뒤 편집하세요.".to_string()
        }else{
            focus_file_context(&state,&json!({"labDir":source}),&job["input"])
        };
        let capsule=|text:&str,limit:usize|text.chars().take(limit).collect::<String>();
        let owner=state.store.get("job",&campaign)
            .and_then(|parent|parent["input"]["rsiObjective"].as_str()
                .map(str::to_string)).unwrap_or_default();
        let meta_instruction=crate::engine::rsi_meta_controller::instruction(
            &job["input"]["rsiMetaPolicy"]);
        let reciprocal_method=crate::engine::rsi_symbiosis::instruction(
            &job["input"]["rsiSymbioticCurriculum"]);
        let inherited_stage=if job["input"]["rsiSuccessorStage"].is_object(){
            format!("MODEL-WRITTEN NEXT DESCENDANT DEVELOPMENT METHOD: {}. The source hash and stage index were rebound by the currently rebuilt child; the expected outcome remains an unmeasured hypothesis.",
                job["input"]["rsiSuccessorStage"])
        }else{String::new()};
        let inherited_method=if job["input"]["rsiInheritedMetaProgram"].is_object(){
            format!("NEXT-GENERATION EXECUTABLE EDIT METHOD (inherited, still an unproven trial): {}. The runtime tool router uses this program, and observed stage receipts—not this text—establish execution.",
                job["input"]["rsiInheritedMetaProgram"])
        }else{String::new()};
        // Several references may be inspected by a compiled planner.
        // The real coding model receives only ONE actively chosen donor.
        let peer_evidence=if job["input"]["rsiMetaPolicy"]["peerMethodTransfer"]["active"]==true {
            crate::engine::rsi_peer_transfer::instruction(&job["input"]["rsiMetaPolicy"])
        }else{
            serde_json::to_string(&job["input"]["rsiPeerLineages"]).unwrap_or_default()
        };
        let independent_effects=serde_json::to_string(
            &job["input"]["rsiInteractionGraph"]).unwrap_or_default();
        let combined_sources=serde_json::to_string(
            &job["input"]["rsiCoupledPortfolio"]).unwrap_or_default();
        let science_enabled=job["input"]["rsiAutonomous"]==true
            && job["input"]["rsiFocusSelection"]["source"]=="rebuilt_descendant_controller"
            && !paired_descendant_target(&job["input"]);
        let scientific_instruction=if science_enabled {
            crate::engine::rsi_recursive_scientist::instruction(
                &job["input"]["rsiRecursiveScientificProgram"])
        }else{String::new()};
        let factorial_instruction=if science_enabled {
            crate::engine::rsi_factorial::instruction(
                &job["input"]["rsiFactorialExperiment"])
        }else{String::new()};
        let algebra_instruction=if science_enabled {
            crate::engine::rsi_improvement_algebra::instruction(
                &job["input"]["rsiImprovementAlgebra"])
        }else{String::new()};
        let method_learning_instruction=if science_enabled {
            crate::engine::rsi_method_generalization::instruction(
                &job["input"]["rsiMethodGeneralization"])
        }else{String::new()};
        let negative_transfer_instruction=if science_enabled {
            crate::engine::rsi_negative_transfer::instruction(
                &job["input"]["rsiNegativeTransferPlan"])
        }else{String::new()};
        let model_lineage_instruction=format!(
            "Approved, actually executed model={}, provider={}, original_owner={},              frozen downstream model-lineage decision={},              external_API_weights_retrained=false.              The improvement-agent model can be changed only among approved              routes by this self-editable compiled policy; selection is not              evidence that its underlying weights or general inference improved.",
            job["input"]["model"],job["input"]["providerId"],
            job["input"]["rsiModelOrigin"],
            capsule(job["input"]["rsiModelEvolutionPlan"].as_str().unwrap_or(""),1600));
        let peer_context=format!(
            "DESCENDANT MODEL SELECTION / MODEL-IMPROVEMENT EXPERIMENT: {}\n            DOWNSTREAM METHOD EFFECTIVENESS (replayed independent holdouts): {}\n            OPEN MODEL-AUTHORED IMPROVEMENT ALGEBRA (executed current source): {}\n            RECURSIVE SCIENTIFIC DISCOVERY (descendant-owned): {}\n            FACTORIAL HOLDOUT EXPERIMENT: {}\nCOMPARATIVE METHOD: {}\nINDEPENDENT OTHER-LINEAGE DESIGN REFERENCES (source evidence only, never automatic patches):\n{}\n            CROSS-SUBSYSTEM HOLDOUT OBSERVATIONS (changed source paths, isolated versus joint edits; hypotheses not causal proof):\n{}\n            NEXT SOURCE-BOUND CO-DESIGN PORTFOLIO (edit and independently evaluate actual implementations):\n{}",
            model_lineage_instruction,method_learning_instruction,
            algebra_instruction,scientific_instruction,factorial_instruction,
            job["input"]["rsiComparativeMode"],capsule(&peer_evidence,3000),
            capsule(&independent_effects,2400),capsule(&combined_sources,1600));
        let peer_context=format!(
            "{}\nCROSS-CAPABILITY FROZEN HOLDOUT DAMAGE / JOINT REPAIR EXPERIMENT:\n{}",
            peer_context,negative_transfer_instruction);
        let frontier_hint=if job["input"]["rsiFocusSelection"]["source"]=="rebuilt_descendant_controller"
            && job["input"]["rsiArchitectureTarget"].is_object() {
            let gap=&job["input"]["rsiArchitectureTarget"];
            format!("NEXT UNEXPLORED SYSTEM ROLE: {}\nCURRENT FILE: {}\nSOURCE SHA-256: {}\nOBSERVED FAILURE SIGNALS: {}\nPRIOR SOURCE ATTEMPTS: {}\nReason: {}. This is only an evidence-grounded exploration target; verify the code and invent a causal design before editing.",
                gap["role"],gap["path"],gap["sourceSha256"],
                gap["usageSignalIds"],gap["sourceAttempts"],gap["reason"])
        }else{String::new()};
        format!("{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n\nNEXT IMPROVEMENT ACTION (measured history): {}\n\nOWNER OBJECTIVE: {}\n\nPREVIOUS VERIFIABLE EVIDENCE:\n{}\n\nRELEVANT RESEARCH:\n{}\n\nCURRENT REVISION SOURCE:\n{}",
            crate::engine::rsi_system::instruction(&blueprint,&owner),
            meta_instruction,reciprocal_method,inherited_stage,inherited_method,
            peer_context,frontier_hint,expected,owner,capsule(&observed,3500),
            capsule(&research,3500),capsule(&focus,6000))
    } else {job["input"]["prompt"].as_str().unwrap_or("").to_string()};
    let prepared = fit_worker_prompt(
        &descendant_prompt, &domain, &capability,
        &history, job["input"]["rsiRemainingCampaignTokens"].as_u64()
    );
    let (prompt, plan) = match prepared {
        Ok(value) => value,
        Err(error) => {
            // Persist preflight as a real budget stop. The parent can conclude
            // without paying for a doomed model call or launching a new step.
            job["status"]=json!("failed");
            job["error"]=json!(error);
            job["result"]=json!({"rsi":{"modelError":error}});
            state.store.put("job",&job).ok_or("세대 예산 중단 기록 실패")?;
            return Err(error);
        }
    };
    job["input"]["prompt"]=json!(prompt);
    job["rsiWorkerContextPlan"]=plan;
    state.store.put("job", &job).ok_or("세대 지시 저장 실패")?;
    let context = JobContext::new(state.clone(), job_id.into());
    let result = jobs::run_prompt(&context, &job).await;
    match result {
        Ok(value) => {
            context.checkpoint(json!({"status":"completed","result":value}))?;
            Ok(())
        }
        Err(error) => {
            context.checkpoint(json!({"status":"failed","error":error}))?;
            Err(error)
        }
    }
}

