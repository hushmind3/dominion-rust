//! independent paired adoption loop and recovery.
//! Only campaign_execution owns these implementations. Other RSI stages use explicit
//! names imported through the stable rsi_evolve facade.
use super::{AppState, Command, DOMAINS, Digest, Duration, Instant, JobContext, Path, PathBuf, Sha256, Value, bind_inherited_target, campaign_model_tokens, choose_focus_files, command, edit_method_reward, elapsed, focus_file_context, generation_context, generation_fits_remaining_budget, generation_prompt_variants, generation_stop_reason, git, history_with_verified_payoffs, identical_failed_patch, json, learned_edit_method, matching_prior_campaign, method_reuse_decision, new_research_findings, paired_descendant_target, pending_trial_candidate, post_adoption_objective, proposed_algorithms, queue_adaptive_search_trial, queue_search_trial, record_method_feedback, record_trial_outcome, run_method_control, measured_method_ablation, remember_research, research_artifacts, research_digest, research_scope, rsi_worktree, save, select, select_domain_for_state, select_generation_strategy, selection_objective, session_research_fingerprint, session_tool_usage, should_stop_no_progress, target_focus_objective, verified_candidate_id, worker, retain_generator, matched_planner_intervention, plan_compiled_descendant, capture_generation_evidence, record_generation_feedback, finalize_generation, GenerationObservation};
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
            if input["rsiSystemEvolution"]==true {
                retain_generator(&context.state.config.data_dir,&head,&binary,
                    record["generatorBinarySha256"].as_str()
                        .ok_or("compiled generator SHA unavailable")?)?;
            }
            built_commit = head.clone();
        }
        let attempt = record["attempts"].as_u64().unwrap_or(0) + 1;
        let mut child_input=prepare_generation_input(context,record,input,&lab,&binary,&head,attempt).await?;
        let history=record["history"].as_array().cloned().unwrap_or_default();
        // This is the system's learned *execution method*, independently of
        // which model is used to write the candidate patch.
        if input["rsiSystemEvolution"]!=true {
            child_input["rsiEditPolicy"]=learned_edit_method(&context.state,&child_input,&history);
        }
        context.state.store.audit("rsi.edit_method_selected",json!({
            "campaignId":context.job_id,"attempt":attempt,
            "generatorCommit":head,"policy":child_input["rsiEditPolicy"]
        }));
        // The resident coordinator sends only immutable owner constraints.
        // The rebuilt descendant gathers and composes all system-design,
        // research, source and failure context from its own compiled code.
        let inherited = if input["rsiSystemEvolution"]==true {String::new()}
            else {generation_context(&context.state, &child_input, record)};
        let digest = if input["rsiSystemEvolution"]==true {String::new()}
            else {research_digest(&context.state, input, record)};
        let focus = if input["rsiSystemEvolution"]==true {String::new()}
            else {focus_file_context(&context.state, record, &child_input)};
        let original = if input["rsiSystemEvolution"]==true {
            input["rsiObjective"].as_str().unwrap_or("").to_string()
        } else {child_input["prompt"].as_str().unwrap_or("").to_string()};
        // Preserve the unmixed prompt parts so an evolved descendant worker
        // can replan without inheriting a stale source excerpt or a stale
        // coordinator-selected file as apparent evidence.
        child_input["rsiPromptBase"]=json!(original);
        child_input["rsiPromptEvidence"]=json!(inherited);
        child_input["rsiPromptResearch"]=json!(digest);
        child_input["rsiSourceAdvanced"]=json!(head != record["baseCommit"].as_str().unwrap_or(""));
        let model_bound = child_input["model"].as_str().is_some_and(|m| !m.is_empty())
            && child_input["providerId"].as_str().is_some_and(|p| !p.is_empty());
        // Re-evaluate the ACTUAL complete parent prompt before any paid model
        // call. A large saved research ledger must not silently consume the
        // budget and leave the next generation unable to start.
        let variants = generation_prompt_variants(
            &original, &inherited, &digest, &focus,
            child_input["rsiInheritedCandidateId"].as_str(),
        );
        let Some((mode, composed, defer_focus, defer_research)) = variants.into_iter()
            .find(|(_, text, _, _)| {
                !model_bound || generation_fits_remaining_budget(
                    record, &json!({"prompt":text}))
            }) else { return Ok("token_budget".into()); };
        child_input["prompt"] = json!(composed);
        child_input["rsiContextPlan"] = json!({
            "mode":mode,"sourceExcerptDeferred":defer_focus && !focus.is_empty(),
            "researchReadLedgerDeferred":defer_research && !digest.is_empty(),
            "evidencePreserved":true
        });
        child_input["rsiRemainingCampaignTokens"] = record["limits"]["tokenBudget"]
            .as_u64()
            .map(|budget| budget.saturating_sub(record["tokens"].as_u64().unwrap_or(0)))
            .map_or(Value::Null, |tokens| json!(tokens));
        // Account only for source context that was ACTUALLY delivered.
        child_input["rsiFocusContextChars"] = json!(if defer_focus {0} else {focus.chars().count()});
        child_input["rsiPromptContextChars"] = json!(child_input["prompt"]
            .as_str().map(|prompt| prompt.chars().count()).unwrap_or(0));
        // If an authorized ablation has an executable inherited method,
        // first measure a real OFF worker from the same immutable source and
        // evaluated task input. Only the subsequent ON worker may be adopted.
        run_method_control(context,record,input,&mut child_input,&lab,&binary,attempt,started).await?;
        if !generation_fits_remaining_budget(record,&child_input) {
            return Ok("token_budget".into());
        }
        let child = context
            .state
            .store
            .put(
                "job",
                &json!({"kind":"rsi_generation","status":"running",
            "input":child_input,"queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1,
            "rsiMethodAblationInputSha256":child_input["rsiMethodAblationInputSha256"]}),
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
            remaining.max(1)
        };
        let output = command(context, worker, Duration::from_secs(worker_timeout)).await;
        let current = context
            .state
            .store
            .get("job", child_id)
            .ok_or("세대 작업 사라짐")?;
        let session = current["sessionId"].as_str();
        let stopped=generation_stop_reason(&current,output.as_ref().err().map(String::as_str));
        let mut generation=capture_generation_evidence(
            context,record,input,&current,&head,attempt,child_id,session,worker_started)?;
        let proposed_candidate = verified_candidate_id(&current["result"]);
        let previous_failed = proposed_candidate.and_then(|id|
            identical_failed_patch(&context.state,
                record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]),id));
        // A byte-identical failed patch from the identical base does not
        // deserve another full expensive A/B run.
        let candidate_id = proposed_candidate.filter(|_|previous_failed.is_none());
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
                    let paths=candidate["paths"].as_array().cloned().unwrap_or_default();
                    generation["changedSourcePaths"]=json!(paths);
                    let scope_valid=crate::engine::rsi_evolve::experiment_contract::observed_scope(
                        &current["input"],&paths);
                    generation["experimentScopeVerified"]=json!(scope_valid);
                    // Finish mutating the journal before borrowing its design.
                    let design=&generation["systemDesign"];
                    let design_complete=input["rsiSystemEvolution"]!=true || (
                        candidate["origin"]=="model_driven_detached_worktree"
                        && design["source"]=="model_authored_system_design"
                        && design["generatorCommit"]==head
                        && design["generationJobId"]==child_id
                        && generation["systemBlueprintVerified"]==true
                        && crate::engine::rsi_system::design_implemented(design,&paths));
                    generation["designImplemented"]=json!(design_complete);
                    // A rejected or neutral patch can still have a fully
                    // replayable independent paired report. Track measurement
                    // separately from *eligible adoption* so the next
                    // generation learns from genuine negative outcomes too.
                    generation["pairedMeasured"]=json!(
                        crate::engine::rsi::replayed_pair_metrics(
                            &context.state,&candidate).is_some());
                    generation["pairedRevalidated"]=json!(
                        crate::engine::rsi::verified_pair_receipt(&context.state,&candidate));
                    let eligible = candidate["status"] == "verified"
                        && candidate["evaluation"]["metrics"]["eligible"] == true
                        && generation["pairedRevalidated"]==true
                        && design_complete && scope_valid;
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
                            "sourceDir":record["sourceDir"],"sourceBranch":record["sourceBranch"],
                            "sourceBaseCommit":record["baseCommit"],
                            "sourceFingerprint":record["sourceFingerprint"],"campaignId":context.job_id,
                            "candidateId":id,"selectedAt":chrono::Utc::now().to_rfc3339()})).ok_or("선택한 개선기 저장 실패")?;
                    }
                }
                Err(error) => generation["error"] = json!(error),
            }
        } else {
            let worker_error = output.as_ref().err().map(String::as_str);
            let child = &current["result"]["rsi"];
            if let Some(previous)=previous_failed.as_deref() {
                generation["candidateId"]=json!(proposed_candidate);
                generation["evaluation"]=json!({"verdict":"duplicate_unproductive_patch",
                    "duplicateOf":previous});
                generation["repeatedEvaluationAvoided"]=json!(true);
                generation["error"]=json!(format!(
                    "동일 기준 커밋에서 이미 개선 효과가 없던 패치를 다시 제안했습니다: {previous}. 새로운 가설과 다른 코드 변경이 필요합니다."));
            } else {
                generation["error"] = json!(worker_error
                    .or_else(|| current["result"]["repairError"].as_str())
                    .or_else(|| child["modelError"].as_str())
                    .or_else(|| child["captureError"].as_str())
                    .or_else(|| current["error"].as_str())
                    .unwrap_or("유효한 변경이나 검사 통과 없음"));
            }
        }
        if input["rsiSystemEvolution"]==true {
            record["tokens"]=json!(campaign_model_tokens(&context.state,&context.job_id));
            generation["metaEvaluation"]=candidate_id.and_then(|id|
                context.state.store.get("rsi_candidate",id)).map(|c|c["metaEvaluation"].clone()).unwrap_or(Value::Null);
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
        let verified_outcomes=history_with_verified_payoffs(
            &context.state,&[generation.clone()]);
        generation["editMethodReward"]=json!(verified_outcomes.first()
            .map(edit_method_reward).unwrap_or(0.0));
        generation["knowledgeArtifactHashes"]=json!(hashes);
        generation["newFindings"]=json!(new_findings);
        generation["stopReason"]=json!(stopped);
        if candidate_id.is_none() && new_findings>0 && output.is_ok() && stopped.is_none()
            && current["result"]["rsi"]["modelError"].is_null() {
            generation["error"]=Value::Null;
        }
        if let Some(session)=session {
            generation["methodExecutions"]=json!(record_method_feedback(&context.state,&generation,session));
        }
        generation["methodAblation"]=measured_method_ablation(&context.state,&generation);
        generation["knowledgeId"]=json!(remember_research(&context.state,&current["input"],&generation,session.unwrap_or("")));
        if let Some(session)=session {
            generation["knowledgeReuse"]=context.state.store.get("session",session).map(|s|s["rsiKnowledgeReuse"].clone()).unwrap_or(Value::Null);
        }
        record_generation_feedback(context,record,input,&mut generation,
            session,attempt,&head);
        let observed=GenerationObservation{
            input,current:&current,stopped,attempt,
            prior_failure:previous_failed.is_some(),has_candidate:candidate_id.is_some(),
            has_research:!artifacts.is_empty(),new_findings,started,
        };
        if let Some(reason)=finalize_generation(context,record,generation,observed)? {
            return Ok(reason);
        }
    }
}

pub(super) fn proof_goal_reached(state:&AppState,input:&Value,record:&Value)->bool {
    if input["rsiStopAfterProof"]!=true {return false;}
    let mut current=record.clone();
    current["systemEvolution"]=input["rsiSystemEvolution"].clone();
    crate::engine::rsi::recursive_proof_report(state,&current)["demonstrated"]==true
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
