//! Cross-campaign comparison uses independently adopted source designs as
//! reference evidence, not transplanted patches or fabricated score labels.
//! A compatible evaluation profile and live source identity are prerequisites.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

pub(crate) fn profile(input:&Value)->Option<String>{
    let cases=input["rsiFrozenCases"].as_array().filter(|items|!items.is_empty())?;
    Some(format!("{:x}",Sha256::digest(serde_json::to_vec(cases).ok()?)))
}

fn verified_design(state:&AppState,generation:&Value)->Option<Value>{
    if generation["accepted"]!=true
        || generation["candidateOrigin"]!="model_driven_detached_worktree"
        || generation["systemDesign"]["source"]!="model_authored_system_design"
        || generation["systemDesign"]["generatorCommit"]!=generation["generatorCommit"]
        || generation["systemDesign"]["generationJobId"]!=generation["jobId"]{
        return None;
    }
    let id=generation["candidateId"].as_str()?;
    let candidate=state.store.get("rsi_candidate",id)?;
    if candidate["status"]!="verified"
        || candidate["evaluation"]["metrics"]["eligible"]!=true
        || candidate["baseCommit"]!=generation["generatorCommit"]
        || candidate["experimentAdoption"]["commit"]!=generation["acceptedCommit"]
        || candidate["experimentAdoption"]["generatorCommit"]!=generation["generatorCommit"]
        || candidate["experimentAdoption"]["productionDeployed"]!=false
        || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {return None;}
    let paths=generation["systemDesign"]["targetFiles"].as_array()
        .filter(|paths|!paths.is_empty())?;
    let candidate_paths=candidate["paths"].as_array()?;
    if !paths.iter().all(|p|p.as_str().is_some_and(|path|
        crate::engine::rsi::safe_path(path)
            && candidate_paths.iter().any(|row|row.as_str()==Some(path))
            && generation["acceptedSourceHashes"][path].as_str()
                .is_some_and(|h|h.len()==64 && h.bytes().all(|c|c.is_ascii_hexdigit())))) {
        return None;
    }
    Some(candidate)
}

