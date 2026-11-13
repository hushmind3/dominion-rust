//! execution campaign — extracted RSI responsibility, sharing only parent interfaces.
use super::*;

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
        let archive = crate::engine::rsi_archive::select(
            &context.state, &prior_history, &head, &child_input["rsiFrozenCases"]);
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
        child_input["rsiIndexId"]=crate::engine::code_index::refresh(&context.state,&lab)?["id"].clone();

        // The compiled successor, not the previous coordinator, now chooses
        // the architecture target and the ranked source files.
        let mut descendant_plan=Value::Null;
        if input["rsiSystemEvolution"]==true {
            child_input["rsiNativeInduction"]=json!(false);
            record["planAttempt"]=json!(attempt);
            record["planIndexId"]=child_input["rsiIndexId"].clone();
            record["planSelectedDomain"]=child_input["rsiSelectedDomain"].clone();
            record["planObjective"]=json!(selection_objective(&child_input));
            record["phase"]=json!("planning_with_compiled_descendant");
            save(context,record)?;
            let mut planner=Command::new(&binary);
            planner.arg("--rsi-worker-plan").arg(&context.job_id)
                .current_dir(&lab)
                .env("DOMINION_DATA",std::fs::canonicalize(&context.state.config.data_dir)
                    .map_err(|e|e.to_string())?)
                .env("DOMINION_WORKSPACE",&lab)
                .env_remove("DOMINION_DESKTOP_PARENT");
            let output=command(context,planner,Duration::from_secs(30)).await?;
            if !output.status.success() || output.stdout.len()>262144 {
                return Err(format!("차세대 실행기의 시스템 계획 실패: {}",
                    String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>()));
            }
            let plan:Value=serde_json::from_slice(&output.stdout)
                .map_err(|e|format!("후손 계획 파싱 실패: {e}"))?;
            if plan["source"]!="compiled_descendant_system_plan_v1"
                || plan["generatorCommit"]!=head
                || plan["campaignId"]!=context.job_id
                || plan["attempt"]!=attempt
                || plan["historyCount"].as_u64()!=Some(record["history"].as_array()
                    .map(Vec::len).unwrap_or(0) as u64)
                || plan["indexId"]!=child_input["rsiIndexId"]
                || !plan["domain"].as_str().is_some_and(|domain|DOMAINS.contains(&domain))
                || plan["systemBlueprint"]["source"]!="compiled_descendant_system_blueprint_v1"
                || plan["systemBlueprint"]["generatorCommit"]!=head
                || plan["systemBlueprint"]["architecture"].as_array()
                    .is_none_or(|v|v.len()<8||v.len()>32) {
                return Err("후손 계획의 소스·세대·색인·전체 시스템 계보 불일치".into());
            }
            crate::engine::rsi_system::validate_plan_evidence(
                &context.state,&lab,&plan,
                child_input["rsiIndexId"].as_str().unwrap_or(""))?;
            crate::engine::rsi_model_ecology::validate(
                &context.state,&plan["modelPolicy"],input,
                plan["domain"].as_str().unwrap_or(""),&head)?;
            crate::engine::rsi_archive::validate_selected(
                &context.state,&plan["archiveSelection"],&head,
                &child_input["rsiFrozenCases"])?;
            crate::engine::rsi_lineage_transfer::validate(
                &context.state,&plan["peerLineages"],&context.job_id,
                &child_input,child_input["rsiIndexId"].as_str().unwrap_or(""))?;
            crate::engine::rsi_intervention_lab::validate(
                &plan["factorialExperiment"],
                plan["architectureFrontier"].as_array().map(Vec::as_slice).unwrap_or(&[]),
                &head,record["history"].as_array().map(Vec::len).unwrap_or(0),
                crate::engine::rsi_lineage_transfer::profile(&child_input).as_deref())?;
            crate::engine::rsi_intervention_lab::validate_receipts(
                &context.state,record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]),
                &plan["factorialExperiment"])?;
            if !plan["negativeTransfer"].is_null() {
                if !crate::engine::rsi_negative_transfer::valid(
                    &plan["negativeTransfer"],
                    plan["architectureFrontier"].as_array().map(Vec::as_slice).unwrap_or(&[]),
                    &head,record["history"].as_array().map(Vec::len).unwrap_or(0)) {
                    return Err("후손 역전이 실험 출처·대상·증거 범위가 잘못됐습니다.".into());
                }
                let original=crate::engine::rsi_system::revalidated_history(
                    &context.state,record["history"].as_array()
                        .map(Vec::as_slice).unwrap_or(&[]));
                crate::engine::rsi_negative_transfer::validate_receipts(
                    &context.state,&original,&plan["negativeTransfer"])?;
            }
            // Enforce the compiled child's actual parent decision,
            // subject to the unchanged previous-generation evidence gate.
            child_input["rsiArchiveSelection"]=plan["archiveSelection"].clone();
            child_input["rsiInheritedCandidateId"]=
                plan["archiveSelection"]["candidateId"].clone();
            child_input["rsiSystemBlueprint"]=plan["systemBlueprint"].clone();
            child_input["rsiArchitectureTarget"]=plan["architectureTarget"].clone();
            child_input["rsiMetaPolicy"]=plan["metaPolicy"].clone();
            child_input["rsiPeerLineages"]=plan["peerLineages"].clone();
            child_input["rsiComparativeMode"]=plan["comparativeMode"].clone();
            child_input["rsiSuccessorStage"]=plan["successorStage"].clone();
            child_input["rsiSymbioticCurriculum"]=plan["symbioticCurriculum"].clone();
            child_input["rsiFactorialExperiment"]=plan["factorialExperiment"].clone();
            child_input["rsiNegativeTransfer"]=plan["negativeTransfer"].clone();
            child_input["rsiEditPolicy"]=plan["editPolicy"].clone();
            child_input["rsiModelEcologyPolicy"]=plan["modelPolicy"].clone();
            if plan["modelPolicy"]["active"]==true {
                child_input["providerId"]=plan["modelPolicy"]["selected"]["providerId"].clone();
                child_input["model"]=plan["modelPolicy"]["selected"]["model"].clone();
            }
            child_input["rsiDescendantPlan"]=plan.clone();
            record["nextGenerationPlan"]=plan.clone();
            descendant_plan=plan;
        }

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
            json!(["code_search", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "web_search", "http_fetch", "algorithm_propose", "unsafe_audit", "rsi_method_propose", "rsi_system_plan"]);
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
        let (target_domain, target_selection, target_strategy)=if descendant_plan.is_object(){
            (descendant_plan["domain"].as_str().ok_or("후손 분야 없음")?.to_string(),
                "compiled_descendant_system_plan",descendant_plan["strategy"].clone())
        }else{
            select_generation_strategy(
                &context.state,&history,&target_objective,continuation,trial,suggested,&head)
        };
        child_input["rsiTargetDomain"]=json!(target_domain);
        child_input["rsiTargetPlan"]=json!({
            "domain":target_domain,"selection":target_selection,
            "strategy":target_strategy,"generatorCommit":head
        });
        if descendant_plan.is_object()
            && child_input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
            let ranked=descendant_plan["rankedFiles"].as_array().cloned().unwrap_or_default();
            child_input["rsiFocusFiles"]=json!(ranked.iter()
                .filter_map(|row|row["path"].as_str()).collect::<Vec<_>>());
            child_input["rsiAutoFocus"]=json!(true);
            child_input["rsiFocusSelection"]=json!({
                "source":"compiled_descendant_system_plan",
                "candidates":ranked,"domain":target_domain,
                "sourceIndexId":child_input["rsiIndexId"],
                "generatorCommit":head
            });
        }else if child_input["rsiFocusFiles"]
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
            child_input["rsiAutoFocus"]=json!(false);
            child_input["rsiFocusSelection"]=json!({"source":"user_selected"});
        }
        // This is the system's learned *execution method*, independently of
        // which model is used to write the candidate patch.
        if descendant_plan.is_object() {
            child_input["rsiEditPolicy"]=descendant_plan["editPolicy"].clone();
        }else{
            child_input["rsiEditPolicy"]=learned_edit_method(&context.state,&child_input,&history);
        }
        context.state.store.audit("rsi.edit_method_selected",json!({
            "campaignId":context.job_id,"attempt":attempt,
            "generatorCommit":head,"policy":child_input["rsiEditPolicy"]
        }));
        let inherited = generation_context(&context.state, &child_input, record);
        let digest = research_digest(&context.state, input, record);
        let focus = focus_file_context(&context.state, record, &child_input);
        let original = child_input["prompt"].as_str().unwrap_or("").to_string();
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
        child_input["rsiFocusGrounded"] = json!(!defer_focus
            && focus.contains("sourceSha256: ") && focus.contains("~~~"));
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
                    Some("code_search" | "workspace_read_range" | "workspace_replace" | "workspace_replace_range")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        // A reused session must never attribute an older worker's stage
        // transitions to this descendant. Match the worker and exact policy.
        let mut program_stages = context.state.store.all("rsi_program_phase")
            .into_iter().filter(|row|
                row["generationJobId"].as_str()==Some(child_id)
                && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
            .collect::<Vec<_>>();
        program_stages.sort_by(|left,right|
            left["recordedAt"].as_str().unwrap_or("").cmp(
                right["recordedAt"].as_str().unwrap_or("")));
        if program_stages.len()>48 {
            program_stages.drain(..program_stages.len()-48);
        }
        // The next generator needs actual ordered tool outcomes, not merely
        // a model-written description. Keep only bounded, non-secret fields.
        let mut tool_timeline = session.map(|id| context.state.store.all("execution_operation")
            .into_iter().filter(|op|op["sessionId"].as_str()==Some(id)
                && op["origin"]=="model")
            .map(|op| json!({
                "runId":op["runId"],"opSeq":op["opSeq"],
                "startedAt":op["startedAt"],
                "tool":op["tool"],"status":op["status"],
                "path":op["arguments"]["path"],
                "result":{"written":op["result"]["written"],
                    "postSha256":op["result"]["postSha256"]},
                "toolReceipt":{"verified":op["toolReceipt"]["verified"]},
            })).collect::<Vec<_>>()).unwrap_or_default();
        tool_timeline.sort_by(|a,b|a["startedAt"].as_str().unwrap_or("")
            .cmp(b["startedAt"].as_str().unwrap_or(""))
            .then_with(||a["opSeq"].as_u64().unwrap_or(0)
                .cmp(&b["opSeq"].as_u64().unwrap_or(0))));
        if tool_timeline.len()>64 {tool_timeline.drain(..tool_timeline.len()-64);}
        let mut rule_executions=context.state.store.all("rsi_rule_execution")
            .into_iter().filter(|row|
                row["generationJobId"].as_str()==Some(child_id)
                && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
            .collect::<Vec<_>>();
        rule_executions.sort_by(|a,b|a["recordedAt"].as_str().unwrap_or("")
            .cmp(b["recordedAt"].as_str().unwrap_or("")));
        if rule_executions.len()>32 {rule_executions.drain(..rule_executions.len()-32);}
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
            "tokenUsage":{"promptTokens":usage["promptTokens"],"completionTokens":usage["completionTokens"],
                "calls":usage["calls"],"estimatedCalls":usage["estimatedCalls"]},
            "sourceContextChars":current["input"]["rsiFocusContextChars"],
            "totalPromptChars":current["input"]["rsiPromptContextChars"],
            "focusFiles":current["input"]["rsiFocusFiles"],
            "scopeExpanded":current["input"]["rsiSourceScopeExpanded"],
            "residualEvidence":current["input"]["rsiResidualEvidence"],
            "costOpportunities":current["input"]["rsiCostOpportunities"],
            "metaTransferTarget":current["input"]["rsiMetaTransferTarget"],
            "focusSelection":current["input"]["rsiFocusSelection"],
            "targetReplan":current["rsiTargetReplan"],
            "knowledgePartition":research_scope(&current["input"]).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&[
                target_focus_objective(&current["input"])]),
            // Attribute the actual selected actor, not the original
            // request route, once a descendant has evolved its model.
            "generatorBinarySha256":record["generatorBinarySha256"],
            "generatorModel":current["input"]["model"],
            "generatorProviderId":current["input"]["providerId"],
            "requestedOriginalModel":input["model"],
            "requestedOriginalProviderId":input["providerId"],
            "domainSelection":current["rsiDomainSelection"],"searchStrategy":current["rsiSearchStrategy"],
            "decisionInput":current["rsiDecisionInput"],"selectedDomainInput":current["input"]["rsiSelectedDomain"]});
        let generation_details = json!({
            "researchDelivered":current["rsiResearchDelivered"],"workerMarker":current["result"]["rsi"]["fixturePhase"],
            "boundaryResult":current["rsiBoundaryResult"],
            "elapsedMs":worker_started.elapsed().as_millis() as u64,"domain":current["rsiDomain"],
            "toolsUsed":tools_used,"builtInToolsUsed":capabilities_used,
            "inheritedImplementations":current["input"]["rsiInheritedImplementations"],
            "hypothesisMessageId":current["result"]["rsi"]["modelMessageId"],
            "researchFingerprint":research_fingerprint,
            "editEvidence":current["result"]["rsi"]["editEvidence"],
            "editPolicy":current["input"]["rsiEditPolicy"],
            "programStages":program_stages,
            "editMethodReward":Value::Null,
            "diffBytes":current["result"]["rsi"]["diffBytes"],
            "captureError":current["result"]["rsi"]["captureError"],
            "repairFailure":current["result"]["repairError"],
            "nativeMutation":current["result"]["nativeMutation"],
            "candidateOrigin":current["result"]["rsi"]["candidateOrigin"],
            "compilerRepairReceipts":current["result"]["compilerRepairReceipts"],
            "accepted":false,"createdAt":chrono::Utc::now().to_rfc3339()});
        generation.as_object_mut().expect("generation object")
            .extend(generation_details.as_object().expect("generation details object").clone());
        generation["requestedModel"]=input["model"].clone();
        generation["modelEcologyPolicy"]=current["input"]["rsiModelEcologyPolicy"].clone();
        generation["modelEcologyExecuted"]=current["rsiModelEcologyExecuted"].clone();
        generation["systemEvolution"]=current["input"]["rsiSystemEvolution"].clone();
        generation["systemBlueprint"]=current["input"]["rsiSystemBlueprint"].clone();
        generation["descendantPlan"]=current["input"]["rsiDescendantPlan"].clone();
        generation["compiledPlanExecuted"]=current["rsiCompiledPlanExecuted"].clone();
        generation["metaPolicy"]=current["input"]["rsiMetaPolicy"].clone();
        generation["recursivePolicyExecuted"]=
            current["input"]["rsiMetaPolicy"]["recursiveProgram"].clone();
        generation["principlePolicyExecuted"]=
            current["input"]["rsiMetaPolicy"]["principleLearning"].clone();
        generation["metaPolicyExecuted"]=current["rsiMetaPolicyExecuted"].clone();
        generation["comparisonProfileSha256"]=json!(
            crate::engine::rsi_lineage_transfer::profile(&current["input"]));
        generation["peerLineages"]=current["input"]["rsiPeerLineages"].clone();
        generation["comparativeMode"]=current["input"]["rsiComparativeMode"].clone();
        generation["successorStage"]=current["input"]["rsiSuccessorStage"].clone();
        generation["symbioticCurriculum"]=current["input"]["rsiSymbioticCurriculum"].clone();
        generation["factorialExperiment"]=current["input"]["rsiFactorialExperiment"].clone();
        generation["negativeTransfer"]=current["input"]["rsiNegativeTransfer"].clone();
        generation["systemDesign"]=session.and_then(|id|context.state.store.get("session",id))
            .map(|row|row["rsiSystemDesign"].clone()).unwrap_or(Value::Null);
        generation["systemBlueprintVerified"]=current["rsiSystemBlueprintVerified"].clone();
        if generation["candidateOrigin"]=="native_inductive_source_mutation" {
            generation["generatorModel"]=json!(format!("dominion-native:{}",
                generation["nativeMutation"]["operator"].as_str().unwrap_or("unknown")));
            generation["generatorProviderId"]=json!("dominion-native");
        }
        // Keep the generation JSON macro below Rust's recursive expansion
        // ceiling and attach bounded evidence only after building the core
        // generation receipt.
        generation["toolTimeline"]=json!(tool_timeline);
        generation["ruleExecutions"]=json!(rule_executions);
        // A successful native write can still produce uncompilable Rust.
        // Keep the *real compiler diagnostic* and the pre-repair history
        // separate from independently eligible improvements. This makes
        // next-generation repair synthesis learn WHY a patch failed.
        let check_diagnostics=current["result"]["rsi"]["checks"].as_array()
            .into_iter().flatten().take(4).map(|check|json!({
                "label":check["label"],"passed":check["passed"],
                "exitCode":check["exitCode"],
                "detail":check["stderr"].as_str()
                    .or_else(||check["error"].as_str()).unwrap_or("")
                    .chars().take(1600).collect::<String>()
            })).collect::<Vec<_>>();
        let prior_diagnostics=current["result"]["previousAttempts"].as_array()
            .into_iter().flatten().take(3).map(|attempt|{
                let first=attempt["result"]["checks"].as_array()
                    .into_iter().flatten().find(|c|c["passed"]!=true);
                json!({
                    "attempt":attempt["attempt"],
                    "checksPassed":attempt["result"]["checksPassed"],
                    "error":first.and_then(|c|c["stderr"].as_str()
                        .or_else(||c["error"].as_str()))
                        .unwrap_or("").chars().take(1200).collect::<String>()
                })
            }).collect::<Vec<_>>();
        generation["inheritedCandidateId"]=current["input"]["rsiInheritedCandidateId"].clone();
        generation["archiveSelection"]=current["input"]["rsiArchiveSelection"].clone();
        generation["inheritedPatchChanged"]=current["result"]["rsi"]["inheritedPatchChanged"].clone();
        generation["verification"]=json!({
            "rawCandidateId":current["result"]["rsi"]["candidateId"],
            "checksPassed":current["result"]["rsi"]["checksPassed"],
            "reason":current["result"]["reason"],
            "checks":check_diagnostics,
            "repairAttempts":current["result"]["repairAttempts"],
            "repairHistory":prior_diagnostics,
        });
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
                    // Preserve independently replayed NEUTRAL and NEGATIVE
                    // experiments too. Such experiments inform future source
                    // selection but never become adopted intelligence.
                    let fresh=crate::engine::rsi::replayed_pair_metrics(
                        &context.state,&candidate);
                    let receipt_valid=fresh.as_ref().is_some_and(|metrics|
                        metrics==&candidate["evaluation"]["metrics"]
                        && generation["evaluation"]["evidenceSha256"]==
                            candidate["evaluation"]["evidenceSha256"]);
                    generation["pairedMeasured"]=json!(receipt_valid);
                    let changed=candidate["paths"].as_array().cloned().unwrap_or_default();
                    generation["changedSourcePaths"]=json!(changed);
                    generation["factorialCompliance"]=
                        crate::engine::rsi_intervention_lab::compliance(
                            &generation["factorialExperiment"],&changed);
                    generation["factorialMeasured"]=json!(receipt_valid
                        && generation["factorialCompliance"]["eligibleForFactorial"]==true
                        && generation["evaluation"]["metrics"]["measurements"].is_array());
                    let eligible = candidate["status"] == "verified"
                        && candidate["evaluation"]["metrics"]["eligible"] == true;
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
        // A just-adopted child has not yet been replayed through the durable
        // history loader. Revalidate its ORIGINAL frozen candidate and paired
        // receipt now; otherwise every successful prediction would be
        // mislabeled as a failure while training the next meta-generation.
        let checked=crate::engine::rsi_system::revalidated_history(
            &context.state,&[generation.clone()]);
        let independently_checked=checked.first().cloned()
            .unwrap_or_else(||generation.clone());
        let independently_checked=&independently_checked;
        generation["pairedRevalidated"]=independently_checked["pairedRevalidated"].clone();
        generation["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
            &generation["metaPolicy"],independently_checked);
        generation["predictionRepairEvidence"]=
            crate::engine::rsi_prequential::repair_execution_evidence(
                independently_checked);
        generation["principleLearningFeedback"]=
            crate::engine::rsi_principle_learning::feedback(
                &generation["metaPolicy"],independently_checked);
        let mut lineage=history.clone();
        lineage.push(independently_checked.clone());
        generation["reciprocalCycleEvidence"]=json!(
            crate::engine::rsi_symbiosis::completed_cycles(&lineage));
        generation["recursiveMetaReturnEvidence"]=json!(
            crate::engine::rsi_recursive_rewrite::ledger(&lineage));
        generation["factorialLearning"]=json!({
            "source":"independently_replayed_source_factorial_trial_v1",
            "planSha256":generation["factorialExperiment"]["sha256"],
            "actualChangedPaths":generation["changedSourcePaths"],
            "originalPairedReceiptReplayed":generation["pairedMeasured"],
            "eligibleForFactorial":generation["factorialMeasured"],
            "factorialCompliance":generation["factorialCompliance"],
            "accepted":generation["accepted"]==true,
            "causalEffectProven":false
        });
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
        let prior_history = record["history"].as_array().cloned().unwrap_or_default();
        if let Some(session)=session {
            // Queue identity and next-generation lookup must use one target
            // objective. Autonomous system prompts are not owner constraints.
            let trial_objective=selection_objective(input);
            let proposals = proposed_algorithms(&context.state, session);
            let active = crate::engine::algorithms::active(&context.state, "selfimprove.search")
                .unwrap_or_else(|| json!({"id":Value::Null,"revision":Value::Null,"implementation":"failure-clusters","parameters":{}}));
            let mut queued = Vec::new();
            let had_pending_trial=record["nextTrialAlgorithmId"].is_string();
            for candidate in proposals
                .iter()
                .filter(|candidate| candidate["kind"] == "selfimprove.search")
            {
                let report = queue_search_trial(
                    &context.state,
                    candidate,
                    &active,
                    &prior_history,
                    &trial_objective,
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
            // Autonomous RSI must be capable of evolving its own search
            // policy even when a small model never calls algorithm_propose.
            // No new generation or paid model invocation is created here.
            if queued.is_empty() && !had_pending_trial
                && input["rsiAutonomous"]==true
                && generation["domainSelection"]!="algorithm_trial" {
                let mut complete=prior_history.clone();
                complete.push(generation.clone());
                if let Some(report)=queue_adaptive_search_trial(
                    &context.state,&complete,&active,
                    &trial_objective,
                    &context.job_id,attempt,
                ){
                    if let Some(id)=report["candidateId"].as_str() {
                        queued.push(id.to_string());
                        generation["autoEvolvedSearch"]=json!({
                            "candidateId":id,
                            "hypothesis":report["candidateRevision"],
                            "source":report["hypothesisSource"],
                            "status":"pending_future_real_trial_not_improvement_proof"
                        });
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
            if record["nextTrialAlgorithmId"].is_null() {
                if let Some(id) = queued.into_iter().next() {
                    record["nextTrialAlgorithmId"] = json!(id);
                }
            }
        }
        let generation_accepted = generation["accepted"] == true;
        // A real, independently adopted NATIVE source mutation can succeed
        // even when a previous model turn exhausted its own output budget.
        // Do not discard that measured descendant merely because the model
        // ended badly. The coordinator's aggregate token/time budgets are
        // enforced again before launching any later generation.
        let stopped = if generation_accepted
            && generation["nativeMutation"]["origin"]=="native_inductive_source_mutation"
            && stopped==Some("token_budget") {
            None
        } else {stopped};
        let accepted_streak = if generation_accepted {
            record["acceptedStreak"].as_u64().unwrap_or(0) + 1
        } else {
            // Research-only or failed generations interrupt a consecutive
            // adoption chain, even when no evaluation was produced.
            0
        };
        record["acceptedStreak"] = json!(accepted_streak);
        let max_streak = record["maxAcceptedStreak"]
            .as_u64()
            .unwrap_or(0)
            .max(accepted_streak);
        record["maxAcceptedStreak"] = json!(max_streak);
        // Three consecutive adopted generations are the documented threshold
        // for calling the capability demonstrated, not a single lucky patch.
        let mut evidence_record=record.clone();
        evidence_record["history"].as_array_mut()
            .ok_or("세대 이력 손상")?.push(generation.clone());
        record["capabilityDemonstrated"] = json!(max_streak >= 3
            && crate::engine::rsi::campaign_capability_proven(&context.state,&evidence_record));
        let generation_knowledge = generation["knowledgeId"].clone();
        let generation_tokens = generation["tokens"].clone();
        let generation_elapsed = generation["elapsedMs"].clone();
        let generation_verdict = generation["evaluation"]["verdict"].clone();
        // Workers read this durable table, not the coordinator's in-memory
        // history. Publish only after research, method feedback and trial
        // outcomes are complete so the next worker can actually continue.
        // Archive a measured neutral stepping stone before the coordinator
        // can stop at its final budget or be interrupted. Waiting for the
        // NEXT call to select() would silently lose a campaign's last
        // independently verified candidate across campaign restarts.
        generation["archivePersistedImmediately"]=json!(
            crate::engine::rsi_archive::persist_completed(
                &context.state,&generation,&head,&current["input"]["rsiFrozenCases"])
        );
        // A real completion timestamp is also necessary to evolve the method
        // from the latest measured outcome across restarted campaigns.
        generation["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
        context.state.store.put("rsi_generation", &generation)
            .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
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
        } else if previous_failed.is_some() {
            "duplicate_unproductive_patch"
        } else if generation_accepted {
            "accepted_implementation"
        } else if candidate_id.is_none()
            && record["history"].as_array().and_then(|rows|rows.last())
                .and_then(|row|row["editEvidence"]["attempts"].as_u64())
                .unwrap_or(0)>0 {
            "edit_attempt_without_candidate"
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
                "editEvidence":record["history"].as_array()
                    .and_then(|rows|rows.last()).map(|row|row["editEvidence"].clone()),
            },
            "reuseDecision": reuse,
        });
        elapsed(record, started);
        save(context, record)?;
        if proof_goal_reached(&context.state,input,record) {
            return Ok("capability_demonstrated".into());
        }
        if let Some(reason)=stopped {return Ok(reason.into());}
        if let Some(error)=generation_model_error(&current) {
            // A late provider failure after source bytes were written,
            // compiled and independently paired-evaluated is not a reason
            // to terminate the lineage before its next measured mutation.
            // An unmeasured error still stops this campaign.
            let paired=record["history"].as_array()
                .and_then(|rows|rows.last())
                .is_some_and(|last|last["evaluation"]["source"]
                    =="server_invoked_local_paired_git_worktrees");
            if !paired {return Err(error.to_string());}
        }
        if no_edit && should_stop_no_progress(streak, research, changed,
            record["nextTrialAlgorithmId"].is_string(),
            input["rsiAutonomous"]==true) {
            return Ok("no_progress".into());
        }
    }
}
