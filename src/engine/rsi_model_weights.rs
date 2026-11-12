//! Opt-in LoRA weight learning from separately authorized training data.
//! A trained adapter is a candidate, NEVER independently validated quality.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs,io::Read,path::{Path,PathBuf},time::Duration};
use tokio::process::Command;

fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|c|c.is_ascii_hexdigit())}
fn sha_file(p:&Path)->Result<String,String>{
    let mut file=fs::File::open(p).map_err(|e|e.to_string())?;
    let mut hash=Sha256::new();
    let mut b=[0u8;65536];
    loop{
        let n=file.read(&mut b).map_err(|e|e.to_string())?;
        if n==0{break;}
        hash.update(&b[..n]);
    }
    Ok(format!("{:x}",hash.finalize()))
}
fn inside(p:&Path,root:&Path)->bool{p.starts_with(root)&&p!=root}
fn safe_filename(s:&str)->bool{
    (5..=96).contains(&s.len())&&s.ends_with(".json")&&
    !s.starts_with('.')&&s.bytes().all(|c|
        c.is_ascii_alphanumeric()||c==b'_'||c==b'-'||c==b'.')
}
pub(crate) fn approved(
    manifest:&Value,training_root:&Path,model_root:&Path
)->Result<(PathBuf,PathBuf),String>{
    if manifest["trainingApproved"]!=true{return Err("학습 데이터 승인 없음".into());}
    let data=fs::canonicalize(manifest["trainJsonl"].as_str()
        .ok_or("데이터 경로 없음")?).map_err(|e|e.to_string())?;
    let model=fs::canonicalize(manifest["baseModelDir"].as_str()
        .ok_or("기본 모델 경로 없음")?).map_err(|e|e.to_string())?;
    if !inside(&data,training_root)||!data.is_file()
        ||data.extension().and_then(|s|s.to_str())!=Some("jsonl"){
        return Err("학습 데이터가 승인된 자료 영역 밖에 있습니다.".into());
    }
    if !inside(&model,model_root)||!model.is_dir(){
        return Err("기본 모델이 로컬 관리 영역 밖에 있습니다.".into());
    }
    let actual_data_hash=sha_file(&data)?;
    if manifest["datasetSha256"].as_str().is_none_or(|s|!hex(s))
        ||manifest["datasetSha256"].as_str()!=Some(actual_data_hash.as_str()){
        return Err("학습 데이터 SHA-256 불일치".into());
    }
    let steps=manifest["maxSteps"].as_u64().unwrap_or(0);
    let rank=manifest["loraRank"].as_u64().unwrap_or(0);
    let context=manifest["maxLength"].as_u64().unwrap_or(0);
    let lr=manifest["learningRate"].as_f64().unwrap_or(0.0);
    if !(1..=3000).contains(&steps)||!(2..=64).contains(&rank)
        ||!(128..=8192).contains(&context)||!lr.is_finite()
        ||!(1e-7..=1e-3).contains(&lr){
        return Err("모델 학습 예산 또는 설정 범위 초과".into());
    }
    Ok((data,model))
}
pub(crate) async fn run_tool(
    state:&AppState,session_id:&str,workspace:&Path,args:&Value
)->Result<Value,String>{
    let session=state.store.get("session",session_id).ok_or("세션 없음")?;
    let id=session["rsiGenerationJobId"].as_str().ok_or("후손 ID 없음")?;
    let child=state.store.get("job",id).ok_or("후손 작업 없음")?;
    if session["rsiRun"]!=true||child["kind"]!="rsi_generation"
        ||child["status"]!="running"
        ||child["input"]["rsiAllowLocalAdapterTraining"]!=true{
        return Err("명시적으로 승인한 로컬 모델 학습 캠페인만 실행할 수 있습니다.".into());
    }
    let name=args["manifestName"].as_str().filter(|s|safe_filename(s))
        .ok_or("학습 명세 파일명 오류")?;
    let mode=args["mode"].as_str().unwrap_or("check");
    if mode!="check"&&mode!="train"{return Err("학습 실행 모드 오류".into());}
    let data_root=fs::canonicalize(state.config.data_dir.join("rsi-training-approved"))
        .map_err(|e|e.to_string())?;
    let model_root=fs::canonicalize(state.config.data_dir.join("models"))
        .map_err(|e|e.to_string())?;
    let manifest_path=fs::canonicalize(data_root.join(name)).map_err(|e|e.to_string())?;
    if !inside(&manifest_path,&data_root){return Err("학습 명세가 승인 경로 밖입니다.".into());}
    let manifest:Value=serde_json::from_slice(&fs::read(&manifest_path)
        .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let (dataset,model)=approved(&manifest,&data_root,&model_root)?;
    let work=fs::canonicalize(workspace).map_err(|e|e.to_string())?;
    let script=fs::canonicalize(work.join("scripts/sub4_adapter_train.py"))
        .map_err(|e|e.to_string())?;
    if !inside(&script,&work)||!script.is_file(){
        return Err("후손 학습 프로그램이 실제 소스에 없습니다.".into());
    }
    let script_hash=sha_file(&script)?;
    let destination=state.config.data_dir.join("rsi-adapter-candidates").join(id);
    if mode=="train" && destination.exists(){
        return Err("같은 후손의 모델 산출물을 덮어쓰지 않습니다.".into());
    }
    let mut cmd=Command::new("python3");
    cmd.kill_on_drop(true);
    cmd.arg(script).arg("--manifest").arg(manifest_path)
        .arg("--output").arg(&destination)
        .current_dir(work).env("PYTHONNOUSERSITE","1");
    if mode=="check"{cmd.arg("--check");}
    let limit=if mode=="check"{30}else{900};
    let result=tokio::time::timeout(Duration::from_secs(limit),cmd.output())
        .await.map_err(|_|"로컬 학습 시간 제한".to_string())?
        .map_err(|e|e.to_string())?;
    if !result.status.success(){
        return Err(String::from_utf8_lossy(&result.stderr)
            .chars().take(1000).collect());
    }
    let output=String::from_utf8_lossy(&result.stdout);
    let parsed=output.lines().rev().find_map(|line|
        line.strip_prefix("DOMINION_ADAPTER_RESULT=")
            .and_then(|s|serde_json::from_str::<Value>(s).ok()))
        .ok_or("구조화된 모델 학습 영수증 없음")?;
    if parsed["datasetSha256"]!=manifest["datasetSha256"]{
        return Err("학습 영수증의 데이터 출처가 다릅니다.".into());
    }
    let mut receipt=json!({
        "source":"owner_approved_local_lora_candidate_v1",
        "generationJobId":id,
        "generatorCommit":child["input"]["rsiGeneratorCommit"],
        "trainingMode":mode,"trainingProgramSha256":script_hash,
        "trainingDataSha256":sha_file(&dataset)?,
        "baseModel":model,"adapterCreated":false,
        "independentAdapterComparison":false,"qualityProven":false,
        "adopted":false
    });
    if mode=="train"{
        let file=parsed["artifact"].as_str().ok_or("가중치 산출물 경로 없음")?;
        if file!="adapter_model.safetensors"{return Err("가중치 파일 형식 오류".into());}
        let root=fs::canonicalize(destination).map_err(|e|e.to_string())?;
        let artifact=fs::canonicalize(root.join(file)).map_err(|e|e.to_string())?;
        let actual_adapter_hash=sha_file(&artifact)?;
        if !inside(&artifact,&root)
            ||parsed["adapterSha256"].as_str()!=Some(actual_adapter_hash.as_str()){
            return Err("로컬 LoRA 파일의 SHA-256 불일치".into());
        }
        receipt["adapterCreated"]=json!(true);
        receipt["adapterSha256"]=parsed["adapterSha256"].clone();
        receipt["candidateDirectory"]=json!(root);
        let _=state.store.put("rsi_adapter_candidate",&receipt);
    }
    state.store.audit("rsi.adapter_training",receipt.clone());
    Ok(receipt)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn invalid_training_approval_is_rejected(){
        assert!(approved(&json!({}),Path::new("/tmp"),Path::new("/tmp/models")).is_err());
    }
    #[test]fn manifest_name_cannot_traverse(){
        assert!(safe_filename("approved.json"));
        assert!(!safe_filename("../approved.json"));
    }
}