fn reference(state:&AppState,record:&Value,index_id:&str,profile:&str)->Option<(i64,Value)>{
    let candidate=verified_design(state,record)?;
    let files=record["systemDesign"]["targetFiles"].as_array()?;
    let mut aligned=Vec::new();let mut identical=0u64;
    for item in files.iter().take(24){
        let path=item.as_str()?;
        let current=crate::engine::code_index::indexed_source_hash(state,index_id,path);
        if let Some(sha)=current {
            let is_same=record["acceptedSourceHashes"][path]==sha;
            if is_same{identical+=1;}
            aligned.push(json!({"path":path,"currentSha256":sha,
                "donorAcceptedSha256":record["acceptedSourceHashes"][path],
                "sameSource":is_same}));
        }
    }
    if aligned.is_empty(){return None;}
    let roles=if record["systemDesign"]["component"]=="system" {
        record["systemDesign"]["components"].as_array().into_iter().flatten()
            .filter_map(Value::as_str)
            .filter(|role|record["systemDesign"]["componentFiles"][*role]
                .as_array().is_some_and(|files|!files.is_empty() && files.iter().all(|file|
                    file.as_str().is_some_and(|path|
                        record["acceptedSourceHashes"][path].as_str()
                            .is_some_and(|sha|sha.len()==64
                                && sha.bytes().all(|b|b.is_ascii_hexdigit()))))))
            .collect::<Vec<_>>()
    }else{record["systemDesign"]["component"].as_str()
        .map(|value|vec![value]).unwrap_or_default()};
    if roles.is_empty(){return None;}
    let roles=json!(roles);
    let gain=candidate["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0);
    let row=json!({
        "source":"independently_measured_peer_design_v1",
        "comparisonProfileSha256":profile,
        "campaignId":record["campaignId"],"generationId":record["id"],
        "candidateId":candidate["id"],"generatorCommit":record["generatorCommit"],
        "acceptedCommit":record["acceptedCommit"],
        "patchSha256":candidate["patchSha256"],"roles":roles,
        "nextDomain":record["systemDesign"]["nextDomain"],
        "alignedFiles":aligned,"identicalSourceFiles":identical,
        "metaHoldoutGains":gain,
        "hypothesis":record["systemDesign"]["hypothesis"].as_str().unwrap_or("")
            .chars().take(400).collect::<String>(),
        "designSummary":record["systemDesign"]["design"].as_str().unwrap_or("")
            .chars().take(800).collect::<String>(),
        "patchImported":false,"qualityProvenForCurrentDescendant":false
    });
    Some(((gain.min(20)*30+identical.min(8)*3) as i64,row))
}

/// Read-only external lineage references selected by the newly rebuilt child.
/// No foreign patch may be adopted, and a suite mismatch is a hard exclusion.
pub(crate) fn peers(
    state:&AppState,campaign:&str,input:&Value,index_id:&str,
    preferred_role:Option<&str>,
)->Value{
    let digest=profile(input);
    let mut candidates=Vec::new();
    let mut seen=BTreeSet::<String>::new();
    if let Some(sha)=digest.as_deref(){
        for record in state.store.all("rsi_generation").into_iter().take(256){
            if record["campaignId"].as_str()==Some(campaign)
                || record["comparisonProfileSha256"].as_str()!=Some(sha){continue;}
            let Some((mut score,reference))=reference(state,&record,index_id,sha) else{continue};
            let key=reference["patchSha256"].as_str().unwrap_or("").to_string();
            if key.is_empty()||!seen.insert(key){continue;}
            if preferred_role.is_some_and(|role|reference["roles"].as_array()
                .is_some_and(|roles|roles.iter().any(|row|row==role))) {
                score+=1000;
            }
            candidates.push((score,reference));
        }
    }
    candidates.sort_by(|(a,left),(b,right)|
        b.cmp(a).then_with(||left["candidateId"].as_str()
            .cmp(&right["candidateId"].as_str())));
    json!({"source":"verified_peer_archive_v1",
        "comparisonProfileSha256":digest,
        "references":candidates.into_iter().take(4).map(|(_,row)|row).collect::<Vec<_>>(),
        "patchImported":false,"qualityProvenForCurrentDescendant":false})
}

/// A resident supervisor verifies immutable facts and original paired
/// receipts. It must not freeze the child's peer ranking algorithm.
pub(crate) fn validate(
    state:&AppState,peers:&Value,campaign:&str,input:&Value,index_id:&str
)->Result<(),String>{
    if peers["source"]!="verified_peer_archive_v1"
        || peers["comparisonProfileSha256"]!=json!(profile(input))
        || peers["patchImported"]!=false
        || peers["qualityProvenForCurrentDescendant"]!=false {
        return Err("타 계보 참고의 평가 조건 또는 증거 정책이 다릅니다.".into());
    }
    let rows=peers["references"].as_array().filter(|rows|rows.len()<=4)
        .ok_or("타 계보 참고는 최대 4개입니다.")?;
    let mut seen=BTreeSet::new();
    for row in rows {
        let id=row["candidateId"].as_str().ok_or("타 계보 후보 ID가 없습니다.")?;
        if !seen.insert(id)
            || row["campaignId"].as_str()==Some(campaign)
            || row["source"]!="independently_measured_peer_design_v1"
            || row["patchImported"]!=false
            || row["qualityProvenForCurrentDescendant"]!=false {
            return Err("중복된 계보 또는 패치 복제 요청은 허용하지 않습니다.".into());
        }
        let record_id=row["generationId"].as_str().ok_or("계보 기록이 없습니다.")?;
        let record=state.store.get("rsi_generation",record_id)
            .ok_or("원본 세대 평가가 없습니다.")?;
        if record["campaignId"]!=row["campaignId"]
            || record["candidateId"]!=id
            || record["comparisonProfileSha256"]!=peers["comparisonProfileSha256"]{
            return Err("다른 계보의 출처가 원본과 다릅니다.".into());
        }
        let profile_hash=profile(input).ok_or("공통 평가 사례 없음")?;
        let (_,expected)=reference(state,&record,index_id,&profile_hash)
            .ok_or("교차 계보의 독립 채택 또는 소스 해시가 무효입니다.")?;
        if expected!=*row{
            return Err("타 계보 참고의 현재 소스·원본 증거가 변했습니다.".into());
        }
    }
    Ok(())
}

pub(crate) fn instruction(peers:&Value)->String {
    let rows=peers["references"].as_array().into_iter().flatten()
        .take(3).map(|row|format!(
            "donor roles={} next domain={} verified hypothesis={} implementation idea={} aligned files={}",
            row["roles"],row["nextDomain"],row["hypothesis"],
            row["designSummary"],row["alignedFiles"]
        )).collect::<Vec<_>>();
    if rows.is_empty(){return String::new();}
    format!(
        "\n[CROSS-LINEAGE EVIDENCE, NOT AN INHERITED PATCH]\n\
        Independent predecessor experiments using exactly the same frozen case profile:\n{}\n\
        Contrast donor successes against YOUR own failed behavior. \
        Transfer a generalizable architecture or algorithm, not donor code \
        or brittle task-specific text. Author a fresh proposal and make the \
        actual Rust edits yourself; only your own independently paired \
        evaluation can establish improvement.\n",rows.join("\n"))
}
