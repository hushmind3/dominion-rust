//! Compiled descendant worker entry, plan attestation and model tool orchestration.
use super::{
    AppState, Value, JobContext, Path, json,
    jobs, rsi, DOMAINS, allowed_trial_campaigns, trial_owned_by,
    focus_file_context, selection_objective, evolved_execution_action, execution_action_contract, research_continuation,
    pending_failed_patch_domain, pending_method_recovery_domain, select_generation_strategy, reconcile_descendant_target, compiled_system_generation_plan,
};

pub async fn plan_generation(path:&str)->Result<(),String>{
    let snapshot:Value=serde_json::from_slice(&std::fs::read(path)
        .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let commit=option_env!("DOMINION_BUILD_COMMIT")
        .ok_or("계획 생성기 바이너리의 커밋 식별자가 없습니다.")?;
    if snapshot["generatorCommit"]!=commit
        || snapshot["input"]["rsiGeneratorCommit"]!=commit
        || snapshot["input"]["rsiSystemEvolution"]!=true {
        return Err("소스 스냅샷과 실제 후손 바이너리가 다릅니다.".into());
    }
    let campaign_id=snapshot["campaignId"].as_str().ok_or("캠페인 ID 없음")?;
    let job=state.store.get("job",campaign_id).ok_or("캠페인 작업 없음")?;
    let campaign=state.store.get("rsi_campaign",campaign_id)
        .ok_or("연속 캠페인 계보 없음")?;
    if job["kind"]!="rsi_evolve" || job["status"]!="running"
        || job["cancelRequested"]==true
        || campaign["headCommit"]!=commit {
        return Err("정지되거나 다른 소스의 캠페인은 계획할 수 없습니다.".into());
    }
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]==campaign_id).collect::<Vec<_>>();
    history.sort_by_key(|row|row["attempt"].as_u64().unwrap_or(0));
    if snapshot["historyCount"].as_u64()!=Some(history.len() as u64)
        || snapshot["attempt"].as_u64()!=Some(history.len() as u64+1)
        || campaign["history"].as_array().map(Vec::len)!=Some(history.len()) {
        return Err("후손이 읽은 이전 세대의 실험 기록이 일치하지 않습니다.".into());
    }
    let plan=compiled_system_generation_plan(
        &state,&snapshot["input"],&history,commit)?;
    println!("{plan}");
    Ok(())
}

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
    // Own the campaign ID independently of the editable child job.
    // The compiled planner records additional runtime fields on job.
    let campaign_value=job["input"]["rsiCampaignId"].clone();
    let campaign=campaign_value.as_str().ok_or("자동 개선 ID 없음")?;
    if state.store.get("job", campaign).is_none_or(|p| {
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
    let continuation = research_continuation(&history);
    // Child workers independently validate trial ownership against the stored
    // campaign receipt. A stale coordinator plan cannot inject a sibling trial.
    let allowed_campaigns=state.store.get("rsi_campaign",campaign)
        .and_then(|record|state.store.get("job",campaign).map(|parent|
            allowed_trial_campaigns(&state,&parent["input"],&record,campaign)))
        .unwrap_or_else(||vec![campaign.to_string()]);
    let trial_id = job["input"]["rsiTrialAlgorithmId"]
        .as_str()
        .filter(|id|state.store.get("algorithm",id)
            .is_some_and(|algorithm|trial_owned_by(&algorithm,&allowed_campaigns)))
        .map(str::to_string);
    let commit=job["input"]["rsiGeneratorCommit"].as_str().unwrap_or("").to_string();
    let failed_patch_domain=pending_failed_patch_domain(
        &state,&job["input"],&commit);
    let repair_domain=failed_patch_domain.clone()
        .or_else(||pending_method_recovery_domain(&job["input"]));
    // Repair the real failed patch before running an unrelated queued
    // search-policy trial. That trial remains pending for a later generation.
    let selected_trial=if failed_patch_domain.is_some(){None}else{trial_id};
    let (domain, domain_selection, strategy) = if job["input"]["rsiSystemEvolution"]==true {
        let plan=job["input"]["rsiDescendantPlan"].clone();
        // Run the CURRENT successor binary's planner again inside the worker.
        // An improved meta/harness/search method may legitimately disagree
        // with its predecessor; it must agree with its OWN program and source.
        let regenerated=compiled_system_generation_plan(
            &state,&job["input"],&history,&commit)?;
        crate::engine::rsi_system::validate_plan_evidence(
            &state,Path::new(job["input"]["rsiSourceDir"].as_str()
                .ok_or("후손 시스템 소스 없음")?),
            &commit,job["input"]["rsiIndexId"].as_str().unwrap_or(""),
            &regenerated,&history)?;
        let domain=regenerated["domain"].as_str()
            .filter(|d|DOMAINS.contains(d))
            .ok_or("후손이 계획한 개선 분야가 유효하지 않습니다.")?;
        let selection=match regenerated["selection"].as_str(){
            Some("algorithm_active")=>"algorithm_active",
            Some("algorithm_trial")=>"algorithm_trial",
            Some("research_continuation")=>"research_continuation",
            _=>return Err("후손의 탐색 전략이 유효하지 않습니다.".into())
        };
        let strategy=regenerated["strategy"].clone();
        if plan!=regenerated
            || job["input"]["rsiMetaPolicy"]!=plan["metaPolicy"]
            || job["input"]["rsiMetaActionPolicy"]!=plan["metaActionPolicy"]
            || job["input"]["rsiPeerLineages"]!=plan["peerLineages"]
            || job["input"]["rsiMutualCurriculum"]!=plan["mutualCurriculum"]
            || job["input"]["rsiFactorialCurriculum"]!=plan["factorialCurriculum"]
            || job["input"]["rsiRecursiveDiscovery"]!=plan["recursiveDiscovery"]
            || job["input"]["rsiRecursiveScienceActive"]!=plan["recursiveScienceActive"]
            || job["input"]["rsiPrincipleProgramme"]!=plan["principleProgramme"]
            || job["input"]["rsiPrincipleActive"]!=plan["principleActive"]
            || job["input"]["rsiOpenProgramPlan"]!=plan["openProgramPlan"]
            || job["input"]["rsiArchiveSelection"]!=plan["archiveSelection"]
            || job["input"]["rsiEditPolicy"]!=plan["editPolicy"]
            || job["input"]["rsiInheritedCandidateId"]
                !=plan["archiveSelection"]["candidateId"]
            || job["input"]["rsiModelRoute"]!=plan["modelRoute"]
            || job["input"]["rsiSuccessorStage"]!=plan["successorStage"]
            || job["input"]["rsiSystemBlueprint"]!=plan["systemBlueprint"]
            || job["input"]["rsiImprovementDirective"]!=plan["improvementDirective"]
            || job["input"]["rsiTargetPlan"]!=plan["targetPlan"] {
            return Err("후손 계획·정책·계보와 자신의 재계산 결과가 다릅니다.".into());
        }
        if !crate::engine::rsi_archive::validate_selection(
            &state,&plan["archiveSelection"],&commit,
            &job["input"]["rsiFrozenCases"],
            job["input"]["rsiSourceBranch"].as_str().unwrap_or("")){
            return Err("후손이 계승한 독립 검증 아카이브 기록의 원본 결속이 다릅니다.".into());
        }
        crate::engine::rsi_meta_controller::validate_peer_lineages(
            &state,&plan["peerLineages"],
            crate::engine::rsi_meta_controller::comparison_profile(&job["input"]).as_deref(),
            campaign)?;
        job["rsiSystemBlueprintVerified"]=json!(true);
        job["rsiImprovementDirectiveVerified"]=json!(true);
        job["rsiMetaPolicyExecuted"]=json!(true);
        job["rsiOpenProgramExecuted"]=json!(
            plan["openProgramPlan"]["stage"].is_object()
            && plan["successorStage"]==plan["openProgramPlan"]["stage"]
            && job["input"]["rsiAutoFocus"]==true
            && plan["openProgramPlan"]["target"]["path"].as_str()
                .is_some_and(|path|plan["focusFiles"].as_array()
                    .is_some_and(|files|files.iter().any(|f|
                        f.as_str()==Some(path)))));
        job["rsiMutualProgrammeExecuted"]=json!(
            plan["mutualCurriculum"]["pair"].is_object());
        job["rsiFactorialPlanExecuted"]=json!(
            plan["factorialCurriculum"]["selected"].is_object());
        job["rsiRecursiveSciencePlanExecuted"]=json!(
            job["input"]["rsiRecursiveScienceActive"]==true
            && job["input"]["rsiAutoFocus"]==true
            && plan["recursiveDiscovery"]["target"].is_object()
            && plan["recursiveDiscovery"]["target"]["path"].as_str()
                .is_some_and(|path|plan["focusFiles"].as_array().is_some_and(
                    |files|files.iter().any(|row|row.as_str()==Some(path)))));
        state.store.audit("rsi.descendant_system_runtime",json!({
            "jobId":job_id,"generatorCommit":commit,
            "source":"same_compiled_descendant_recomputed_its_own_policy",
            "sourceTreeSha256":plan["systemBlueprint"]["sourceTreeSha256"],
            "architectureComponents":plan["systemBlueprint"]["architecture"]
                .as_array().map(Vec::len).unwrap_or(0),
            "decision":plan["improvementDirective"]["mode"],
            "metaPolicyRevision":plan["metaPolicy"]["revision"],
            "metaSource":plan["metaPolicy"]["metaMechanismSource"],
            "metaActionPolicy":plan["metaActionPolicy"],
            "mutualDevelopment":plan["metaPolicy"]["nextMutualRedesign"],
            "reciprocalCurriculum":plan["mutualCurriculum"]["revision"],
            "factorialCurriculum":plan["factorialCurriculum"]["sha256"],
            "factorialPhase":plan["factorialCurriculum"]["selected"]["phase"],
            "recursiveDiscoveryRevision":plan["recursiveDiscovery"]["revision"],
            "recursiveDiscoveryPhase":plan["recursiveDiscovery"]["phase"],
            "forecast":plan["metaPolicy"]["taskOutcomeForecast"],
            "rankingAuthority":"compiled_descendant_not_resident_supervisor"
        }));
        (domain.to_string(),selection,strategy)
    } else {
        select_generation_strategy(&state, &history, &objective,
            if repair_domain.is_some(){None}else{continuation},selected_trial,
            repair_domain,&commit)
    };
    if failed_patch_domain.is_some() {
        job["rsiRepairPinnedDomain"]=json!(domain);
        job["rsiRepairCandidateId"]=
            job["input"]["rsiFailedPatchRecovery"]["candidateId"].clone();
    }
    // Rebuilt descendants independently recompute the learned policy. Make
    // its executable edit decision available before they replan the target.
    let action=evolved_execution_action(&state,&job["input"],&history);
    job["rsiActionDirective"]=action.clone();
    job["input"]["rsiActionDirective"]=action.clone();
    if let Err(error)=reconcile_descendant_target(&state,&mut job,&history,
        &domain,domain_selection,&strategy,&commit) {
        // Preserve the stop reason in the durable worker record. A mismatched
        // plan must not leave a pending job or silently execute stale context.
        job["status"]=json!("failed");
        job["error"]=json!(error);
        job["result"]=json!({"rsi":{"modelError":error}});
        state.store.put("job",&job).ok_or("차세대 재계획 실패 기록 손실")?;
        return Err(error);
    }
    // A compiled successor must inherit the EXACT independently adopted
    // executable improvement-method source and then apply its CURRENT policy
    // to an actual source focus. This receipt describes an observation, not
    // a causal claim or an automatic improvement score.
    job["rsiMetaMethodInheritance"]=if job["input"]["rsiSystemEvolution"]==true {
        let independently_replayed=crate::engine::rsi_system::revalidated_method_history(
            &state,&history);
        crate::engine::rsi_system::inherited_method_receipt(
            &independently_replayed,
            &job["input"]["rsiSystemBlueprint"],
            &job["input"]["rsiMetaPolicy"],&commit,
            &job["input"]["rsiFocusFiles"])
    }else{Value::Null};
    job["input"]["rsiMetaMethodInheritance"]=
        job["rsiMetaMethodInheritance"].clone();
    if job["rsiMetaMethodInheritance"]["rebuildConfirmed"]==true{
        state.store.audit("rsi.compiled_inherited_meta_method",json!({
            "jobId":job_id,"generatorCommit":commit,
            "parent":job["rsiMetaMethodInheritance"]["parentAcceptedCommit"],
            "sourceChanges":job["rsiMetaMethodInheritance"]["sourceChanges"],
            "decisionDifference":job["rsiMetaMethodInheritance"]["observedPlanDifference"],
            "sourceFocused":job["rsiMetaMethodInheritance"]["chosenTargetActuallyFocused"],
            "causalQualityProof":false
        }));
    }
    // The next model call must use a receipt that this rebuilt descendant
    // can independently reproduce from the SAME frozen task lineage.
    let expected_objective=crate::engine::memory::stable_id(&[
        job["input"]["rsiObjective"].as_str().unwrap_or("")
    ]);
    if !crate::engine::rsi_model_ecology::valid_receipt(
        &state,&job["input"],&history,&commit,&domain,
        &expected_objective) {
        let error="실제 모델 호출 경로와 후손의 학습된 모델 선택 증거가 일치하지 않습니다.";
        job["status"]=json!("failed");
        job["error"]=json!(error);
        state.store.put("job",&job).ok_or("모델 선택 검증 기록 손실")?;
        return Err(error.into());
    }
    job["rsiModelRouteExecuted"]=json!(
        job["input"]["rsiModelRoute"]["source"]
            =="compiled_descendant_evidence_model_ecology_v1"
        && job["input"]["rsiModelRoute"]["selected"].is_object()
        && job["input"]["providerId"].as_str().is_some_and(|s|!s.is_empty())
        && job["input"]["model"].as_str().is_some_and(|s|!s.is_empty())
        && job["input"]["rsiModelRoute"]["selected"]["providerId"]
            ==job["input"]["providerId"]
        && job["input"]["rsiModelRoute"]["selected"]["model"]
            ==job["input"]["model"]);
    job["rsiDomain"] = json!(domain);
    job["rsiDomainSelection"] = json!(domain_selection);
    job["rsiSearchStrategy"] = strategy.clone();
    job["rsiSelectionObjective"] = json!(objective);
    job["rsiDecisionInput"] = job["input"]["rsiDecisionInput"].clone();
    job["input"]["rsiDomainSelection"] = json!(domain_selection);
    job["input"]["rsiSearchStrategy"] = strategy;
    let feedback = history
        .iter().rev()
        .take(8)
        .map(|row| {
            json!({"domain":row["domain"],"accepted":row["accepted"],
        "tokens":row["tokens"],"elapsedMs":row["elapsedMs"],"verdict":row["evaluation"]["verdict"],
        "measurements":row["evaluation"]["metrics"]["measurements"],"error":row["error"]})
        })
        .collect::<Vec<_>>();
    let feedback = json!(feedback)
        .to_string()
        .chars()
        .take(12_000)
        .collect::<String>();
    let focus = job["input"]["rsiFocusFiles"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(8).collect::<Vec<_>>().join(", ");
    let capability = if focus.is_empty() { domain.clone() } else { format!("{domain} · {focus}") };
    job["rsiDomain"] = json!(domain);
    job["input"]["rsiDomain"]=json!(domain);
    let action_prompt=if job["input"]["providerId"].as_str().is_some_and(|id|!id.is_empty())
        && job["input"]["model"].as_str().is_some_and(|id|!id.is_empty()) {
        format!("\n### 이번 세대의 실행 행동\n{}\n관측된 이전 세대 실패: {}\n",
            execution_action_contract(&action),action)
    } else {String::new()};
    let mut instructions=if job["input"]["rsiSystemEvolution"]==true {
        let objective=state.store.get("job",&campaign)
            .and_then(|parent|parent["input"]["rsiObjective"].as_str()
                .map(str::to_string)).unwrap_or_default();
        // A past regression supplied RESEARCH DIGEST as "CURRENT TARGET
        // SOURCE" instead of source bytes. Small models then spent the entire
        // 10-minute generation rediscovering code, never writing any Rust.
        // The compiled descendant must re-read the already-indexed ACTUAL
        // checkout and deliver line-numbered, source-hashed excerpts for its
        // own architecture target. Stale index paths are never trusted.
        let current_source=focus_file_context(&state,
            &json!({"labDir":job["input"]["rsiSourceDir"]}),&job["input"]);
        let research=job["input"]["rsiPromptResearch"].as_str().unwrap_or("")
            .chars().take(1100).collect::<String>();
        let meta_instruction=crate::engine::rsi_meta_controller::instruction(
            &job["input"]["rsiMetaPolicy"]);
        let model_route_instruction=if job["rsiModelRouteExecuted"]==true {
            format!("MODEL ECOLOGY EXECUTED: actualProvider={} actualModel={}                 selectedMode={} independentSameSourceEvidence={}                 externalAPIWeightsModified=false. This model was selected for                 REAL inference in this generation. A successful source patch                 does NOT demonstrate that its foundation-model weights improved.                 You may redesign the router in src/engine/rsi_model_ecology.rs                 and compare NEW configured models in later equal-profile trials.",
                job["input"]["providerId"],job["input"]["model"],
                job["input"]["rsiModelRoute"]["selectionMode"],
                job["input"]["rsiModelRoute"]["selectedEvidence"])
        }else{String::new()};
        let peer_instruction=crate::engine::rsi_meta_controller::peer_instruction(
            &job["input"]["rsiPeerLineages"],&job["input"]["rsiMetaPolicy"]);
        let mutual_instruction=if job["input"]["rsiFactorialCurriculum"]
            ["selected"]["isolationRequired"]==true {
            String::new()
        }else{
            crate::engine::rsi_mutual_evolution::instruction(
                &job["input"]["rsiMutualCurriculum"])
        };
        let factorial_instruction=crate::engine::rsi_causal_curriculum::instruction(
            &job["input"]["rsiFactorialCurriculum"]);
        let topology_instruction=crate::engine::rsi_open_topology::instruction(
            &job["input"]["rsiSystemBlueprint"]["openTopology"]);
        let discovery_instruction=if job["input"]["rsiRecursiveScienceActive"]==true
            && job["input"]["rsiAutoFocus"]==true{
            crate::engine::rsi_recursive_discovery::instruction(
                &job["input"]["rsiRecursiveDiscovery"])
        }else{String::new()};
        let principle_instruction=if job["input"]["rsiPrincipleActive"]==true
            && job["input"]["rsiAutoFocus"]==true {
            crate::engine::rsi_principle_lab::instruction(
                &job["input"]["rsiPrincipleProgramme"])
        }else{String::new()};
        let open_program_instruction=if job["rsiOpenProgramExecuted"]==true {
            crate::engine::rsi_open_program::instruction(
                &job["input"]["rsiOpenProgramPlan"])
        }else{String::new()};
        let target=job["input"]["rsiDescendantPlan"]["architectureTarget"].clone();
        let inherited_step=job["input"]["rsiSuccessorStage"].clone();
        let inherited_task=if let Some(objective)=inherited_step["objective"].as_str(){
            format!("\n\nADOPTED PREDECESSOR'S NEXT-SYSTEM DESIGN STAGE (NOT a proof): phase={} role={} domain={} path={} objective={}. Re-read the actual CURRENT source and design the implementation yourself; independent evaluation decides acceptance.",
                inherited_step["phase"],inherited_step["role"],
                inherited_step["domain"],inherited_step["path"],objective)
        }else{String::new()};
        job["rsiSourceContextDelivered"]=json!(!current_source.is_empty());
        state.store.audit("rsi.compiled_descendant_source_bound",json!({
            "jobId":job_id,"generatorCommit":commit,
            "architectureTarget":target,
            "sourceContextChars":current_source.chars().count(),
            "sourceHashGrounded":current_source.contains("sourceSha256:"),
            "qualityProven":false
        }));
        format!("{}\n\nNEXT SOURCE-EDIT ACTION: {}\n\nREAL PREDECESSOR RESULTS:\n{}\n\nTARGET COMPONENT IN MY COMPILED PLAN: {}\n\nCURRENT SOURCE WITH REAL LINE NUMBERS AND sourceSha256:\n{}\n\nPRIOR RESEARCH (not current source):\n{}\n\nYou have the current source now. Start with rsi_system_plan and perform a concrete workspace_replace or workspace_replace_function on the indicated file. Do not loop over code_search or workspace_read_range when the source excerpt is sufficient.",
            format!("{}\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",crate::engine::rsi_system::instruction(
                &job["input"]["rsiSystemBlueprint"],&objective),
                meta_instruction,peer_instruction,
                if job["input"]["rsiRecursiveScienceActive"]==true
                    && job["input"]["rsiAutoFocus"]==true
                    && job["input"]["rsiRecursiveDiscovery"]["target"].is_object(){
                    String::new()
                }else{mutual_instruction},
                if job["input"]["rsiRecursiveScienceActive"]==true
                    && job["input"]["rsiAutoFocus"]==true
                    && job["input"]["rsiRecursiveDiscovery"]["target"].is_object(){
                    String::new()
                }else{factorial_instruction},
                topology_instruction,discovery_instruction,
                principle_instruction,open_program_instruction,model_route_instruction),
            job["input"]["rsiImprovementDirective"],feedback,target,
            current_source,format!("{research}{inherited_task}"))
    }else{
        format!("{}{}\n이번 탐색 분야: {domain}. 사용자 목표와 연결된 능력: {capability}. 전체 제품 기능·화면·외부 연결도 개선 대상입니다. 과거 성공·실패와 측정값을 참고하되 관측되지 않은 성능 향상을 주장하지 마세요.\n이 실험 계보의 과거 결과: {}",
            job["input"]["prompt"].as_str().unwrap_or(""),action_prompt,feedback)
    };
    if job["rsiMetaMethodInheritance"]["rebuildConfirmed"]==true {
        instructions.push_str(&format!(
            "\n\nINHERITED EDITABLE METHOD OBSERVATION (not quality proof): {}. \
             This binary was rebuilt from an independently adopted meta-method \
             change. The child source target and edit focus are executable; \
             propose a further concrete change only from the CURRENT code and \
             independently evaluated results.",
            job["rsiMetaMethodInheritance"]
        ));
    }
    job["input"]["prompt"]=json!(instructions);
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
