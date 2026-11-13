//! execution worker — extracted RSI responsibility, sharing only parent interfaces.
use super::*;

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
    let campaign = job["input"]["rsiCampaignId"]
        .as_str()
        .ok_or("자동 개선 ID 없음")?
        .to_string();
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
    let objective = selection_objective(&job["input"]);
    let paired_target=paired_descendant_target(&job["input"]);
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
    let compiled_plan=compiled_system_target(&job,&commit,history.len())?;
    if compiled_plan.is_some() {
        crate::engine::rsi_lineage_transfer::validate(
            &state,&job["input"]["rsiPeerLineages"],
            job["input"]["rsiCampaignId"].as_str().unwrap_or(""),
            &job["input"],job["input"]["rsiIndexId"].as_str().unwrap_or(""))?;
        let refreshed_history=crate::engine::rsi_system::revalidated_history(&state,&history);
        let root=Path::new(job["input"]["rsiSourceDir"].as_str()
            .ok_or("후손 소스가 없습니다.")?);
        let plan=&job["input"]["rsiDescendantPlan"];
        crate::engine::rsi_system::validate_plan_evidence(
            &state,root,plan,job["input"]["rsiIndexId"].as_str().unwrap_or(""))?;
        crate::engine::rsi_archive::validate_selected(
            &state,&plan["archiveSelection"],&commit,
            &job["input"]["rsiFrozenCases"])?;
        crate::engine::rsi_intervention_lab::validate(
            &plan["factorialExperiment"],
            plan["architectureFrontier"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            &commit,history.len(),
            crate::engine::rsi_lineage_transfer::profile(&job["input"]).as_deref())?;
        crate::engine::rsi_intervention_lab::validate_receipts(
            &state,&history,&plan["factorialExperiment"])?;
        // The source blueprint still anchors this binary's exact inherited
        // revision, but source ranking and meta synthesis are intentionally
        // not replayed by an unmodified predecessor policy.
        let model=crate::engine::rsi_system::blueprint(root,&commit,&refreshed_history);
        if plan["systemBlueprint"]!=model
            || job["input"]["rsiSystemBlueprint"]!=model
            || plan["metaPolicy"]!=job["input"]["rsiMetaPolicy"]
            || plan["architectureTarget"]!=job["input"]["rsiArchitectureTarget"]
            || plan["archiveSelection"]!=job["input"]["rsiArchiveSelection"]
            || job["input"]["rsiInheritedCandidateId"]!=plan["archiveSelection"]["candidateId"]
            || plan["successorStage"]!=job["input"]["rsiSuccessorStage"]
            || plan["symbioticCurriculum"]!=job["input"]["rsiSymbioticCurriculum"]
            || plan["factorialExperiment"]!=job["input"]["rsiFactorialExperiment"]{
            return Err("후손 바이너리·설계 revision·작업 입력이 다릅니다.".into());
        }
    }
    let (mut domain, mut domain_selection, mut strategy)=if let Some((domain,selection,strategy))=&compiled_plan {
        (domain.clone(),*selection,strategy.clone())
    } else {
        select_generation_strategy(&state,&history,&objective,
            continuation.clone(),trial_id.clone(),selected_domain.clone(),&commit)
    };
    // An adopted binary can revise its own targeting logic. Keep the original
    // parent decision unless the recompiled generator reaches a different
    // conclusion on the same durable history and objective.
    if compiled_plan.is_none() && job["input"]["rsiSourceAdvanced"]==true
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
    // The coordinator that queued this child may be the OLD generator. A
    // recompiled descendant must actually use its own revised policy
    // synthesis, not inherit an obsolete parent-selected edit method.
    let mut descendant_method=if compiled_plan.is_some() {
        job["input"]["rsiDescendantPlan"]["editPolicy"].clone()
    }else{
        learned_edit_method(&state,&job["input"],&history)
    };
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
    if compiled_plan.is_some() {
        let policy=&job["input"]["rsiModelEcologyPolicy"];
        if policy.is_object() {
            crate::engine::rsi_model_ecology::validate(
                &state,policy,
                &state.store.get("job",&campaign)
                    .ok_or("원본 자동 개선 작업 없음")?["input"],
                &domain,&commit)?;
            if policy["active"]==true {
                if job["input"]["providerId"]!=policy["selected"]["providerId"]
                    ||job["input"]["model"]!=policy["selected"]["model"] {
                    return Err("후손 실행 모델과 컴파일된 모델 정책이 다릅니다.".into());
                }
            }
        }
        job["rsiCompiledPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":commit,
            "domain":domain,
            "selection":domain_selection,
            "strategy":strategy,
            "sourceIndexId":job["input"]["rsiIndexId"],
            "attempt":job["input"]["rsiDescendantPlan"]["attempt"]
        });
        job["rsiMetaPolicyExecuted"]=json!({
            "source":"running_compiled_descendant_meta_controller",
            "generatorCommit":commit,
            "metaRevision":job["input"]["rsiMetaPolicy"]["revision"],
            "forecast":job["input"]["rsiMetaPolicy"]["forecast"],
            "selectedRole":job["input"]["rsiMetaPolicy"]["target"]["role"]
        });
    }
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
    let system_prompt=if job["input"]["rsiSystemEvolution"]==true {
        let history=crate::engine::rsi_system::revalidated_history(&state,&history);
        let path=job["input"]["rsiSourceDir"].as_str()
            .ok_or("후손 소스 경로가 없습니다.")?;
        let model=crate::engine::rsi_system::blueprint(Path::new(path),&commit,&history);
        if model!=job["input"]["rsiSystemBlueprint"] {
            return Err("후손 바이너리와 시스템 자기모형의 revision·계보가 다릅니다.".into());
        }
        job["rsiSystemBlueprintVerified"]=json!(true);
        let directive=crate::engine::rsi_system::instruction(
            &model,job["input"]["rsiObjective"].as_str().unwrap_or(""));
        let target=&job["input"]["rsiArchitectureTarget"];
        let frontier_context=if target["domain"]==domain {
            format!("\nCompiled descendant architecture target: role={} source={} currentSha256={} reason={}. Inspect source and make an actual cross-module change when needed. This is a proposed experiment, not a proven gain.\n",
                target["role"],target["path"],target["sourceSha256"],target["reason"])
        }else{String::new()};
        let meta_directive=crate::engine::rsi_meta_controller::instruction(
            &job["input"]["rsiMetaPolicy"]);
        let recursive_directive=crate::engine::rsi_recursive_rewrite::instruction(
            &job["input"]["rsiMetaPolicy"]);
        let principle_directive=crate::engine::rsi_principle_learning::instruction(
            &job["input"]["rsiMetaPolicy"]);
        let actor_directive=crate::engine::rsi_model_ecology::instruction(
            &job["input"]["rsiModelEcologyPolicy"]);
        let peer_context=crate::engine::rsi_lineage_transfer::instruction(
            &job["input"]["rsiPeerLineages"]);
        let mutual_context=crate::engine::rsi_symbiosis::instruction(
            &job["input"]["rsiSymbioticCurriculum"]);
        let factorial_context=crate::engine::rsi_intervention_lab::instruction(
            &job["input"]["rsiFactorialExperiment"]);
        let stage=&job["input"]["rsiSuccessorStage"];
        let stage_context=if stage.is_object() {
            format!("\n[EXECUTABLE INHERITED SUCCESSOR PROGRAM] Design={} phase={} objective={} source={} sourceSha256={} learningPolicy={} toolPolicy={} executionPolicy={}. This is your parent's independently adopted task plan; actually implement the stage and improve the meta-program.\n",
                stage["designId"],stage["stageIndex"],stage["objective"],
                stage["path"],stage["sourceSha256"],stage["learningPolicy"],
                stage["toolPolicy"],stage["executionPolicy"])
        }else{String::new()};
        format!("{directive}\n{meta_directive}\n{recursive_directive}\n{principle_directive}\n{actor_directive}\n{peer_context}\n{mutual_context}\n{factorial_context}\n{stage_context}\n{frontier_context}\n{}",job["input"]["prompt"].as_str().unwrap_or(""))
    }else{job["input"]["prompt"].as_str().unwrap_or("").to_string()};
    let prepared = fit_worker_prompt(
        &system_prompt, &domain, &capability,
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
    // Record the audit cursor just before the real child tool-using model
    // turn. A reused session or thousands of unrelated audit events may
    // neither add credit nor hide earlier competing generation calls.
    job["rsiAuditStartRowid"]=json!(state.store.audit_rowid_cursor()
        .map_err(|e|format!("후손 모델 감사 시작점 조회 실패: {e}"))?);
    state.store.put("job",&job).ok_or("후손 모델 감사 시작점 보존 실패")?;
    let result = jobs::run_prompt(&context, &job).await;
    // Close the audit interval immediately after the real model/tool turn.
    // The persisted finite range is replayable after campaign restarts and
    // cannot accidentally incorporate a later user or descendant session.
    let mut finished=state.store.get("job",job_id)
        .ok_or("후손 모델 감사 종료 기록 없음")?;
    finished["rsiAuditEndRowid"]=json!(state.store.audit_rowid_cursor()
        .map_err(|e|format!("후손 모델 감사 종료점 조회 실패: {e}"))?);
    state.store.put("job",&finished).ok_or("후손 모델 감사 종료점 보존 실패")?;
    // Count actual provider-specific inference calls made by this
    // descendant, not the model's declaration or the plan's intent.
    if job["input"]["rsiModelEcologyPolicy"]["active"]==true {
        let finished=state.store.get("job",job_id)
            .ok_or("후손 모델 실제 호출 기록을 읽지 못했습니다.")?;
        let receipt=crate::engine::rsi_model_ecology::executed(&state,&finished);
        let mut updated=finished;
        updated["rsiModelEcologyExecuted"]=receipt;
        state.store.put("job",&updated).ok_or("후손 모델 호출 증거 저장 실패")?;
    }
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
