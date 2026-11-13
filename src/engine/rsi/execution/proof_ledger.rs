//! Immutable generation archival and single-source recursive completion
//! verdict. Future worker history only becomes visible after these writes.
use super::*;

pub(super) fn persist(
    context:&JobContext,record:&mut Value,input:&Value,
    current:&Value,mut generation:Value
)->Result<(),String>{
        // ARCHIVE BEFORE THE CAMPAIGN DECIDES TO STOP. A campaign can reach
        // its token/time/max-attempt limit immediately after this generation:
        // there may never be a next descendant planning/select() call.
        // Persist only an original, independently replayed neutral experiment;
        // the source branch, frozen plan and candidate-bound receipt are
        // checked again by rsi_archive::record_completed.
        let archive_receipt=crate::engine::rsi_archive::record_completed(
            &context.state,&generation,
            &current["input"]["rsiFrozenCases"],
            current["input"]["rsiSourceBranch"].as_str().unwrap_or(""))?;
        generation["archivePersistence"]=archive_receipt.clone();
        record["archiveRefreshObservation"]=archive_receipt;
        // Workers read this durable table, not the coordinator's in-memory
        // history. Publish only after research, method feedback, archive and
        // trial outcomes are complete so the next worker actually continues.
        context.state.store.put("rsi_generation", &generation)
            .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
        record["history"]
            .as_array_mut()
            .ok_or("세대 목록 손상")?
            .push(generation);
        // One source-replayed verifier owns BOTH the campaign stop decision
        // and every API completion field. The chain must actually return to
        // editing the inherited improvement method with downstream evidence.
        let closure=crate::engine::rsi::campaign_recursive_evidence(
            &context.state,record);
        record["recursiveClosure"]=closure.clone();
        record["capabilityDemonstrated"]=json!(closure["proven"]==true);
        record["systemCapabilityDemonstrated"]=json!(
            closure["proven"]==true
                && input["rsiSystemEvolution"]==true
                && crate::engine::rsi_system::system_capability_proven(
                    &context.state,&record["history"]));
        // Completed real descendants contribute evidence to NEXT-METHOD
        // productivity, never to an already frozen current generation.
        // The durable rsi_generation rows outlive campaigns and are replayed
        // again in the next rebuilt binary before any method preference.
        if let Some(profile)=crate::engine::rsi_meta_controller::comparison_profile(
            &current["input"]) {
            let all=context.state.store.all("rsi_generation");
            let observed=all.iter().filter(|g|
                g["sourceBranch"]==current["input"]["rsiSourceBranch"]
                && g["objectiveId"]==crate::engine::memory::stable_id(&[
                    current["input"]["rsiObjective"].as_str().unwrap_or("")
                ])
                && g["knowledgePartition"]==
                    research_scope(&current["input"]).partition_key
                && g["generatorProviderId"]==current["input"]["providerId"]
                && g["generatorModel"]==current["input"]["model"])
                .cloned().collect::<Vec<_>>();
            let comparison=crate::engine::rsi_successor_productivity::compare(
                &context.state,&observed,&profile,
                current["rsiDomain"].as_str().unwrap_or(""));
            record["methodSuccessorProductivity"]=json!({
                "source":comparison["source"],"status":comparison["status"],
                "selectionEligible":comparison["selectionEligible"],
                "winnerRevision":comparison["winnerRevision"],
                "matchedWins":comparison["matchedWins"],
                "comparedContexts":comparison["comparedContexts"],
                "causalSuperiorityProven":false
            });
        }
    Ok(())
}
