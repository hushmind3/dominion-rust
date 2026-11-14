//! Standalone real-SDK comparison harness. This is deliberately NOT called
//! from DOMINION's production tool runtime: an SDK tool receipt does not yet
//! satisfy DOMINION's SQLite effect ledger / user approval contract.
//!
//! Run only with explicitly configured Copilot auth or BYOK and user consent.
use github_copilot_sdk::{Client, ClientOptions, MessageOptions, SessionConfig};
use serde_json::{json, Value};
use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn output_text(value: &Value) -> Option<&str> {
    value
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/message/content").and_then(Value::as_str))
        .or_else(|| value.get("text").and_then(Value::as_str))
}

fn classify(events: &[github_copilot_sdk::SessionEvent], marker: &str) -> Value {
    // The final assistant message is the ONLY answer accepted by the oracle.
    // Raw tool responses, user prompts, logs and intermediate prose cannot pass.
    let answer=events.iter().rev().find(|event|
        event.event_type=="assistant.message" && event.agent_id.is_none()
    ).and_then(|event|output_text(&event.data)).unwrap_or("");
    let invoked=events.iter().filter(|e|
        e.event_type.contains("tool") && (e.event_type.ends_with(".start")
            ||e.event_type.ends_with(".started")||e.event_type.ends_with(".execution_start"))
    ).count();
    let tools_completed=events.iter().filter(|e|
        e.event_type.contains("tool") && (e.event_type.ends_with(".complete")
            ||e.event_type.ends_with(".completed")||e.event_type.ends_with(".execution_complete"))
    ).count();
    let usage_events=events.iter().filter(|e|
        e.event_type.contains("usage")).map(|e|json!({
            "eventType":e.event_type,
            "data":e.data
        })).collect::<Vec<_>>();
    json!({
        "oraclePassed":!marker.is_empty() && answer.contains(marker),
        "finalAnswerPresent":!answer.is_empty(),
        "modelAnswer":answer,
        "eventCount":events.len(),
        "observedToolStartEvents":invoked,
        "observedToolCompletionEvents":tools_completed,
        "usageEvents":usage_events,
        "usageEventCount":events.iter().filter(|e|e.event_type.contains("usage")).count(),
        "actualBilledKrw":null
    })
}

fn configuration() -> Result<(PathBuf,Option<String>,PathBuf),String> {
    if env::var("DOMINION_SDK_EVAL_ALLOW_MODEL_CALLS").as_deref()!=Ok("1") {
        return Err("실제 Copilot 모델 사용을 허용하려면 DOMINION_SDK_EVAL_ALLOW_MODEL_CALLS=1을 지정하세요. Copilot 이용량 또는 BYOK 비용이 발생할 수 있습니다.".into());
    }
    let evidence=env::var_os("DOMINION_SDK_EVAL_OUTPUT").map(PathBuf::from)
        .unwrap_or_else(||PathBuf::from("sdk-probe-result.json"));
    let sandbox=env::var_os("DOMINION_SDK_EVAL_WORKSPACE").map(PathBuf::from)
        .unwrap_or_else(||env::temp_dir().join("dominion-sdk-probe"));
    let model=env::var("DOMINION_SDK_EVAL_MODEL").ok().filter(|x|!x.trim().is_empty());
    Ok((evidence,model,sandbox))
}

