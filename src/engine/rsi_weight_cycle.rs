//! Real parameter-learning RSI cycle: authorized LoRA training ->
//! independent blind frozen holdout prediction -> measured candidate adoption
//! -> local inference provider -> subsequent compiled descendants.
//!
//! Source planning never promotes model weights. Training never receives
//! holdout labels. A trained file is a CANDIDATE until independent Rust
//! comparison finds improvement without regressions across two capabilities.
//! The trainer AND the evaluation protocol can be revised by later descendants;
//! owner authorization and separation of frozen labels remain external facts.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs,
    io::Read,path::{Path,PathBuf},process::Stdio,
    sync::{Mutex,OnceLock},time::Duration};
use tokio::process::{Child,Command};

fn file_sha(path:&Path)->Result<String,String>{
    let mut f=fs::File::open(path).map_err(|e|e.to_string())?;
    let mut hash=Sha256::new();
    let mut buf=[0u8;65536];
    loop {
        let count=f.read(&mut buf).map_err(|e|e.to_string())?;
        if count==0{break;}
        hash.update(&buf[..count]);
    }
    Ok(format!("{:x}",hash.finalize()))
}
fn sha(v:&Value)->bool {
    v.as_str().is_some_and(|s|s.len()==64
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn name(v:&str)->bool {
    !v.is_empty()&&v.len()<=110&&v.bytes()
        .all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')
}
fn manifest_file(v:&str)->bool{
    (6..=110).contains(&v.len())&&v.ends_with(".json")
        && v.bytes().all(|b|b.is_ascii_alphanumeric()
            ||b==b'-'||b==b'_'||b==b'.')
        && !v.starts_with('.')
}
fn inside(child:&Path,root:&Path)->bool{
    child.starts_with(root)&&child!=root
}
fn actual_path(value:&Value,root:&Path,file:bool)->Result<PathBuf,String>{
    let text=value.as_str().ok_or("승인한 데이터 경로가 없습니다.")?;
    let resolved=fs::canonicalize(text).map_err(|e|e.to_string())?;
    if !inside(&resolved,root)
        ||(file&&!resolved.is_file())
        ||(!file&&!resolved.is_dir()){
        return Err("승인 영역 밖의 데이터 또는 모델 경로".into());
    }
    Ok(resolved)
}
fn approval(manifest:&Value,root:&Path,models:&Path)
    ->Result<(PathBuf,PathBuf,PathBuf),String>{
    if manifest["trainingApproved"]!=true
        ||manifest["holdoutEvaluationApproved"]!=true{
        return Err("데이터 소유자의 명시적 학습·평가 승인 필요".into());
    }
    let train=actual_path(&manifest["trainJsonl"],root,true)?;
    let holdout=actual_path(&manifest["holdoutJsonl"],root,true)?;
    let model=actual_path(&manifest["baseModelDir"],models,false)?;
    if train==holdout
        ||train.extension().and_then(|x|x.to_str())!=Some("jsonl")
        ||holdout.extension().and_then(|x|x.to_str())!=Some("jsonl")
        ||!sha(&manifest["datasetSha256"])
        ||!sha(&manifest["holdoutSha256"])
        ||file_sha(&train)?!=manifest["datasetSha256"].as_str().unwrap_or("")
        ||file_sha(&holdout)?!=manifest["holdoutSha256"].as_str().unwrap_or(""){
        return Err("동결 평가 데이터 분리 또는 SHA-256 검증 실패".into());
    }
    let steps=manifest["maxSteps"].as_u64().unwrap_or(0);
    let rank=manifest["loraRank"].as_u64().unwrap_or(0);
    let length=manifest["maxLength"].as_u64().unwrap_or(0);
    let lr=manifest["learningRate"].as_f64().unwrap_or(0.0);
    if !(1..=2000).contains(&steps)
        ||!(2..=64).contains(&rank)
        ||!(128..=4096).contains(&length)
        ||!lr.is_finite()||!(1e-7..=1e-3).contains(&lr){
        return Err("모델 파라미터 학습 예산이 유효하지 않습니다.".into());
    }
    Ok((train,holdout,model))
}
fn eval_cases(train:&Path,holdout:&Path)->Result<Vec<Value>,String>{
    let training=fs::read_to_string(train).map_err(|e|e.to_string())?;
    let mut trained=BTreeSet::new();
    for (i,line) in training.lines().enumerate(){
        if i>=4000{return Err("학습 사례 한도 초과".into());}
        let row:Value=serde_json::from_str(line).map_err(|e|e.to_string())?;
        if row["split"]!="train"||row["approvedForTraining"]!=true{
            return Err("승인되지 않은 학습 사례".into());
        }
        let p=row["prompt"].as_str().ok_or("학습 프롬프트 없음")?;
        trained.insert(p.to_owned());
    }
    let holdout_text=fs::read_to_string(holdout).map_err(|e|e.to_string())?;
    let mut seen=BTreeSet::new();
    let mut domains=BTreeSet::new();
    let mut cases=Vec::new();
    for line in holdout_text.lines(){
        if cases.len()>=64{return Err("동결 평가 사례 한도 초과".into());}
        let row:Value=serde_json::from_str(line).map_err(|e|e.to_string())?;
        let id=row["id"].as_str().filter(|s|name(s))
            .ok_or("동결 평가 사례 ID 오류")?;
        let prompt=row["prompt"].as_str()
            .filter(|s|!s.is_empty()&&s.len()<=8192)
            .ok_or("동결 프롬프트 오류")?;
        let answer=row["expected"].as_str()
            .filter(|s|!s.is_empty()&&s.len()<=4096)
            .ok_or("동결 정답 오류")?;
        let domain=row["domain"].as_str()
            .filter(|s|name(s))
            .ok_or("동결 평가 지능 영역 누락")?;
        if row["split"]!="holdout"||!seen.insert(id.to_string())
            ||trained.contains(prompt)
            ||!answer.is_char_boundary(answer.len()){
            return Err("학습·동결 평가 누출 또는 사례 중복".into());
        }
        domains.insert(domain.to_string());
        cases.push(row);
    }
    if cases.len()<4||domains.len()<2{
        return Err("독립 평가에는 두 지능 영역에 걸친 4개 이상의 사례가 필요합니다.".into());
    }
    Ok(cases)
}
fn receipt(stdout:&[u8],prefix:&str)->Result<Value,String>{
    let output=String::from_utf8_lossy(stdout);
    output.lines().rev().find_map(|s|s.strip_prefix(prefix)
        .and_then(|data|serde_json::from_str(data).ok()))
        .ok_or("실제 실행기의 구조화된 학습·평가 영수증이 없습니다.".into())
}
async fn invoke(
    script:&Path,mode:&str,args:&[String],cwd:&Path,seconds:u64,prefix:&str
)->Result<Value,String>{
    let mut command=Command::new("python3");
    command.arg(script).arg("--mode").arg(mode).args(args)
        .current_dir(cwd).env("PYTHONNOUSERSITE","1")
        .env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1")
        .kill_on_drop(true);
    let output=tokio::time::timeout(
        Duration::from_secs(seconds),command.output())
        .await.map_err(|_|"로컬 모델 학습·평가 제한시간 초과".to_string())?
        .map_err(|e|e.to_string())?;
    if !output.status.success(){
        return Err(String::from_utf8_lossy(&output.stderr)
            .chars().take(500).collect());
    }
    receipt(&output.stdout,prefix)
}
fn path_pair(state:&AppState,manifest_name:&str)
    ->Result<(Value,PathBuf,PathBuf,PathBuf,String),String>{
    if !manifest_file(manifest_name){
        return Err("학습 명세 파일명이 유효하지 않습니다.".into());
    }
    let root=fs::canonicalize(state.config.data_dir.join("rsi-training-approved"))
        .map_err(|e|e.to_string())?;
    let models=fs::canonicalize(state.config.data_dir.join("models"))
        .map_err(|e|e.to_string())?;
    let manifest_path=fs::canonicalize(root.join(manifest_name))
        .map_err(|e|e.to_string())?;
    if !inside(&manifest_path,&root)||!manifest_path.is_file(){
        return Err("명세가 승인된 데이터 영역 밖입니다.".into());
    }
    let manifest:Value=serde_json::from_slice(&fs::read(&manifest_path)
        .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let (training,holdout,base)=approval(&manifest,&root,&models)?;
    let sha=file_sha(&manifest_path)?;
    Ok((manifest,training,holdout,base,sha))
}
fn acceptance(cases:&[Value],predictions:&Value)->Result<Value,String>{
    if predictions["source"]!="separate_blind_base_vs_lora_inference_v1"{
        return Err("다른 추론 실행기의 동결 평가 결과".into());
    }
    let rows=predictions["predictions"].as_array()
        .filter(|r|r.len()==cases.len())
        .ok_or("동결 평가 사례 수가 서로 다릅니다.")?;
    let mut ids=BTreeSet::new();
    let mut received=BTreeMap::new();
    for row in rows{
        let id=row["id"].as_str().ok_or("추론 ID 누락")?;
        if !ids.insert(id.to_owned())
            ||!row["baseline"].is_string()||!row["candidate"].is_string(){
            return Err("중복되거나 누락된 동결 평가 답변".into());
        }
        received.insert(id.to_owned(),row);
    }
    let mut wins=0u64;let mut losses=0u64;let mut base_hits=0u64;
    let mut model_hits=0u64;let mut domains=BTreeSet::new();
    let mut gains=BTreeSet::new();
    for case in cases{
        let domain=case["domain"].as_str().unwrap_or("");
        let expected=case["expected"].as_str().unwrap_or("").trim();
        let Some(out)=received.get(case["id"].as_str().unwrap_or("")) else{
            return Err("동결 사례에 대응하는 모델 출력을 찾지 못했습니다.".into());
        };
        let base=out["baseline"].as_str().unwrap_or("").trim()==expected;
        let updated=out["candidate"].as_str().unwrap_or("").trim()==expected;
        domains.insert(domain.to_string());
        if base{base_hits+=1;}
        if updated{model_hits+=1;}
        if !base&&updated{wins+=1;gains.insert(domain.to_owned());}
        if base&&!updated{losses+=1;}
    }
    let accepted=wins>=2&&losses==0&&gains.len()>=2
        &&domains.len()>=2&&model_hits>base_hits;
    Ok(json!({"source":"rust_blind_frozen_base_vs_adapter_assessor_v1",
        "pairedCases":cases.len(),"domains":domains.len(),
        "baselineCorrect":base_hits,"candidateCorrect":model_hits,
        "improvedCases":wins,"regressions":losses,
        "improvedDomains":gains.len(),"eligible":accepted,
        "adoptionAuthority":"separate_rust_holdout_assessor",
        "foundationModelWeightsChanged":false,
        "loraAdapterWeightsChanged":true,
        "unseenTaskGeneralizationProven":false}))
}
static SERVER:OnceLock<Mutex<Option<(String,Child)>>>=OnceLock::new();
fn server()->&'static Mutex<Option<(String,Child)>>{
    SERVER.get_or_init(||Mutex::new(None))
}
async fn activate(state:&AppState,id:&str,workspace:&Path,script:&Path,
    record:&Value)->Result<Value,String>{
    let adapter=fs::canonicalize(record["adapterDirectory"].as_str()
        .ok_or("채택한 어댑터 디렉터리가 없습니다.")?)
        .map_err(|e|e.to_string())?;
    let root=fs::canonicalize(state.config.data_dir.join("rsi-adapter-candidates"))
        .map_err(|e|e.to_string())?;
    if !inside(&adapter,&root)||!adapter.is_dir()
        ||file_sha(&adapter.join("adapter_model.safetensors"))?
            !=record["adapterSha256"].as_str().unwrap_or("")
        ||file_sha(script)?!=record["programSha256"].as_str().unwrap_or(""){
        return Err("어댑터 실제 가중치 또는 실행 원리 SHA-256이 바뀌었습니다.".into());
    }
    let base=fs::canonicalize(record["baseModelDir"].as_str()
        .ok_or("기준 모델 경로가 없습니다.")?)
        .map_err(|e|e.to_string())?;
    let models=fs::canonicalize(state.config.data_dir.join("models"))
        .map_err(|e|e.to_string())?;
    if !inside(&base,&models){return Err("기준 모델 경로가 관리 영역 밖입니다.".into());}
    let alias=format!("dominion-adapter-{id}");
    // Reserve a free loopback port rather than overwriting the host's
    // llama.cpp/MLX runtime or an unrelated local inference service.
    let binding=std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e|e.to_string())?;
    let port=binding.local_addr().map_err(|e|e.to_string())?.port();
    drop(binding);
    let old=state.store.get("rsi_adapter_active","selected");
    if let Some(active)=old {
        if let Some(previous)=active["providerId"].as_str(){
            if let Some(mut provider)=state.store.get("provider",previous){
                provider["enabled"]=json!(false);
                let _=state.store.put("provider",&provider);
            }
        }
        let mut stopped=active;
        stopped["active"]=json!(false);
        let _=state.store.put("rsi_adapter_active",&stopped);
    }
    {
        let mut guard=server().lock().map_err(|_|"모델 실행기 잠금 오류")?;
        if let Some((_,old))=guard.as_mut(){let _=old.start_kill();}
        *guard=None;
        let mut cmd=Command::new("python3");
        cmd.arg(script).args(["--mode","serve","--base"])
            .arg(base).arg("--adapter").arg(adapter)
            .arg("--alias").arg(&alias).arg("--port").arg(port.to_string())
            .current_dir(workspace)
            .env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1")
            .stdout(Stdio::null()).stderr(Stdio::null());
        let child=cmd.spawn().map_err(|e|e.to_string())?;
        *guard=Some((alias.clone(),child));
    }
    let client=reqwest::Client::new();
    for _ in 0..90{
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(response)=client.get(format!("http://127.0.0.1:{port}/v1/models"))
            .timeout(Duration::from_secs(2)).send().await{
            if response.status().is_success()&&response.json::<Value>().await
                .is_ok_and(|j|j["data"].as_array().is_some_and(|items|
                    items.iter().any(|m|m["id"]==alias))){
                let provider_id=format!("rsi-weights-{id}");
                let saved=state.store.put("provider",&json!({
                    "id":provider_id,"name":"DOMINION Verified LoRA",
                    "baseUrl":format!("http://127.0.0.1:{port}/v1"),
                    "kind":"openai","local":true,"enabled":true,
                    "toolMode":"compat","model":alias,
                    "contextTokens":4096,
                    "description":"Measured, independently accepted model-weight adapter"
                })).ok_or("모델 제공자 등록 실패")?;
                state.bus.record("provider",&saved);
                let active=json!({"id":"selected","adapterId":id,
                    "providerId":provider_id,"model":alias,
                    "baseUrl":format!("http://127.0.0.1:{port}/v1"),
                    "sha256":record["adapterSha256"],
                    "evaluationSha256":record["evaluationSha256"],
                    "source":"independently_adopted_live_model_weights_v1",
                    "active":true});
                state.store.put("rsi_adapter_active",&active)
                    .ok_or("실제 추론 모델 계승 기록 실패")?;
                state.store.audit("rsi.adapter_activated",active.clone());
                return Ok(active);
            }
        }
    }
    Err("채택된 모델의 실제 로컬 추론 실행기가 준비되지 않았습니다.".into())
}

/// RSI-tool entry point. The campaign OWNER must approve both training and
/// deployment. A descended model may redesign the trainer's source, but
/// cannot grant itself data authorization or promote a failed comparison.
pub(crate) async fn tool(
    state:&AppState,session_id:&str,workspace:&Path,args:&Value
)->Result<Value,String>{
    let session=state.store.get("session",session_id)
        .ok_or("모델 학습 세션 없음")?;
    let id=session["rsiGenerationJobId"].as_str()
        .filter(|value|name(value)).ok_or("독립 후손 작업 ID 없음")?;
    let job=state.store.get("job",id).ok_or("후손 작업 없음")?;
    if session["rsiRun"]!=true||job["kind"]!="rsi_generation"
        ||job["status"]!="running"
        ||job["input"]["rsiAllowLocalAdapterTraining"]!=true{
        return Err("소유자가 로컬 가중치 학습을 명시적으로 승인한 RSI 후손만 허용".into());
    }
    let mode=args["mode"].as_str().unwrap_or("check");
    if !matches!(mode,"check"|"train"|"assess"|"activate"){
        return Err("모델 가중치 학습 단계 오류".into());
    }
    let name=args["manifestName"].as_str()
        .filter(|value|manifest_file(value))
        .ok_or("모델 학습 승인 명세 이름 없음")?;
    let (manifest,train,holdout,base,manifest_hash)=path_pair(state,name)?;
    let root=fs::canonicalize(workspace).map_err(|e|e.to_string())?;
    let script=fs::canonicalize(root.join("src/engine/rsi_adapter_cycle.py"))
        .map_err(|e|e.to_string())?;
    if !inside(&script,&root)||!script.is_file(){
        return Err("후손의 실제 모델 가중치 학습 소스가 없습니다.".into());
    }
    let script_hash=file_sha(&script)?;
    let candidates=state.config.data_dir.join("rsi-adapter-candidates");
    fs::create_dir_all(&candidates).map_err(|e|e.to_string())?;
    let dir=candidates.join(id);
    let receipt_key=id.to_string();
    if mode=="check"||mode=="train"{
        let prepared=state.config.data_dir.join("rsi-adapter-manifests");
        fs::create_dir_all(&prepared).map_err(|e|e.to_string())?;
        // Do not give the mutable Python training program any frozen labels
        // or even the holdout path; train-only manifest is independently made.
        let sanitized=json!({
            "trainingApproved":true,"trainJsonl":train,
            "baseModelDir":base,"datasetSha256":manifest["datasetSha256"],
            "maxSteps":manifest["maxSteps"],"loraRank":manifest["loraRank"],
            "maxLength":manifest["maxLength"],
            "learningRate":manifest["learningRate"]
        });
        let cleaned=prepared.join(format!("{id}.json"));
        fs::write(&cleaned,serde_json::to_vec(&sanitized)
            .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if mode=="train" && (dir.exists()
            ||state.store.get("rsi_adapter_candidate",&receipt_key).is_some()){
            return Err("동일 후손의 가중치 후보 덮어쓰기 금지".into());
        }
        let result=invoke(&script,mode,&[
            "--manifest".into(),cleaned.to_string_lossy().to_string(),
            "--output".into(),dir.to_string_lossy().to_string()
        ],&root,if mode=="train"{1800}else{30},
            "DOMINION_ADAPTER_RESULT=").await?;
        if result["datasetSha256"]!=manifest["datasetSha256"]{
            return Err("훈련 프로그램 영수증과 승인 데이터 SHA 불일치".into());
        }
        if mode=="check"{
            return Ok(json!({"source":"approved_weight_learning_preflight_v1",
                "status":"checked","trainingApproved":true,
                "scriptSha256":script_hash,"manifestSha256":manifest_hash,
                "modelPath":base,"qualityProven":false}));
        }
        let weights=fs::canonicalize(dir.join("adapter_model.safetensors"))
            .map_err(|e|e.to_string())?;
        let canonical=fs::canonicalize(&dir).map_err(|e|e.to_string())?;
        if !inside(&weights,&canonical)
            ||result["adapterSha256"].as_str()!=Some(file_sha(&weights)?.as_str()){
            return Err("실제 PEFT 모델 가중치 체크포인트 SHA 불일치".into());
        }
        let record=json!({"id":receipt_key,
            "source":"real_peft_parameter_update_candidate_v1",
            "status":"trained_unproven",
            "generationJobId":id,
            "generatorCommit":job["input"]["rsiGeneratorCommit"],
            "manifestSha256":manifest_hash,
            "trainingDataSha256":manifest["datasetSha256"],
            "holdoutSha256":manifest["holdoutSha256"],
            "programSha256":script_hash,
            "adapterSha256":result["adapterSha256"],
            "adapterDirectory":canonical,"baseModelDir":base,
            "qualityProven":false,"evaluation":null,"adopted":false});
        state.store.put("rsi_adapter_candidate",&record)
            .ok_or("학습된 후보 가중치 영수증 저장 실패")?;
        state.store.audit("rsi.adapter_trained",record.clone());
        return Ok(record);
    }
    let candidate=state.store.get("rsi_adapter_candidate",&receipt_key)
        .ok_or("실제로 학습된 모델 가중치 후보가 없습니다.")?;
    if candidate["manifestSha256"]!=manifest_hash
        ||candidate["trainingDataSha256"]!=manifest["datasetSha256"]
        ||candidate["holdoutSha256"]!=manifest["holdoutSha256"]
        ||candidate["programSha256"]!=script_hash {
        return Err("학습 후보 생성 때의 데이터·학습 원리 출처와 다릅니다.".into());
    }
    if mode=="assess"{
        if candidate["status"]!="trained_unproven"
            ||state.store.get("rsi_adapter",&receipt_key).is_some(){
            return Err("가중치 후보는 한 번만 별도 평가합니다.".into());
        }
        let cases=eval_cases(&train,&holdout)?;
        let mut prompts=String::new();
        for item in &cases{
            prompts.push_str(&json!({"id":item["id"],"prompt":item["prompt"]}).to_string());
            prompts.push('\n');
        }
        let prompts_path=dir.join("blind-prompts.jsonl");
        fs::write(&prompts_path,prompts).map_err(|e|e.to_string())?;
        let predictions=invoke(&script,"predict",&[
            "--base".into(),base.to_string_lossy().to_string(),
            "--adapter".into(),dir.to_string_lossy().to_string(),
            "--prompts".into(),prompts_path.to_string_lossy().to_string(),
            "--max-new-tokens".into(),"64".into()
        ],&root,1200,"DOMINION_EVAL_RESULT=").await?;
        let evaluation=acceptance(&cases,&predictions)?;
        let mut summary=evaluation.clone();
        summary["sourceProgramSha256"]=json!(script_hash);
        summary["adapterSha256"]=candidate["adapterSha256"].clone();
        summary["trainDataSha256"]=manifest["datasetSha256"].clone();
        summary["holdoutSha256"]=manifest["holdoutSha256"].clone();
        summary["comparisonProfileSha256"]=json!(format!("{:x}",Sha256::digest(
            serde_json::to_vec(&json!({"holdout":manifest["holdoutSha256"],
                "generationMaxTokens":64,"metric":"exact_match_v1",
                "pairedBaseline":true}))
                .unwrap_or_default())));
        let adopted=summary["eligible"]==true;
        let evidence_hash=format!("{:x}",Sha256::digest(
            serde_json::to_vec(&summary).unwrap_or_default()));
        summary["evaluationSha256"]=json!(evidence_hash);
        let mut revised=candidate.clone();
        revised["evaluation"]=summary.clone();
        revised["status"]=json!(if adopted{"independently_adopted"}
            else{"independent_evaluation_failed"});
        revised["adopted"]=json!(adopted);
        state.store.put("rsi_adapter_candidate",&revised)
            .ok_or("독립 모델 평가 기록 저장 실패")?;
        if adopted{
            let accepted=json!({"id":receipt_key,"status":"accepted",
                "source":"independently_evaluated_parameter_evolution_v1",
                "generatorCommit":candidate["generatorCommit"],
                "adapterDirectory":candidate["adapterDirectory"],
                "baseModelDir":candidate["baseModelDir"],
                "adapterSha256":candidate["adapterSha256"],
                "programSha256":script_hash,
                "evaluation":summary,"evaluationSha256":evidence_hash,
                "qualityProvenForFrozenCases":true,
                "unseenTaskGeneralizationProven":false});
            state.store.put("rsi_adapter",&accepted)
                .ok_or("독립 채택 가중치 보존 실패")?;
        }
        state.store.audit("rsi.adapter_assessed",summary.clone());
        return Ok(summary);
    }
    if job["input"]["rsiAllowLocalAdapterDeployment"]!=true{
        return Err("채택한 모델의 실제 추론 사용에 대한 별도 소유자 승인이 필요합니다.".into());
    }
    let adopted=state.store.get("rsi_adapter",&receipt_key)
        .filter(|r|r["status"]=="accepted"
            && r["adapterSha256"]==candidate["adapterSha256"])
        .ok_or("독립 평가에서 채택된 모델 가중치만 사용할 수 있습니다.")?;
    activate(state,id,&root,&script,&adopted).await
}

#[cfg(test)]
mod tests{
    use super::*;
    fn case(id:&str,domain:&str)->Value{
        json!({"id":id,"domain":domain,"split":"holdout",
            "prompt":format!("question-{id}"),"expected":"right"})
    }
    #[test]
    fn blind_assessment_requires_new_gains_in_two_domains_and_no_regressions(){
        let cases=vec![case("a","memory"),case("b","tools"),
            case("c","memory"),case("d","tools")];
        let good=json!({"source":"separate_blind_base_vs_lora_inference_v1",
            "predictions":[
                {"id":"a","baseline":"wrong","candidate":"right"},
                {"id":"b","baseline":"wrong","candidate":"right"},
                {"id":"c","baseline":"right","candidate":"right"},
                {"id":"d","baseline":"right","candidate":"right"}]});
        assert_eq!(acceptance(&cases,&good).unwrap()["eligible"],true);
        let mut bad=good.clone();
        bad["predictions"][2]["candidate"]=json!("wrong");
        assert_eq!(acceptance(&cases,&bad).unwrap()["eligible"],false);
        bad=good;
        bad["predictions"][1]["id"]=json!("a");
        assert!(acceptance(&cases,&bad).is_err());
    }
    #[test]
    fn approval_and_path_traversal_are_not_evaluation_evidence(){
        assert!(!manifest_file("../secret.json"));
        assert!(manifest_file("approved.json"));
        assert!(!manifest_file(".hidden.json"));
        assert!(approval(&json!({}),Path::new("/tmp"),
            Path::new("/tmp/models")).is_err());
    }
}
