//! Compiled descendant execution: standalone plan subprocess and one durable generation.
//! Source-level policy lives in the child binary; the coordinator only authenticates identities.
use super::*;

/// A short-lived native planning subprocess runs before a paid model worker.
/// The executable identity and plan fields are checked by the coordinator.
pub async fn worker_plan(campaign_id:&str) -> Result<(),String> {
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let campaign=state.store.get("rsi_campaign",campaign_id).ok_or("계획할 캠페인이 없습니다.")?;
    let job=state.store.get("job",campaign_id).ok_or("캠페인 작업을 찾을 수 없습니다.")?;
    let binary_commit=option_env!("DOMINION_BUILD_COMMIT").ok_or("개선기 revision 식별자가 없습니다.")?;
    let plan=native_generation_plan(&state,&campaign,&job,binary_commit)?;
    println!("{plan}");
    Ok(())
}

/// A rebuilt descendant may select a better domain than its frozen parent.
/// Recompose its actual source evidence before the model executes; merely
/// changing rsiDomain would send instructions for the wrong implementation.
pub(super) fn reconcile_descendant_target(
    state:&AppState,job:&mut Value,history:&[Value],
    domain:&str,selection:&str,strategy:&Value,commit:&str
)->Result<bool,String>{
    let executed=generation_target_plan(domain,selection,strategy,commit);
    let prior=job["input"]["rsiTargetPlan"].clone();
    if !prior.is_object() || prior==executed {return Ok(false);}
    if prior["generatorCommit"]!=commit {
        return Err("rsi_target_plan_mismatch: 후손 바이너리 revision이 부모 계획과 다릅니다.".into());
    }
    if job["input"]["rsiSystemEvolution"]==true {
        // A fixed coordinator must not silently replace the compiled
        // descendant's independently authored target with its old ranking.
        return Err("rsi_descendant_ownership_mismatch: 후손의 자기설계 계획이 실행 정책과 달라 재검토해야 합니다.".into());
    }
    let base=job["input"]["rsiPromptBase"].as_str()
        .ok_or("rsi_target_plan_mismatch: 원본 사용자 지시문이 없어 재계획할 수 없습니다.")?.to_string();
    let inherited=job["input"]["rsiPromptInherited"].as_str()
        .ok_or("rsi_target_plan_mismatch: 이전 경험 문맥이 없어 재계획할 수 없습니다.")?.to_string();
    let research=job["input"]["rsiPromptResearch"].as_str()
        .ok_or("rsi_target_plan_mismatch: 이전 연구 문맥이 없어 재계획할 수 없습니다.")?.to_string();
    let source=job["input"]["rsiSourceDir"].as_str()
        .ok_or("rsi_target_plan_mismatch: 재계획할 소스 경로가 없습니다.")?.to_string();
    let indexed=crate::engine::code_index::get(state,Path::new(&source))
        .ok_or("rsi_target_plan_mismatch: 현재 소스 색인이 없습니다.")?;
    if indexed["id"]!=job["input"]["rsiIndexId"] {
        return Err("rsi_target_plan_mismatch: 부모 소스 색인과 후손 checkout이 다릅니다.".into());
    }
    let previous_focus=job["input"]["rsiFocusFiles"].clone();
    job["input"]["rsiTargetDomain"]=json!(domain);
    if job["input"]["rsiAutoFocus"]==true {
        let policy=strategy["id"].as_str()
            .and_then(|id|state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(state,"selfimprove.search"));
        let ranked=choose_focus_files(state,&job["input"],history,policy.as_ref());
        job["input"]["rsiFocusFiles"]=json!(ranked.iter()
            .filter_map(|row|row["path"].as_str()).collect::<Vec<_>>());
        job["input"]["rsiFocusSelection"]=json!({
            "source":"descendant_indexed_evidence",
            "candidates":ranked,"domain":domain,
            "sourceIndexId":indexed["id"],
            "policyRevision":policy.as_ref().map(|item|item["revision"].clone())
        });
    }
    let focus=focus_file_context(state,&json!({"labDir":source}),&job["input"]);
    let available=job["input"]["rsiRemainingCampaignTokens"].as_u64();
    let (prompt,fit)=fit_parent_generation_context(
        &base,&inherited,&research,&focus,available
    ).ok_or("rsi_token_budget: 재계획한 근거 문맥이 남은 예산에 들어가지 않습니다.")?;
    job["input"]["prompt"]=json!(prompt);
    job["input"]["rsiParentContextFit"]=fit.clone();
    job["input"]["rsiPromptContextChars"]=json!(prompt.chars().count());
    job["input"]["rsiFocusContextChars"]=json!(
        if fit["sourceExcerptDeferred"]==true {0}else{focus.chars().count()}
    );
    job["input"]["rsiTargetPlan"]=executed.clone();
    job["input"]["rsiDecisionInput"]=json!({
        "source":"descendant_authoritative_replan",
        "previous":job["input"]["rsiDecisionInput"],
        "nextDomain":domain,"indexId":indexed["id"]
    });
    let receipt=json!({
        "reason":"descendant_authoritative_selection",
        "coordinatorPlan":prior,"descendantPlan":executed,
        "priorFocusFiles":previous_focus,
        "descendantFocusFiles":job["input"]["rsiFocusFiles"],
        "sourceIndexId":indexed["id"],
        "generatorCommit":commit,
        "parentContextFit":fit
    });
    job["input"]["rsiTargetReplan"]=receipt.clone();
    job["rsiTargetReplan"]=receipt;
    Ok(true)
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
        .filter(|g| g["campaignId"] == json!(campaign))
        .collect::<Vec<_>>();
    history.sort_by_key(|g|g["attempt"].as_u64().unwrap_or(0));
    let objective = selection_objective(&job["input"]);
    let action_history=improvement_history(&state,&job["input"],&history,&campaign);
    // The actual newly compiled agent obeys its own source-based design
    // decision. Previous measured plans remain evidence, not higher-order
    // commands from a frozen resident coordinator.
    let system_owned=job["input"]["rsiSystemEvolution"]==true
        && job["input"]["rsiSystemAgenda"]["source"]==
            "compiled_descendant_self_controller_v1"
        && job["input"]["rsiSelectedDomain"].as_str()
            .is_some_and(|domain|DOMAINS.contains(&domain));
    let inherited=if job["input"]["rsiSystemEvolution"]==true
        && job["input"]["selectionPolicy"]=="automatic" {
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    let continuation=if system_owned||inherited.is_some(){None}
        else{research_continuation(&action_history)};
    let trial_id=if system_owned||inherited.is_some(){None}else{
        job["input"]["rsiTrialAlgorithmId"].as_str()
            .filter(|value|!value.is_empty()).map(str::to_string)
    };
    let selected_domain=if system_owned{
        job["input"]["rsiSelectedDomain"].as_str().map(str::to_string)
    }else{inherited.or_else(||job["input"]["rsiSelectedDomain"]
        .as_str().filter(|domain|DOMAINS.contains(domain))
        .map(str::to_string))};
    let commit=job["input"]["rsiGeneratorCommit"].as_str().unwrap_or("").to_string();
    let (domain, domain_selection, strategy) =
        select_generation_strategy(&state,
            if trial_id.is_some(){&history}else{&action_history},
            &objective, continuation, trial_id, selected_domain, &commit);
    // The improved generator must execute the same action it planned before
    // the model call. Reject an outdated or injected retry directive.
    let expected_directive=generation_improvement_directive(&job["input"],&action_history);
    if job["input"]["rsiImprovementDirective"].is_object()
        && job["input"]["rsiImprovementDirective"]!=expected_directive {
        return Err("rsi_improvement_directive_mismatch: 개선 행동과 세대 이력이 일치하지 않습니다.".into());
    }
    let directive=if job["input"]["rsiImprovementDirective"].is_object() {
        job["input"]["rsiImprovementDirective"].clone()
    } else {
        expected_directive
    };
    // Follow the rebuilt descendant's actual selection. The recomposed prompt
    // must refer to its files, not the parent's now-obsolete focus.
    if let Err(error)=reconcile_descendant_target(&state,&mut job,&action_history,
        &domain,domain_selection,&strategy,&commit) {
        job["status"]=json!("failed");
        job["error"]=json!(error);
        job["result"]=json!({"rsi":{"modelError":error,"phase":"descendant_replan"}});
        state.store.put("job",&job).ok_or("후손 세대 재계획 실패 기록 저장 실패")?;
        return Err(error);
    }
    job["rsiDomain"] = json!(domain);
    job["rsiDomainSelection"] = json!(domain_selection);
    job["rsiSearchStrategy"] = strategy.clone();
    job["rsiSelectionObjective"] = json!(objective);
    job["rsiDecisionInput"] = job["input"]["rsiDecisionInput"].clone();
    job["input"]["rsiDomainSelection"] = json!(domain_selection);
    job["input"]["rsiSearchStrategy"] = strategy;
    // Rebuild the entire architecture view in the actual descendant process;
    // a stale parent-side manifest cannot override the new revision.
    if job["input"]["rsiGeneratorCompetition"].is_object()
        &&!crate::engine::rsi_model_learning::competition_matches(
            &job["input"]["rsiGeneratorCompetition"],&job["input"]) {
        return Err("후손의 실제 API/로컬 모델과 재컴파일된 개선기 모델 실험 계획이 다릅니다.".into());
    }
    if job["input"]["rsiSystemEvolution"]==true {
        let source=job["input"]["rsiSourceDir"].as_str()
            .ok_or("후손 시스템의 실제 소스 경로가 없습니다.")?;
        let model=crate::engine::rsi_system::blueprint(Path::new(source),&commit,&history);
        if model!=job["input"]["rsiSystemBlueprint"] {
            return Err("후손의 실행 코드·시스템 자기모형·독립 계보가 불일치합니다.".into());
        }
        crate::engine::rsi_system::validate_system_agenda(
            &state,&model,&job["input"]["rsiSystemAgenda"],
            job["input"]["rsiIndexId"].as_str().unwrap_or(""),
            Path::new(source),&commit,history.len())?;
        let expected_snapshot=method_feedback::method_snapshot(&state,&history,&commit);
        let expected_feedback=method_feedback::downstream_feedback(&state,&history,&commit);
        if job["input"]["rsiMethodSnapshot"]!=expected_snapshot
            || job["input"]["rsiDownstreamFeedback"]!=expected_feedback
            || job["input"]["rsiNativePlan"]["methodSnapshot"]!=expected_snapshot
            || job["input"]["rsiNativePlan"]["downstreamFeedback"]!=expected_feedback {
            return Err("후손이 실행한 개선법 소스 또는 독립 측정 피드백이 원본과 다릅니다.".into());
        }
        let expected_meta=crate::engine::rsi_meta_controller::synthesize(
            &history,
            job["input"]["rsiSystemAgenda"]["frontier"].as_array()
                .map(Vec::as_slice).unwrap_or(&[]),
            DOMAINS,
            crate::engine::rsi_system::inherited_domain(&history).as_deref());
        if job["input"]["rsiMetaPolicy"]!=expected_meta {
            return Err("실제 후손의 메타 학습 정책과 상속된 계획이 다릅니다.".into());
        }
        let expected_factorial=crate::engine::rsi_causal_ecology::design(
            &history,job["input"]["rsiSystemAgenda"]["frontier"]
                .as_array().map(Vec::as_slice).unwrap_or(&[]),
            &expected_meta["reciprocalExperiment"],&commit);
        if job["input"]["rsiFactorialExperiment"]!=expected_factorial
            || job["input"]["rsiSystemAgenda"]["factorialExperiment"].is_object()
                && job["input"]["rsiSystemAgenda"]["factorialExperiment"]!=expected_factorial {
            return Err("새 컴파일된 후손의 동결 실험 요인 설계가 다릅니다.".into());
        }
        let expected_laboratory=crate::engine::rsi_recursive_laboratory::plan(
            &history,job["input"]["rsiSystemAgenda"]["frontier"]
                .as_array().map(Vec::as_slice).unwrap_or(&[]),
            &expected_factorial,&expected_meta,&commit);
        if job["input"]["rsiRecursiveLaboratory"]!=expected_laboratory
            || !crate::engine::rsi_recursive_laboratory::valid(
                &expected_laboratory,
                job["input"]["rsiSystemAgenda"]["frontier"]
                    .as_array().map(Vec::as_slice).unwrap_or(&[]),
                &expected_factorial,&expected_meta,&commit,history.len())
            || job["input"]["rsiSystemAgenda"]["recursiveLaboratory"].is_object()
                && job["input"]["rsiSystemAgenda"]["recursiveLaboratory"]!=expected_laboratory {
            return Err("현재 후손의 재귀 연구 프로그램과 실제 실행 계획이 다릅니다.".into());
        }
        let expected_principles=crate::engine::rsi_principle_learner::learn(
            &history,job["input"]["rsiSystemAgenda"]["frontier"]
                .as_array().map(Vec::as_slice).unwrap_or(&[]));
        let expected_curriculum=crate::engine::rsi_principle_learner::plan(
            &history,job["input"]["rsiSystemAgenda"]["frontier"]
                .as_array().map(Vec::as_slice).unwrap_or(&[]),
            &expected_principles,&commit);
        if job["input"]["rsiPrincipleModel"]!=expected_principles
            || job["input"]["rsiPrincipleCurriculum"]!=expected_curriculum
            || !crate::engine::rsi_principle_learner::valid(
                &expected_curriculum,&expected_principles,
                job["input"]["rsiSystemAgenda"]["frontier"]
                    .as_array().map(Vec::as_slice).unwrap_or(&[]),
                &commit,history.len())
            || job["input"]["rsiSystemAgenda"]["principleCurriculum"].is_object()
                && job["input"]["rsiSystemAgenda"]["principleCurriculum"]!=expected_curriculum {
            return Err("재컴파일된 후손의 개선 원리 모델과 능력개선 실행 계획이 다릅니다.".into());
        }
        let expected_growth=crate::engine::rsi_whole_agent::plan(
            &history,job["input"]["rsiSystemAgenda"]["frontier"]
                .as_array().map(Vec::as_slice).unwrap_or(&[]),&commit);
        if job["input"]["rsiWholeDominionGrowth"]!=expected_growth
            || !crate::engine::rsi_whole_agent::valid(
                &expected_growth,
                job["input"]["rsiSystemAgenda"]["frontier"]
                    .as_array().map(Vec::as_slice).unwrap_or(&[]),
                &commit,history.len())
            || job["input"]["rsiSystemAgenda"]["wholeDominionGrowth"].is_object()
                &&job["input"]["rsiSystemAgenda"]["wholeDominionGrowth"]!=expected_growth {
            return Err("차세대 도미니언의 전체 지능 실험 계획과 실제 실행 코드가 다릅니다.".into());
        }
        let profile=crate::engine::rsi_meta_controller::comparison_profile(&job["input"]);
        crate::engine::rsi_meta_controller::validate_peer_lineages(
            &state,&job["input"]["rsiPeerLineages"],profile.as_deref(),&campaign)?;
        let expected_hybrid=crate::engine::rsi_meta_controller::compositional_hypothesis(
            &job["input"]["rsiPeerLineages"],&expected_meta);
        if job["input"]["rsiCrossLineageHypothesis"]!=expected_hybrid {
            return Err("후손의 교차 계보 개선 설계와 독립 증거가 일치하지 않습니다.".into());
        }
        job["rsiMetaPolicyExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":commit,
            "metaRevision":expected_meta["revision"],
            "metaForecastTargetRole":expected_meta["target"]["role"],
            "actualExecutedRole":job["input"]["rsiSystemAgenda"]["target"]["role"],
            "forecast":expected_meta["forecast"],
            "methodSnapshot":expected_snapshot,
            "downstreamFeedback":expected_feedback
        });
        job["rsiSystemBlueprintVerified"]=json!(true);
        job["rsiSelfControllerVerified"]=json!(true);
        // An accepted model-authored research program becomes executable
        // inside THIS improved descendant, not a descriptive handoff note.
        if let Some(program)=crate::engine::rsi_system::inherited_program(&history){
            job["input"]["rsiExecutionMethod"]["actionProgram"]=program.clone();
            job["rsiInheritedMetaMethod"]=program;
        }
    }
    let focus = job["input"]["rsiFocusFiles"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(8).collect::<Vec<_>>().join(", ");
    let capability = if focus.is_empty() { domain.clone() } else { format!("{domain} · {focus}") };
    let available=job["input"]["rsiRemainingCampaignTokens"].as_u64();
    let system_prompt=if job["input"]["rsiSystemEvolution"]==true {
        let goal=job["input"]["rsiObjective"].as_str().unwrap_or("");
        let instruction=crate::engine::rsi_system::instruction(
            &job["input"]["rsiSystemBlueprint"],goal);
        let own_agenda=crate::engine::rsi_system::agenda_instruction(
            &job["input"]["rsiSystemAgenda"]);
        let meta=crate::engine::rsi_meta_controller::instruction(
            &job["input"]["rsiMetaPolicy"]);
        let causal=crate::engine::rsi_causal_ecology::instruction(
            &job["input"]["rsiFactorialExperiment"]);
        let scientific=crate::engine::rsi_recursive_laboratory::instruction(
            &job["input"]["rsiRecursiveLaboratory"]);
        let principle=crate::engine::rsi_principle_learner::instruction(
            &job["input"]["rsiPrincipleCurriculum"],
            &job["input"]["rsiPrincipleModel"]);
        let whole_growth=crate::engine::rsi_whole_agent::instruction(
            &job["input"]["rsiWholeDominionGrowth"]);
        let competition=&job["input"]["rsiGeneratorCompetition"];
        let model_experiment=if competition["source"]==
            "compiled_open_model_scaffold_competition_v1" {
            format!("[MODEL-AGNOSTIC AGENT EVOLUTION] Current API/local                 generator={} model={} decision={} incumbent={}                 comparedEvidence={}. The evaluation unit is the COMPILABLE                 DOMINION AGENT (model + harness + tools + memory + learner),                 not only trained weights. The externally configured API may                 generate source rewrites and entirely new improvement methods,                 but DO NOT claim it changed API parameters. Analyze why the                 current model and its scaffolding fail to improve next                 descendants, rewrite relevant source, and let the frozen paired                 evaluator independently measure gains. Future descendants may                 rewrite this model search policy in rsi_model_learning.rs.\n",
                competition["selectedProviderId"],competition["selectedModel"],
                competition["decision"],competition["previousProviderId"],
                competition["modelEvidenceSha256"])
        }else{String::new()};
        let hypothesis=&job["input"]["rsiMetaMethodHypothesis"];
        let hybrid=&job["input"]["rsiCrossLineageHypothesis"];
        let cross_lineage=job["input"]["rsiPeerLineages"]["references"].as_array()
            .into_iter().flatten().take(3).map(|row|format!(
                "other lineage roles={} hypothesis={} priorDesign={}",
                row["roles"],row["hypothesis"],row["designSummary"]))
            .collect::<Vec<_>>().join("\n");
        let verified_feedback=&job["input"]["rsiDownstreamFeedback"];
        let causal_feedback=if verified_feedback["source"]==
            "independent_downstream_feedback_for_method_rewrite_v1" {
            format!("\n[ORIGINAL PAIRED DESCENDANT FEEDBACK — SOURCE REWRITE TRIAL]\n                Your predecessor compiled method path={} sha={}.                 Its actual rebuilt task descendant improved domain={}                 with independently verified candidate={} pairedReceipt={}.                 This evidence was replayed from original generation jobs;                 now REVISE the exact method source, with a new falsifiable                 hypothesis for why B succeeded/failed. The frozen evaluator                 must separately accept the new method version.                 Feedback inheritance alone is NOT a causal ablation.\n",
                verified_feedback["methodPath"],verified_feedback["methodSourceSha256"],
                verified_feedback["taskDomain"],verified_feedback["taskCandidateId"],
                verified_feedback["taskEvaluationSha256"])
        }else{String::new()};
        let method=if job["rsiInheritedMetaMethod"].is_object(){
            format!("\nINHERITED EXECUTABLE SELF-IMPROVEMENT TRIAL: {}\n",
                job["rsiInheritedMetaMethod"]["sequence"])
        }else{String::new()};
        format!("{instruction}\n\n{own_agenda}\n\n{meta}\n\n{causal}\n\n{scientific}\n\n{principle}\n\n{whole_growth}\n\n{model_experiment}\n\n            EDITABLE IMPROVEMENT METHOD HYPOTHESIS: {hypothesis}\n\n            COMPATIBLE PEER-LINEAGE RESEARCH (references only):\n{cross_lineage}\n            SELF-COMPOSED CROSS-LINEAGE NEW DESIGN EXPERIMENT: {hybrid}            {method}{causal_feedback}\n\n{}",
            job["input"]["prompt"].as_str().unwrap_or(""))
    }else{job["input"]["prompt"].as_str().unwrap_or("").to_string()};
    // Source-edit procedures are inherited code-level intelligence. The
    // worker receives the actual learned stage program and its measured
    // source provenance, not a claim that method evolution succeeded.
    let genome=&job["input"]["rsiEditPolicy"];
    let archive=&job["input"]["rsiArchiveSelection"];
    let edit_inheritance=if genome["program"].is_array(){
        format!("\n[INHERITED EXECUTABLE IMPROVEMENT METHOD]\n            Program={} revision={} mutation={} independentRuleCount={}.             The live tool runtime enforces these stages against actual             source reads and tool receipts; verified gains or regressions             alone train the next program. The cross-campaign Pareto archive             selected={} restored={}. A neutral archived patch is only a             stepping stone; source adoption and better DOMINION behavior             still require the same independent paired evaluator.\n",
            genome["program"],genome["revision"],genome["mutation"],
            genome["rules"].as_array().map(Vec::len).unwrap_or(0),
            archive["candidateId"],job["input"]["rsiArchiveRestored"])
    }else{String::new()};
    let system_prompt=format!("{system_prompt}{edit_inheritance}");
    let (prompt,fit)=fit_worker_generation_context(
        &system_prompt,
        &domain,&capability,&history,&directive,available)?;
    job["rsiDomain"] = json!(domain);
    job["input"]["rsiDomain"]=json!(domain);
    job["input"]["prompt"]=json!(prompt);
    job["rsiWorkerContextFit"]=fit;
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