fn paired_gate(m:&Value,case_sha:&str)->Result<bool,String>{
    if m["source"]!="separate_parent_paired_lora_oracle_v1"
        ||m["caseSha256"].as_str()!=Some(case_sha)
        ||m["privateExamplesReturned"]!=false{
        return Err("평가기 출처 또는 비공개 사례 증거가 유효하지 않습니다.".into());
    }
    let n=m["measuredExamples"].as_u64().filter(|v|(8..=256).contains(v))
        .ok_or("독립 평가의 사례 수가 부족하거나 비정상적입니다.")?;
    let wins=m["individuallyImproved"].as_u64().filter(|v|*v<=n)
        .ok_or("쌍별 모델 개선 사례 수가 유효하지 않습니다.")?;
    let groups=m["domains"].as_object().filter(|v|v.len()>=2 && v.len()<=32)
        .ok_or("평가가 여러 지능 분야를 포함하지 않습니다.")?;
    if m["distinctDomains"].as_u64()!=Some(groups.len() as u64){
        return Err("분야 수와 실제 평가 계수가 다릅니다.".into());
    }
    let mean=|v:&Value,key:&str|v[key].as_f64()
        .filter(|value|value.is_finite()&&*value>=0.0&&*value<100.0);
    let before=mean(m,"baselineMeanLoss").ok_or("기준 모델 손실 오류")?;
    let after=mean(m,"candidateMeanLoss").ok_or("후보 모델 손실 오류")?;
    let mut counted=0u64;
    let mut weighted_before=0.0;
    let mut weighted_after=0.0;
    let mut no_regression=true;
    for row in groups.values(){
        let k=row["examples"].as_u64().filter(|v|*v>=2)
            .ok_or("분야별 비교 사례가 너무 적습니다.")?;
        let a=mean(row,"baselineMeanLoss").ok_or("분야 기준 손실 오류")?;
        let b=mean(row,"candidateMeanLoss").ok_or("분야 후보 손실 오류")?;
        counted=counted.saturating_add(k);
        weighted_before+=a*k as f64;
        weighted_after+=b*k as f64;
        if b>a+0.02{no_regression=false;}
    }
    if counted!=n
        ||(weighted_before/n as f64-before).abs()>0.000001
        ||(weighted_after/n as f64-after).abs()>0.000001{
        return Err("쌍 비교의 전체 수치와 분야별 수치가 일치하지 않습니다.".into());
    }
    Ok(before-after>=0.02
        && wins.saturating_mul(3)>=n.saturating_mul(2)
        && no_regression)
}

