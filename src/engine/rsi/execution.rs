//! Durable multi-generation RSI orchestration and recorded source-edit execution.
//! Private to the RSI coordinator; no frozen evaluator or model weight changes.
use super::{
    Value, JobContext, Duration, Instant, Path,
    PathBuf, Command, Digest, Sha256, json,
    experience, rsi, rsi_worktree, tools, DOMAINS,
    limits, select_domain_for_state, command, git, save,
    elapsed, select, run, verified_candidate_id, allowed_trial_campaigns,
    trial_owned_by, research_digest, focus_excerpt_has_source, bind_action_to_source, choose_focus_files,
    focus_file_context, generation_context, research_scope, selection_objective, research_artifacts,
    remember_research, session_research_fingerprint, generation_stop_reason, generation_fits_remaining_budget, should_stop_no_progress,
    new_research_findings, session_tool_usage, previous_autonomous_failures, retry_learned_model_failure, generation_model_error,
    adopt, worker, method_reuse_decision, is_synthesized_source_repair, record_method_feedback,
    proposed_algorithms, queue_search_trial, propose_autonomous_search_trial, pending_trial_candidate, record_trial_outcome,
    observed_edit_outcome, observed_action_value, evolved_execution_action, research_continuation, generation_target_plan,
    pending_failed_patch_domain, pending_method_recovery_domain, select_generation_strategy, sub2_edit_policy_reward,
};

