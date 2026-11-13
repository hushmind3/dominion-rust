//! independent paired adoption loop and recovery.
//! Only campaign_execution owns these implementations. Other RSI stages use explicit
//! names imported through the stable rsi_evolve facade.
use super::{AppState, Command, DOMAINS, Digest, Duration, Instant, JobContext, Path, PathBuf, Sha256, Value, bind_inherited_target, campaign_model_tokens, choose_focus_files, command, edit_method_reward, elapsed, focus_file_context, generation_context, generation_fits_remaining_budget, generation_prompt_variants, generation_stop_reason, git, history_with_verified_payoffs, identical_failed_patch, json, learned_edit_method, matching_prior_campaign, method_reuse_decision, new_research_findings, paired_descendant_target, pending_trial_candidate, post_adoption_objective, proposed_algorithms, queue_adaptive_search_trial, queue_search_trial, record_method_feedback, record_trial_outcome, remember_research, research_artifacts, research_digest, research_scope, rsi_worktree, save, select, select_domain_for_state, select_generation_strategy, selection_objective, session_research_fingerprint, session_tool_usage, should_stop_no_progress, target_focus_objective, verified_candidate_id, worker, retain_generator, matched_planner_intervention, plan_compiled_descendant, capture_generation_evidence};
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
        let mut child_input = input.clone();
        child_input["rsiSourceDir"] = json!(lab);
        child_input["rsiKnowledgeSource"]=record["sourceDir"].clone();
        child_input["rsiBaseCommit"] = json!(head);
        child_input["rsiCampaignId"] = json!(context.job_id);
        child_input["rsiGeneratorCommit"] = json!(head);
        // Give the next worker the REAL previous rejected source bytes as its
        // detached starting point. Re-running on the pristine baseline loses
        // the prior implementation and repeatedly produces a neutral patch.
        // The original head remains the immutable A/B baseline.
        let prior_history=record["history"].as_array().cloned().unwrap_or_default();
        let expanded=post_adoption_objective(input,&prior_history);
        if let Some(next)=&expanded {
            child_input["rsiFocusFiles"]=json!([]);
            child_input["rsiAutonomous"]=json!(true);
            child_input["rsiObjective"]=next["objective"].clone();
            child_input["prompt"]=next["prompt"].clone();
            child_input["rsiResidualEvidence"]=next["residuals"].clone();
            child_input["rsiCostOpportunities"]=next["costOpportunities"].clone();
            child_input["rsiMetaTransferTarget"]=next["metaTransferTarget"].clone();
            child_input["rsiSourceScopeExpanded"]=json!(true);
            child_input["rsiSelectedDomain"]=if next["metaTransferTarget"]==true {
                json!("improver")
            } else {
                next["costOpportunities"].as_array().and_then(|costs|costs.first())
                    .map(|row|row["domain"].clone())
                    .or_else(||next["residuals"].as_array()
                        .and_then(|rows|rows.first())
                        .map(|row|row["domain"].clone()))
                    .unwrap_or(Value::Null)
            };
        }
        // Diagnose through the rebuilt executable, never through the old server's
        // implementation. Fixed-model source improvement remains model-authored.
        if input["rsiAutonomous"]==true && input["rsiNativeInduction"]!=true && input["rsiSystemEvolution"]!=true {
            let mut probe=Command::new(&binary);probe.arg("--rsi-diagnose");
            let output=command(context,probe,Duration::from_secs(10)).await?;
            if !output.status.success() { return Err("선택 버전의 능력 진단 실행 실패".into()); }
            let diagnosis:Value=serde_json::from_slice(&output.stdout).map_err(|e|e.to_string())?;
            if diagnosis["generatorCommit"]!=head {return Err("진단 실행기 버전 불일치".into());}
            record["diagnosis"]=diagnosis.clone();
            if diagnosis["failed"].as_u64().unwrap_or(0)>0 {
                child_input["rsiDiagnostic"]=diagnosis.clone();
                child_input["rsiSelectedDomain"]=diagnosis["domain"].clone();
                child_input["rsiFocusFiles"]=diagnosis["focusFiles"].clone();
                child_input["rsiObjective"]=json!("실제 작업 분해 진단에서 실패한 순차 동작 인식을 개선한다. 기존 단일 명사와 기존 분해 동작을 보존하고 예문 전용 분기를 만들지 않는다.");
                child_input["prompt"]=json!(format!(
                    "DOMINION 실제 소스 자기개선입니다. 현재 실행 버전에서 관측한 개발 진단: {diagnosis}\n실패 원인을 소스에서 찾아 실제 코드를 수정하세요. 예문을 하드코딩하거나 평가기·테스트·진단 코드를 수정하지 마세요. 확인 평가는 별도로 동결되어 있습니다. workspace_read_range로 해당 구현과 해시를 확인한 뒤 workspace_replace_range로 근거 있는 최소 변경을 작성하세요. 빌드와 독립 비교는 실행기가 수행합니다."));
            }
        }
        // The coordinator is not the archive-selection brain during whole-
        // system evolution. The compiled successor chooses its own stepping
        // stone from the same independently sealed candidate evidence.
        let archive=if input["rsiSystemEvolution"]==true {Value::Null}
            else {crate::engine::rsi_archive::select(
                &context.state, &prior_history, &head, &child_input["rsiFrozenCases"])};
        child_input["rsiInheritedCandidateId"]=archive["candidateId"].clone();
        child_input["rsiArchiveSelection"]=archive;
        let objective = input["rsiObjective"].as_str().unwrap_or("");
        let mut allowed_campaigns = vec![context.job_id.to_string()];
        if let Some(prior)=matching_prior_campaign(&context.state,input,record) {
            if let Some(id)=prior["id"].as_str() { allowed_campaigns.push(id.to_string()); }
        }
        // An old queued candidate may have been replaced or claimed by a
        // sibling while the coordinator was rebuilding the next worker.
        let trial_algorithm = record["nextTrialAlgorithmId"].as_str()
            .filter(|id| !id.is_empty())
            .and_then(|id| context.state.store.get("algorithm",id))
            .filter(|algorithm| algorithm["rsiTrial"]["campaignId"]
                .as_str().is_some_and(|id|allowed_campaigns.iter().any(|owned|owned==id)))
            .and_then(|algorithm|algorithm["id"].as_str().map(str::to_string))
            .or_else(|| pending_trial_candidate(&context.state,objective,&allowed_campaigns)
                .and_then(|algorithm|algorithm["id"].as_str().map(str::to_string)));
        child_input["rsiAllowedTrialCampaigns"]=json!(allowed_campaigns);
        if let Some(id) = trial_algorithm {
            child_input["rsiTrialAlgorithmId"] = json!(id);
            record["nextTrialAlgorithmId"] = json!(id);
        } else {
            record["nextTrialAlgorithmId"] = Value::Null;
        }
        bind_inherited_target(&mut child_input,&record["nextDecision"]);
        // After an independently accepted generation, the next task is
        // selected from the *measured paired residuals*, not from a stale
        // parent_next_decision computed for the former source revision.
        // Reassert after the legacy nextDecision assignment above.
        if let Some(next)=&expanded {
            let measured_domain=if next["metaTransferTarget"]==true {
                Some("improver")
            }else {
                next["costOpportunities"].as_array().and_then(|rows|rows.first())
                    .and_then(|row|row["domain"].as_str())
                    .or_else(||next["residuals"].as_array()
                        .and_then(|rows|rows.first())
                        .and_then(|row|row["domain"].as_str()))
            };
            if let Some(domain)=measured_domain.filter(|d|DOMAINS.contains(d)) {
                child_input["rsiSelectedDomain"]=json!(domain);
                child_input["rsiDecisionInput"]=json!({
                    "basis":"independently_measured_post_adoption_target",
                    "previousAcceptedCommit":next["previousAcceptedCommit"],
                    "targetDomain":domain,
                    "metaTransferTarget":next["metaTransferTarget"]
                });
            }
        }
        if input["rsiSystemEvolution"]==true {
            // A system-evolution campaign must not re-pin every generation to
            // its original improver file. The rebuilt descendant selects
            // subsequent targets; the coordinator only preserves measured
            // post-adoption constraints and the owner's explicit file choice.
            let first_system_generation=attempt==1
                && !prior_history.iter().any(|row|row["accepted"]==true);
            let owner_focus=input["rsiFocusFiles"].as_array()
                .is_some_and(|paths|!paths.is_empty());
            let evidence=if first_system_generation {
                crate::engine::rsi_meta::development_evidence(&context.state)
            } else {None};
            if first_system_generation && !owner_focus && input["rsiAutonomous"]==true {
                child_input["rsiSelectedDomain"]=json!("improver");
                child_input["rsiDecisionInput"]=json!({
                    "basis":"initial_system_evolution","targetDomain":"improver"
                });
                child_input["rsiObjective"]=json!("DOMINION 자체 개선기의 알고리즘·하네스·탐색·학습을 실제 소스로 개선하여 같은 모델과 예산에서 후속 코드 개선 성과를 높인다.");
            } else if expanded.is_none() && !paired_descendant_target(&child_input) {
                // Never inherit a stale parent decision after an adoption.
                child_input["rsiSelectedDomain"]=Value::Null;
                child_input["rsiDecisionInput"]=Value::Null;
            }
            if !owner_focus {
                child_input["rsiFocusFiles"]=json!([]);
            }
            child_input["rsiSystemEvidence"]=json!(evidence);
            child_input["prompt"]=json!(format!(
                "DOMINION 재귀적 시스템 자기개선 작업입니다. 선택한 API 또는 로컬 모델을 이용하여 알고리즘, 후보 생성·선택, 실행 하네스, 문맥·기억·학습·도구 코드를 직접 개선할 수 있습니다. 전체 소스가 탐색 범위이며, 검증된 후손은 다음 세대의 개선 계획도 생성해야 합니다.\n사용자가 지정한 목표: {}\n현재 세대의 개발 증거(현재 소스에서도 원인을 확인할 것): {}\n수정된 개선기를 실제 재빌드하고 이전 개선기와 동일 모델·예산의 후속 소스 수정 성과를 독립 평가합니다. 제품 버그 하나의 수정으로 이 과제를 대체하지 마세요. 평가기 변경이나 무검증 성공 주장은 허용되지 않습니다. 실제 소스 변경을 검증 가능한 근거로 남기세요.",
                child_input["rsiObjective"],child_input["rsiSystemEvidence"]));
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
            json!(["code_search", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "web_search", "http_fetch", "algorithm_propose", "unsafe_audit", "rsi_method_propose"]);
        let history=record["history"].as_array().cloned().unwrap_or_default();
        let target_objective=selection_objective(&child_input);
        let continuation=history.last()
            .filter(|_|expanded.is_none())
            .filter(|row|row["candidateId"].is_null()
                && row["newFindings"].as_u64().unwrap_or(0)>0)
            .and_then(|row|row["domain"].as_str())
            .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
        // A pending exploration formula must not hijack the first
        // measured post-adoption meta/transfer target. Keep the trial in
        // durable state; only defer applying it to this descendant.
        let trial=child_input["rsiTrialAlgorithmId"].as_str()
            .filter(|_|expanded.is_none())
            .filter(|_|child_input["rsiAllowedTrialCampaigns"].is_array())
            .map(str::to_string);
        let suggested=child_input["rsiSelectedDomain"].as_str().map(str::to_string);
        let (target_domain, target_selection, target_strategy)=select_generation_strategy(
            &context.state,&history,&target_objective,continuation,trial,suggested,&head);
        child_input["rsiTargetDomain"]=json!(target_domain);
        child_input["rsiTargetPlan"]=json!({
            "domain":target_domain,"selection":target_selection,
            "strategy":target_strategy,"generatorCommit":head
        });
        if input["rsiSystemEvolution"]==true
            && input["rsiFocusFiles"].as_array().is_none_or(|files|files.is_empty()) {
            // Only the rebuilt descendant ranks system-evolution files.
            // The old resident coordinator must not pre-commit a stale guess.
            child_input["rsiFocusFiles"]=json!([]);
            child_input["rsiAutoFocus"]=json!(true);
            child_input["rsiFocusSelection"]=json!({
                "source":"pending_rebuilt_descendant_controller"
            });
        } else if child_input["rsiFocusFiles"]
            .as_array().is_none_or(|files|files.is_empty()) {
            let policy=target_strategy["id"].as_str()
                .and_then(|id|context.state.store.get("algorithm",id))
                .or_else(||crate::engine::algorithms::active(&context.state,"selfimprove.search"));
            let ranked=choose_focus_files(&context.state,&child_input,&history,policy.as_ref());
            let selected=ranked.iter().filter_map(|row|row["path"].as_str())
                .map(str::to_string).collect::<Vec<_>>();
            child_input["rsiFocusFiles"]=json!(selected);
            child_input["rsiAutoFocus"]=json!(true);
            child_input["rsiFocusSelection"]=json!({
                "source":"indexed_evidence","candidates":ranked,
                "domain":target_domain,
                "policyRevision":policy.as_ref().map(|row|row["revision"].clone()),
                "sourceIndexId":child_input["rsiIndexId"]
            });
        } else {
            child_input["rsiAutoFocus"]=json!(child_input["rsiDiagnostic"].is_object() || input["rsiSystemEvolution"]==true);
            child_input["rsiFocusSelection"]=json!({"source":if child_input["rsiDiagnostic"].is_object(){"runtime_diagnosis"}else{"user_selected"}});
        }
        // The running descendant owns its next strategy, the frozen supervisor
        // checks every replay receipt and records the chosen executable plan.
        if input["rsiSystemEvolution"]==true {
            plan_compiled_descendant(context,record,input,&mut child_input,
                &lab,&binary,head.clone(),attempt,&history).await?;
        }
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
                        && generation["pairedRevalidated"]==true && design_complete;
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
