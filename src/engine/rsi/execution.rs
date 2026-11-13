//! Cohesive source-level RSI coordinator subsystem.
//! This is code that runs in the compiled child, not model-weight training.
use super::*;

#[path = "execution/build.rs"]
mod build;
use build::ensure_compiled_generator;
#[path = "execution/evidence.rs"]
mod evidence;
use evidence::collect_generation_evidence;

pub(super) async fn run_loop(
    context: &JobContext,
    job: &Value,
    record: &mut Value,
    started: Instant,
) -> Result<String, String> {
    let input = &job["input"];
    let lab = PathBuf::from(record["labDir"].as_str().ok_or("실험 경로 없음")?);
    let target=generator_target_dir(&context.state.config.data_dir,&context.job_id);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let target = std::fs::canonicalize(target).map_err(|e| e.to_string())?;
    let binary = target
        .join("release")
        .join(format!("dominion-rust{}", std::env::consts::EXE_SUFFIX));
    let mut built_commit = String::new();
    loop {
        let remaining = record["limits"]["timeBudgetSeconds"]
            .as_u64()
            .map(|budget| {
                budget.saturating_sub(
                    started
                        .elapsed()
                        .as_secs()
                        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0) / 1000),
                )
            })
            .unwrap_or(u64::MAX);
        if remaining == 0 {
            return Ok("time_budget".into());
        }
        if record["attempts"].as_u64().unwrap_or(0)
            >= record["limits"]["maxAttempts"].as_u64().unwrap_or(10)
        {
            return Ok("attempt_limit".into());
        }
        if !generation_fits_remaining_budget(record,input) {
            return Ok("token_budget".into());
        }
        let head = record["headCommit"]
            .as_str()
            .ok_or("세대 커밋 없음")?
            .to_string();
        ensure_compiled_generator(context,record,&lab,&binary,&target,
            &head,&mut built_commit,remaining).await?;
        let attempt = record["attempts"].as_u64().unwrap_or(0) + 1;
        let mut child_input = input.clone();
        child_input["rsiSourceDir"] = json!(lab);
        child_input["rsiKnowledgeSource"]=record["sourceDir"].clone();
        child_input["rsiBaseCommit"] = json!(head);
        child_input["rsiCampaignId"] = json!(context.job_id);
        child_input["rsiGeneratorCommit"] = json!(head);
        // Whole-system evolution uses a real model-authored redesign in the
        // new compiled worker, not a predefined native optimizer.
        if input["rsiSystemEvolution"]==true {
            child_input["rsiNativeInduction"]=json!(false);
            if input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
                child_input["rsiFocusFiles"]=json!([]);
                child_input["rsiAutoFocus"]=json!(true);
                // The worker and its compiled planner must interpret an
                // autonomous system redesign consistently, even after the
                // controller chooses concrete source targets.
                child_input["rsiAutonomous"]=json!(true);
            }
        }
        let objective = input["rsiObjective"].as_str().unwrap_or("");
        let trial_algorithm = record["nextTrialAlgorithmId"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                pending_trial_candidate(&context.state, objective)
                    .and_then(|algorithm| algorithm["id"].as_str().map(str::to_string))
            });
        if let Some(id) = trial_algorithm {
            child_input["rsiTrialAlgorithmId"] = json!(id);
            record["nextTrialAlgorithmId"] = json!(id);
        } else {
            record["nextTrialAlgorithmId"] = Value::Null;
        }
        if let Some(domain) = record["nextDecision"]["nextDomain"]
            .as_str()
            .filter(|domain| DOMAINS.contains(domain))
        {
            child_input["rsiSelectedDomain"] = json!(domain);
            child_input["rsiDecisionInput"] = record["nextDecision"].clone();
        } else {
            child_input["rsiSelectedDomain"] = Value::Null;
            child_input["rsiDecisionInput"] = Value::Null;
        }
        child_input["rsiIndexId"]=crate::engine::code_index::refresh(&context.state,&lab)?["id"].clone();
        let inherited_candidates=context.state.store.all("experience").into_iter()
            .filter(|item|item["status"]=="active" && item["partitionKey"]==research_scope(&child_input).partition_key
                && item["outcome"]=="success" && item["evidence"]["accepted"]==true)
            .collect::<Vec<_>>();
        let mut inherited_implementations=Vec::new();
        for item in inherited_candidates {
            let Some(commit)=item["implementation_ref"]["commit"].as_str() else {continue};
            // Earlier accepted capabilities remain in descendant binaries.
            if git(context,&lab,&["merge-base","--is-ancestor",commit,&head]).await.is_ok() {
                inherited_implementations.push(json!({"experienceId":item["id"],"implementation_ref":item["implementation_ref"],
                    "status":"available_in_binary","generatorCommit":head,"binarySha256":record["generatorBinarySha256"]}));
            }
            if inherited_implementations.len()>=4 {break;}
        }
        child_input["rsiInheritedImplementations"]=json!(inherited_implementations);
        child_input["rsiAvailableTools"] =
            json!(["code_search", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_replace_function", "workspace_write", "web_search", "http_fetch", "algorithm_propose", "unsafe_audit", "rsi_method_propose", "rsi_system_plan"]);

        // After a measured native adoption, the descendant must be offered
        // the next domain rather than re-solving a saturated original target.
        // The new generator still independently plans, compiles and evaluates.
        record["planObjectiveOverride"]=Value::Null;
        if let Some(domain)=native_descendant_target(input,
            record["history"].as_array().map(Vec::as_slice).unwrap_or(&[])) {
            // The newly built planner reads the durable parent campaign.
            // Publish the measured next objective before launching it, so
            // planner and coordinator agree on the next domain and digest.
            record["planObjectiveOverride"]=json!(domain);
            child_input["rsiObjective"]=json!(domain);
            // An observed native post-adoption target is explicit enough to
            // constrain domain selection even in an automatic campaign.
            child_input["rsiAutonomous"]=json!(false);
            child_input["rsiFocusFiles"]=json!([]);
            child_input["rsiSelectedDomain"]=json!(domain);
            child_input["rsiDecisionInput"]=json!({
                "basis":"independently_adopted_native_descendant",
                "domain":domain,"parentCommit":head
            });
            child_input["prompt"]=json!(format!(
                "Improve the DOMINION {domain} subsystem, preserving all prior accepted source revisions. Implement and evaluate a new measurable optimization against the frozen paired holdout."));
        }
        // Run a genuine model-scaffold research competition for the source
        // generator (including owner-configured API routes), independently
        // of any external model-specific intervention. Every trial uses
        // the SAME frozen source-paired oracle and is attributed to its real model.
        let generator_competition=
            crate::engine::rsi_model_learning::live_generator_competition(
                &context.state,
                record["history"].as_array().map(Vec::as_slice)
                    .unwrap_or(&[]),
                &child_input,&head,attempt,
                (input["rsiAutoGeneratorPortfolio"]==true
                    || (input["rsiAutonomous"]==true
                        &&input["rsiSystemEvolution"]==true))
                    &&input["rsiLockGeneratorModel"]!=true
            ).await;
        if generator_competition["selectedRouteEnabled"]==true {
            child_input["providerId"]=
                generator_competition["selectedProviderId"].clone();
            child_input["model"]=generator_competition["selectedModel"].clone();
        }
        child_input["rsiGeneratorCompetition"]=generator_competition;
        let recorded_history=record["history"].as_array().cloned().unwrap_or_default();
        let action_history=improvement_history(
            &context.state,&child_input,&recorded_history,&context.job_id);
        let target_objective=selection_objective(&child_input);
        let target_trial=child_input["rsiTrialAlgorithmId"].as_str().map(str::to_string);
        let parent_domain=child_input["rsiSelectedDomain"].as_str().map(str::to_string);
        let native_planning=child_input["model"].as_str().is_some_and(|model|!model.is_empty())
            && child_input["providerId"].as_str().is_some_and(|provider|!provider.is_empty());
        if native_planning {
            // The previous generation's nextDomain is useful evidence, not an
            // instruction that freezes a new generator's source-level policy.
            // Its rationale remains in rsiDecisionInput for inspection.
            child_input["rsiSelectedDomain"]=Value::Null;
        }
        // Legacy, model-free native fixtures use the deterministic coordinator
        // plan. A real model generation instead delegates planning and file
        // ranking to its own newly built executable before spending tokens.
        let (target_domain,target_selection,target_strategy,worker_ranked)=if native_planning {
            record["planAttempt"]=json!(attempt);
            record["planIndexId"]=child_input["rsiIndexId"].clone();
            record["planTrialAlgorithmId"]=child_input["rsiTrialAlgorithmId"].clone();
            record["planSelectedDomain"]=child_input["rsiSelectedDomain"].clone();
            record["phase"]=json!("planning_next_generator");
            save(context,record)?;
            let mut planner=Command::new(&binary);
            planner.arg("--rsi-worker-plan").arg(&context.job_id)
                .current_dir(&lab)
                .env("DOMINION_DATA",std::fs::canonicalize(&context.state.config.data_dir)
                    .map_err(|e|e.to_string())?)
                .env("DOMINION_WORKSPACE",&lab)
                .env_remove("DOMINION_DESKTOP_PARENT");
            let output=command(context,planner,Duration::from_secs(30)).await?;
            if !output.status.success() {
                return Err(format!("차세대 실행기가 자체 목표를 설계하지 못했습니다: {}",
                    String::from_utf8_lossy(&output.stderr).chars().take(700).collect::<String>()));
            }
            if output.stdout.len()>64*1024 {
                return Err("차세대 실행기의 계획 출력이 허용 범위를 넘었습니다.".into());
            }
            let plan:Value=serde_json::from_slice(&output.stdout)
                .map_err(|e|format!("차세대 실행기의 목표 계획 형식 오류: {e}"))?;
            let domain=plan["domain"].as_str().filter(|d|DOMAINS.contains(d))
                .ok_or("차세대 실행기가 허용되지 않은 개선 분야를 선택했습니다.")?;
            let selection=plan["selection"].as_str()
                .filter(|s|matches!(*s,"algorithm_active"|"algorithm_trial"|"research_continuation"))
                .ok_or("차세대 실행기의 선택 방식이 유효하지 않습니다.")?;
            let strategy=plan["strategy"].clone();
            if !strategy.is_object() && !strategy.is_null() {
                return Err("차세대 실행기 전략의 자료형이 올바르지 않습니다.".into());
            }
            let expected_target=generation_target_plan(domain,selection,&strategy,&head);
            if plan["source"]!="compiled_generator_plan_v1"
                || plan["generatorCommit"]!=head
                || plan["attempt"].as_u64()!=Some(attempt)
                || plan["historyCount"].as_u64()!=Some(recorded_history.len() as u64)
                || plan["objectiveId"]!=crate::engine::memory::stable_id(&[&target_objective])
                || plan["indexId"]!=child_input["rsiIndexId"]
                || plan["targetPlan"]!=expected_target {
                return Err("차세대 계획과 동결된 세대 계보·인덱스·revision이 다릅니다.".into());
            }
            let directive=&plan["improvementDirective"];
            let last_id=action_history.last().map(|row|row["id"].clone())
                .unwrap_or(Value::Null);
            if !directive.is_object()
                || directive["sourceGenerationId"]!=last_id
                || !matches!(directive["mode"].as_str(),
                    Some("discover"|"implement_research"|"repair_counterexample"|
                         "revise_candidate"|"change_approach"|"extend_verified_capability"))
            {
                return Err("차세대 실행기의 실패 학습·개선 지시가 세대 이력과 일치하지 않습니다.".into());
            }
            // The independent supervisor replays method A and actual B job
            // receipts instead of trusting an arbitrary descendant plan.
            if plan["methodSnapshot"]!=method_feedback::method_snapshot(
                &context.state,&recorded_history,&head)
                || plan["downstreamFeedback"]!=method_feedback::downstream_feedback(
                    &context.state,&recorded_history,&head) {
                return Err("실제 개선법 계승과 독립 과제 피드백 출처가 맞지 않습니다.".into());
            }
            let rows=plan["rankedFiles"].as_array()
                .ok_or("차세대 실행기의 소스 탐색 결과가 배열이 아닙니다.")?.clone();
            if rows.len()>2 || rows.iter().any(|row|row["path"].as_str()
                .is_none_or(|path|!crate::engine::rsi::safe_path(path))) {
                return Err("차세대 실행기의 소스 후보 경로가 유효하지 않습니다.".into());
            }
            let selected_domain=domain.to_string();
            let selected_mode=selection.to_string();
            record["nextGenerationPlan"]=plan;
            save(context,record)?;
            (selected_domain,selected_mode,strategy,Some(rows))
        } else {
            let (domain,selection,strategy)=select_generation_strategy(
                &context.state,if target_trial.is_some(){&recorded_history}else{&action_history},&target_objective,
                research_continuation(&action_history),target_trial,parent_domain,&head);
            (domain,selection.to_string(),strategy,None)
        };
        child_input["rsiTargetDomain"]=json!(target_domain);
        if input["rsiSystemEvolution"]==true && native_planning {
            let model=&record["nextGenerationPlan"]["systemBlueprint"];
            let agenda=&record["nextGenerationPlan"]["systemAgenda"];
            // The old coordinator verifies immutable source identity only:
            // it must not recompute or veto the new intelligence's ranking.
            crate::engine::rsi_system::validate_system_agenda(
                &context.state,model,agenda,
                child_input["rsiIndexId"].as_str().unwrap_or(""),
                &lab,&head,recorded_history.len())?;
            if child_input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty)
                && child_input["selectionPolicy"]=="automatic"
                && agenda["target"].is_object()
                && agenda["target"]["domain"]!=target_domain
                && crate::engine::rsi_system::inherited_domain(&recorded_history)
                    .as_deref()!=Some(target_domain.as_str()) {
                return Err("후손의 재설계 목표와 실제 선택 분야가 다릅니다.".into());
            }
            let experiment=&record["nextGenerationPlan"]["experimentDesign"];
            verify_experiment_design(experiment,&head,&recorded_history,
                agenda,&lab,&record["nextGenerationPlan"]["methodSnapshot"])?;
            if experiment["protocol"]=="method_on_off"
                && child_input["rsiMethodAblation"]!=true {
                return Err("대조 실험에 필요한 사용자 허용이 없습니다.".into());
            }
            let meta=&record["nextGenerationPlan"]["metaPolicy"];
            if meta["source"]!="compiled_editable_meta_controller_v1"
                || meta["historyCount"].as_u64()!=Some(recorded_history.len() as u64)
                || meta["target"].is_object()
                    && !agenda["frontier"].as_array().is_some_and(|f|
                        f.contains(&meta["target"])) {
                return Err("후손의 수정 가능한 메타 개선기 정책이 유효하지 않습니다.".into());
            }
            let factorial=&record["nextGenerationPlan"]["factorialExperiment"];
            if agenda["factorialExperiment"].is_object()
                && agenda["factorialExperiment"]!=*factorial {
                return Err("후손의 실제 실험 단계가 세대 계획과 다릅니다.".into());
            }
            let laboratory=&record["nextGenerationPlan"]["recursiveLaboratory"];
            if !crate::engine::rsi_recursive_laboratory::valid(
                laboratory,agenda["frontier"].as_array()
                    .map(Vec::as_slice).unwrap_or(&[]),
                factorial,meta,&head,recorded_history.len())
                || agenda["recursiveLaboratory"].is_object()
                    && agenda["recursiveLaboratory"]!=*laboratory {
                return Err("실제 후손의 자기발전 연구 제어기와 소스 계획이 다릅니다.".into());
            }
            let predictor=&record["nextGenerationPlan"]["principleModel"];
            let learned=&record["nextGenerationPlan"]["principleCurriculum"];
            if !crate::engine::rsi_principle_learner::valid(
                learned,predictor,agenda["frontier"].as_array()
                    .map(Vec::as_slice).unwrap_or(&[]),
                &head,recorded_history.len())
                || agenda["principleCurriculum"].is_object()
                    && agenda["principleCurriculum"]!=*learned {
                return Err("후손의 개선 원리 학습기와 실제 소스 계획이 다릅니다.".into());
            }
            let whole=&record["nextGenerationPlan"]["wholeDominion"];
            if !crate::engine::rsi_whole_agent::valid(
                whole,agenda["frontier"].as_array()
                    .map(Vec::as_slice).unwrap_or(&[]),&head,recorded_history.len())
                || agenda["wholeDominionGrowth"].is_object()
                    && agenda["wholeDominionGrowth"]!=*whole {
                return Err("실제 후손의 전체 도미니언 성장 계획과 소스 수정 대상이 다릅니다.".into());
            }
            let peers=&record["nextGenerationPlan"]["peerLineages"];
            let profile=crate::engine::rsi_meta_controller::comparison_profile(&child_input);
            crate::engine::rsi_meta_controller::validate_peer_lineages(
                &context.state,peers,profile.as_deref(),&context.job_id)?;
            child_input["rsiSystemBlueprint"]=model.clone();
            child_input["rsiSystemAgenda"]=agenda.clone();
            child_input["rsiMetaPolicy"]=meta.clone();
            child_input["rsiExperimentDesign"]=
                record["nextGenerationPlan"]["experimentDesign"].clone();
            // Normal API opts in to control experiments; the newly compiled
            // policy chooses which generation needs that measured protocol.
            if child_input["rsiMethodAblation"]==true {
                let use_control=child_input["rsiExperimentDesign"]["protocol"]=="method_on_off";
                child_input["rsiMethodAblation"]=json!(use_control);
            }
            child_input["rsiFactorialExperiment"]=factorial.clone();
            child_input["rsiRecursiveLaboratory"]=laboratory.clone();
            child_input["rsiPrincipleModel"]=predictor.clone();
            child_input["rsiPrincipleCurriculum"]=learned.clone();
            child_input["rsiWholeDominionGrowth"]=whole.clone();
            child_input["rsiPeerLineages"]=peers.clone();
            child_input["rsiMethodSnapshot"]=
                record["nextGenerationPlan"]["methodSnapshot"].clone();
            child_input["rsiDownstreamFeedback"]=
                record["nextGenerationPlan"]["downstreamFeedback"].clone();
            child_input["rsiMetaMethodHypothesis"]=
                record["nextGenerationPlan"]["metaMethodHypothesis"].clone();
            child_input["rsiCrossLineageHypothesis"]=
                record["nextGenerationPlan"]["crossLineageHypothesis"].clone();
            child_input["rsiSelectedDomain"]=json!(target_domain);
            child_input["rsiDecisionInput"]=json!({
                "source":"compiled_descendant_self_controller_v1",
                "generatorCommit":head,"target":agenda["target"],
                "sourceTreeSha256":model["sourceTreeSha256"]
            });
        }
        // The actual new generator's domain is known now. Incorporate
        // independently measured cross-objective examples only within this
        // selected domain; unverified events stay objective-local.
        let learned_method=learn_execution_method(&context.state,&child_input,
            record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]));
        child_input["rsiExecutionMethod"]=learned_method.clone();
        context.state.store.put("rsi_execution_method",&learned_method);
        // Inherit the MAIN compositional controller, including source-grounding
        // stages and rules derived from actually measured child edits.
        // Program generation itself earns no score; independent paired
        // descendants must execute a rule before it can accumulate payoff.
        let durable_evidence=durable_edit_policy_history(
            &context.state,&child_input,&recorded_history);
        let edit_policy=crate::engine::rsi_edit_genome::evolve(
            &durable_evidence,verified_genome_reward);
        child_input["rsiEditPolicy"]=edit_policy;

        child_input["rsiImprovementDirective"]=if native_planning {
            record["nextGenerationPlan"]["improvementDirective"].clone()
        } else {
            generation_improvement_directive(&child_input,&action_history)
        };
        // A rejected source patch must be physically present in the next
        // isolated worker. A prompt that only mentions the failed patch cannot
        // repair it: each generation otherwise starts from a clean base.
        // Preserve the pinned base, then let the child edit the restored bytes.
        if matches!(child_input["rsiImprovementDirective"]["mode"].as_str(),
            Some("revise_candidate"|"repair_counterexample")) {
            if let Some(last)=action_history.last() {
                if last["accepted"]!=true
                    && last["generatorCommit"].as_str()==Some(head.as_str()) {
                    if let Some(id)=last["attemptedCandidateId"].as_str()
                        .or_else(||last["candidateId"].as_str()) {
                        let transferable=context.state.store.get("rsi_candidate",id)
                            .is_some_and(|candidate|{
                                if candidate["baseCommit"].as_str()!=Some(head.as_str())
                                    || candidate["rsiJobId"]!=last["jobId"] {
                                    return false;
                                }
                                let compiler_failure=last["candidateCapture"]["checksPassed"]==false
                                    && candidate["status"]=="proposed";
                                let plateau=last["candidateCapture"]["checksPassed"]==true
                                    && last["evaluation"]["verdict"]=="inconclusive"
                                    && crate::engine::rsi::verify_revisable_quality_receipt(
                                        &context.state,&candidate).is_ok();
                                compiler_failure || plateau
                            });
                        // A verified neutral native source patch already
                        // consumed one operator. Do not restore it into a
                        // dirty worktree: that prevents the next native
                        // hypothesis from being attempted at all.
                        let native_alternative=transferable
                            && context.state.store.get("rsi_candidate",id)
                                .is_some_and(|candidate|
                                    native_plateau_prefers_next_operator(
                                        &child_input,&candidate));
                        if transferable && !native_alternative {
                            child_input["rsiRepairCandidateId"]=json!(id);
                            child_input["rsiRepairParentJobId"]=last["jobId"].clone();
                        }
                    }
                }
            }
        }
        // Cross-campaign restart-durable Pareto stepping stones from main.
        // The archive replays unchanged oracle, baseline and original pair.
        // A neutral patch can seed the next worktree only when its own repair
        // restorer rechecks the original source and paired evidence.
        let frozen_cases=if child_input["rsiFrozenCases"].as_array().is_some(){
            child_input["rsiFrozenCases"].clone()
        }else{json!(crate::engine::rsi_worktree::metric_cases())};
        let archive=crate::engine::rsi_archive::select(
            &context.state,
            record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            &head,&frozen_cases);
        child_input["rsiArchiveSelection"]=archive.clone();
        if child_input["rsiRepairCandidateId"].is_null()
            && child_input["rsiNativeInduction"]!=true
            && archive["candidateId"].is_string(){
            if let Some(id)=archive["candidateId"].as_str(){
                if let Some(candidate)=context.state.store.get("rsi_candidate",id){
                    if candidate["baseCommit"]==head
                        &&candidate["status"]=="inconclusive"
                        &&crate::engine::rsi::verify_revisable_quality_receipt(
                            &context.state,&candidate).is_ok(){
                        if let Some(parent)=candidate["rsiJobId"].as_str(){
                            let run=context.state.store.get("rsi_run",
                                &format!("rsi-run-{parent}"));
                            if run.is_some_and(|row|
                                row["candidateId"]==id
                                &&row["checksPassed"]==true
                                &&row["baseCommit"]==head) {
                                child_input["rsiRepairCandidateId"]=json!(id);
                                child_input["rsiRepairParentJobId"]=json!(parent);
                                child_input["rsiInheritedCandidateId"]=json!(id);
                                child_input["rsiArchiveRestored"]=json!(true);
                            }
                        }
                    }
                }
            }
        }
        child_input["rsiTargetPlan"]=generation_target_plan(
            &target_domain,&target_selection,&target_strategy,&head);
        child_input["rsiNativePlan"]=if native_planning {
            record["nextGenerationPlan"].clone()
        } else {
            Value::Null
        };
        let target_policy=target_strategy["id"].as_str()
            .and_then(|id|context.state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(&context.state,"selfimprove.search"));
        if child_input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
            let ranked=worker_ranked.unwrap_or_else(||choose_focus_files(
                &context.state,&child_input,&action_history,target_policy.as_ref()));
            let selected=ranked.iter().filter_map(|row|row["path"].as_str())
                .map(str::to_string).collect::<Vec<_>>();
            child_input["rsiFocusFiles"]=json!(selected);
            child_input["rsiAutoFocus"]=json!(true);
            child_input["rsiFocusSelection"]=json!({
                "source":"indexed_evidence",
                "candidates":ranked,
                "policyRevision":target_policy.as_ref().map(|policy|policy["revision"].clone()),
                "domain":target_domain,
                "sourceIndexId":child_input["rsiIndexId"]
            });
        } else {
            child_input["rsiAutoFocus"]=json!(false);
            child_input["rsiFocusSelection"]=json!({"source":"user_selected"});
        }
        // Repair generations must read the exact file touched by the failed
        // source patch. Re-running the ordinary domain planner can otherwise
        // select unrelated files and strand the inherited code once again.
        if let Some(id)=child_input["rsiRepairCandidateId"].as_str().map(str::to_string) {
            if let Some(candidate)=context.state.store.get("rsi_candidate",&id) {
                child_input["rsiFocusFiles"]=candidate["paths"].clone();
                child_input["rsiAutoFocus"]=json!(true);
                child_input["rsiFocusSelection"]=json!({
                    "source":"measured_candidate_descendant_revision",
                    "candidateId":id,
                    "paths":candidate["paths"],
                    "generatorCommit":head
                });
            }
        }
        let inherited = generation_context(&context.state, input, record);
        let digest = research_digest(&context.state, input, record);
        let focus = focus_file_context(&context.state, record, &child_input);
        let model_bound=child_input["model"].as_str().is_some_and(|s|!s.is_empty())
            && child_input["providerId"].as_str().is_some_and(|s|!s.is_empty());
        // Fixture workers have no model costs. Real workers need both a
        // budget-fitting inherited context and space for their own directives.
        let available=if model_bound {generation_token_quota(record)} else {None};
        let Some((composed,fit))=fit_parent_generation_context(
            child_input["prompt"].as_str().unwrap_or(""),
            &inherited,&digest,&focus,available
        ) else {return Ok("token_budget".into());};
        // Retain semantic source components, not just the flattened prompt.
        // A rebuilt descendant can choose a different domain and regenerate
        // source evidence without losing the owner's goal or learned research.
        child_input["rsiPromptBase"]=input["prompt"].clone();
        child_input["rsiPromptInherited"]=json!(inherited);
        child_input["rsiPromptResearch"]=json!(digest);
        child_input["prompt"]=json!(composed);
        child_input["rsiParentContextFit"]=fit;
        child_input["rsiRemainingCampaignTokens"]=json!(available);
        child_input["rsiGenerationTokenLimit"]=json!(available);
        if model_bound && !generation_fits_remaining_budget(record,&child_input) {
            return Ok("token_budget".into());
        }
        // Record prompt composition so real-model savings can be measured
        // separately from completion tokens and tool-call counts.
        child_input["rsiFocusContextChars"] = json!(focus.chars().count());
        child_input["rsiPromptContextChars"] = json!(child_input["prompt"]
            .as_str().map(|prompt| prompt.chars().count()).unwrap_or(0));
        // Real source-task ablation uses a different detached worktree and
        // independent paired evaluator. It never adopts the control patch.
        run_method_control(context,record,&mut child_input,
            &lab,&binary,&head,started).await?;
        if !generation_fits_remaining_budget(record,&child_input) {
            return Ok("token_budget".into());
        }
        let child = context
            .state
            .store
            .put(
                "job",
                &json!({"kind":"rsi_generation","status":"running",
            "input":child_input,"queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1}),
            )
            .ok_or("세대 작업 저장 실패")?;
        let child_id = child["id"].as_str().ok_or("세대 작업 ID 없음")?;
        record["attempts"] = json!(attempt);
        record["activeGenerationJobId"] = json!(child_id);
        record["activeBaseTokens"] = record["tokens"].clone();
        record["phase"] = json!("diagnosing_and_editing");
        save(context, record)?;
        let mut worker = Command::new(&binary);
        worker
            .arg("--rsi-worker-job")
            .arg(child_id)
            .current_dir(&lab)
            .env(
                "DOMINION_DATA",
                std::fs::canonicalize(&context.state.config.data_dir).map_err(|e| e.to_string())?,
            )
            .env("DOMINION_WORKSPACE", &lab)
            .env_remove("DOMINION_DESKTOP_PARENT");
        let worker_started = Instant::now();
        let remaining = record["limits"]["timeBudgetSeconds"]
            .as_u64()
            .map(|budget| {
                budget.saturating_sub(
                    started
                        .elapsed()
                        .as_secs()
                        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0) / 1000),
                )
            })
            .unwrap_or(u64::MAX);
        let worker_timeout = if remaining == u64::MAX {
            7 * 24 * 3600
        } else {
            remaining.clamp(1, 900)
        };
        let output = command(context, worker, Duration::from_secs(worker_timeout)).await;
        let current = context
            .state
            .store
            .get("job", child_id)
            .ok_or("세대 작업 사라짐")?;
        let session = current["sessionId"].as_str();
        let stopped=generation_stop_reason(&current,output.as_ref().err().map(String::as_str));
        let mut generation=collect_generation_evidence(
            context,record,&current,attempt,child_id,&head,
            worker_started.elapsed().as_millis() as u64);
        // Keep serde_json's macro recursion bounded while preserving
        // native provenance as first-class generation evidence.
        generation["editPolicy"]=current["input"]["rsiEditPolicy"].clone();
        let mut timeline=session.map(|id|context.state.store.all("execution_operation")
            .into_iter().filter(|op|op["sessionId"]==id)
            .collect::<Vec<_>>()).unwrap_or_default();
        timeline.sort_by_key(|op|op["opSeq"].as_u64().unwrap_or(u64::MAX));
        generation["toolTimeline"]=json!(timeline);
        generation["ruleExecutions"]=session.map(|id|context.state.store.all("rsi_rule_execution")
            .into_iter().filter(|row|row["sessionId"]==id)
            .collect::<Vec<_>>()).map(|mut rows|{
                rows.sort_by_key(|row|row["toolCalls"].as_u64().unwrap_or(u64::MAX));
                json!(rows)
            }).unwrap_or_else(||json!([]));
        generation["archiveSelection"]=current["input"]["rsiArchiveSelection"].clone();
        generation["inheritedCandidateId"]=current["input"]["rsiInheritedCandidateId"].clone();
        generation["systemBlueprint"]=current["input"]["rsiSystemBlueprint"].clone();
        generation["systemAgenda"]=current["input"]["rsiSystemAgenda"].clone();
        generation["recursiveLaboratory"]=current["input"]["rsiRecursiveLaboratory"].clone();
        generation["principleModel"]=current["input"]["rsiPrincipleModel"].clone();
        generation["principleCurriculum"]=current["input"]["rsiPrincipleCurriculum"].clone();
        generation["wholeDominionGrowth"]=current["input"]["rsiWholeDominionGrowth"].clone();
        generation["generatorCompetition"]=
            current["input"]["rsiGeneratorCompetition"].clone();
        generation["controllerPlan"]=current["input"]["rsiNativePlan"]["systemAgenda"].clone();
        generation["systemEvolution"]=current["input"]["rsiSystemEvolution"].clone();
        generation["systemBlueprintVerified"]=current["rsiSystemBlueprintVerified"].clone();
        generation["selfControllerVerified"]=current["rsiSelfControllerVerified"].clone();
        generation["inheritedMetaMethod"]=current["rsiInheritedMetaMethod"].clone();
        generation["metaPolicy"]=current["input"]["rsiMetaPolicy"].clone();
        generation["metaPolicyExecuted"]=current["rsiMetaPolicyExecuted"].clone();
        generation["methodSnapshot"]=current["input"]["rsiMethodSnapshot"].clone();
        generation["experimentDesign"]=current["input"]["rsiExperimentDesign"].clone();
        generation["experimentDesignExecuted"]=
            current["rsiExperimentDesignExecuted"].clone();
        generation["downstreamFeedback"]=current["input"]["rsiDownstreamFeedback"].clone();
        generation["metaMethodHypothesis"]=current["input"]["rsiMetaMethodHypothesis"].clone();
        generation["peerLineages"]=current["input"]["rsiPeerLineages"].clone();
        generation["crossLineageHypothesis"]=
            current["input"]["rsiCrossLineageHypothesis"].clone();
        generation["comparisonProfileSha256"]=json!(
            crate::engine::rsi_meta_controller::comparison_profile(&current["input"]));
        generation["systemDesign"]=session.and_then(|id|context.state.store.get("session",id))
            .map(|row|row["rsiSystemDesign"].clone()).unwrap_or(Value::Null);
        generation["candidateOrigin"]=current["result"]["rsi"]["candidateOrigin"].clone();
        generation["nativeMutation"]=current["result"]["rsi"]["nativeMutation"].clone();
        let capture = &current["result"]["rsi"];
        // Capture evidence even when compilation failed. candidateId remains
        // reserved for candidates eligible for the frozen paired comparison.
        if let Some(id)=capture["candidateId"].as_str() {
            generation["attemptedCandidateId"]=json!(id);
            generation["candidateCapture"]=json!({
                "candidateId":id,"checksPassed":capture["checksPassed"],
                "checks":capture["checks"],"captureError":capture["captureError"],
                "modelError":capture["modelError"],"diffBytes":capture["diffBytes"]
            });
        }
        let candidate_id = verified_candidate_id(&current["result"]);
        let waiting_before = record["waitingMs"].as_u64().unwrap_or(0);
        if let Some(id) = candidate_id {
            record["phase"] = json!("measuring");
            save(context, record)?;
            generation["candidateId"] = json!(id);
            let compared = rsi_worktree::compare_at(
                context,
                id,
                &lab,
                Path::new(
                    record["sourceDir"]
                        .as_str()
                        .ok_or("고정 평가기 경로 없음")?,
                ),
            )
            .await;
            match compared {
                Ok(evaluation) => {
                    generation["evaluation"] = evaluation;
                    let candidate = context
                        .state
                        .store
                        .get("rsi_candidate", id)
                        .ok_or("후보 소실")?;
                    // Real candidate diff paths are required to distinguish
                    // isolated A/B experiments from joint or confounded edits.
                    // Preserve them for both rejected and adopted source trials.
                    generation["changedSourcePaths"]=candidate["paths"].clone();
                    let evidence_valid=crate::engine::rsi::verify_paired_receipt(
                        &context.state,&candidate).is_ok();
                    generation["pairedRevalidated"]=json!(evidence_valid);
                    let eligible = candidate["status"] == "verified"
                        && candidate["evaluation"]["metrics"]["eligible"] == true
                        && evidence_valid;
                    let winner = if eligible {
                        "candidate"
                    } else if candidate["status"] == "rejected" {
                        "baseline"
                    } else {
                        "inconclusive"
                    };
                    record["comparison"] = json!({"attempt":attempt,"candidateId":id,"winner":winner,
                        "metrics":candidate["evaluation"]["metrics"],"baselineCommit":head,
                        "evaluationId":candidate["evaluation"]["id"],"canSelectCandidate":eligible});
                    let choice = select(context, record).await?;
                    generation["selection"] = json!(choice);
                    if eligible && choice == "candidate" {
                        let commit = adopt(context, &lab, &candidate, &head).await?;
                        // A success signal is useful only for the actual file
                        // changed by a verified candidate, at an exact revision.
                        // Save post-adoption hashes for subsequent generations.
                        let mut accepted_hashes=serde_json::Map::new();
                        for path in candidate["paths"].as_array().into_iter().flatten()
                            .filter_map(Value::as_str) {
                            if !crate::engine::rsi::safe_path(path) {continue;}
                            if let Ok(bytes)=std::fs::read(lab.join(path)) {
                                accepted_hashes.insert(path.to_string(),
                                    json!(format!("{:x}",Sha256::digest(&bytes))));
                            }
                        }
                        generation["acceptedSourceHashes"]=Value::Object(accepted_hashes);
                        generation["accepted"] = json!(true);
                        generation["acceptedCommit"] = json!(commit);
                        generation["candidateTarget"]=candidate["target"].clone();
                        record["headCommit"] = json!(commit);
                        record["acceptedGenerations"] =
                            json!(record["acceptedGenerations"].as_u64().unwrap_or(0) + 1);
                        let mut saved = candidate;
                        saved["experimentAdoption"] = json!({"campaignId":context.job_id,
                            "commit":commit,"generatorCommit":head,"productionDeployed":false});
                        context
                            .state
                            .store
                            .put("rsi_candidate", &saved)
                            .ok_or("채택 기록 저장 실패")?;
                        context.state.store.put("rsi_current",&json!({"id":"selected-engine","commit":commit,
                            "sourceDir":record["sourceDir"],"sourceBaseCommit":record["baseCommit"],
                            "sourceFingerprint":record["sourceFingerprint"],"campaignId":context.job_id,
                            "candidateId":id,"selectedAt":chrono::Utc::now().to_rfc3339()})).ok_or("선택한 개선기 저장 실패")?;
                    }
                }
                Err(error) => generation["error"] = json!(error),
            }
        } else {
            let worker_error = output.as_ref().err().map(String::as_str);
            let child = &current["result"]["rsi"];
            let validation_feedback=candidate_capture_feedback(child);
            generation["error"] = json!(validation_feedback.as_deref()
                .or(worker_error)
                .or_else(|| child["modelError"].as_str())
                .or_else(|| child["captureError"].as_str())
                .or_else(|| current["error"].as_str())
                .unwrap_or("유효한 변경이나 검사 통과 없음"));
        }
        generation["elapsedMs"] = json!((worker_started.elapsed().as_millis() as u64)
            .saturating_sub(
                record["waitingMs"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_sub(waiting_before)
            ));
        let artifacts=session.map(|s|research_artifacts(&context.state,s)).unwrap_or_default();
        let hashes=artifacts.iter().map(|a|a["hash"].clone()).collect::<Vec<_>>();
        let new_findings=new_research_findings(&context.state,&current["input"],child_id,
            &hashes,record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]));
        generation["knowledgeArtifactHashes"]=json!(hashes);
        generation["newFindings"]=json!(new_findings);
        generation["stopReason"]=json!(stopped);
        if generation["systemEvolution"]==true {
            generation["metaCodeChanged"]=
                json!(crate::engine::rsi_meta_controller::meta_source_changed(
                    &generation));
            generation["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
                &generation["metaPolicy"],&generation);
        }
        if candidate_id.is_none() && new_findings>0 && output.is_ok() && stopped.is_none()
            && current["result"]["rsi"]["modelError"].is_null() {
            generation["error"]=Value::Null;
        }
        if let Some(session)=session {
            generation["methodExecutions"]=json!(record_method_feedback(&context.state,&generation,session));
        }
        generation["methodContrast"]=crate::engine::rsi_evolve::method_contrast_evidence(
            &context.state,&generation);
        generation["knowledgeId"]=json!(remember_research(&context.state,&current["input"],&generation,session.unwrap_or("")));
        if let Some(session)=session {
            generation["knowledgeReuse"]=context.state.store.get("session",session).map(|s|s["rsiKnowledgeReuse"].clone()).unwrap_or(Value::Null);
        }
        generation["improvementActionObservation"]=improvement_action_observation(
            &generation,current["result"]["rsi"]["diffBytes"].as_u64().unwrap_or(0));
        let prior_history = record["history"].as_array().cloned().unwrap_or_default();
        if let Some(session)=session {
            let proposals = proposed_algorithms(&context.state, session);
            let active = crate::engine::algorithms::active(&context.state, "selfimprove.search")
                .unwrap_or_else(|| json!({"id":Value::Null,"revision":Value::Null,"implementation":"failure-clusters","parameters":{}}));
            let mut queued = Vec::new();
            for candidate in proposals
                .iter()
                .filter(|candidate| candidate["kind"] == "selfimprove.search")
            {
                let report = queue_search_trial(
                    &context.state,
                    candidate,
                    &active,
                    &prior_history,
                    input["rsiObjective"].as_str().unwrap_or(""),
                    &context.job_id,
                    attempt,
                );
                if report["status"] == "pending_trial" {
                    if let Some(id) = report["candidateId"].as_str() {
                        queued.push(id.to_string());
                    }
                }
            }
            if !queued.is_empty() {
                generation["searchTrialsQueued"] = json!(queued);
            }
            record["nextTrialAlgorithmId"] = Value::Null;
            if generation["domainSelection"].as_str() == Some("algorithm_trial") {
                if let Some(id) = generation["searchStrategy"]["id"].as_str() {
                    let outcome =
                        record_trial_outcome(&context.state, id, &generation, &prior_history);
                    // A research continuation did not run the formula. Keep the
                    // candidate pending for the next already scheduled generation.
                    if outcome["status"] == "not_used" {
                        record["nextTrialAlgorithmId"] = json!(id);
                    } else if outcome["status"] == "pending_second_trial" {
                        record["nextTrialAlgorithmId"] = json!(id);
                    }
                    generation["trialOutcome"] = outcome;
                }
            }
            if record["nextTrialAlgorithmId"].is_null() {
                if let Some(id) = queued.into_iter().next() {
                    record["nextTrialAlgorithmId"] = json!(id);
                }
            }
        }
        let generation_accepted = generation["accepted"] == true;
        let accepted_streak = if generation_accepted {
            record["acceptedStreak"].as_u64().unwrap_or(0) + 1
        } else {
            0
        };
        record["acceptedStreak"] = json!(accepted_streak);
        let max_streak = record["maxAcceptedStreak"]
            .as_u64()
            .unwrap_or(0)
            .max(accepted_streak);
        record["maxAcceptedStreak"] = json!(max_streak);
        // Capability requires measured held-out evidence, not a streak of
        // adopted patches whose comparison was inconclusive or regressed.
        let verified_streak = if generation_is_verified(&generation) {
            record["verifiedStreak"].as_u64().unwrap_or(0) + 1
        } else {
            0
        };
        record["verifiedStreak"] = json!(verified_streak);
        let verified_max = record["maxVerifiedStreak"]
            .as_u64()
            .unwrap_or(0)
            .max(verified_streak);
        record["maxVerifiedStreak"] = json!(verified_max);
        let design=generation["experimentDesign"].clone();
        let edited=design["target"]["path"].as_str().is_some_and(|path|
            generation["changedSourcePaths"].as_array()
                .is_some_and(|paths|paths.iter().any(|row|row==path)));
        generation["experimentOutcome"]=json!({
            "source":"independent_source_experiment_outcome_v1",
            "policyRevision":design["policyRevision"],
            "sourceEditObserved":edited,
            "generatorCommit":head,
            "evaluationId":generation["evaluation"]["evaluationId"],
            "verifiedQualityGain":edited&&generation["accepted"]==true
                &&generation["pairedRevalidated"]==true
                &&generation_is_verified(&generation),
            "protocol":design["protocol"],
            "actualWorkerExecuted":generation["experimentDesignExecuted"]==design
        });
        generation["evidenceVerified"]=json!(
            generation["accepted"]==true
            &&generation["pairedRevalidated"]==true
            &&generation_is_verified(&generation));
        let mut history_now = record["history"].as_array().cloned().unwrap_or_default();
        history_now.push(generation.clone());
        let proven_history=revalidated_history(&context.state,&history_now);
        record["improvementActionResults"]=json!(improvement_action_results(&proven_history));
        let (proven_streak,proven_max)=verified_capability_streak(&proven_history);
        record["verifiedStreak"]=json!(proven_streak);
        record["maxVerifiedStreak"]=json!(proven_max);
        let meta_transfer = meta_transfer_proven(&proven_history);
        record["metaTransferProven"] = json!(meta_transfer);
        record["threeGenerationProof"]=recursive_closure_report(&proven_history);
        record["recursiveProof"]=full_cycle_report(&context.state,&history_now);
        record["capabilityDemonstrated"]=
            record["recursiveProof"]["closedLoopObserved"].clone();
        let generation_knowledge = generation["knowledgeId"].clone();
        let generation_tokens = generation["tokens"].clone();
        let generation_elapsed = generation["elapsedMs"].clone();
        let generation_verdict = generation["evaluation"]["verdict"].clone();
        // Workers read this durable table, not the coordinator's in-memory
        // history. Publish only after research, method feedback and trial
        // outcomes are complete so the next worker can actually continue.
        // Store::get supplies createdAt at seconds resolution, obscuring
        // which completion followed which within the same second.
        generation["completedAt"]=json!(chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis,true));
        context.state.store.put("rsi_generation", &generation)
            .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
        // The next generation learns from the tool *actually used*, candidate
        // checks and paired verdict. This trial receipt is observational until
        // an independently adopted descendant provides quality evidence.
        context.state.store.put("rsi_execution_method_trial",&json!({
            "id":format!("rsi-method-trial-{}-{attempt}",context.job_id),
            "methodId":generation["executionMethod"]["id"],
            "methodRevision":generation["executionMethod"]["revision"],
            "preferredTool":generation["executionMethod"]["preferredTool"],
            "jobId":child_id,"campaignId":context.job_id,
            "generatorCommit":head,"observedWrites":generation["writeReceipts"],
            "methodActions":generation["methodActions"],
            "candidateId":generation["candidateId"],
            "attemptedCandidateId":generation["attemptedCandidateId"],
            "accepted":generation["accepted"],
            "evaluation":generation["evaluation"],
            "tokens":generation["tokens"],
            "causalGain":"unmeasured"
        }));
        record["history"]
            .as_array_mut()
            .ok_or("세대 목록 손상")?
            .push(generation);
        // New findings, not a write count or query spelling, justify continuing
        // a research-only generation within the configured campaign budgets.
        let no_edit = candidate_id.is_none()
            && current["result"]["rsi"]["diffBytes"].as_u64().unwrap_or(0) == 0;
        let research = !artifacts.is_empty();
        let changed = new_findings>0;
        let streak = if no_edit {
            record["noProgressStreak"].as_u64().unwrap_or(0) + 1
        } else {
            0
        };
        record["noProgressStreak"] = json!(streak);
        record["activeGenerationJobId"] = Value::Null;
        let decision_objective = selection_objective(input);
        let decision_history = record["history"].as_array().cloned().unwrap_or_default();
        let decision_basis = if let Some(reason)=stopped {
            reason
        } else if generation_accepted {
            "accepted_implementation"
        } else if new_findings > 0 {
            "research_findings"
        } else if no_edit {
            "no_progress"
        } else {
            "implementation_not_adopted"
        };
        let next_domain = select_domain_for_state(&context.state, &decision_history, &decision_objective);
        let reuse = method_reuse_decision(&context.state, input, &next_domain);
        record["nextDecision"] = json!({
            "basis": decision_basis,
            "fromGeneration": attempt,
            "nextDomain": next_domain,
            "knowledgeId": generation_knowledge,
            "headCommit": record["headCommit"],
            "trialAlgorithmId": record["nextTrialAlgorithmId"],
            "observed": {
                "accepted": generation_accepted,
                "tokens": generation_tokens,
                "elapsedMs": generation_elapsed,
                "verdict": generation_verdict,
            },
            "reuseDecision": reuse,
        });
        elapsed(record, started);
        save(context, record)?;
        if record["capabilityDemonstrated"]==true
            && record["metaTransferProven"]==true
            && input["rsiStopAfterProof"]==true {
            return Ok("capability_demonstrated".into());
        }
        if let Some(reason)=stopped {return Ok(reason.into());}
        if let Some(error)=generation_model_error(&current) {
            if target_plan_mismatch(error) {
                let mismatches = record["targetPlanMismatches"].as_u64().unwrap_or(0) + 1;
                record["targetPlanMismatches"] = json!(mismatches);
                save(context, record)?;
                if mismatches >= 3 {
                    return Ok("target_plan_mismatch_limit".into());
                }
                continue;
            }
            return Err(error.to_string());
        }
        if no_edit && should_stop_no_progress(streak, research, changed)
            && !corrective_retry_allowed(&decision_history,streak) {
            return Ok("no_progress".into());
        }
    }
}

