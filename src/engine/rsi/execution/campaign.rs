//! execution campaign: compiled descendant RSI responsibility.
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
        // The original owner choice is immutable across this campaign.
        // Future source-compiled descendants may select only from an
        // explicitly approved pool; no model outside it can be invoked.
        child_input["rsiModelOrigin"]=json!({
            "providerId":input["providerId"],"model":input["model"]
        });
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
        // Whole-system RSI delegates parent selection to the just-rebuilt
        // descendant. A resident coordinator must not preselect a parent using
        // its old Rust policy. Ordinary campaigns keep their existing path.
        if input["rsiSystemEvolution"]!=true {
            let archive=crate::engine::rsi_archive::select(
                &context.state,&prior_history,&head,&child_input["rsiFrozenCases"]);
            child_input["rsiInheritedCandidateId"]=archive["candidateId"].clone();
            child_input["rsiArchiveSelection"]=archive;
        }else{
            child_input["rsiInheritedCandidateId"]=Value::Null;
            child_input["rsiArchiveSelection"]=Value::Null;
        }
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
        // The accepted *rebuilt* generator, not the original resident server,
        // plans the next system-evolution task. The resident coordinator owns
        // budgets, frozen evaluation and promotion; these fields are mutable
        // runtime policy and must be allowed to evolve with source revisions.
        if input["rsiSystemEvolution"]==true {
            record["phase"]=json!("planning_next_generator");
            save(context,record)?;
            let plan_dir=context.state.config.data_dir.join("rsi-controller-plans");
            std::fs::create_dir_all(&plan_dir).map_err(|e|e.to_string())?;
            let plan_path=plan_dir.join(format!("{}-{attempt}.json",context.job_id));
            let snapshot=json!({
                "generatorCommit":head,"attempt":attempt,"input":child_input
            });
            std::fs::write(&plan_path,serde_json::to_vec(&snapshot).map_err(|e|e.to_string())?)
                .map_err(|e|e.to_string())?;
            let mut planner=Command::new(&binary);
            planner.arg("--rsi-plan-generation").arg(&plan_path)
                .current_dir(&lab)
                .env("DOMINION_DATA",&context.state.config.data_dir)
                .env("DOMINION_WORKSPACE",&lab)
                .env_remove("DOMINION_DESKTOP_PARENT");
            let output=command(context,planner,Duration::from_secs(30)).await;
            let _=std::fs::remove_file(&plan_path);
            let output=output?;
            if !output.status.success() {
                return Err(format!("재빌드된 개선기의 다음 세대 계획 실패: {}",
                    String::from_utf8_lossy(&output.stderr)));
            }
            let plan:Value=serde_json::from_slice(&output.stdout)
                .map_err(|e|format!("후손 계획 출력 형식 오류: {e}"))?;
            if plan["source"]!="compiled_descendant_system_controller_v2"
                || plan["generatorCommit"]!=head
                || plan["historyCount"].as_u64()!=Some(history.len() as u64) {
                return Err("후손 계획과 실행 소스·세대 계보가 다릅니다.".into());
            }
            if !crate::engine::rsi_model_lineage::valid(
                &plan["modelEvolutionPlan"],&child_input,&head,history.len()){
                return Err("후손 모델 선택이 원래 사용자 모델·승인된 후보·근거와 다릅니다.".into());
            }
            if input["rsiAllowModelEvolution"]==true {
                // Only owner-approved, enabled and locally eligible models
                // may be selected. This is a permission gate, not a model-
                // authored assertion of its own superiority.
                let original=&plan["modelEvolutionPlan"];
                let approved=&input["rsiApprovedModelPool"];
                if approved!=&original["approvedPool"]
                    || original["selected"]==original["baseline"]
                        && original["baseline"].is_null(){
                    return Err("모델 후보의 원래 승인 내용 또는 기본 모델이 없습니다.".into());
                }
                let p=original["selected"]["providerId"].as_str()
                    .ok_or("선택 모델의 제공자 없음")?;
                let m=original["selected"]["model"].as_str()
                    .ok_or("선택 모델의 ID 없음")?;
                let registered=context.state.store.get("provider",p)
                    .ok_or("승인된 모델 제공자가 현재 없습니다.")?;
                let live_route=crate::engine::gateway::route_from_provider(
                    &context.state,&registered,Some(m))?;
                if registered["enabled"]==false
                    || !crate::engine::gateway::provider_eligible(&registered)
                    || (input["rsiLocalOnly"]==true && !live_route.local) {
                    return Err("선택된 모델의 실제 실행 승인 상태가 다릅니다.".into());
                }
                child_input["providerId"]=json!(p);
                child_input["model"]=json!(m);
            }
            child_input["rsiModelEvolutionPlan"]=plan["modelEvolutionPlan"].clone();
            if plan["systemBlueprint"]["generatorCommit"]!=head
                || plan["systemBlueprint"]["historyCount"].as_u64()!=Some(history.len() as u64)
                || plan["systemBlueprint"]["architecture"].as_array()
                    .is_none_or(|items|items.is_empty() || items.len()>64)
                || !plan["improvementDirective"].is_object()
                || plan["metaPolicy"]["source"]!="compiled_editable_meta_controller_v1"
                || plan["metaPolicy"]["historyCount"].as_u64()!=Some(history.len() as u64) {
                return Err("후손 시스템 자기모형·메타 개선기·계보가 일치하지 않습니다.".into());
            }
            // The invariant is the actual source, not an older generation's
            // fixed role taxonomy or copied architecture ranking.
            crate::engine::rsi_system::validate_plan_evidence(
                &context.state,&lab,&head,
                child_input["rsiIndexId"].as_str().unwrap_or(""),&plan)?;
            crate::engine::rsi_symbiosis::validate_curriculum(
                &plan["symbioticCurriculum"],&plan["architectureTarget"],
                plan["architectureFrontier"].as_array()
                    .ok_or("후손 상호개선 계획의 소스 그래프가 없습니다.")?)?;
            if !crate::engine::rsi_negative_transfer::valid(
                &plan["negativeTransferPlan"],
                plan["architectureFrontier"].as_array()
                    .ok_or("후손 능력 간 간섭의 실제 소스 근거 없음")?,
                &head,history.len()) {
                return Err("동결 평가 결과에서 발견된 능력 간 간섭·수정 소스 근거가 유효하지 않습니다.".into());
            }
            let pair=&plan["negativeTransferPlan"]["selected"];
            if input["rsiAutonomous"]==true
                && input["rsiFocusFiles"].as_array().is_none_or(|rows|rows.is_empty())
                && !matches!(input["rsiFocusSelection"]["source"].as_str(),
                    Some("owner_selected"|"user_selected"))
                && !paired_descendant_target(&child_input)
                && pair.is_object() {
                let expected=[&pair["victim"],&pair["source"]];
                let chosen=plan["focusFiles"].as_array()
                    .ok_or("간섭 복구 후손의 실제 두 수정 파일이 없습니다.")?;
                let ranked=plan["ranked"].as_array()
                    .ok_or("간섭 복구 후손의 소스 후보 근거가 없습니다.")?;
                if chosen.len()!=2 || ranked.len()<2
                    || expected.iter().enumerate().any(|(i,row)|{
                        chosen[i]!=row["path"]
                            || ranked[i]["path"]!=row["path"]
                            || ranked[i]["sourceHash"]!=row["sourceSha256"]
                            || ranked[i]["sourceIndexId"]!=child_input["rsiIndexId"]
                    }) {
                    return Err("후손의 공동 복구 대상과 실제 소스 인덱스 해시가 다릅니다.".into());
                }
            }
            if !crate::engine::rsi_interaction::valid_graph(
                &plan["interactionGraph"])
                || (plan["coupledPortfolio"].is_object()
                    && !crate::engine::rsi_interaction::valid_portfolio(
                        &plan["coupledPortfolio"],&plan["interactionGraph"],
                        plan["architectureFrontier"].as_array()
                            .ok_or("후손 상호능력 구성의 소스 그래프가 없습니다.")?)) {
                return Err("후손의 교차 능력 평가 근거 또는 소스 결합 계획이 변경됐습니다.".into());
            }
            if !crate::engine::rsi_factorial::valid(
                &plan["factorialExperiment"],
                plan["architectureFrontier"].as_array().ok_or(
                    "실험 설계용 탐색 근거 없음")?,
                &head,history.len(),&plan["interactionGraph"])
                || !crate::engine::rsi_recursive_scientist::valid(
                    &plan["recursiveScientificProgram"],
                    plan["architectureFrontier"].as_array().ok_or(
                        "자기개선 과학 탐색 근거 없음")?,
                    &head,history.len(),
                    &plan["factorialExperiment"],&plan["coupledPortfolio"]) {
                return Err("후손의 실험 설계 및 재귀과학 프로그램이 소스와 불일치합니다.".into());
            }
            if !crate::engine::rsi_method_generalization::valid(
                &plan["methodGeneralization"],&head,history.len()) {
                return Err("후손 개선 방법의 독립 평가 학습 근거가 유효하지 않습니다.".into());
            }
            // Pure typed contract: immutable evaluator scope and source-
            // compiled experiment policy are validated before dispatch.
            if crate::engine::rsi_evolve::meta_experiment_policy::decode(
                &plan["experimentPolicy"],&head,&child_input).is_none()
                || !plan["metaOperatorActive"].is_boolean() {
                return Err("후손 메타 실험 정책 또는 동결 평가 계약이 유효하지 않습니다.".into());
            }
            if !crate::engine::rsi_improvement_algebra::valid(
                &plan["improvementAlgebra"],
                plan["architectureFrontier"].as_array()
                    .ok_or("개선 대수의 실제 소스 역할 근거 없음")?,
                &head,history.len()) {
                return Err("후손 개선 대수의 규칙 출처·실행 소스가 유효하지 않습니다.".into());
            }
            if plan["successorStage"].is_object()
                && (plan["successorStage"]["source"]!=
                    "rebound_verified_model_successor_program_v1"
                    || plan["successorStage"]["path"]!=plan["architectureTarget"]["path"]
                    || plan["successorStage"]["sourceSha256"]!=
                        plan["architectureTarget"]["sourceSha256"]) {
                return Err("후손 개선 프로그램이 현재 설계 소스와 불일치합니다.".into());
            }
            let profile=crate::engine::rsi_meta_controller::comparison_profile(&child_input);
            crate::engine::rsi_meta_controller::validate_peer_lineages(
                &context.state,&plan["peerLineages"],profile.as_deref(),&context.job_id)?;
            let domain=plan["domain"].as_str()
                .filter(|d|DOMAINS.contains(d)).ok_or("후손이 유효하지 않은 개선 영역을 선택했습니다.")?;
            let focus=plan["focusFiles"].as_array()
                .ok_or("후손 계획에 소스 후보 목록이 없습니다.")?;
            if focus.len()>8 || focus.iter().any(|file|file.as_str().is_none_or(|p|
                !crate::engine::rsi::safe_path(p)
                    || !lab.join(p).is_file()
                    || crate::engine::code_index::indexed_source_hash(
                        &context.state,child_input["rsiIndexId"].as_str().unwrap_or(""),p).is_none())) {
                return Err("후손이 허용되지 않은 소스 파일을 선택했습니다.".into());
            }
            child_input["rsiSelectedDomain"]=json!(domain);
            child_input["rsiTargetDomain"]=json!(domain);
            child_input["rsiTargetPlan"]=json!({
                "domain":domain,"selection":plan["selection"],
                "strategy":plan["strategy"],"generatorCommit":head
            });
            let explicit_owner_focus=input["rsiFocusFiles"].as_array()
                .is_some_and(|files|!files.is_empty());
            if !explicit_owner_focus {
                child_input["rsiFocusFiles"]=json!(focus);
                child_input["rsiAutoFocus"]=json!(true);
            }
            child_input["rsiFocusSelection"]=json!({
                "source":if explicit_owner_focus {"owner_selected"} else {"rebuilt_descendant_controller"},
                "domain":domain,"candidates":plan["ranked"],
                "sourceIndexId":child_input["rsiIndexId"],
                "generatorCommit":head
            });
            child_input["rsiSystemBlueprint"]=plan["systemBlueprint"].clone();
            child_input["rsiArchitectureFrontier"]=plan["architectureFrontier"].clone();
            child_input["rsiArchitectureTarget"]=plan["architectureTarget"].clone();
            child_input["rsiMetaPolicy"]=plan["metaPolicy"].clone();
            child_input["rsiSymbioticCurriculum"]=plan["symbioticCurriculum"].clone();
            child_input["rsiInteractionGraph"]=plan["interactionGraph"].clone();
            child_input["rsiNegativeTransferPlan"]=plan["negativeTransferPlan"].clone();
            child_input["rsiCoupledPortfolio"]=plan["coupledPortfolio"].clone();
            child_input["rsiFactorialExperiment"]=plan["factorialExperiment"].clone();
            child_input["rsiRecursiveScientificProgram"]=plan["recursiveScientificProgram"].clone();
            child_input["rsiImprovementAlgebra"]=plan["improvementAlgebra"].clone();
            child_input["rsiMethodGeneralization"]=plan["methodGeneralization"].clone();
            child_input["rsiExperimentPolicy"]=plan["experimentPolicy"].clone();
            child_input["rsiMetaOperatorActive"]=plan["metaOperatorActive"].clone();
            child_input["rsiMetaRedesignTarget"]=plan["metaRedesignTarget"].clone();
            child_input["rsiMetaRedesignActive"]=plan["metaRedesignActive"].clone();
            child_input["rsiModelEvolutionPlan"]=plan["modelEvolutionPlan"].clone();
            child_input["rsiSuccessorStage"]=plan["successorStage"].clone();
            child_input["rsiPeerLineages"]=plan["peerLineages"].clone();
            child_input["rsiComparativeMode"]=plan["comparativeMode"].clone();
            child_input["rsiImprovementDirective"]=plan["improvementDirective"].clone();
            // The descendant is free to evolve its archive ranking, but
            // neither a copied pointer nor a plausible patch hash can bypass
            // the old evaluator's original paired receipt and frozen oracle.
            crate::engine::rsi_archive::validate_selected(
                &context.state,&plan["archiveSelection"],
                &head,&child_input["rsiFrozenCases"])?;
            child_input["rsiInheritedCandidateId"]=plan["archiveSelection"]["candidateId"].clone();
            child_input["rsiArchiveSelection"]=plan["archiveSelection"].clone();
            child_input["rsiEditPolicy"]=plan["editPolicy"].clone();
            child_input["rsiInheritedMetaProgram"]=plan["inheritedProgram"].clone();
            child_input["rsiControllerPlan"]=plan.clone();
            context.state.store.audit("rsi.descendant_controller_plan",json!({
                "campaignId":context.job_id,"attempt":attempt,
                "generatorCommit":head,"domain":domain,
                "focusFiles":focus,"selection":plan["selection"]
            }));
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
        child_input["rsiPromptContextChars"] = json!(child_input["prompt"]
            .as_str().map(|prompt| prompt.chars().count()).unwrap_or(0));
        // Run a separate method-disabled descendant against the SAME sealed
        // input BEFORE queuing the treatment. Real model and evaluation
        // receipts belong to this campaign and count against its budgets.
        run_control(context,record,input,&mut child_input,&lab,&binary,started).await?;
        // The OFF arm may exhaust the elapsed campaign time. Refuse to
        // launch the treatment as an unaccounted extra worker.
        if record["limits"]["timeBudgetSeconds"].as_u64().is_some_and(|budget|
            started.elapsed().as_secs().saturating_sub(
                record["waitingMs"].as_u64().unwrap_or(0)/1000)>=budget) {
            return Ok("time_budget".into());
        }
        // The OFF arm consumes real tokens; recompute the ON worker's budget
        // from the surviving campaign balance instead of the stale pre-OFF
        // estimate. The worker performs its own provider-aware preflight.
        child_input["rsiRemainingCampaignTokens"]=record["limits"]["tokenBudget"]
            .as_u64().map(|budget|budget.saturating_sub(
                record["tokens"].as_u64().unwrap_or(0)))
            .map_or(Value::Null,|tokens|json!(tokens));
        if !generation_fits_remaining_budget(record,input) {
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
        let observation=collect_generation_observation(&context.state,&current,child_id);
        let tokens=observation.tokens;
        let usage=&observation.usage;
        record["tokensEstimated"] = json!(
            record["tokensEstimated"] == true || usage["estimatedCalls"].as_u64().unwrap_or(0) > 0
        );
        record["tokens"] = json!(record["tokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(tokens));
        let mut generation=assemble_generation_receipt(
            context,input,&current,record,&head,child_id,attempt,worker_started,
            observation);
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
            // Record the candidate's REAL changed paths, including rejected
            // comparisons; a declared design goal cannot masquerade as an
            // observed component intervention in later descendants.
            generation["changedSourcePaths"] = context.state.store
                .get("rsi_candidate", id)
                .map(|candidate| candidate["paths"].clone())
                .unwrap_or(Value::Null);
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
                    // This bit records ACTUAL independent pair evaluation,
                    // including rejected candidates. A self-reported patch
                    // result or only a successful compile must not train
                    // the next generation's model selection policy.
                    generation["pairedMeasured"]=json!(
                        candidate["evaluation"]["source"]==
                            "server_invoked_local_paired_git_worktrees"
                        && candidate["evaluation"]["metrics"].is_object());
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
                        generation["pairedRevalidated"]=json!(true);
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
        let replayed_adoption=verified_outcomes.first()
            .is_some_and(|row|row["evidenceVerified"]==true);
        let proven_model_edit=crate::engine::rsi::model_driven_generation_provenance(
            &generation);
        generation["modelDrivenIndependentProof"]=json!(
            proven_model_edit && replayed_adoption);
        generation["nativeIndependentProof"]=json!(
            generation["candidateOrigin"]=="native_inductive_source_mutation"
                && replayed_adoption);
        generation["modelCallsObserved"]=json!(
            generation["tokenUsage"]["calls"].as_u64().unwrap_or(0));
        // Provenance is independently re-read from worker and tool receipts.
        generation["operatorExecution"]=crate::engine::rsi_operator_provenance::observe(
            &context.state,&generation);
        // Actual OFF/ON source-task quality difference, recalculated later by
        // the independent closure evaluator from durable paired receipts.
        generation["methodAblation"]=measured_method_ablation(&context.state,&generation);
        generation["proofCategory"]=json!(if proven_model_edit && replayed_adoption {
            "independently_adopted_model_invoked_source_edit"
        }else if generation["candidateOrigin"]=="native_inductive_source_mutation"
            && replayed_adoption {
            "independently_adopted_native_source_mutation_no_model_claim"
        }else{
            "no_independently_proven_model_driven_improvement"
        });
        generation["knowledgeArtifactHashes"]=json!(hashes);
        generation["newFindings"]=json!(new_findings);
        generation["stopReason"]=json!(stopped);
        // Stage receipts are written by the real model/tool router.
        // A bound program is NOT claimed to have executed without receipts.
        generation["inheritedMetaProgramObserved"]=json!(
            generation["inheritedMetaProgramBound"]["source"]==
                "compiled_successor_runtime_edit_policy"
            && generation["programStages"].as_array().is_some_and(|steps|
                steps.iter().any(|step|
                    step["programRevision"]==generation["editPolicy"]["revision"]
                    && step["phase"]=="edit")));
        if input["rsiSystemEvolution"]==true {
            let observed=crate::engine::rsi_meta_controller::feedback(
                &generation["metaPolicy"],&generation);
            generation["metaLearning"]=observed;
            // Provisional trace only: independent adoption becomes true
            // after rsi_system replays the original paired receipt.
            generation["predictionRepairEvidence"]=
                crate::engine::rsi_prequential::repair_execution_evidence(&generation);
        }
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
        // Do not calculate a weaker 3-adoption success flag here. The
        // authoritative receipt-backed A→B→A verdict is recomputed after the
        // generation, its candidate and original comparison are persisted.
        let generation_knowledge = generation["knowledgeId"].clone();
        let generation_tokens = generation["tokens"].clone();
        let generation_elapsed = generation["elapsedMs"].clone();
        let generation_verdict = generation["evaluation"]["verdict"].clone();
        // Workers read this durable table, not the coordinator's in-memory
        // history. Publish only after research, method feedback and trial
        // outcomes are complete so the next worker can actually continue.
        // A real completion timestamp is also necessary to evolve the method
        // from the latest measured outcome across restarted campaigns.
        // Persist the neutral stepping stone even when this is the final
        // generation and no later select() occurs. A rejected/invalid patch
        // earns no archive credit, and storage failure is explicit.
        generation["neutralArchivePersisted"]=json!(
            crate::engine::rsi_archive::persist_completed(
                &context.state,&generation,&head,&input["rsiFrozenCases"]));
        generation["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
        context.state.store.put("rsi_generation", &generation)
            .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
        record["history"]
            .as_array_mut()
            .ok_or("세대 목록 손상")?
            .push(generation);
        // Stored history is authoritative; the same audit function is used by
        // the UI and by the stop gate. Source evolution and completed recursive
        // METHOD improvement remain different claims.
        crate::engine::rsi::apply_campaign_verdict(&context.state,record);
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

