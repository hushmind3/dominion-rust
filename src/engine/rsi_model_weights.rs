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
