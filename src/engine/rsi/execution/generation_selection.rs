//! Evaluate a real candidate through the frozen paired evaluator; only
//! this stage may request isolated experimental source adoption.
//! Source acceptance, source hashes, and campaign candidate receipts remain
//! under independent supervisor ownership, not model-authored meta policy.
use super::*;

pub(super) async fn evaluate_and_adopt_generation(
    context:&JobContext,record:&mut Value,input:&Value,current:&Value,
    lab:&PathBuf,head:&String,attempt:u64,child_id:&str,
    proposed_candidate:Option<&str>,previous_failed:Option<&str>,
    output_error:Option<&str>,generation:&mut Value,
)->Result<(Option<String>,u64),String>{
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
                        && design["generatorCommit"]==head.as_str()
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
            let worker_error = output_error;
            let child = &current["result"]["rsi"];
            if let Some(previous)=previous_failed {
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
        Ok((candidate_id.map(str::to_owned),waiting_before))
}
