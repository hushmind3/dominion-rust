//! Optional sidecar: invoke GitHub's actual Copilot CLI harness through the
//! official github-copilot-sdk Rust crate. No mock model or fake tool results.
use github_copilot_sdk::{Client,ClientOptions,MessageOptions,MessageSource,SessionConfig};
use github_copilot_sdk::types::{ProviderConfig,SessionEvent};
use serde_json::{json,Value};
use std::{io::{Read,Write},time::Duration};

#[derive(Default)]
struct Meters{
    saw_usage:bool,
    lost_events:bool,
    prompt_tokens:u64,
    completion_tokens:u64,
    cache_read_tokens:u64,
    cache_write_tokens:u64,
    model_calls:u64,
    tool_starts:u64,
    tool_success:u64,
    tool_errors:u64,
}
impl Meters{
    fn observe(&mut self,event:&SessionEvent){
        match event.event_type.as_str(){
            "assistant.usage"=>{
                self.saw_usage=true;
                self.model_calls=self.model_calls.saturating_add(1);
                for (key,target) in [
                    ("inputTokens",&mut self.prompt_tokens),
                    ("outputTokens",&mut self.completion_tokens),
                    ("cacheReadTokens",&mut self.cache_read_tokens),
                    ("cacheWriteTokens",&mut self.cache_write_tokens)
                ]{
                    *target=target.saturating_add(event.data[key].as_u64().unwrap_or(0));
                }
            }
            "tool.execution_start"=>self.tool_starts=self.tool_starts.saturating_add(1),
            "tool.execution_complete"=>{
                if event.data["success"]==true {
                    self.tool_success=self.tool_success.saturating_add(1);
                }else{self.tool_errors=self.tool_errors.saturating_add(1);}
            }
            _=>{}
        }
    }
    fn into_json(self)->Value{
        // Input tokens already INCLUDE cached input. Do not double count.
        let complete=self.saw_usage&&!self.lost_events;
        json!({
            "source":"github-copilot-sdk:assistant.usage",
            "measured":complete,
            "eventLossDetected":self.lost_events,
            "calls":if complete{json!(self.model_calls)}else{Value::Null},
            "promptTokens":if complete{json!(self.prompt_tokens)}else{Value::Null},
            "completionTokens":if complete{json!(self.completion_tokens)}else{Value::Null},
            "totalTokens":if complete {
                json!(self.prompt_tokens.saturating_add(self.completion_tokens))
            }else{Value::Null},
            "cacheReadTokens":if complete{json!(self.cache_read_tokens)}else{Value::Null},
            "cacheWriteTokens":if complete{json!(self.cache_write_tokens)}else{Value::Null},
            "toolStarts":self.tool_starts,
            "toolSuccessEvents":self.tool_success,
            "toolErrorEvents":self.tool_errors,
            "estimatedKrw":null,"actualBilledKrw":null
        })
    }
}