#[tokio::main]
async fn main()->Result<(),Box<dyn std::error::Error>>{
    let (output,model,sandbox)=configuration()?;
    fs::create_dir_all(&sandbox)?;
    let sandbox=sandbox.canonicalize()?;
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let marker=format!("DOMINION_SDK_ORACLE_{stamp:032x}");
    // Write the target ONLY to the fixture, never to the request prompt.
    fs::write(sandbox.join("proof.txt"),format!(
        "This is a read-only agent evaluation fixture.\nThe exact marker: {marker}\n"
    ))?;
    let mut cfg=SessionConfig::default();
    cfg.working_directory=Some(sandbox.clone());
    // This is a fresh disposable fixture directory, not the user's repo.
    cfg.trust_working_directory=Some(github_copilot_sdk::types::WorkingDirectoryTrust::Session);
    cfg.available_tools=Some(vec!["view".into()]);
    cfg.enable_host_git_operations=Some(false);
    cfg.enable_file_hooks=Some(false);
    cfg.enable_skills=Some(false);
    cfg.enable_config_discovery=Some(false);
    if let Some(model)=&model{cfg.model=Some(model.clone());}
    // Never allow arbitrary shell, write, network or MCP effects. This probe
    // tests the SDK's tool-based reading, not its unsandboxed write abilities.
    let cfg=cfg.approve_permissions_if(|data|
        data.extra.get("tool").and_then(Value::as_str)==Some("view")
    );
    let started=Instant::now();
    let client=Client::start(ClientOptions::default()).await?;
    let result=async {
        let session=client.create_session(cfg).await?;
        let task="Use the view tool to read proof.txt in this workspace. Reply with the exact marker after 'The exact marker:' on one line. Do not guess or invent a marker.";
        let response=session.send_and_wait(
            MessageOptions::new(task).with_wait_timeout(Duration::from_secs(180))
        ).await;
        let events=session.get_events().await.unwrap_or_default();
        session.disconnect().await.ok();
        Ok::<_,github_copilot_sdk::Error>((response,events))
    }.await;
    client.stop().await.ok();
    let elapsed=started.elapsed().as_millis();
    let (answer,events)=result?;
    let evaluation=classify(&events,&marker);
    let record=json!({
        "runtime":"github-copilot-sdk",
        "versionRequirement":"github-copilot-sdk 1.0.34",
        "mode":"read-only; view tool allowlisted; no shell/write/MCP",
        "authentication":"user-supplied Copilot auth or BYOK",
        "model":model,
        "workspace":sandbox.to_string_lossy(),
        "elapsedMs":elapsed,
        "sdkRequestReturned":answer.as_ref().is_ok(),
        "sdkError":answer.as_ref().err().map(ToString::to_string),
        "evaluation":evaluation,
        "benchmarkLevel":"real SDK run if completed; independent exact fixture oracle",
        "notes":[
            "A tool-start event is not evidence that a tool completed.",
            "A successful response without the exact marker is a failure.",
            "No price is invented when the SDK does not provide billed KRW.",
            "This is NOT a production DOMINION agent runtime or code-edit acceptance test."
        ]
    });
    if let Some(parent)=output.parent().filter(|p|!p.as_os_str().is_empty()){fs::create_dir_all(parent)?;}
    fs::write(&output,serde_json::to_vec_pretty(&record)?)?;
    println!("{}",serde_json::to_string_pretty(&record)?);
    if record["evaluation"]["oraclePassed"]!=true {
        std::process::exit(2);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(t:&str,content:&str,agent_id:Option<&str>)->github_copilot_sdk::SessionEvent{
        serde_json::from_value(json!({
            "id":"event-1","timestamp":"2026-11-14T00:00:00Z",
            "parentId":null,"agentId":agent_id,
            "type":t,"data":{"content":content}
        })).expect("SDK session event shape")
    }
    #[test]
    fn oracle_cannot_be_spoofed_by_tool_or_subagent(){
        let events=vec![
            event("tool.execution_complete","TARGET",None),
            event("assistant.message","TARGET",Some("subagent")),
            event("assistant.message","did not find it",None)
        ];
        assert_eq!(classify(&events,"TARGET")["oraclePassed"],false);
    }
    #[test]
    fn only_final_root_answer_passes(){
        let events=vec![event("assistant.message","TARGET",None)];
        assert_eq!(classify(&events,"TARGET")["oraclePassed"],true);
        assert_eq!(classify(&events,"WRONG")["oraclePassed"],false);
    }
}