pub(super) async fn run_loop(
    context: &JobContext,
    job: &Value,
    record: &mut Value,
    started: Instant,
) -> Result<String, String> {
    let input = &job["input"];
    let lab = PathBuf::from(record["labDir"].as_str().ok_or("실험 경로 없음")?);
    let target = context
        .state
        .config
        .data_dir
        .join("rsi-build")
        .join("generator");
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let target = std::fs::canonicalize(target).map_err(|e| e.to_string())?;
    if record["attempts"]==0 && record["history"].as_array().is_some_and(Vec::is_empty) {
        let prior=previous_autonomous_failures(&context.state,input,&context.job_id);
        let outcome=propose_autonomous_search_trial(
            &context.state,&prior,&selection_objective(input),
            &context.job_id,0,
            input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty));
        if outcome["status"]=="pending_trial" {
            record["nextTrialAlgorithmId"]=outcome["candidateId"].clone();
            record["autonomousWarmStart"]=json!({
                "proposal":outcome,"sourceGenerations":prior.len(),
                "note":"prior recorded failures are hypotheses, not proven gains"
            });
            save(context,record)?;
        }
    }
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
        if built_commit != head {
            record["phase"] = json!("building_generator");
            save(context, record)?;
            let mut build = Command::new(rsi_worktree::cargo_executable());
            build
                .current_dir(&lab)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "--bin",
                    "dominion-rust",
                    "--target-dir",
                ])
                .arg(&target)
                .env("DOMINION_RSI_BUILD_COMMIT", &head);
            let build_timeout = if remaining == u64::MAX {
                3600
            } else {
                remaining.min(600)
            };
            let output = command(context, build, Duration::from_secs(build_timeout)).await?;
            if !output.status.success() {
                return Err(format!(
                    "차세대 개선기 빌드 실패: {}",
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                ));
            }
            let mut identity = Command::new(&binary);
            identity.arg("--rsi-worker-identity");
            let output = command(context, identity, Duration::from_secs(10)).await?;
            if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != head {
                return Err("실제 개선기 바이너리와 세대 커밋이 다릅니다.".into());
            }
            record["generatorBinarySha256"] = json!(format!(
                "{:x}",
                Sha256::digest(std::fs::read(&binary).map_err(|e| e.to_string())?)
            ));
            record["actualGeneratorCommit"] = json!(head);
            built_commit = head.clone();
        }
        let attempt = record["attempts"].as_u64().unwrap_or(0) + 1;
        let mut child_input = input.clone();
        child_input["rsiSourceDir"] = json!(lab);
        child_input["rsiKnowledgeSource"]=record["sourceDir"].clone();
        child_input["rsiBaseCommit"] = json!(head);
        child_input["rsiCampaignId"] = json!(context.job_id);
        child_input["rsiGeneratorCommit"] = json!(head);
        // In whole-system evolution, the child is a model-bearing redesign
        // agent. It may modify algorithms, harness, memory, learning, tools
        // and its own improvement policy; native fixed patches cannot stand
        // in for that new system's judgment or implementation.
        if input["rsiSystemEvolution"]==true {
            child_input["rsiNativeInduction"]=json!(false);
            if input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
                child_input["rsiFocusFiles"]=json!([]);
                child_input["rsiAutonomous"]=json!(true);
                child_input["rsiAutoFocus"]=json!(true);
            }
        }
        let objective=selection_objective(input);
        let allowed_campaigns=allowed_trial_campaigns(
            &context.state,input,record,&context.job_id);
        let trial_algorithm = record["nextTrialAlgorithmId"]
            .as_str()
            .filter(|id|context.state.store.get("algorithm",id)
                .is_some_and(|algorithm|trial_owned_by(&algorithm,&allowed_campaigns)))
            .map(str::to_string)
            .or_else(|| {
                pending_trial_candidate(&context.state,&objective,&allowed_campaigns)
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
            json!(["code_search", "workspace_read_range", "workspace_replace", "workspace_replace_function", "workspace_write", "web_search", "http_fetch", "algorithm_propose", "unsafe_audit", "rsi_method_propose", "rsi_system_plan"]);
        let recorded_history=record["history"].as_array().cloned().unwrap_or_default();
        child_input["rsiActionDirective"]=evolved_execution_action(
            &context.state,&child_input,&recorded_history);
        // Do not lose a genuine but noncompiling source candidate between
        // generations. Preserve the exact source patch as an executable
        // repair starting point, never as an adopted capability.
        if let Some(prior)=recorded_history.last().filter(|prior|
            prior["accepted"]!=true
            && prior["staticChecksPassed"]==false
            && prior["generatorCommit"]==head
            && prior["sourceDiffBytes"].as_u64().unwrap_or(0)>0
            // Do not replay a failed patch unchanged for another generation.
            // The child must have produced a different source diff to earn
            // another repair continuation.
            && (prior["recoveredFailedPatch"]["candidateId"].is_null()
                || prior["recoveredFailedPatch"]["candidateId"]
                    !=prior["unverifiedCandidateId"])) {
            if let (Some(id),Some(failed_job))=(
                prior["unverifiedCandidateId"].as_str(),
                prior["jobId"].as_str()
            ) {
                if let Some(candidate)=context.state.store.get("rsi_candidate",id)
                    .filter(|candidate|
                        candidate["baseCommit"]==head
                        && candidate["experimentCampaignId"]==context.job_id
                        && candidate["patchSha256"].is_string()
                        && candidate["patch"].as_str().is_some_and(|patch|!patch.is_empty())) {
                    let evidence=context.state.store.get("rsi_run",&format!("rsi-run-{failed_job}"))
                        .unwrap_or(Value::Null);
                    let failed_checks=evidence["checks"].as_array()
                        .into_iter().flatten().filter(|check|check["passed"]!=true)
                        .take(2).map(|check|json!({
                            "label":check["label"],
                            "diagnostic":check["primaryDiagnostic"].as_str()
                                .filter(|s|!s.is_empty())
                                .or_else(||check["stderr"].as_str())
                                .or_else(||check["error"].as_str())
                                .unwrap_or("").chars().take(1400).collect::<String>()
                        })).collect::<Vec<_>>();
                    let focus_path=candidate["paths"].as_array()
                        .into_iter().flatten().filter_map(Value::as_str)
                        .filter(|path|crate::engine::rsi::safe_path(path))
                        .find(|path|failed_checks.iter().any(|check|
                            check["diagnostic"].as_str()
                                .is_some_and(|message|message.contains(*path))))
                        .or_else(||candidate["paths"].as_array()
                            .into_iter().flatten().filter_map(Value::as_str)
                            .find(|path|crate::engine::rsi::safe_path(path)))
                        .map(str::to_string);
                    child_input["rsiFailedPatchRecovery"]=json!({
                        "candidateId":id,"baseCommit":head,
                        "failedGenerationJobId":failed_job,
                        "focusPath":focus_path,
                        "checks":failed_checks
                    });
                }
            }
        }
        let target_objective=selection_objective(&child_input);
        // Compute the same domain decision that the worker will use. Keep it
        // distinct from rsiSelectedDomain, which represents an explicit choice.
        let failed_patch_domain=pending_failed_patch_domain(
            &context.state,&child_input,&head);
        let target_trial=if failed_patch_domain.is_some(){None}
            else {child_input["rsiTrialAlgorithmId"].as_str().map(str::to_string)};
        // Use the same actual broken-file pin in coordinator and worker.
        // Otherwise the second process rejects or replans the stale target
        // before it can repair the inherited candidate.
        let repair_domain=failed_patch_domain
            .or_else(||pending_method_recovery_domain(&child_input));
        let (target_domain,target_selection,target_strategy)=select_generation_strategy(
            &context.state,&recorded_history,&target_objective,
            if repair_domain.is_some(){None}else{research_continuation(&recorded_history)},
            target_trial,repair_domain,&head);
        child_input["rsiTargetDomain"]=json!(target_domain);
        child_input["rsiTargetPlan"]=generation_target_plan(
            &target_domain,target_selection,&target_strategy,&head);
        let target_policy=target_strategy["id"].as_str()
            .and_then(|id|context.state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(&context.state,"selfimprove.search"));
        if child_input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
            let ranked=choose_focus_files(&context.state,&child_input,&recorded_history,
                target_policy.as_ref());
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
        // The learned policy must change the worker's next execution target,
        // not only its instruction text. Keep the originally ranked options
        // as evidence while narrowing an implementation stage to one source.
        bind_action_to_source(&context.state,&mut child_input,&recorded_history);
        if input["rsiSystemEvolution"]==true {
            // Sub1's compiled-planner principle, extended with Sub3's full
            // source self-model. This EXECUTES the rebuilt descendant instead
            // of asking the old coordinator to impersonate its next plan.
            record["phase"]=json!("planning_next_generator");
            save(context,record)?;
            let plan_dir=context.state.config.data_dir.join("rsi-controller-plans");
            std::fs::create_dir_all(&plan_dir).map_err(|e|e.to_string())?;
            let plan_path=plan_dir.join(format!("{}-{attempt}.json",context.job_id));
            let snapshot=json!({
                "campaignId":context.job_id,
                "generatorCommit":head,
                "attempt":attempt,
                "historyCount":recorded_history.len(),
                "input":child_input
            });
            std::fs::write(&plan_path,
                serde_json::to_vec(&snapshot).map_err(|e|e.to_string())?)
                .map_err(|e|e.to_string())?;
            let mut planner=Command::new(&binary);
            planner.arg("--rsi-plan-generation").arg(&plan_path)
                .current_dir(&lab)
                .env("DOMINION_DATA",std::fs::canonicalize(
                    &context.state.config.data_dir).map_err(|e|e.to_string())?)
                .env("DOMINION_WORKSPACE",&lab)
                .env_remove("DOMINION_DESKTOP_PARENT");
            let result=command(context,planner,Duration::from_secs(45)).await;
            let _=std::fs::remove_file(&plan_path);
            let result=result?;
            if !result.status.success() {
                return Err(format!("실제 후손의 전체 시스템 계획 실행 실패: {}",
                    String::from_utf8_lossy(&result.stderr).chars().take(1500).collect::<String>()));
            }
            if result.stdout.len()>2*1024*1024 {
                return Err("후손 계획이 허용된 크기를 초과했습니다.".into());
            }
            let plan:Value=serde_json::from_slice(&result.stdout)
                .map_err(|e|format!("후손 계획 JSON 오류: {e}"))?;
            // A supervisor must never impersonate the new descendant's
            // strategy choice. Verify only source identities and independently
            // paired foreign-lineage receipts, not an old meta equation.
            crate::engine::rsi_system::validate_plan_evidence(
                &context.state,&lab,&head,
                child_input["rsiIndexId"].as_str().unwrap_or(""),
                &plan,&recorded_history)?;
            crate::engine::rsi_meta_controller::validate_peer_lineages(
                &context.state,&plan["peerLineages"],
                crate::engine::rsi_meta_controller::comparison_profile(&child_input).as_deref(),
                &context.job_id)?;
            let domain=plan["domain"].as_str().filter(|d|DOMAINS.contains(d))
                .ok_or("후손이 유효하지 않은 개선 분야를 계획했습니다.")?;
            // The supervisor validates endpoint ownership, pinned frozen
            // suite and revision digest, NEVER recomputes an old selector's
            // ranking law. New descendants may evolve that learning law.
            let original_objective=crate::engine::memory::stable_id(&[
                child_input["rsiObjective"].as_str().unwrap_or("")
            ]);
            if !crate::engine::rsi_model_ecology::valid_facts(
                &context.state,&child_input,&plan["modelRoute"],&head,
                domain,&original_objective){
                return Err("후손의 모델 선택이 승인된 제공자·평가 계보와 일치하지 않습니다.".into());
            }
            if !crate::engine::rsi_archive::validate_selection(
                &context.state,&plan["archiveSelection"],&head,
                &child_input["rsiFrozenCases"],
                child_input["rsiSourceBranch"].as_str().unwrap_or("")){
                return Err("후손이 고른 영구 아카이브 부모의 독립 검증 또는 브랜치가 일치하지 않습니다.".into());
            }
            let focus=plan["focusFiles"].as_array()
                .ok_or("후손의 소스 계획이 없습니다.")?;
            if plan["source"].as_str().is_none_or(str::is_empty)
                || plan["generatorCommit"]!=head
                || plan["historyCount"].as_u64()!=Some(recorded_history.len() as u64)
                || plan["indexId"]!=child_input["rsiIndexId"]
                || plan["objectiveId"]!=crate::engine::memory::stable_id(
                    &[&selection_objective(&child_input)])
                || focus.len()>3 || focus.iter().any(|row|row.as_str()
                    .is_none_or(|p|!crate::engine::rsi::safe_path(p)
                        || !lab.join(p).is_file()
                        || (crate::engine::code_index::indexed_source_hash(
                            &context.state,child_input["rsiIndexId"].as_str().unwrap_or(""),p)
                            .is_none()
                            && !plan["systemBlueprint"]["sourceInventory"]
                                .as_array().is_some_and(|sources|sources.iter().any(|row|
                                    row["path"]==p && row["sha256"].as_str()
                                        .is_some_and(|sha|sha.len()==64))))))
            {
                return Err("후손 소스·계보·인덱스·전체 구조 계획이 일치하지 않습니다.".into());
            }
            child_input["rsiSelectedDomain"]=json!(domain);
            child_input["rsiTargetDomain"]=json!(domain);
            child_input["rsiTargetPlan"]=plan["targetPlan"].clone();
            if input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
                child_input["rsiFocusFiles"]=json!(focus);
                child_input["rsiAutoFocus"]=json!(true);
            }
            child_input["rsiFocusSelection"]=json!({
                "source":"rebuilt_descendant_system_controller",
                "candidates":plan["ranked"],
                "domain":domain,
                "generatorCommit":head,
                "sourceIndexId":child_input["rsiIndexId"]
            });
            child_input["rsiSystemBlueprint"]=plan["systemBlueprint"].clone();
            child_input["rsiImprovementDirective"]=plan["improvementDirective"].clone();
            child_input["rsiMetaPolicy"]=plan["metaPolicy"].clone();
            child_input["rsiMetaActionPolicy"]=plan["metaActionPolicy"].clone();
            child_input["rsiPeerLineages"]=plan["peerLineages"].clone();
            child_input["rsiMutualCurriculum"]=plan["mutualCurriculum"].clone();
            child_input["rsiFactorialCurriculum"]=plan["factorialCurriculum"].clone();
            child_input["rsiRecursiveDiscovery"]=plan["recursiveDiscovery"].clone();
            child_input["rsiRecursiveScienceActive"]=plan["recursiveScienceActive"].clone();
            child_input["rsiPrincipleProgramme"]=plan["principleProgramme"].clone();
            child_input["rsiPrincipleActive"]=plan["principleActive"].clone();
            child_input["rsiOpenProgramPlan"]=plan["openProgramPlan"].clone();
            child_input["rsiArchiveSelection"]=plan["archiveSelection"].clone();
            child_input["rsiInheritedCandidateId"]=plan["archiveSelection"]["candidateId"].clone();
            child_input["rsiEditPolicy"]=plan["editPolicy"].clone();
            child_input["rsiModelRoute"]=plan["modelRoute"].clone();
            if plan["modelRoute"]["selected"].is_object(){
                child_input["providerId"]=plan["modelRoute"]["selected"]["providerId"].clone();
                child_input["model"]=plan["modelRoute"]["selected"]["model"].clone();
            }
            child_input["rsiDescendantPlan"]=plan.clone();
            child_input["rsiSuccessorStage"]=plan["successorStage"].clone();
            record["nextGenerationPlan"]=plan.clone();
            context.state.store.audit("rsi.rebuilt_descendant_planned",json!({
                "campaignId":context.job_id,"generatorCommit":head,
                "attempt":attempt,"domain":domain,
                "source":"compiled_descendant_system_controller_v3",
                "modelImprovementDemonstrated":false
            }));
            save(context,record)?;
            bind_action_to_source(&context.state,&mut child_input,&recorded_history);
        }
        // Real API/local model selection is a source-backed compiled policy.
        // The owner's baseline remains the default unless explicit opt-in
        // opens a configured model pool. Each actual child receives the
        // selected provider/model, not just a prompt-side recommendation.
        child_input["rsiOriginalProviderId"]=input["providerId"].clone();
        child_input["rsiOriginalModel"]=input["model"].clone();
        // The actual selection was emitted and source-bound by the rebuilt
        // child's compiled planner. An old resident supervisor may not
        // substitute its own model-ranking equation.
        record["nextModelRoute"]=child_input["rsiModelRoute"].clone();
        let inherited = generation_context(&context.state, input, record);
        let digest = research_digest(&context.state, input, record);
        let focus = focus_file_context(&context.state, record, &child_input);
        child_input["rsiFocusGrounded"]=json!(focus_excerpt_has_source(&child_input,&focus));
        // Freeze independent inputs so the *new* binary can re-select a target
        // and rebuild an honest source context instead of reusing old excerpts.
        child_input["rsiPromptBase"]=child_input["prompt"].clone();
        child_input["rsiPromptEvidence"]=json!(inherited);
        child_input["rsiPromptResearch"]=json!(digest);
        child_input["rsiRemainingCampaignTokens"]=record["limits"]["tokenBudget"]
            .as_u64().map(|budget|budget.saturating_sub(
                record["tokens"].as_u64().unwrap_or(0)))
            .map_or(Value::Null,|remaining|json!(remaining));
        child_input["prompt"] = json!(format!(
            "{}\n\n{}{}{}",
            child_input["prompt"].as_str().unwrap_or(""),
            inherited,
            digest,
            focus
        ));
        // The earlier preflight only sees the base prompt. Inherited research,
        // source excerpts and method feedback can make the *real* generation
        // substantially larger; never spawn a paid worker that cannot fit.
        // Controlled native fixtures have no model/provider and spend no
        // prompt tokens. Do not block those real-binary lineage tests merely
        // because their small synthetic budget cannot fit an LLM prompt.
        let model_bound=child_input["model"].as_str().is_some_and(|s|!s.is_empty())
            && child_input["providerId"].as_str().is_some_and(|s|!s.is_empty());
        if model_bound && !generation_fits_remaining_budget(record,&child_input) {
            return Ok("token_budget".into());
        }
        // Record prompt composition so real-model savings can be measured
        // separately from completion tokens and tool-call counts.
        child_input["rsiFocusContextChars"] = json!(focus.chars().count());
        child_input["rsiPromptContextChars"] = json!(child_input["prompt"]
            .as_str().map(|prompt| prompt.chars().count()).unwrap_or(0));
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
        let tools_used = session
            .map(|session_id| session_tool_usage(&context.state, session_id))
            .unwrap_or_default();
        let research_fingerprint = session
            .map(|session_id| session_research_fingerprint(&context.state, session_id))
            .unwrap_or_default();
        let capabilities_used = tools_used
            .iter()
            .filter(|row| {
                matches!(
                    row["tool"].as_str(),
                    Some("code_search" | "workspace_read_range" | "workspace_replace" | "workspace_replace_function")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let usage = session
            .map(|s| context.state.store.session_usage(s))
            .unwrap_or(Value::Null);
        let tokens = usage["totalTokens"].as_u64().unwrap_or(0);
        record["tokensEstimated"] = json!(
            record["tokensEstimated"] == true || usage["estimatedCalls"].as_u64().unwrap_or(0) > 0
        );
        record["tokens"] = json!(record["tokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(tokens));
        let mut generation = json!({"id":format!("{}-{attempt}",context.job_id),"campaignId":context.job_id,
            "attempt":attempt,"generatorCommit":head,"jobId":child_id,"tokens":tokens,
            "focusFiles":current["input"]["rsiFocusFiles"],
            "focusSelection":current["input"]["rsiFocusSelection"],
            "targetReplan":current["input"]["rsiTargetReplan"],
            "knowledgePartition":research_scope(&current["input"]).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&[
                current["input"]["rsiObjective"].as_str().unwrap_or("")
            ]),
            "tokenUsage":{"promptTokens":usage["promptTokens"],"completionTokens":usage["completionTokens"],
                "calls":usage["calls"],"estimatedCalls":usage["estimatedCalls"]},
            "sourceContextChars":current["input"]["rsiFocusContextChars"],
            "totalPromptChars":current["input"]["rsiPromptContextChars"],
            "generatorBinarySha256":record["generatorBinarySha256"],"generatorModel":current["input"]["model"],"generatorProviderId":current["input"]["providerId"],
            "domainSelection":current["rsiDomainSelection"],"searchStrategy":current["rsiSearchStrategy"],
            "decisionInput":current["rsiDecisionInput"],"selectedDomainInput":current["input"]["rsiSelectedDomain"],
            "researchDelivered":current["rsiResearchDelivered"],"workerMarker":current["result"]["rsi"]["fixturePhase"],
            "boundaryResult":current["rsiBoundaryResult"],
            "elapsedMs":worker_started.elapsed().as_millis() as u64,"domain":current["rsiDomain"],
            "toolsUsed":tools_used,"builtInToolsUsed":capabilities_used,
            "editActions":current["result"]["rsi"]["editActions"],
            "recoveredFailedPatch":current["rsiRecoveredPatch"],
            "sourceDiffBytes":current["result"]["rsi"]["diffBytes"],
            "sourceCaptureError":current["result"]["rsi"]["captureError"],
            "unverifiedCandidateId":current["result"]["rsi"]["candidateId"],
            "staticChecksPassed":current["result"]["rsi"]["checksPassed"],
            "inheritedImplementations":current["input"]["rsiInheritedImplementations"],
            "actionDirective":current["rsiActionDirective"],
            "hypothesisMessageId":current["result"]["rsi"]["modelMessageId"],
            "researchFingerprint":research_fingerprint,
            "accepted":false,"createdAt":chrono::Utc::now().to_rfc3339()});
        // Keep the large generation JSON below the serde_json macro's nesting
        // limit; these are real worker receipts, added after construction.
        generation["novelSourceDiffBytes"]=
            current["result"]["rsi"]["novelDiffBytes"].clone();
        generation["inheritedPatchUnchanged"]=
            current["result"]["rsi"]["inheritedPatchUnchanged"].clone();
        // Read the actual session written by the native execution runtime,
        // not the coordinator's intention or an assistant message.
        let action_session=session.and_then(|sid|context.state.store.get("session",sid))
            .unwrap_or(Value::Null);
        // Preserve the ACTUAL model used by this compiled descendant for
        // independent model-route learning. The campaign input is only a
        // baseline and must not impersonate a dynamically selected provider.
        generation["modelRoute"]=current["input"]["rsiModelRoute"].clone();
        generation["modelRouteExecuted"]=current["rsiModelRouteExecuted"].clone();
        generation["originalModel"]=current["input"]["rsiOriginalModel"].clone();
        generation["originalProviderId"]=current["input"]["rsiOriginalProviderId"].clone();
        generation["systemBlueprint"]=current["input"]["rsiSystemBlueprint"].clone();
        generation["systemEvolution"]=current["input"]["rsiSystemEvolution"].clone();
        generation["metaPolicy"]=current["input"]["rsiMetaPolicy"].clone();
        generation["metaPolicyExecuted"]=current["rsiMetaPolicyExecuted"].clone();
        generation["metaMethodInheritance"]=
            current["rsiMetaMethodInheritance"].clone();
        generation["downstreamMethodFeedback"]=
            current["input"]["rsiDescendantPlan"]["downstreamMethodFeedback"].clone();
        generation["returnMethodFeedbackExecuted"]=
            current["rsiMetaReturnExecuted"].clone();
        generation["metaControlPolicy"]=current["input"]["rsiMetaActionPolicy"].clone();
        generation["peerLineages"]=current["input"]["rsiPeerLineages"].clone();
        generation["mutualCurriculum"]=current["input"]["rsiMutualCurriculum"].clone();
        generation["factorialCurriculum"]=current["input"]["rsiFactorialCurriculum"].clone();
        generation["factorialPlanExecuted"]=current["rsiFactorialPlanExecuted"].clone();
        generation["recursiveDiscovery"]=current["input"]["rsiRecursiveDiscovery"].clone();
        generation["recursiveScienceActive"]=current["input"]["rsiRecursiveScienceActive"].clone();
        generation["principleProgramme"]=current["input"]["rsiPrincipleProgramme"].clone();
        generation["principleActive"]=current["input"]["rsiPrincipleActive"].clone();
        generation["openProgramPlan"]=current["input"]["rsiOpenProgramPlan"].clone();
        generation["openProgramExecuted"]=current["rsiOpenProgramExecuted"].clone();
        generation["archiveSelection"]=current["input"]["rsiArchiveSelection"].clone();
        generation["editPolicy"]=current["input"]["rsiEditPolicy"].clone();
        generation["sourceBranch"]=current["input"]["rsiSourceBranch"].clone();
        generation["inheritedCandidateId"]=current["input"]["rsiInheritedCandidateId"].clone();
        generation["inheritedCandidateActuallyEdited"]=
            current["result"]["rsi"]["inheritedCandidateActuallyEdited"].clone();
        generation["recursiveSciencePlanExecuted"]=current["rsiRecursiveSciencePlanExecuted"].clone();
        generation["mutualProgrammeExecuted"]=current["rsiMutualProgrammeExecuted"].clone();
        generation["successorStage"]=current["input"]["rsiSuccessorStage"].clone();
        generation["comparisonProfileSha256"]=json!(
            crate::engine::rsi_meta_controller::comparison_profile(&current["input"]));
        generation["evaluationProfileSha256"]=generation["comparisonProfileSha256"].clone();
        generation["descendantPlan"]=current["input"]["rsiDescendantPlan"].clone();
        generation["systemBlueprintVerified"]=current["rsiSystemBlueprintVerified"].clone();
        generation["systemDesign"]=action_session["rsiSystemDesign"].clone();
        // The rule-induction trainer sees ONLY actual tool operations,
        // including failed calls, in their original run's causal order.
        // This prevents model prose and tool-menu suggestions from becoming
        // success examples. No source content or hidden holdout is copied.
        let mut trace=session.map(|sid|context.state.store.all("execution_operation")
            .into_iter().filter(|op|op["sessionId"]==sid)
            .collect::<Vec<_>>()).unwrap_or_default();
        trace.sort_by(|a,b|a["runId"].as_str().cmp(&b["runId"].as_str())
            .then_with(||a["opSeq"].as_u64().cmp(&b["opSeq"].as_u64())));
        trace.truncate(128);
        let timeline=trace.iter().map(|op|json!({
            "id":op["id"],"runId":op["runId"],"opSeq":op["opSeq"],
            "tool":op["tool"],"status":op["status"],
            "path":op["arguments"]["path"],
            "result":op["result"],"toolReceipt":op["toolReceipt"]
        })).collect::<Vec<_>>();
        let edit_attempts=trace.iter().filter(|op|
            matches!(op["tool"].as_str(),
                Some("workspace_replace"|"workspace_replace_function"
                    |"workspace_replace_range"|"workspace_write"))).count();
        let verified_writes=trace.iter().filter(|op|
            op["status"]=="completed" && op["result"]["written"]==true
            && op["toolReceipt"]["verified"]==true).count();
        let failed_ops=trace.iter().filter(|op|op["status"]!="completed"
            || (matches!(op["tool"].as_str(),
                Some("workspace_replace"|"workspace_replace_function"
                    |"workspace_replace_range"|"workspace_write"))
                && op["result"]["written"]!=true))
            .take(8).map(|op|format!("{}: {}",
                op["tool"].as_str().unwrap_or("unknown"),
                op["error"].as_str()
                    .or_else(||op["result"]["error"].as_str())
                    .unwrap_or("no_verified_write")))
            .collect::<Vec<_>>();
        generation["toolTimeline"]=json!(timeline);
        generation["editEvidence"]=json!({
            "attempts":edit_attempts,"verifiedWrites":verified_writes,
            "recentFailures":failed_ops,
            "source":"actual_execution_operation_store"
        });
        generation["programStages"]=json!(session.map(|sid|
            context.state.store.all("rsi_program_phase").into_iter()
                .filter(|event|event["sessionId"]==sid
                    && event["generationJobId"]==child_id)
                .take(48).collect::<Vec<_>>()).unwrap_or_default());
        generation["ruleExecutions"]=json!(session.map(|sid|
            context.state.store.all("rsi_rule_execution").into_iter()
                .filter(|event|event["sessionId"]==sid
                    && event["generationJobId"]==child_id)
                .take(48).collect::<Vec<_>>()).unwrap_or_default());
        generation["actionExecution"]=json!({
            "focusApplied":action_session["rsiPolicyFocus"]==true,
            "editCheckpointReached":action_session["rsiPolicyCheckpointReached"]==true,
            "checkpointExecutionId":action_session["rsiPolicyCheckpointExecutionId"],
            "sourceGrounded":current["input"]["rsiFocusGrounded"]==true,
            "sourceTarget":current["input"]["rsiFocusSelection"]["executionTarget"],
            "policyRevision":action_session["rsiActionPolicyRevision"],
        });
        let candidate_id = verified_candidate_id(&current["result"]);
        let novel_source_bytes=current["result"]["rsi"]["novelDiffBytes"]
            .as_u64().or_else(||current["result"]["rsi"]["diffBytes"].as_u64())
            .unwrap_or(0);
        generation["actionObservation"]=observed_edit_outcome(
            &tools_used,&current["result"]["rsi"]["editActions"],
            candidate_id.is_some(),novel_source_bytes);
        let repair_modes=current["result"]["previousAttempts"].as_array()
            .into_iter().flatten()
            .filter_map(|item|item["repairMethod"].as_str())
            .take(2).collect::<Vec<_>>();
        let repair_attempts=current["result"]["repairAttempts"].as_u64()
            .unwrap_or(0);
        let changed_source=novel_source_bytes>0;
        generation["selfRepair"]=json!({
            "attempts":repair_attempts,
            "modes":repair_modes,
            "sourceEdited":changed_source,
            "candidateCaptured":candidate_id.is_some(),
            "outcome":if repair_attempts==0 {"not_attempted"}
                else if candidate_id.is_some() {"candidate_after_recovery"}
                else if changed_source {"source_changed_no_candidate"}
                else {"no_patch_after_recovery"},
            "qualityGain":Value::Null
        });
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
                    // The independent paired oracle remains the only source
                    // of quality credit. A whole-system generation additionally
                    // needs a MODEL-authored design tied to its changed files.
                    generation["candidateOrigin"]=candidate["origin"].clone();
                    generation["factorialChangedPaths"]=candidate["paths"].clone();
                    // compare_at returns a flat runner receipt, while the
                    // authenticated store candidate carries its verified
                    // source tag. Preserve that source in the generation
                    // row so policy tournaments and rule induction can
                    // consume ACTUAL completed runs, not a missing field.
                    generation["evaluation"]["source"]=
                        candidate["evaluation"]["source"].clone();
                    // Replayed neutral/rejected pairs are genuine negative
                    // training examples. verified_pair_receipt only accepts
                    // IMPROVED candidates, causing prior tournaments to
                    // silently exclude their controls. A valid paired
                    // measurement is not necessarily a quality improvement.
                    generation["pairedRevalidated"]=json!(
                        rsi::replayed_pair_metrics(&context.state,&candidate).is_some());
                    let model_design=&generation["systemDesign"];
                    let authored_system_change=input["rsiSystemEvolution"]!=true
                        || (candidate["origin"]=="model_driven_detached_worktree"
                            && model_design["source"]=="model_authored_system_design"
                            && model_design["generatorCommit"]==head
                            && model_design["generationJobId"]==child_id
                            // A bundle must change EVERY declared file, with
                            // disjoint ownership. One partial diff cannot
                            // adopt a six-component architecture proposal.
                            && crate::engine::rsi_system::design_implemented(
                                model_design,
                                candidate["paths"].as_array().map(Vec::as_slice)
                                    .unwrap_or(&[]))
                            && generation["systemBlueprintVerified"]==true);
                    let eligible = candidate["status"] == "verified"
                        && candidate["evaluation"]["metrics"]["eligible"] == true
                        && generation["pairedRevalidated"]==true
                        && authored_system_change;
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
                        // Store the independently adopted source fingerprint with
                        // the candidate receipt as well as the generation summary.
                        // Replayed method learning must match BOTH records.
                        generation["acceptedSourceHashes"]=Value::Object(accepted_hashes.clone());
                        generation["accepted"] = json!(true);
                        generation["acceptedCommit"] = json!(commit);
                        record["headCommit"] = json!(commit);
                        record["acceptedGenerations"] =
                            json!(record["acceptedGenerations"].as_u64().unwrap_or(0) + 1);
                        let mut saved = candidate;
                        saved["experimentAdoption"] = json!({"campaignId":context.job_id,
                            "commit":commit,"generatorCommit":head,"productionDeployed":false,
                            "acceptedSourceHashes":accepted_hashes});
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
            generation["error"] = json!(worker_error
                .or_else(|| child["modelError"].as_str())
                .or_else(|| child["captureError"].as_str())
                .or_else(|| current["error"].as_str())
                .unwrap_or("유효한 변경이나 검사 통과 없음"));
        }
        // The independent evaluator, not the code generator, supplies the
        // outcome of the native operator. Persist both failed and successful
        // applications so descendants can learn from execution evidence.
        if let Some(mut mutation)=context.state.store.get("rsi_mutation",child_id) {
            mutation["candidateId"]=generation["candidateId"].clone();
            mutation["evaluation"]=generation["evaluation"].clone();
            mutation["accepted"]=json!(generation["accepted"]==true);
            mutation["acceptedCommit"]=generation["acceptedCommit"].clone();
            mutation["completedAt"]=json!(chrono::Utc::now().to_rfc3339());
            mutation["qualityGain"]=if generation["accepted"]==true {
                generation["evaluation"]["metrics"]["metricGains"].clone()
            }else{Value::Null};
            context.state.store.put("rsi_mutation",&mutation)
                .ok_or("코드 변형 결과의 지속 저장 실패")?;

            generation["nativeMutation"]=mutation.clone();
            // Sub3's proof distinguished the actual native generator from
            // the requested LLM. No model calls or tokens may be credited
            // to a source mutation that executed without an LLM turn.
            generation["requestedModel"]=input["model"].clone();
            generation["generatorModel"]=json!(format!("dominion-native:{}",
                mutation["operator"].as_str().unwrap_or("unknown")));
            generation["generatorProviderId"]=json!("dominion-native");
        }
        // Credit model-authored descendants only to the actual, complete
        // audited model identity for THIS session. No globally truncated
        // "recent audit" scan and no trust in the requested provider name.
        let model_audit=session.map(|id|
            context.state.store.session_model_attribution(id))
            .unwrap_or(Value::Null);
        let model_consistent=model_audit["verifiedHomogeneous"]==true
            && model_audit["providerId"]==generation["generatorProviderId"]
            && model_audit["model"]==generation["generatorModel"]
            && model_audit["calls"]==generation["tokenUsage"]["calls"]
            && generation["nativeMutation"].is_null();
        generation["modelAttribution"]=model_audit;
        generation["modelAttributionConsistent"]=json!(model_consistent);
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
        if candidate_id.is_none() && new_findings>0 && output.is_ok() && stopped.is_none()
            && current["result"]["rsi"]["modelError"].is_null() {
            generation["error"]=Value::Null;
        }
        if let Some(session)=session {
            generation["methodExecutions"]=json!(record_method_feedback(&context.state,&generation,session));
            // Capture newly authored method PROGRAMS separately from execution
            // receipts. A later child can only claim inherited method learning
            // when the exact earlier method revision actually executed with
            // verified postconditions on source that was independently adopted.
            generation["methodProposals"]=context.state.store.get("session",session)
                .map(|row|row["rsiMethodProposals"].clone()).unwrap_or(Value::Null);
        }
        generation["knowledgeId"]=json!(remember_research(&context.state,&current["input"],&generation,session.unwrap_or("")));
        if let Some(session)=session {
            generation["knowledgeReuse"]=context.state.store.get("session",session).map(|s|s["rsiKnowledgeReuse"].clone()).unwrap_or(Value::Null);
        }
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
                    &selection_objective(input),
                    &context.job_id,
                    attempt,
                );
                if report["status"] == "pending_trial" {
                    if let Some(id) = report["candidateId"].as_str() {
                        queued.push(id.to_string());
                    }
                }
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
            // First record the policy's actual execution result. Otherwise an
            // unresolved trial is still marked pending when the coordinator
            // tries to synthesize its replacement, ending the campaign early.
            if queued.is_empty() && record["nextTrialAlgorithmId"].is_null() {
                let mut observed=prior_history.clone();
                observed.push(generation.clone());
                let synthesized=propose_autonomous_search_trial(
                    &context.state,&observed,
                    &selection_objective(input),
                    &context.job_id,attempt,
                    current["input"]["rsiAutoFocus"]==true);
                if synthesized["status"]=="pending_trial" {
                    if let Some(id)=synthesized["candidateId"].as_str() {
                        queued.push(id.to_string());
                    }
                    generation["autonomousSearchProposal"]=synthesized;
                }
            }
            if !queued.is_empty() {
                generation["searchTrialsQueued"] = json!(queued.clone());
            }
            if record["nextTrialAlgorithmId"].is_null() {
                if let Some(id) = queued.into_iter().next() {
                    record["nextTrialAlgorithmId"] = json!(id);
                }
            }
        }
        let generation_accepted = generation["accepted"] == true;
        // Research-only or failed generations interrupt a consecutive chain.
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
        // Streaks are descriptive. Full method -> B -> method closure is
        // verified only AFTER the generation is durably committed below.
        // Distinguish an actual independently adopted pair of source edits
        // from an A->B improvement HYPOTHESIS and from a verified B->A return.
        // This observation is persisted before the next compiled descendant
        // reads history and updates the cross-capability policy.
        let mutual_pair=&generation["mutualCurriculum"]["pair"];
        let source_pair_adopted=mutual_pair.is_object()
            && generation["accepted"]==true
            && generation["pairedRevalidated"]==true
            && generation["systemDesign"]["interactionEdges"].as_array()
                .is_some_and(|edges|edges.iter().any(|edge|
                    edge["fromRole"]==mutual_pair["fromRole"]
                    && edge["toRole"]==mutual_pair["toRole"]
                    && edge["fromPath"]==mutual_pair["fromPath"]
                    && edge["toPath"]==mutual_pair["toPath"]))
            && generation["acceptedSourceHashes"][mutual_pair["fromPath"]
                .as_str().unwrap_or("")].is_string()
            && generation["acceptedSourceHashes"][mutual_pair["toPath"]
                .as_str().unwrap_or("")].is_string();
        // The compiled plan alone is not an experiment attempt. A real,
        // witnessed source change or a captured compared candidate is needed
        // before its intervention may advance the next descendant's phase.
        let factorial_source_written=generation["editActions"].as_array()
            .is_some_and(|rows|rows.iter().any(|row|
                row["written"]==true
                && row["path"].as_str().is_some_and(
                    crate::engine::rsi::safe_path)));
        let factorial_candidate_changed=generation["factorialChangedPaths"]
            .as_array().is_some_and(|files|!files.is_empty());
        generation["factorialExperimentExecuted"]=json!(
            generation["factorialPlanExecuted"]==true
            && (factorial_source_written||factorial_candidate_changed));
        // A planned scientific stage never advances an inherited lineage.
        // Witness its actual edited target in a VERIFIED, independently
        // adopted candidate (not a model claim or an unadopted source diff).
        let scientific_target=generation["recursiveDiscovery"]["target"]["path"].as_str();
        let scientific_edit_accepted=generation["recursiveSciencePlanExecuted"]==true
            && generation["metaPolicyExecuted"]==true
            && generation["accepted"]==true
            && generation["pairedRevalidated"]==true
            && scientific_target.is_some_and(|path|
                crate::engine::rsi::safe_path(path)
                && generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|sha|sha.len()==64
                        && sha.bytes().all(|b|b.is_ascii_hexdigit()))
                && generation["factorialChangedPaths"].as_array()
                    .is_some_and(|files|files.iter().any(|file|
                        file.as_str()==Some(path))));
        generation["recursiveScienceExecuted"]=json!(scientific_edit_accepted);
        let mut science_history=record["history"].as_array().cloned()
            .unwrap_or_default();
        science_history.push(generation.clone());
        generation["recursiveScientificLineage"]=
            crate::engine::rsi_recursive_discovery::completed_cycles(&science_history);
        generation["recursiveScienceObservation"]=json!({
            "source":"paired_adopted_recursive_science_source_receipt_v1",
            "planRevision":generation["recursiveDiscovery"]["revision"],
            "phase":generation["recursiveDiscovery"]["phase"],
            "sourceTarget":generation["recursiveDiscovery"]["target"]["path"],
            "actualSourceChangeIndependentlyAdopted":scientific_edit_accepted,
            "causalCapabilityGainProven":false
        });
        generation["mutualObservation"]=json!({
            "source":"observed_cross_intelligence_programme_v1",
            "executedCompiledCurriculum":generation["mutualProgrammeExecuted"],
            "programmeRevision":generation["mutualCurriculum"]["revision"],
            "sourcePairIndependentlyAdopted":source_pair_adopted,
            "returnStageAttempted":
                generation["successorStage"]["phase"]=="return_improvement_to_primary",
            "returnStageIndependentlyAdopted":
                generation["successorStage"]["phase"]=="return_improvement_to_primary"
                    && generation["accepted"]==true
                    && generation["pairedRevalidated"]==true,
            "causalTransferProven":false
        });
        // The descendant's forecast was frozen before editing. Real assessed
        // outcomes now update task-level error available to its successor.
        generation["evidenceVerified"]=json!(
            generation["accepted"]==true
            && generation["pairedRevalidated"]==true
            && generation["evaluation"]["metrics"]["eligible"]==true
            && generation["candidateOrigin"]=="model_driven_detached_worktree"
            && generation["acceptedSourceHashes"].as_object()
                .is_some_and(|m|!m.is_empty()));
        // Inspect the exact code bytes AFTER actual program/rule execution;
        // editing the method generator is merely a proposal until a separate
        // adopted source improvement is independently recorded.
        let method_reward=sub2_edit_policy_reward(&generation);
        generation["editMethodExecutionProof"]=json!({
            "source":"ordered_tool_stage_to_adopted_source_receipt_v1",
            "revision":generation["editPolicy"]["revision"],
            "parentRevision":generation["editPolicy"]["parentRevision"],
            "rewriteProgram":generation["editPolicy"]["rewriteProgram"],
            "executedStages":generation["programStages"].as_array()
                .map(Vec::len).unwrap_or(0),
            "firedRules":generation["ruleExecutions"].as_array()
                .map(Vec::len).unwrap_or(0),
            "matchedAdoptedSourceWrite":method_reward>=1.0,
            "qualityEvidence":if method_reward>=1.0 {
                "independently_adopted_source_with_prior_executed_policy"
            }else{"not_proven"},
            "causalPolicySuperiorityProven":false,
            "independentlyMeasuredAdoption":generation["accepted"]==true
                && generation["pairedRevalidated"]==true
        });
        generation["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
            &generation["metaPolicy"],&generation);
        // The inherited, compiled method is not scored by its own presence.
        // Publish whether its REAL successor was independently assessed,
        // adopted and authored a further change to executable meta source.
        generation["metaMethodOutcome"]=
            crate::engine::rsi_system::observed_method_outcome(&generation);
        // The immediate quality receipt and the future descendant's
        // productivity are SEPARATE learning targets. Future descendants
        // reconstruct the latter only after real independent evaluations.
        generation["principleObservation"]=
            crate::engine::rsi_principle_lab::observation(&generation);
        generation["modelRouteObservation"]=
            crate::engine::rsi_model_ecology::observation(&generation);
        // Persist the real outcome of the newly executed recovery policy.
        // A ranking score or a source write is not a validated improvement;
        // this is the observed action utility that the next built generator
        // may learn from. Keep the exact policy revision and actual outcome.
        generation["actionPolicyOutcome"]=json!({
            "revision":generation["actionDirective"]["policy"]["revision"],
            "mode":generation["actionDirective"]["mode"],
            "trainingEvidence":generation["actionDirective"]["policy"]["trainingGenerationIds"],
            "policyStatus":generation["actionDirective"]["policy"]["status"],
            "focusApplied":generation["actionExecution"]["focusApplied"],
            "editCheckpointReached":generation["actionExecution"]["editCheckpointReached"],
            "observedUtility":observed_action_value(&generation),
            "actualSourceDiffBytes":generation["actionObservation"]["sourceDiffBytes"],
            "actualCandidateId":generation["candidateId"],
            "accepted":generation["accepted"],
            "metric":"observed_action_utility_not_independent_quality_gain",
            "qualityGain":Value::Null
        });
        let generation_knowledge = generation["knowledgeId"].clone();
        let generation_tokens = generation["tokens"].clone();
        let generation_elapsed = generation["elapsedMs"].clone();
        let generation_verdict = generation["evaluation"]["verdict"].clone();
        // ARCHIVE BEFORE THE CAMPAIGN DECIDES TO STOP. A campaign can reach
        // its token/time/max-attempt limit immediately after this generation:
        // there may never be a next descendant planning/select() call.
        // Persist only an original, independently replayed neutral experiment;
        // the source branch, frozen plan and candidate-bound receipt are
        // checked again by rsi_archive::record_completed.
        let archive_receipt=crate::engine::rsi_archive::record_completed(
            &context.state,&generation,
            &current["input"]["rsiFrozenCases"],
            current["input"]["rsiSourceBranch"].as_str().unwrap_or(""))?;
        generation["archivePersistence"]=archive_receipt.clone();
        record["archiveRefreshObservation"]=archive_receipt;
        // Workers read this durable table, not the coordinator's in-memory
        // history. Publish only after research, method feedback, archive and
        // trial outcomes are complete so the next worker actually continues.
        context.state.store.put("rsi_generation", &generation)
            .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
        record["history"]
            .as_array_mut()
            .ok_or("세대 목록 손상")?
            .push(generation);
        // One source-replayed verifier owns BOTH the campaign stop decision
        // and every API completion field. The chain must actually return to
        // editing the inherited improvement method with downstream evidence.
        let closure=crate::engine::rsi::campaign_recursive_evidence(
            &context.state,record);
        record["recursiveClosure"]=closure.clone();
        record["capabilityDemonstrated"]=json!(closure["proven"]==true);
        record["systemCapabilityDemonstrated"]=json!(
            closure["proven"]==true
                && input["rsiSystemEvolution"]==true
                && crate::engine::rsi_system::system_capability_proven(
                    &context.state,&record["history"]));
        // Completed real descendants contribute evidence to NEXT-METHOD
        // productivity, never to an already frozen current generation.
        // The durable rsi_generation rows outlive campaigns and are replayed
        // again in the next rebuilt binary before any method preference.
        if let Some(profile)=crate::engine::rsi_meta_controller::comparison_profile(
            &current["input"]) {
            let all=context.state.store.all("rsi_generation");
            let observed=all.iter().filter(|g|
                g["sourceBranch"]==current["input"]["rsiSourceBranch"]
                && g["objectiveId"]==crate::engine::memory::stable_id(&[
                    current["input"]["rsiObjective"].as_str().unwrap_or("")
                ])
                && g["knowledgePartition"]==
                    research_scope(&current["input"]).partition_key
                && g["generatorProviderId"]==current["input"]["providerId"]
                && g["generatorModel"]==current["input"]["model"])
                .cloned().collect::<Vec<_>>();
            let comparison=crate::engine::rsi_successor_productivity::compare(
                &context.state,&observed,&profile,
                current["rsiDomain"].as_str().unwrap_or(""));
            record["methodSuccessorProductivity"]=json!({
                "source":comparison["source"],"status":comparison["status"],
                "selectionEligible":comparison["selectionEligible"],
                "winnerRevision":comparison["winnerRevision"],
                "matchedWins":comparison["matchedWins"],
                "comparedContexts":comparison["comparedContexts"],
                "causalSuperiorityProven":false
            });
        }
        // New findings, not a write count or query spelling, justify continuing
        // a research-only generation within the configured campaign budgets.
        let no_edit = candidate_id.is_none() && novel_source_bytes==0;
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
        let pending_repair=session.and_then(|sid|context.state.store.get("session",sid))
            .and_then(|saved|saved["rsiMethodProposals"].as_array().cloned())
            .unwrap_or_default().into_iter().find(|method|
                is_synthesized_source_repair(method)
                && crate::engine::procedure::applies(method,
                    input["rsiObjective"].as_str().unwrap_or(""),None,None));
        let preferred_domain=pending_repair.as_ref()
            .and_then(|method|method["domain"].as_str())
            .filter(|domain|DOMAINS.contains(domain));
        // A new executable procedure gets one concrete follow-up attempt.
        // A pending source-search algorithm trial still wins at selection time.
        let next_domain=preferred_domain.map(str::to_string)
            .unwrap_or_else(||select_domain_for_state(
                &context.state,&decision_history,&decision_objective));
        let reuse = method_reuse_decision(&context.state, input, &next_domain);
        record["nextDecision"] = json!({
            "basis": decision_basis,
            "fromGeneration": attempt,
            "nextDomain": next_domain,
            "knowledgeId": generation_knowledge,
            "headCommit": record["headCommit"],
            "trialAlgorithmId": record["nextTrialAlgorithmId"],
            "methodRecoveryCandidateId":reuse["repairMethodId"],
            "methodRecoverySource":"observed_failed_procedure",
            "actionPolicy":decision_history.last().map(|g|g["actionPolicyOutcome"].clone()),
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
        // A fully evidenced three-generation meta-transfer is the campaign
        // objective. Do not spend another local-model generation after the
        // proof is persisted; preserve its final verified receipt.
        if record["capabilityDemonstrated"] == true {
            return Ok("capability_demonstrated".into());
        }
        if let Some(reason)=stopped {return Ok(reason.into());}
        if let Some(error)=generation_model_error(&current) {
            let remaining=record["limits"]["timeBudgetSeconds"].as_u64()
                .map(|limit|limit.saturating_sub(started.elapsed().as_secs()
                    .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0)/1000)))
                .unwrap_or(u64::MAX);
            let retries=record["modelErrorRecoveryCount"].as_u64().unwrap_or(0);
            if retry_learned_model_failure(
                error, decision_history.last().unwrap_or(&Value::Null),
                pending_repair.is_some(),
                retries, remaining,
                record["attempts"].as_u64().unwrap_or(0),
                record["limits"]["maxAttempts"].as_u64().unwrap_or(10)
            ) {
                record["modelErrorRecoveryCount"]=json!(retries+1);
                record["lastModelErrorRecovery"]=json!({
                    "failedGenerationJobId":child_id,
                    "error":error,
                    "learnedMethodId":pending_repair.as_ref().map(|m|m["id"].clone()),
                    "remainingSeconds":remaining,
                    "note":"one_followup_descendant_trial_not_a_source_improvement"
                });
                record["phase"]=json!("learned_source_failure_recovery");
                save(context,record)?;
                continue;
            }
            return Err(error.to_string());
        }
        let trial_pending=record["nextTrialAlgorithmId"].as_str().is_some_and(|id|
            context.state.store.get("algorithm",id)
                .is_some_and(|algorithm|matches!(algorithm["rsiTrial"]["status"].as_str(),
                    Some("pending_trial"|"pending_second_trial"))))
            || pending_trial_candidate(&context.state,
                &selection_objective(input),
                &allowed_trial_campaigns(&context.state,input,record,&context.job_id)).is_some();
        if no_edit && pending_repair.is_none()
            && should_stop_no_progress(streak, research, changed,trial_pending) {
            return Ok("no_progress".into());
        }
    }
}

/// A terminal per-turn model timeout must not discard an executable source
/// repair that DOMINION just induced from real tool failures. Permit at most
/// one descendant attempt if there is still meaningful owner time and another
/// generation slot. Provider outages and errors without observed failed edits
/// remain terminal. Learning a method is not an independently measured gain.
