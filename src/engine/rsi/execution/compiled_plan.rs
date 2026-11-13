//! Running compiled descendant planner in the isolated child binary.
//! Owns plan serialization, independent evidence validation, and applying the
//! returned action to the next real worker. The campaign only coordinates.
use super::*;

/// A narrow planning execution boundary instead of passing an editable
/// AppState+generation JSON across multiple unrelated campaign phases.
pub(super) struct PlannerRun<'a> {
    pub(super) context: &'a JobContext,
    pub(super) parent_input: &'a Value,
    pub(super) child_input: &'a mut Value,
    pub(super) record: &'a mut Value,
    pub(super) commit: &'a str,
    pub(super) history: &'a [Value],
    pub(super) lab: &'a Path,
    pub(super) binary: &'a Path,
    pub(super) attempt: u64,
}

pub(super) async fn run(req:PlannerRun<'_>)->Result<(),String> {
    let PlannerRun {
        context,parent_input:input,child_input,record,commit,history,lab,binary,attempt
    }=req;
    let head=commit;
    let recorded_history=history;
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
                "input":child_input.clone()
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
            // A compiled policy's source and frozen-suite identity becomes
            // the exact contract that the worker must revalidate before
            // any model/tool call. Never author this on the old supervisor.
            child_input["rsiExperimentContract"]=plan["experimentContract"].clone();
            child_input["rsiExperimentOperator"]=plan["experimentOperator"].clone();
            crate::engine::rsi_evolve::contract::validate(&plan,child_input)?;
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
    Ok(())
}
