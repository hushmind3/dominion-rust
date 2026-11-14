//! Opt-in local Qwen-Agent SDK adapter. The Apache-2.0 upstream SDK owns its
//! tool-calling loop, while Dominion retains durable jobs, owner opt-in,
//! workspace boundary validation and SHA-verified resulting artifacts.
//! Installation: pip install 'qwen-agent>=0.0.26,<0.1'. Never a default route.
use crate::{engine::jobs::JobContext,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::io::AsyncWriteExt;

fn local_server(raw:&str)->Result<String,String>{
    let url=reqwest::Url::parse(raw).map_err(|_|"로컬 모델 서버 URL이 잘못됐습니다.")?;
    if url.scheme()!="http" || url.username()!="" || url.password().is_some() ||
        !matches!(url.host_str(),Some("127.0.0.1"|"::1"|"[::1]")) {
        return Err("Qwen-Agent SDK 연결은 loopback HTTP 모델 서버에서만 허용합니다. 외부 유료 API는 사용할 수 없습니다.".into())
    }
    if url.port_or_known_default().is_none(){return Err("로컬 서버 포트가 없습니다.".into())}
    Ok(raw.trim_end_matches('/').to_string())
}
/// Shared API/worker admission. Client-supplied base URL is NEVER trusted.
/// Only real saved/enabled local providers and an explicit owner write approval.
pub(crate) fn authorize(state:&AppState,input:&Value)->Result<Value,String>{
    if input["approvedLocalWrite"]!=true{
        return Err("로컬 워크스페이스 파일 수정에 대한 명시적 승인이 필요합니다.".into())
    }
    let prompt=input["prompt"].as_str().filter(|x|!x.trim().is_empty()&&x.len()<=12_000)
        .ok_or("작업 지시문은 1~12000자여야 합니다.")?;
    let id=input["providerId"].as_str().filter(|x|!x.is_empty())
        .ok_or("저장된 로컬 모델 제공자 ID가 필요합니다.")?;
    let provider=state.store.get("provider",id)
        .filter(|p|p["enabled"]!=false)
        .ok_or("연결된 활성 모델 제공자를 찾을 수 없습니다.")?;
    let model=input["model"].as_str().filter(|s|!s.is_empty())
        .or_else(||provider["model"].as_str().filter(|s|!s.is_empty()))
        .ok_or("로컬 모델 이름이 필요합니다.")?;
    if model.len()>256{return Err("모델 이름이 너무 깁니다.".into())}
    let base=local_server(provider["baseUrl"].as_str()
        .ok_or("로컬 제공자 서버 URL이 없습니다.")?)?;
    let max_seconds=input["maxSeconds"].as_u64().unwrap_or(420).clamp(30,1_800);
    let max_edits=input["maxEdits"].as_u64().unwrap_or(12).clamp(1,40);
    Ok(json!({
        "prompt":prompt,"providerId":id,"model":model,"modelServer":base,
        "maxSeconds":max_seconds,"maxEdits":max_edits,"approvedLocalWrite":true,
        "sdk":"QwenLM/Qwen-Agent","sdkLicense":"Apache-2.0",
    }))
}
fn read_receipts(workspace:&Path,proofs:&Value)->Result<Vec<Value>,String>{
    let Some(records)=proofs.as_array().filter(|r|!r.is_empty()&&r.len()<=40)else{
        return Err("실제 파일 수정 증거가 없습니다.".into())
    };
    let mut verified=Vec::new();
    for proof in records {
        let relative=proof["path"].as_str().ok_or("수정 파일 경로가 없습니다.")?;
        let path=Path::new(relative);
        if path.is_absolute()||relative.contains('\\')||path.components().any(|piece|
            !matches!(piece,std::path::Component::Normal(_))){
            return Err("파일 수정 영수증에 잘못된 경로가 있습니다.".into())
        }
        let absolute=std::fs::canonicalize(workspace.join(path))
            .map_err(|_|"SDK 기록의 수정 파일이 사라졌습니다.")?;
        if !absolute.starts_with(workspace)||!absolute.is_file(){
            return Err("수정 파일이 승인된 작업공간을 벗어났습니다.".into())
        }
        let data=std::fs::read(&absolute).map_err(|e|e.to_string())?;
        if data.len()>100_000{return Err("SDK가 제한보다 큰 파일을 기록했습니다.".into())}
        let actual=format!("{:x}",Sha256::digest(&data));
        if proof["afterSha256"].as_str()!=Some(actual.as_str())||proof["changed"]!=true{
            return Err("SDK가 신고한 파일 해시와 실제 디스크가 다릅니다.".into())
        }
        verified.push(json!({
            "path":relative,"sha256":actual,"bytes":data.len(),
            "beforeSha256":proof["beforeSha256"],"tool":proof["tool"],
            "verification":"rust_disk_sha256",
        }));
    }
    Ok(verified)
}
pub async fn run(context:&JobContext,job:&Value)->Result<Value,String>{
    // Unknown external process outcome after crash: never auto-replay edits.
    if job["sdkState"]["inFlight"]==true {
        return Ok(json!({"status":"needs_review",
            "reason":"이전 Qwen-Agent SDK 실행 도중 중단되었습니다. 실제 파일 변경을 확인해야 합니다."}))
    }
    let state=&context.state;
    let input=authorize(state,&job["input"])?;
    let workdir=std::fs::canonicalize(&state.config.workspace)
        .map_err(|_|"작업공간 디렉터리가 없습니다.")?;
    if !workdir.is_dir(){return Err("작업공간이 디렉터리가 아닙니다.".into())}
    let script=std::env::var("DOMINION_QWEN_SDK_SCRIPT")
        .map(PathBuf::from).unwrap_or_else(|_|PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/agent_sdk/qwen_agent_runner.py"));
    if !script.is_file(){return Err("Qwen-Agent SDK 작업기 파일이 없습니다.".into())}
    let mut spec=input.clone();
    spec["workspace"]=json!(workdir.to_string_lossy().to_string());
    context.checkpoint(json!({"sdkState":{"inFlight":true,
        "sdk":"QwenLM/Qwen-Agent","startedAt":chrono::Utc::now().to_rfc3339(),
        "approvedLocalWrite":true,"model":input["model"],"providerId":input["providerId"]}}))?;
    let python=std::env::var("DOMINION_QWEN_SDK_PYTHON").unwrap_or_else(|_|{
        if cfg!(windows){"python".into()}else{"python3".into()}
    });
    let mut command=tokio::process::Command::new(python);
    command.arg(&script).current_dir(&workdir).kill_on_drop(true)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .env("PYTHONDONTWRITEBYTECODE","1");
    let mut child=command.spawn().map_err(|e|format!(
        "Qwen-Agent SDK Python을 시작할 수 없습니다. pip install qwen-agent 필요: {e}"))?;
    if let Some(mut stdin)=child.stdin.take(){
        stdin.write_all(format!("{}\n",spec).as_bytes()).await
            .map_err(|e|format!("SDK 입력 전송 실패: {e}"))?;
        drop(stdin);
    }
    let deadline=Duration::from_secs(input["maxSeconds"].as_u64().unwrap_or(420));
    let result=tokio::time::timeout(deadline,child.wait_with_output()).await;
    let output=match result {
        Ok(Ok(output))=>output,
        Ok(Err(error))=>return Ok(json!({"status":"needs_review",
            "reason":format!("SDK 실행 중 외부 프로세스 오류: {error}"),
            "sdk":"QwenLM/Qwen-Agent","modelUsage":Value::Null})),
        Err(_)=>return Ok(json!({"status":"needs_review",
            "reason":"로컬 모델 실행 제한시간 초과. 파일이 수정됐을 수 있으므로 재호출하지 않습니다.",
            "sdk":"QwenLM/Qwen-Agent","modelUsage":Value::Null})),
    };
    if output.stdout.len()>1_000_000 || output.stderr.len()>1_000_000{
        return Ok(json!({"status":"needs_review",
            "reason":"SDK 출력이 제한을 초과했습니다. 변경된 파일을 확인하세요."}))
    }
    let value:Value=match serde_json::from_slice(&output.stdout){
        Ok(value)=>value,
        Err(_)=>return Ok(json!({"status":"needs_review",
            "reason":"SDK가 올바른 JSON 결과를 남기지 않았습니다.",
            "sdkExitCode":output.status.code(),"modelUsage":Value::Null})),
    };
    if !output.status.success()||value["sdk"]!="QwenLM/Qwen-Agent"{
        return Ok(json!({"status":"needs_review",
            "reason":"SDK 실행 결과 또는 종료 상태를 검증하지 못했습니다.",
            "sdkExitCode":output.status.code(),"modelUsage":Value::Null}))
    }
    let receipts=match read_receipts(&workdir,&value["fileReceipts"]){
        Ok(receipts)=>receipts,
        Err(reason)=>return Ok(json!({"status":"needs_review","reason":reason,
            "sdk":"QwenLM/Qwen-Agent","sdkReportedStatus":value["status"],
            "modelUsage":Value::Null})),
    };
    let complete=value["status"]=="completed" && value["completedWithVerifiedWrites"]==true;
    let result=json!({
        "status":if complete{"completed"}else{"needs_review"},
        "sdk":"QwenLM/Qwen-Agent","sdkLicense":"Apache-2.0",
        "providerId":input["providerId"],"model":input["model"],
        "output":value["output"],"fileReceipts":receipts,
        "verifiedFileWrites":receipts.len(),
        "verifiedBy":"rust_disk_sha256_after_sdk",
        "independentTaskQualityVerified":false,
        "modelUsage":Value::Null,
        "providerBillingKrw":Value::Null,
        "reason":value["reason"],
    });
    context.checkpoint(json!({"sdkState":{"inFlight":false,
        "fileReceipts":receipts,"status":result["status"]}}))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state()->(AppState,PathBuf){
        let root=std::env::temp_dir().join(format!("sdk-local-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        (AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path())),root)
    }
    #[test]
    fn only_explicit_approved_local_provider_can_use_sdk(){
        let (state,root)=state();
        state.store.put("provider",&json!({"id":"local","baseUrl":"http://127.0.0.1:11434/v1",
            "model":"qwen3:4b","enabled":true}));
        assert!(authorize(&state,&json!({"providerId":"local","prompt":"edit x"})).is_err());
        let granted=json!({"providerId":"local","prompt":"edit x","approvedLocalWrite":true});
        let spec=authorize(&state,&granted).unwrap();
        assert_eq!(spec["model"],"qwen3:4b");
        assert_eq!(spec["modelServer"],"http://127.0.0.1:11434/v1");
        assert!(local_server("https://api.example.com/v1").is_err());
        assert!(local_server("http://192.168.0.1:11434/v1").is_err());
        assert!(local_server("http://user:pw@127.0.0.1/v1").is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn sdk_receipt_must_match_real_disk_sha_and_stay_in_workspace(){
        let (_state,root)=state();
        std::fs::write(root.join("x.txt"),"real content").unwrap();
        let hash=format!("{:x}",Sha256::digest(b"real content"));
        let proof=json!([{"path":"x.txt","afterSha256":hash,"changed":true,
            "beforeSha256":Value::Null,"tool":"workspace_write"}]);
        assert_eq!(read_receipts(&root,&proof).unwrap().len(),1);
        let wrong=json!([{"path":"x.txt","afterSha256":"false","changed":true}]);
        assert!(read_receipts(&root,&wrong).is_err());
        assert!(read_receipts(&root,&json!([{"path":"../x.txt","afterSha256":hash,"changed":true}])).is_err());
        let _=std::fs::remove_dir_all(root);
    }
}
