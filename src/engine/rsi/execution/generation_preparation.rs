//! Prepare the actual next source-generation job from the campaign's frozen
//! inputs and historical evidence. The rebuilt descendant owns its plan;
//! this coordinator only stages source, research, and inherited constraints.
use super::*;

pub(super) async fn prepare_generation_input(
    context:&JobContext,record:&mut Value,input:&Value,lab:&PathBuf,
    binary:&PathBuf,head:&String,attempt:u64,
)->Result<Value,String>{
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
        Ok(child_input)
}
