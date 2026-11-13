//! Supervisor-approved replay of the running recompiled descendant's
//! system plan. Keeps model-evolved strategy separate from the campaign's
//! immutable budget, adoption and independently evaluated source ledger.
use super::*;

pub(super) async fn plan_compiled_descendant(
    context:&JobContext,
    record:&mut Value,
    input:&Value,
    child_input:&mut Value,
    lab:&PathBuf,
    binary:&PathBuf,
    head:String,
    attempt:u64,
    history:&[Value],
)->Result<(),String>{
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
        if plan["systemBlueprint"]["generatorCommit"]!=head
            || plan["systemBlueprint"]["historyCount"].as_u64()!=Some(history.len() as u64)
            || plan["systemBlueprint"]["architecture"].as_array().is_none_or(|items|
                items.is_empty() || items.len()>64 || items.iter().any(|item|
                    item["role"].as_str().is_none_or(|role|role.is_empty() || role.len()>64)))
            || !plan["systemBlueprint"]["sourceTreeSha256"].as_str()
                .is_some_and(|hash|hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit()))
            || !plan["improvementDirective"].is_object()
            || plan["lineageCapsule"]["source"]!="descendant_curated_lineage_v1"
            || plan["lineageCapsule"]["historyCount"].as_u64()!=Some(history.len() as u64)
            || plan["systemBlueprint"]["lineageSha256"]!=plan["lineageCapsule"]["sha256"]
            || !plan["lineageCapsule"]["sha256"].as_str().is_some_and(|hash|
                hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())) {
            return Err("후손 시스템 자기모형·실패 학습·계보가 일치하지 않습니다.".into());
        }
        crate::engine::rsi_system::validate_plan_evidence(&context.state,&lab,&head,
            child_input["rsiIndexId"].as_str().unwrap_or(""),&plan)?;
        if !crate::engine::rsi_meta_controller::valid_policy(
            &plan["metaPolicy"],history.len())
            || (plan["architectureTarget"].is_object()
                && plan["metaPolicy"]["target"]!=plan["architectureTarget"]) {
            return Err("후손의 수정 가능한 메타 개선기 계획이 일치하지 않습니다.".into());
        }
        if !crate::engine::rsi_interaction::valid_graph(&plan["interactionGraph"])
            || plan["metaPolicy"]["interactionGraphSha256"]!=plan["interactionGraph"]["sha256"]
            || (plan["architectureTarget"].is_object()
                && !crate::engine::rsi_interaction::valid_portfolio(
                    &plan["coDesignPortfolio"],&plan["interactionGraph"],
                    plan["architectureFrontier"].as_array()
                        .ok_or("후손의 전체 소스 후보가 없습니다.")?))
            || (plan["architectureTarget"].is_null()
                && !plan["coDesignPortfolio"].is_null()) {
            return Err("후손의 상호 지능 개선 인과 가설·복합 설계가 일치하지 않습니다.".into());
        }
        if input["rsiAdaptiveApiRouting"]==true {
            if !crate::engine::rsi_api_route_learning::verify_replay(
                &context.state,&history) {
                return Err("API 경로 학습 출처의 호출 감사·평가 원본 불일치".into());
            }
            if !crate::engine::rsi_api_route_learning::approved_choice(
                &plan["apiRoutePolicy"],&input["rsiApprovedApiRoutes"])
                || plan["apiRoutePolicy"]["ownerOriginal"]
                    !=input["rsiOriginalApiRoute"]
                || plan["apiRoutePolicy"]["generatorCommit"]!=head {
                return Err("후손이 승인되지 않은 API 제공자·모델을 선택했습니다.".into());
            }
            let selection=&plan["apiRoutePolicy"]["chosen"];
            let provider_id=selection["providerId"].as_str().ok_or("API 제공자 없음")?;
            let model=selection["model"].as_str().ok_or("API 모델 없음")?;
            let provider=context.state.store.get("provider",provider_id)
                .ok_or("실행 모델 제공자가 연결 해제됐습니다.")?;
            let currently_registered=plan["apiRoutePolicy"]["chosen"]
                ==input["rsiOriginalApiRoute"]
                || provider["model"].as_str()==Some(model)
                || provider["rsiApprovedModels"].as_array().is_some_and(|models|
                    models.iter().any(|row|row.as_str()==Some(model)));
            if !currently_registered
                || !crate::engine::gateway::provider_eligible(&provider)
                || crate::engine::gateway::route_from_provider(
                    &context.state,&provider,Some(model)).is_err() {
                return Err("후손의 API 실행 모델이 현재 등록·사용 가능하지 않습니다.".into());
            }
        }else if plan["apiRoutePolicy"].is_object(){
            return Err("소유자 승인 없는 API 모델 재선택입니다.".into());
        }
        if plan["principleLearning"].is_object()
            && !crate::engine::rsi_principle_learning::verify_replay(
                &context.state,&history) {
            return Err("외부 감독자가 학습 근거 후보의 실제 독립 평가를 재검증하지 못했습니다.".into());
        }
        if plan["principleLearning"].is_object()
            && (!crate::engine::rsi_principle_learning::valid(
                &plan["principleLearning"],&history)
                || plan["metaPolicy"]["principleLearningSha256"]
                    !=plan["principleLearning"]["sha256"]) {
            return Err("후손의 개선 원리 학습 증거가 독립 평가 이력과 다릅니다.".into());
        }
        // The evaluator validates the typed, source-bound protocol but
        // never recomputes the descendant's mutable operator scoring rule.
        if !crate::engine::rsi_evolve::meta_evolution::valid(
            &plan["metaEvolutionProgram"],&history,
            plan["architectureFrontier"].as_array()
                .ok_or("진화 프로토콜에 소스 후보가 없습니다.")?,&head)
            || (plan["metaEvolutionProgram"].is_object()
                && (plan["metaPolicy"]["selectionMode"]
                        !="authored_recursive_evolution_protocol"
                    || plan["metaPolicy"]["evolutionProtocolRevision"]
                        !=plan["metaEvolutionProgram"]["revision"]
                    || plan["metaPolicy"]["target"]
                        !=plan["metaEvolutionProgram"]["target"])) {
            return Err("후손 진화 프로토콜의 출처·정책·실행 대상이 불일치합니다.".into());
        }
        if plan["openImprovementProgram"].is_object()
            && (!crate::engine::rsi_program_graph::valid(
                &plan["openImprovementProgram"],&history,
                plan["architectureFrontier"].as_array()
                    .ok_or("열린 후손 프로그램 소스 후보가 없습니다.")?,&head)
                || plan["metaPolicy"]["openImprovementRevision"]
                    !=plan["openImprovementProgram"]["revision"]
                || plan["metaPolicy"]["target"]
                    !=plan["openImprovementProgram"]["target"]) {
            return Err("후손의 열린 자기개선 프로그램 계보·목표가 일치하지 않습니다.".into());
        }
        if plan["reflexiveProgram"].is_object()
            && (!crate::engine::rsi_reflexive::valid(
                &plan["reflexiveProgram"],&history,
                plan["architectureFrontier"].as_array()
                    .ok_or("후손의 재귀 실험 소스 후보가 없습니다.")?,
                &head)
                || plan["metaPolicy"]["reflexiveProgramSha256"]
                    !=plan["reflexiveProgram"]["sha256"]
                || plan["metaPolicy"]["target"]
                    !=plan["reflexiveProgram"]["target"]
                || (plan["reflexiveProgram"]["phase"]=="execute_revised_experiment"
                    && (plan["reflexiveProgram"]["experimentPlanSha256"]
                        !=plan["factorialExperiment"]["sha256"]
                        || plan["reflexiveProgram"]["target"]
                            !=plan["factorialExperiment"]["selected"]["target"]
                        || plan["metaPolicy"]["revisedExperimentPlanSha256"]
                            !=plan["factorialExperiment"]["sha256"]))) {
            return Err("후손의 실제 재귀적 자기수정 계보·정책·소스가 다릅니다.".into());
        }
        if plan["factorialExperiment"].is_object()
            && !crate::engine::rsi_factorial::valid(
                &plan["factorialExperiment"],
                plan["architectureFrontier"].as_array()
                    .ok_or("후손의 실험 소스 후보가 없습니다.")?,
                &head,history.len(),&plan["interactionGraph"]) {
            return Err("후손의 분리 실험 계획이 현재 소스·동결 평가 증거와 일치하지 않습니다.".into());
        }
        if (plan["mutualProgram"].is_object()
            && plan["metaPolicy"]["intergenerationalGraphSha256"]
                !=plan["mutualProgram"]["transferGraph"]["sha256"])
            || !crate::engine::rsi_mutual_program::valid_program(
            &plan["mutualProgram"],plan["architectureFrontier"]
                .as_array().ok_or("후손 소스 인덱스 없음")?,
            &head,history.len(),&plan["mutualProgram"]["transferGraph"])
            || (plan["architectureTarget"].is_object()
                && matches!(plan["mutualProgram"]["phase"].as_str(),
                    Some("complement"|"return"))
                && (plan["mutualProgram"]["target"]!=plan["architectureTarget"]
                    || plan["metaPolicy"]["mutualProgramRevision"]!=plan["mutualProgram"]["revision"])) {
            return Err("후손의 역방향 지능 개선 프로그램이 현재 소스와 다릅니다.".into());
        }
        if plan["successorStage"].is_object() {
            let stage=&plan["successorStage"];
            if stage["source"]!="inherited_model_authored_improvement_program_v1"
                || plan["architectureTarget"]["inheritedProgramStage"]!=true
                || stage["path"]!=plan["architectureTarget"]["path"]
                || stage["sourceSha256"]!=plan["architectureTarget"]["sourceSha256"]
                || stage["role"]!=plan["architectureTarget"]["role"] {
                return Err("후손의 개선 프로그램 단계가 선택한 실제 소스와 다릅니다.".into());
            }
        }
        let frozen=crate::engine::rsi_profile::from_input(&child_input)?;
        crate::engine::rsi_system::validate_peer_references(
            &context.state,&plan["peerLineages"],frozen["sha256"].as_str().unwrap_or(""))?;
        // The resident supervisor replays the transfer from immutable
        // adopted job/method/case receipts. No fabricated controller flag
        // may select a method, and the worker must execute the EXACT target.
        let expected_transfer=crate::engine::rsi_system::method_transfer_target(
            &context.state,history,child_input);
        if plan["methodTransfer"]!=expected_transfer
            || (expected_transfer.is_object()
                && plan["domain"]!=expected_transfer["domain"])
            || (plan["feedbackRepairSelected"]==true && plan["domain"]!="improver") {
            return Err("후손의 방법 전이·피드백 목표가 실제 채택 기록과 다릅니다.".into());
        }
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
        // Matched old/new compiled policy intervention does not call a model.
        // The real worker always receives the CURRENT plan, never the shadow.
        let intervention=match matched_planner_intervention(
            context,&lab,&snapshot,&plan,
            record["generatorBinarySha256"].as_str().unwrap_or(""),
            &history).await {
            Ok(value)=>value,
            Err(reason)=>json!({
                "source":"matched_compiled_planner_intervention_v1",
                "status":"unavailable","reason":reason,"qualityProven":false
            })
        };
        child_input["rsiBehaviorCounterfactual"]=intervention.clone();
        if intervention.is_object() {
            context.state.store.audit("rsi.matched_compiled_planner",json!({
                "campaignId":context.job_id,"attempt":attempt,
                "previousCommit":intervention["previousGeneratorCommit"],
                "descendantCommit":head,"status":intervention["status"],
                "previousActionSha256":intervention["previousActionSha256"],
                "descendantActionSha256":intervention["descendantActionSha256"]
            }));
        }
        // The previous resident coordinator's cost-only suggestion is not
        // an owner constraint. Rebind it ONLY after independently checking
        // the compiled method transfer or the B-feedback meta-rewrite.
        if plan["methodTransfer"].is_object() || plan["feedbackRepairSelected"]==true {
            let superseded=child_input["rsiDecisionInput"].clone();
            child_input["rsiDecisionInput"]=json!({
                "basis":"verified_compiled_recursive_method_target",
                "targetDomain":domain,
                "supersededSupervisorTarget":superseded["targetDomain"],
                "supersededDecisionBasis":superseded["basis"],
                "methodTransfer":plan["methodTransfer"],
                "feedbackRepairSelected":plan["feedbackRepairSelected"],
                "sourceGeneratorCommit":head,
                "previousAcceptedGeneration":history.iter().rev()
                    .find(|g|g["accepted"]==true).map(|g|g["id"].clone())
            });
            context.state.store.audit("rsi.compiled_method_target_overrode_supervisor",json!({
                "campaignId":context.job_id,"attempt":attempt,
                "supersededDomain":superseded["targetDomain"],
                "compiledDomain":domain,"methodId":plan["methodTransfer"]["methodId"],
                "feedbackRepair":plan["feedbackRepairSelected"]
            }));
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
        child_input["rsiArchitectureFrontier"]=plan["architectureFrontier"].clone();
        child_input["rsiArchitectureTarget"]=plan["architectureTarget"].clone();
        child_input["rsiSystemBlueprint"]=plan["systemBlueprint"].clone();
        child_input["rsiImprovementDirective"]=plan["improvementDirective"].clone();
        child_input["rsiMethodTransfer"]=plan["methodTransfer"].clone();
        child_input["rsiFeedbackRepairSelected"]=plan["feedbackRepairSelected"].clone();
        if plan["methodTransfer"].is_object() {
            let objective=plan["methodTransfer"]["taskObjective"].as_str()
                .filter(|task|!task.trim().is_empty() && task.len()<=3000)
                .ok_or("실행 가능 개선법에 연결된 실제 후손 작업 목표가 없습니다.")?;
            child_input["rsiObjective"]=json!(objective);
            child_input["rsiRequestedMethodId"]=plan["methodTransfer"]["methodId"].clone();
            child_input["rsiMethodTransferTaskSource"]=json!(
                "compiled_adopted_procedure_applicability_contract");
        }
        child_input["rsiLineageCapsule"]=plan["lineageCapsule"].clone();
        child_input["rsiPeerLineages"]=plan["peerLineages"].clone();
        child_input["rsiComparativeMode"]=plan["comparativeMode"].clone();
        child_input["rsiMetaPolicy"]=plan["metaPolicy"].clone();
        child_input["rsiSuccessorStage"]=plan["successorStage"].clone();
        child_input["rsiInteractionGraph"]=plan["interactionGraph"].clone();
        child_input["rsiCoDesignPortfolio"]=plan["coDesignPortfolio"].clone();
        child_input["rsiFactorialExperiment"]=plan["factorialExperiment"].clone();
        child_input["rsiReflexiveProgram"]=plan["reflexiveProgram"].clone();
        child_input["rsiOpenImprovementProgram"]=plan["openImprovementProgram"].clone();
        child_input["rsiMetaEvolutionProgram"]=plan["metaEvolutionProgram"].clone();
        child_input["rsiPrincipleLearning"]=plan["principleLearning"].clone();
        child_input["rsiApiRoutePolicy"]=plan["apiRoutePolicy"].clone();
        if input["rsiAdaptiveApiRouting"]==true {
            child_input["providerId"]=plan["apiRoutePolicy"]["chosen"]["providerId"].clone();
            child_input["model"]=plan["apiRoutePolicy"]["chosen"]["model"].clone();
        }
        child_input["rsiMutualProgram"]=plan["mutualProgram"].clone();
        if !crate::engine::rsi_archive::valid_selection(
            &context.state,&plan["archiveSelection"],&head,
            &child_input["rsiFrozenCases"]) {
            return Err("후손 아카이브 후보의 영구 영수증·평가 원본 검증 실패".into());
        }
        if !plan["archiveSelection"].is_null() {
            let id=plan["archiveSelection"]["candidateId"].as_str()
                .ok_or("후손 보관소 후보 ID가 없습니다.")?;
            let candidate=context.state.store.get("rsi_candidate",id)
                .ok_or("후손 보관소 후보가 현재 기록에 없습니다.")?;
            if candidate["patchSha256"]!=plan["archiveSelection"]["patchSha256"]
                || candidate["baseCommit"]!=head
                || candidate["pairedComparisonComplete"]!=true {
                return Err("후손 보관소 선택의 실제 패치·출처가 맞지 않습니다.".into());
            }
        }
        child_input["rsiInheritedCandidateId"]=plan["archiveSelection"]["candidateId"].clone();
        child_input["rsiArchiveSelection"]=plan["archiveSelection"].clone();
        child_input["rsiEditPolicy"]=plan["editPolicy"].clone();
        child_input["rsiControllerPlan"]=plan.clone();
        context.state.store.audit("rsi.descendant_controller_plan",json!({
            "campaignId":context.job_id,"attempt":attempt,
            "generatorCommit":head,"domain":domain,
            "focusFiles":focus,"selection":plan["selection"]
        }));
    Ok(())
}