#[cfg(test)]
mod model_promotion_tests{
    use super::*;
    #[test]
    fn promotion_must_have_independent_evidence_and_exact_campaign(){
        let proof=json!({"source":"independent_parent_model_adapter_pair_v1",
            "status":"promoted","pairedModelImprovementMeasured":true,
            "campaignId":"campaign-a","generatorCommit":"a".repeat(40),
            "adapterSha256":"b".repeat(64),
            "privateCasesSha256":"c".repeat(64),
            "independentEvaluatorSha256":"d".repeat(64),
            "adoptionScope":"isolated_descendant_inference_only",
            "productionDeployed":false,
            "measurements":{"source":"separate_parent_paired_lora_oracle_v1"}});
        assert!(promotion_scope_valid(&proof,"campaign-a"));
        assert!(!promotion_scope_valid(&proof,"campaign-b"));
        let mut weak=proof.clone();
        weak["pairedModelImprovementMeasured"]=json!(false);
        assert!(!promotion_scope_valid(&weak,"campaign-a"));
    }
    fn evidence()->Value{
        json!({
            "source":"separate_parent_paired_lora_oracle_v1",
            "caseSha256":"a".repeat(64),"privateExamplesReturned":false,
            "measuredExamples":12,"distinctDomains":3,
            "individuallyImproved":10,
            "baselineMeanLoss":1.2,"candidateMeanLoss":0.9,
            "domains":{
                "learning":{"examples":4,"baselineMeanLoss":1.2,"candidateMeanLoss":0.9},
                "memory":{"examples":4,"baselineMeanLoss":1.2,"candidateMeanLoss":0.9},
                "tools":{"examples":4,"baselineMeanLoss":1.2,"candidateMeanLoss":0.9}
            }
        })
    }
    #[test]
    fn only_consistent_multidomain_paired_improvement_can_promote(){
        assert_eq!(paired_gate(&evidence(),&"a".repeat(64)).unwrap(),true);
    }
    #[test]
    fn any_domain_regression_disqualifies_the_candidate(){
        let mut m=evidence();
        m["domains"]["memory"]["candidateMeanLoss"]=json!(1.3);
        m["candidateMeanLoss"]=json!((0.9+1.3+0.9)/3.0);
        assert!(!paired_gate(&m,&"a".repeat(64)).unwrap());
    }
    #[test]
    fn inconsistent_aggregate_or_wrong_dataset_sha_fails_closed(){
        let mut m=evidence();
        m["measuredExamples"]=json!(11);
        assert!(paired_gate(&m,&"a".repeat(64)).is_err());
        assert!(paired_gate(&evidence(),&"b".repeat(64)).is_err());
    }
    #[test]
    fn a_single_domain_cannot_disguise_itself_as_two(){
        let mut m=evidence();
        m["domains"]=json!({"memory":{"examples":12,
            "baselineMeanLoss":1.2,"candidateMeanLoss":0.9}});
        assert!(paired_gate(&m,&"a".repeat(64)).is_err());
    }
}
// The parent coordinator calls this after a descendant has finished running.
// It is deliberately separate from the editable generation's source oracle.
// Private evaluation cases never enter the prompt or generation JSON.
pub(crate) async fn evaluate_candidate(
    state:&AppState,campaign_id:&str,generation:&Value,coordinator_source:&Path
)->Value{
    let candidate=&generation["modelAdapterCandidate"];
    if candidate["adapterCreated"]!=true || candidate["trainingMode"]!="train"
        || candidate["source"]!="owner_approved_local_lora_candidate_v1"
        || candidate["generationJobId"]!=generation["jobId"]
        || candidate["generatorCommit"]!=generation["generatorCommit"]{
        return Value::Null;
    }
    let report=async {
        let base=fs::canonicalize(candidate["baseModel"].as_str()
            .ok_or("기본 모델이 없습니다.")?).map_err(|e|e.to_string())?;
        let model_root=fs::canonicalize(state.config.data_dir.join("models"))
            .map_err(|e|e.to_string())?;
        if !inside(&base,&model_root)||!base.is_dir(){return Err("기본 모델 경로가 바뀌었습니다.".into());}
        let adapter=fs::canonicalize(candidate["candidateDirectory"].as_str()
            .ok_or("후보 어댑터가 없습니다.")?).map_err(|e|e.to_string())?;
        let adapter_root=fs::canonicalize(state.config.data_dir.join("rsi-adapter-candidates"))
            .map_err(|e|e.to_string())?;
        if !inside(&adapter,&adapter_root)||!adapter.is_dir(){return Err("후보 디렉터리 무효".into());}
        let expected=candidate["adapterSha256"].as_str().filter(|s|hex(s))
            .ok_or("후보 해시 없음")?;
        if sha_file(&adapter.join("adapter_model.safetensors"))?!=expected{
            return Err("평가 전 후보 모델 가중치 불일치".into());
        }
        let private_root=fs::canonicalize(state.config.data_dir.join("rsi-adapter-eval-private"))
            .map_err(|e|format!("독립 평가 자료가 준비되지 않았습니다: {e}"))?;
        let manifest:Value=serde_json::from_slice(&fs::read(private_root.join("manifest.json"))
            .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if manifest["independentEvaluationApproved"]!=true{
            return Err("독립 평가 승인 없음".into());
        }
        let dataset=fs::canonicalize(private_root.join("cases.jsonl"))
            .map_err(|e|e.to_string())?;
        if !inside(&dataset,&private_root)||!dataset.is_file(){
            return Err("독립 평가 자료 경로 오류".into());
        }
        let case_sha=manifest["casesSha256"].as_str().filter(|s|hex(s))
            .ok_or("독립 평가 사례 해시 없음")?;
        if sha_file(&dataset)?!=case_sha
            ||candidate["trainingDataSha256"].as_str()==Some(case_sha){
            return Err("평가 사례 해시 불일치 또는 학습 자료 재사용".into());
        }
        let trusted=fs::canonicalize(coordinator_source.join("scripts/sub4_adapter_eval.py"))
            .map_err(|e|format!("분리된 상위 실행기에 평가기가 없습니다: {e}"))?;
        let trusted_root=fs::canonicalize(coordinator_source)
            .map_err(|e|e.to_string())?;
        if !inside(&trusted,&trusted_root)||!trusted.is_file(){
            return Err("후보가 평가기 경로를 바꾸었습니다.".into());
        }
        let script_sha=sha_file(&trusted)?;
        let mut evaluator=Command::new("python3");
        evaluator.kill_on_drop(true);
        evaluator.arg(&trusted).arg("--base").arg(&base)
            .arg("--adapter").arg(&adapter).arg("--cases").arg(&dataset)
            .arg("--case-sha").arg(case_sha)
            .env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1");
        let result=tokio::time::timeout(Duration::from_secs(900),
            evaluator.output()).await
            .map_err(|_|"독립 모델 평가 시간이 초과되었습니다.".to_string())?
            .map_err(|e|e.to_string())?;
        if !result.status.success(){
            return Err(String::from_utf8_lossy(&result.stderr)
                .chars().take(500).collect::<String>());
        }
        let stdout=String::from_utf8_lossy(&result.stdout);
        let metrics=stdout.lines().rev().find_map(|line|line.strip_prefix(
            "DOMINION_ADAPTER_EVAL=").and_then(|s|serde_json::from_str::<Value>(s).ok()))
            .ok_or("평가기가 쌍 비교 영수증을 반환하지 않았습니다.")?;
        let promoted=paired_gate(&metrics,case_sha)?;
        Ok::<Value,String>(json!({
            "source":"independent_parent_model_adapter_pair_v1",
            "campaignId":campaign_id,"generationJobId":generation["jobId"],
            "candidateId":candidate["id"],"generatorCommit":generation["generatorCommit"],
            "baseModel":base,"candidateDirectory":adapter,"adapterSha256":expected,
            "privateCasesSha256":case_sha,"independentEvaluatorSha256":script_sha,
            "measurements":metrics,
            "pairedModelImprovementMeasured":promoted,
            "status":if promoted{"promoted"}else{"rejected"},
            "adoptionScope":"isolated_descendant_inference_only",
            "productionDeployed":false
        }))
    }.await;
    let receipt=match report {
        Ok(row)=>row,
        Err(error)=>json!({"source":"independent_parent_model_adapter_pair_v1",
            "campaignId":campaign_id,"generationJobId":generation["jobId"],
            "status":"inconclusive","error":error,
            "pairedModelImprovementMeasured":false,"productionDeployed":false})
    };
    state.store.audit("rsi.adapter_independent_comparison",receipt.clone());
    let _=state.store.put("rsi_adapter_evaluation",&receipt);
    if receipt["status"]=="promoted" {
        let mut selected=receipt.clone();
        selected["id"]=json!(format!("campaign-selected-adapter-{campaign_id}"));
        if state.store.put("rsi_promoted_adapter",&selected).is_none(){
            return json!({"status":"inconclusive","error":"어댑터 채택 영수증 저장 실패"});
        }
    }
    receipt
}

fn promotion_scope_valid(receipt:&Value,campaign:&str)->bool{
    receipt["source"]=="independent_parent_model_adapter_pair_v1"
        && receipt["status"]=="promoted"
        && receipt["pairedModelImprovementMeasured"]==true
        && receipt["campaignId"]==campaign
        && receipt["productionDeployed"]==false
        && receipt["adoptionScope"]=="isolated_descendant_inference_only"
        && receipt["generatorCommit"].as_str().is_some_and(|s|
            s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        && receipt["adapterSha256"].as_str().is_some_and(hex)
        && receipt["privateCasesSha256"].as_str().is_some_and(hex)
        && receipt["independentEvaluatorSha256"].as_str().is_some_and(hex)
        && receipt["measurements"]["source"]=="separate_parent_paired_lora_oracle_v1"
}
/// Only verified local weights from this exact RSI campaign are passed to
/// the next source-compiled descendant. Never auto-deploy to a live user.
pub(crate) fn selected_for_child(state:&AppState,campaign_id:&str)->Value{
    let id=format!("campaign-selected-adapter-{campaign_id}");
    let Some(receipt)=state.store.get("rsi_promoted_adapter",&id)
        else{return Value::Null};
    if !promotion_scope_valid(&receipt,campaign_id){return Value::Null;}
    // An adapter that improves token-level tests but repeatedly fails to
    // produce accepted source improvements should not monopolize all future
    // RSI generations. Revert to the prior provider after multiple measured
    // negative outcomes; this is conservative rollback, NOT causal blame.
    let observations=state.store.all("rsi_generation").into_iter()
        .filter(|g|g["campaignId"]==campaign_id
            && g["adaptedModelActuallyUsed"]["promotionId"]==receipt["id"]
            && g["adaptedModelActuallyUsed"]["adapterSha256"]==
                receipt["adapterSha256"]
            && g["adaptedModelActuallyUsed"]["actuallyUsedInGeneration"]==true
            && g["pairedMeasured"]==true).collect::<Vec<_>>();
    if observations.len()>=3 && observations.iter().all(|g|
        g["accepted"]!=true || g["pairedRevalidated"]!=true){
        return Value::Null;
    }
    let Some(location)=receipt["candidateDirectory"].as_str()
        else{return Value::Null};
    let Some(expected)=receipt["adapterSha256"].as_str().filter(|s|hex(s))
        else{return Value::Null};
    let root=match fs::canonicalize(state.config.data_dir.join("rsi-adapter-candidates")){
        Ok(x)=>x,Err(_)=>return Value::Null};
    let path=match fs::canonicalize(location){Ok(x)=>x,Err(_)=>return Value::Null};
    if !inside(&path,&root)||sha_file(&path.join("adapter_model.safetensors"))
        .ok().as_deref()!=Some(expected){return Value::Null;}
    receipt
}


/// An actual inference service is started from the adopted successor's
/// source tree, with the independently promoted adapter loaded into PEFT.
/// A path string or prompt directive is never counted as model usage.
pub(crate) struct AdaptedInference{
    pub provider_id:String,
    pub endpoint:String,
    pub evidence:Value,
    process:tokio::process::Child,
}
impl Drop for AdaptedInference{
    fn drop(&mut self){
        let _=self.process.start_kill();
    }
}
pub(crate) async fn launch(
    state:&AppState,job:&Value
)->Result<Option<AdaptedInference>,String>{
    let promoted=&job["input"]["rsiSelectedAdapter"];
    if !promoted.is_object(){return Ok(None);}
    let campaign=job["input"]["rsiCampaignId"].as_str()
        .ok_or("모델 계보의 캠페인 ID 없음")?;
    let current=selected_for_child(state,campaign);
    if current.is_null() || current["id"]!=promoted["id"]
        || current["adapterSha256"]!=promoted["adapterSha256"]
        || current["status"]!="promoted"
        || job["input"]["rsiAllowLocalAdapterTraining"]!=true {
        return Err("독립 평가에서 선택되지 않은 모델 어댑터입니다.".into());
    }
    let root=fs::canonicalize(job["input"]["rsiSourceDir"].as_str()
        .ok_or("개선 후손 소스가 없습니다.")?).map_err(|e|e.to_string())?;
    let script=fs::canonicalize(root.join("scripts/sub4_adapter_server.py"))
        .map_err(|e|e.to_string())?;
    if !inside(&script,&root)||!script.is_file(){
        return Err("승격된 모델용 후손 추론기 소스가 없습니다.".into());
    }
    let script_sha=sha_file(&script)?;
    let model=fs::canonicalize(promoted["baseModel"].as_str()
        .ok_or("로컬 기본 모델 경로가 없습니다.")?).map_err(|e|e.to_string())?;
    let model_root=fs::canonicalize(state.config.data_dir.join("models"))
        .map_err(|e|e.to_string())?;
    if !inside(&model,&model_root)||!model.is_dir(){
        return Err("후손의 기본 모델 출처가 유효하지 않습니다.".into());
    }
    let dir=fs::canonicalize(promoted["candidateDirectory"].as_str()
        .ok_or("LoRA 어댑터 디렉터리 없음")?).map_err(|e|e.to_string())?;
    let sha=promoted["adapterSha256"].as_str().filter(|s|hex(s))
        .ok_or("LoRA 해시가 올바르지 않습니다.")?;
    if sha_file(&dir.join("adapter_model.safetensors"))?!=sha{
        return Err("채택된 모델 가중치가 변경됐습니다.".into());
    }
    let listener=std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e|e.to_string())?;
    let port=listener.local_addr().map_err(|e|e.to_string())?.port();
    drop(listener);
    let mut process=Command::new("python3");
    process.arg(&script).arg("--base").arg(&model)
        .arg("--adapter").arg(&dir).arg("--sha").arg(sha)
        .arg("--port").arg(port.to_string())
        .env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1")
        .env("PYTHONNOUSERSITE","1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut process=process.spawn().map_err(|e|e.to_string())?;
    let endpoint=format!("http://127.0.0.1:{port}");
    let client=reqwest::Client::new();
    let mut ready=false;
    for _ in 0..120 {
        if process.try_wait().map_err(|e|e.to_string())?.is_some(){
            return Err("채택된 PEFT 추론 서버가 시작되지 못했습니다.".into());
        }
        if let Ok(resp)=client.get(format!("{endpoint}/health"))
            .timeout(Duration::from_secs(1)).send().await {
            if resp.status().is_success(){ready=true;break;}
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if !ready{return Err("LoRA 추론 서비스 준비 시간 제한".into());}
    let provider_id=format!("rsi-adapter-{}",job["id"].as_str()
        .ok_or("후손 ID 없음")?);
    // Never register this service globally: its sole owner is the active
    // source-editing descendant session, not provider discovery or other
    // user chats. Its loopback endpoint is passed only to that child job.
    let evidence=json!({
        "source":"running_adapted_descendant_inference_v1",
        "promotionId":promoted["id"],
        "campaignId":campaign,
        "adapterSha256":sha,"inferenceSourceSha256":script_sha,
        "modelWeightsVerifiedAtLaunch":true,
        "actualServerReady":true,"qualityProvenByThisCall":false
    });
    Ok(Some(AdaptedInference{provider_id,endpoint,evidence,process}))
}
