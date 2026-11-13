//! Immutable cross-campaign evidence verifier. The candidate's planner can
//! rank or describe peer generations, but cannot rewrite this verifier.
use super::{design_implemented,design_roles};
use serde_json::{json,Value};
use std::collections::BTreeSet;

/// The old supervisor verifies archived comparison receipts, never the peer
/// agent's proposal text or source patch as an instruction.
pub(crate) fn validate_peer_references(
    state:&crate::state::AppState,reference_set:&Value,profile_sha:&str,
)->Result<(),String> {
    if reference_set["source"]!="cross_campaign_comparative_archive_v1"
        || reference_set["profileSha256"]!=profile_sha
        || reference_set["automaticPatchReuse"]!=false {
        return Err("교차 계보 출처·평가 프로필 오류".into());
    }
    let references=reference_set["references"].as_array()
        .filter(|rows|rows.len()<=4).ok_or("교차 계보 참조 수 오류")?;
    let mut seen=BTreeSet::new();
    for row in references {
        let id=row["candidateId"].as_str().ok_or("교차 계보 후보 ID 없음")?;
        if !seen.insert(id) || row["source"]!="independently_paired_peer_lineage_v1"
            || row["evaluationProfileSha256"]!=profile_sha
            || row["automaticPatchReuse"]!=false {
            return Err("중복·비검증 교차 계보 참조".into());
        }
        let generation_id=row["generationId"].as_str()
            .ok_or("교차 계보 세대 ID 없음")?;
        let original=state.store.get("rsi_generation",generation_id)
            .ok_or("교차 계보 세대 소실")?;
        let candidate=state.store.get("rsi_candidate",id).ok_or("교차 계보 후보 소실")?;
        let design=&original["systemDesign"];
        if original["campaignId"]!=row["campaignId"]
            || original["candidateId"]!=id
            || original["accepted"]!=true
            || original["evaluationProfileSha256"]!=profile_sha
            || original["evaluation"]["metrics"]["eligible"]!=true
            || design["source"]!="model_authored_system_design"
            || design["generatorCommit"]!=original["generatorCommit"]
            || design["generationJobId"]!=original["jobId"]
            || !design_implemented(design,candidate["paths"].as_array()
                .ok_or("교차 계보 실제 소스 파일 없음")?) {
            return Err("교차 계보 출처·후손 설계의 원본 기록 오류".into());
        }
        if candidate["status"]!="verified"
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || candidate["patchSha256"]!=row["patchSha256"]
            || candidate["baseCommit"]!=row["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=row["acceptedCommit"]
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {
            return Err("교차 계보 후보의 독립 채택 증거 오류".into());
        }
        // The reference is a model-visible hint. Its narrative and metrics must
        // also originate from the independently adopted source, not merely the
        // candidate ID and patch receipt. No unverified peer can rewrite the
        // descendant's next hypothesis through a forged reference summary.

        let expected_roles=design_roles(design);
        let hypothesis=design["hypothesis"].as_str().unwrap_or("")
            .chars().take(420).collect::<String>();
        let summary=design["design"].as_str().unwrap_or("")
            .chars().take(700).collect::<String>();
        let aligned=row["alignedSourceFiles"].as_array()
            .filter(|paths|!paths.is_empty() && paths.len()<=24)
            .ok_or("교차 계보의 소스 참조 목록 오류")?;
        let mut seen_paths=BTreeSet::new();
        if aligned.iter().any(|path|path.as_str().is_none_or(|path|
            !seen_paths.insert(path.to_string())
                || !design["targetFiles"].as_array().is_some_and(|targets|
                    targets.iter().any(|target|target.as_str()==Some(path))))) {
            return Err("교차 계보 소스 참조가 원본 설계에 없습니다.".into());
        }
        let identical=row["identicalSourceFiles"].as_u64()
            .filter(|count|*count<=aligned.len() as u64)
            .ok_or("교차 계보 동일 소스 수 오류")?;
        if row["components"]!=json!(expected_roles)
            || row["nextDomain"]!=design["nextDomain"]
            || row["hypothesis"]!=json!(hypothesis)
            || row["designSummary"]!=json!(summary)
            || row["improvedHoldouts"].as_u64()
                !=Some(candidate["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0))
            || row["metaHoldoutGains"].as_u64()
                !=Some(candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0))
            || row["sourceCompatibility"]!=json!(
                if identical>0 {"some_identical"}else{"requires_reimplementation"})
            || row["qualityProvenForThisDescendant"]!=false {
            return Err("교차 계보의 설명·성과가 원본 독립 평가 영수증과 다릅니다.".into());
        }
    }
    Ok(())
}

