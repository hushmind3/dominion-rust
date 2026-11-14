//! Optional official GitHub Copilot Rust SDK executor.
//!
//! Uses the production Copilot CLI harness rather than reimplementing a
//! planning/tool/edit loop. The SDK is MIT-licensed, but auth and model usage
//! are separate from the SDK license. The user must explicitly select this
//! execution mode, build the opt-in feature and configure Copilot credentials.
use crate::state::AppState;
use serde_json::{json,Value};

pub struct SdkStepResult {
    pub message: Value,
    /// Opaque native SDK events, not falsely counted as verified operations.
    pub events: Vec<Value>,
    /// The official runtime's session ID, used for provenance.
    pub runtime_session: String,
}

pub fn enabled()->bool{cfg!(feature="copilot-harness")}

#[cfg(feature="copilot-harness")]
pub async fn run(
    state:&AppState,session_id:&str,prompt:&str,model:Option<&str>,
    allow_tools:bool,timeout_secs:u64,
)->Result<SdkStepResult,String>{
    use github_copilot_sdk::{Client,ClientOptions,SessionConfig,MessageOptions};
    use github_copilot_sdk::handler::{ApproveAllHandler,DenyAllHandler};
    use std::{sync::{Arc,Mutex},time::Duration};

    if prompt.trim().is_empty()||prompt.len()>32*1024{
        return Err("Copilot SDK 실행 입력은 1~32768바이트여야 합니다.".into());
    }
    let policy=state.store.get("policy","default").unwrap_or_default();
    // The workflow step alone can never turn on broad approval. The explicit
    // owner setting and the step's opt-in must BOTH be present.
    let approval_allowed=allow_tools&&policy["autoApproveAll"]==true;
    if allow_tools&&!approval_allowed{
        return Err("Copilot 도구 자동 실행에는 설정의 전체 승인과 워크플로 단계 선택이 모두 필요합니다.".into());
    }
    let mut options=ClientOptions::default();
    options.working_directory=state.config.workspace.clone();
    let client=Client::start(options).await
        .map_err(|e|format!("Copilot SDK 런타임 시작 실패: {e}"))?;
    let mut config=SessionConfig::default();
    config=if approval_allowed{
        config.with_permission_handler(Arc::new(ApproveAllHandler))
    }else{
        config.with_permission_handler(Arc::new(DenyAllHandler))
    };
    if let Some(model)=model.filter(|s|!s.trim().is_empty()){
        config=config.with_model(model);
    }
    let session=match client.create_session(config).await{
        Ok(value)=>value,
        Err(e)=>{let _=client.stop().await;return Err(format!("Copilot SDK 세션 실패: {e}"))}
    };
    let remote_session=session.id().to_string();
    let events=Arc::new(Mutex::new(Vec::<Value>::new()));
    let trace=events.clone();
    let mut stream=session.subscribe();
    let collector=tokio::spawn(async move{
        while let Ok(event)=stream.recv().await{
            // External runtime events remain advisory. The DOMINION
            // execution_operation table owns verified local tool receipts.
            if matches!(event.event_type.as_str(),"tool.execution_start"|"tool.execution_complete"|
                "tool.execution_error"|"assistant.message"|"session.usage_info"|"session.error"){
                if let Ok(mut observations)=trace.lock(){
                    if observations.len()<512{
                        observations.push(json!({"type":event.event_type,
                            "eventId":event.id,"data":event.data}));
                    }
                }
            }
        }
    });
    let wait=Duration::from_secs(timeout_secs.clamp(15,900));
    let result=session.send_and_wait(MessageOptions::new(prompt.to_string())
        .with_wait_timeout(wait)).await;
    let _=session.disconnect().await;
    let _=client.stop().await;
    collector.abort();
    let observed=events.lock().map_err(|e|e.to_string())?.clone();
    let event=result.map_err(|e|format!("Copilot SDK 실행 실패: {e}"))?
        .ok_or("Copilot SDK가 완료된 assistant 메시지를 반환하지 않았습니다.")?;
    if event.event_type!="assistant.message"{
        return Err(format!("Copilot SDK가 완료 메시지 대신 {}를 반환했습니다.",event.event_type));
    }
    let content=event.data["content"].as_str()
        .or_else(||event.data["message"]["content"].as_str())
        .filter(|text|!text.trim().is_empty())
        .ok_or("Copilot SDK 메시지에 결과 본문이 없습니다.")?;
    // Persist a normal DOMINION assistant message so Chat/Activity/Memory
    // can reuse the result just like a native model step.
    let saved=state.store.write_message(&json!({
        "id":uuid::Uuid::new_v4().to_string(),"sessionId":session_id,
        "role":"assistant","content":content,
        "metadata":{"source":"github-copilot-sdk","runtimeSession":remote_session,
            "observedSdkEventCount":observed.len(),"externalBillingUnverified":true}
    })).ok_or("Copilot SDK 산출물을 DOMINION 세션에 보존하지 못했습니다.")?;
    state.store.audit("agent.sdk_turn",json!({
        "sessionId":session_id,"runtimeSession":remote_session,
        "model":model,"automaticTools":approval_allowed,
        "observedEvents":observed.len(),"billingVerified":false,
        "sdk":"github-copilot-sdk"
    }));
    Ok(SdkStepResult{message:saved,events:observed,runtime_session:remote_session})
}

#[cfg(not(feature="copilot-harness"))]
pub async fn run(
    _state:&AppState,_session_id:&str,_prompt:&str,_model:Option<&str>,
    _allow_tools:bool,_timeout_secs:u64,
)->Result<SdkStepResult,String>{
    Err("공식 Copilot SDK 실행기를 사용하려면 DOMINION을 --features copilot-harness로 빌드하고 Copilot CLI 인증을 구성해야 합니다. 기존 로컬 모델은 SDK 없이 계속 작동합니다.".into())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn sdk_is_opt_in_not_required_by_normal_local_build(){
        if !cfg!(feature="copilot-harness"){assert!(!enabled())}
    }
}