async fn execute(request:Value)->Result<Value,String>{
    if request["protocolVersion"]!=1{return Err("지원하지 않는 SDK 프로토콜 버전".into());}
    let prompt=request["prompt"].as_str().filter(|s|!s.is_empty()&&s.len()<=30_000)
        .ok_or("SDK 입력 프롬프트가 없습니다.")?;
    let model=request["model"].as_str().filter(|s|!s.is_empty()&&s.len()<=128)
        .ok_or("SDK 모델을 선택하세요.")?;
    let workdir=request["workspace"].as_str().filter(|s|std::path::Path::new(s).is_absolute())
        .ok_or("SDK 작업공간 절대 경로가 없습니다.")?;
    let provider=request["provider"].as_str().unwrap_or("");
    let policy=request["toolPolicy"].as_str().unwrap_or("");
    if !matches!(provider,"github"|"ollama") || !matches!(policy,"read_only"|"trusted_full"){
        return Err("지원하지 않는 SDK 공급자 또는 도구 정책".into());
    }
    let mut cfg=SessionConfig::default()
        .with_model(model)
        .with_working_directory(workdir);
    if provider=="ollama" {
        let base=std::env::var("DOMINION_SDK_OLLAMA_URL")
            .unwrap_or_else(|_|"http://127.0.0.1:11434/v1".into());
        if !(base.starts_with("http://127.0.0.1:")||
             base.starts_with("http://localhost:")||
             base.starts_with("http://[::1]:")){
            return Err("Ollama SDK 엔드포인트는 서버 관리자가 지정한 루프백 주소만 허용합니다.".into());
        }
        let mut byok=ProviderConfig::default();
        byok.provider_type=Some("openai".into());
        byok.model_provider=Some("ollama".into());
        byok.base_url=base;
        cfg=cfg.with_provider(byok);
    }
    cfg=if policy=="trusted_full" {
        // The parent has already checked BOTH owner confirmation and admin
        // DOMINION_COPILOT_TRUSTED_TOOLS=1; refuse to trust stdin alone.
        if std::env::var("DOMINION_COPILOT_TRUSTED_TOOLS").as_deref()!=Ok("1"){
            return Err("SDK 도구 전체 권한을 서버에서 허용하지 않았습니다.".into());
        }
        cfg.approve_all_permissions()
    }else {
        // Default SDK evaluation runs without privileged shell/edit permissions.
        cfg.deny_all_permissions()
    };
    let client=Client::start(ClientOptions::default()).await
        .map_err(|e|format!("Copilot CLI 시작 실패: {e}"))?;
    let session=match client.create_session(cfg).await{
        Ok(session)=>session,
        Err(error)=>{client.stop().await.ok();return Err(format!("SDK 세션 생성 실패: {error}"));}
    };
    let session_id=session.id().to_string();
    let mut events=session.subscribe();
    let collector=tokio::spawn(async move {
        let mut meters=Meters::default();
        // Terminal event is queued to subscribers before send_and_wait returns.
        // An idle timeout covers runtimes which keep observers connected.
        loop{
            match tokio::time::timeout(Duration::from_secs(2),events.recv()).await{
                Ok(Ok(event))=>meters.observe(&event),
                Ok(Err(error))=>{
                    if matches!(error.kind(),github_copilot_sdk::subscription::RecvErrorKind::Lagged(_)){
                        meters.lost_events=true;
                        continue;
                    }
                    break;
                },
                _=>break
            }
        }
        meters
    });
    let seconds=request["timeoutSeconds"].as_u64().unwrap_or(600).clamp(30,600);
    let reply=session.send_and_wait(
        MessageOptions::new(prompt)
            .with_source(MessageSource::Agent("dominion-workflow".into()))
            .with_wait_timeout(Duration::from_secs(seconds))
    ).await;
    // Explicitly release the SDK's own process and event stream.
    let _=session.disconnect().await;
    let _=client.stop().await;
    let meters=collector.await.map_err(|e|format!("SDK 사용량 수집 실패: {e}"))?;
    let response=reply.map_err(|e|format!("SDK 작업 실행 실패: {e}"))?
        .ok_or("SDK가 최종 어시스턴트 메시지를 반환하지 않았습니다.")?;
    let content=response.data["content"].as_str()
        .filter(|s|!s.trim().is_empty())
        .ok_or("SDK가 최종 텍스트 결과를 반환하지 않았습니다.")?;
    if content.len()>100_000{return Err("SDK 최종 응답이 너무 깁니다.".into());}
    Ok(json!({
        "protocolVersion":1,"status":"completed",
        "harness":"github-copilot-sdk","harnessSdkVersion":"1.0.34",
        "sdkSessionId":session_id,"content":content,
        "usage":meters.into_json(),
        "artifacts":[],"toolReceiptAuthority":"SDK events only (not DOMINION verified file receipts)",
        "qualityVerified":false,"actualBilledKrw":null
    }))
}

#[tokio::main]
async fn main(){
    let mut input=String::new();
    let result=std::io::stdin().take(65_537).read_to_string(&mut input)
        .map_err(|e|e.to_string())
        .and_then(|_|serde_json::from_str::<Value>(&input).map_err(|e|e.to_string()));
    let output=match result{
        Ok(request)=>execute(request).await,
        Err(error)=>Err(format!("SDK 작업 입력 형식 오류: {error}"))
    };
    let (response,code)=match output{
        Ok(record)=>(record,0),
        Err(error)=>{
            // Errors are intentionally not printed verbatim, because the SDK
            // runtime can include credential or endpoint details in an error.
            eprintln!("Copilot SDK 작업이 실패했습니다: {}",if error.contains("SDK") {
                "세부 진단은 로컬 SDK 로그에서 확인하세요."
            } else {"작업 입력 또는 권한 정책 오류"} );
            (json!({"protocolVersion":1,"status":"failed",
                "harness":"github-copilot-sdk","qualityVerified":false}),2)
        }
    };
    let mut stdout=std::io::stdout().lock();
    let _=writeln!(stdout,"{response}");
    std::process::exit(code);
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn usage_meter_does_not_double_count_cache_tokens(){
        let mk=|kind:&str,data:Value|SessionEvent{
            id:"test".into(),timestamp:"2026-11-14T00:00:00Z".into(),
            parent_id:None,ephemeral:Some(true),agent_id:None,
            debug_cli_received_at_ms:None,debug_ws_forwarded_at_ms:None,
            event_type:kind.into(),data
        };
        let mut meter=Meters::default();
        meter.observe(&mk("assistant.usage",json!({
            "inputTokens":100,"outputTokens":21,"cacheReadTokens":80,
            "cacheWriteTokens":0
        })));
        meter.observe(&mk("tool.execution_start",json!({"toolCallId":"a"})));
        meter.observe(&mk("tool.execution_complete",json!({"success":true})));
        let result=meter.into_json();
        assert_eq!(result["totalTokens"],121);
        assert_eq!(result["cacheReadTokens"],80);
        assert_eq!(result["toolSuccessEvents"],1);
        assert_eq!(result["actualBilledKrw"],Value::Null);
    }
    #[test]
    fn no_usage_event_does_not_claim_zero_tokens(){
        let meter=Meters::default().into_json();
        assert_eq!(meter["measured"],false);
        assert_eq!(meter["totalTokens"],Value::Null);
    }
}
