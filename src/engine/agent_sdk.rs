//! Optional official GitHub Copilot SDK harness adapter.
//! Native Rust chat/tools remain the default. This is a *real* separate
//! SDK process, never an OpenAI-compatible completion pretending to be one.
//! Authentication is owned by the CLI or local Ollama; no keys enter job JSON.
use serde_json::{json,Value};
use std::{path::Path,process::Stdio,time::Duration};
use tokio::io::AsyncWriteExt;

pub fn validate(step:&Value,request:&Value,scheduled:bool)->Result<(),String>{
    if step["harness"].as_str().unwrap_or("native")!="copilot-sdk"{return Ok(());}
    if !matches!(step["sdkProvider"].as_str(),Some("ollama"|"github")){
        return Err("Copilot SDK 실행은 Ollama 또는 GitHub 계정 중 하나를 선택해야 합니다.".into());
    }
    if !step["model"].as_str().is_some_and(|s|!s.is_empty()&&s.len()<=128){
        return Err("Copilot SDK 모델 ID가 없습니다.".into());
    }
    let policy=step["sdkTools"].as_str().unwrap_or("read_only");
    if !matches!(policy,"read_only"|"trusted_full"){
        return Err("Copilot SDK 도구 권한 정책이 올바르지 않습니다.".into());
    }
    if policy=="trusted_full" {
        if scheduled{return Err("반복 자동화에 신뢰 도구 전체 승인은 허용하지 않습니다.".into());}
        if request["sdkToolsApproved"]!=true ||
            std::env::var("DOMINION_COPILOT_TRUSTED_TOOLS").as_deref()!=Ok("1") {
            return Err("신뢰 도구 전체 실행에는 사용자의 명시 승인과 서버 관리자의 별도 활성화가 필요합니다.".into());
        }
    }
    if step["sdkProvider"]=="github"&&!scheduled&&request["sdkCostApproved"]!=true{
        return Err("GitHub Copilot 모델 사용 전에 명시적인 비용 승인이 필요합니다.".into());
    }
    Ok(())
}

pub async fn run(step:&Value,prompt:&str,workspace:&Path)->Result<Value,String>{
    let executable=std::env::var("DOMINION_COPILOT_SDK_RUNNER")
        .map_err(|_|"Copilot SDK 실행기가 설치되지 않았습니다. DOMINION_COPILOT_SDK_RUNNER를 설정하세요.")?;
    if !Path::new(&executable).is_absolute(){
        return Err("Copilot SDK 실행기는 절대 경로로 지정해야 합니다.".into());
    }
    if prompt.is_empty()||prompt.len()>30_000{
        return Err("SDK 지시문 길이가 허용 범위를 벗어났습니다.".into());
    }
    let workspace=workspace.canonicalize()
        .map_err(|_|"SDK 작업공간 경로를 찾을 수 없습니다.")?;
    let request=json!({
        "protocolVersion":1,"prompt":prompt,
        "model":step["model"],"provider":step["sdkProvider"],
        "toolPolicy":step["sdkTools"].as_str().unwrap_or("read_only"),
        "workspace":workspace.to_string_lossy(),
        "timeoutSeconds":600
    });
    let mut command=tokio::process::Command::new(executable);
    command.stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).kill_on_drop(true);
    let mut child=command.spawn()
        .map_err(|error|format!("Copilot SDK 실행기를 열지 못했습니다: {error}"))?;
    let payload=serde_json::to_vec(&request).map_err(|e|e.to_string())?;
    if let Some(mut stdin)=child.stdin.take(){
        stdin.write_all(&payload).await.map_err(|e|format!("SDK 실행 요청 전송 실패: {e}"))?;
    }else{return Err("SDK 실행기 입력 파이프가 없습니다.".into());}
    // The CLI can run tools before a timeout or process failure. The parent
    // workflow stays inFlight until a verified terminal response is recorded.
    let output=tokio::time::timeout(Duration::from_secs(660),child.wait_with_output())
        .await.map_err(|_|"SDK 실행 시간이 초과됐습니다. 실행한 도구의 외부 효과를 확인해야 합니다.")?
        .map_err(|e|format!("SDK 프로세스 종료 상태를 확인할 수 없습니다: {e}"))?;
    if output.stdout.len()>1_048_576||output.stderr.len()>32_768 {
        return Err("SDK 실행기 응답 크기가 제한을 초과했습니다.".into());
    }
    if !output.status.success(){
        // CLI diagnostics may accidentally include secrets. Do not echo raw
        // subprocess stderr back to UI/audit.
        return Err(format!("Copilot SDK 실행 실패 (exit={:?}). 상세 상태는 로컬 실행기 로그에서 검토하세요.",
            output.status.code()));
    }
    parse_response(&output.stdout)
}
fn parse_response(bytes:&[u8])->Result<Value,String>{
    let value:Value=serde_json::from_slice(bytes)
        .map_err(|_|"SDK 실행기가 유효한 JSON 결과를 반환하지 않았습니다.")?;
    if value["protocolVersion"]!=1 || value["status"]!="completed"
        ||value["harness"]!="github-copilot-sdk" {
        return Err("SDK 실행기가 완료된 작업 증거를 반환하지 않았습니다.".into());
    }
    if !value["content"].as_str().is_some_and(|s|!s.trim().is_empty()&&s.len()<=100_000){
        return Err("SDK 응답이 비어 있거나 결과 크기 제한을 초과했습니다.".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn sdk_never_activates_or_spends_without_explicit_input(){
        let native=json!({"model":"any"});
        assert!(validate(&native,&json!({}),false).is_ok());
        let local=json!({"harness":"copilot-sdk","sdkProvider":"ollama","model":"qwen3:4b"});
        assert!(validate(&local,&json!({}),false).is_ok());
        let hosted=json!({"harness":"copilot-sdk","sdkProvider":"github","model":"auto"});
        assert!(validate(&hosted,&json!({}),false).is_err());
        assert!(validate(&hosted,&json!({"sdkCostApproved":true}),false).is_ok());
        let trusted=json!({"harness":"copilot-sdk","sdkProvider":"ollama",
            "model":"qwen3:4b","sdkTools":"trusted_full"});
        assert!(validate(&trusted,&json!({}),false).is_err());
        assert!(validate(&trusted,&json!({"sdkToolsApproved":true}),true).is_err());
        assert!(validate(&json!({"harness":"copilot-sdk","sdkProvider":"unknown",
            "model":"auto"}),&json!({}),false).is_err());
    }
    #[test]
    fn sdk_result_requires_genuine_typed_terminal_envelope(){
        let ok=br#"{"protocolVersion":1,"harness":"github-copilot-sdk",
            "status":"completed","content":"done","usage":{"source":"sdk_event"}}"#;
        assert_eq!(parse_response(ok).unwrap()["content"],"done");
        assert!(parse_response(br#"{"status":"completed","content":"fake"}"#).is_err());
        assert!(parse_response(br#"{"protocolVersion":1,"harness":"github-copilot-sdk",
            "status":"completed","content":""}"#).is_err());
        assert!(parse_response(b"not-json").is_err());
    }
}
