//! Bounded Codex App Server stdio JSON-RPC client for one approved advisory
//! call. No second scheduler, native tool access or auto-approved server action.
//! Based on the 2026 App Server initialize -> thread/start -> turn/start protocol.
use super::{bounded_answer,MAX_OUTPUT};
use serde_json::{json,Value};
use std::path::Path;
use tokio::io::{AsyncRead,AsyncReadExt,AsyncWrite,AsyncWriteExt};

const MAX_EVENT_BYTES:usize=MAX_OUTPUT;

/// JSONL framing with a hard cumulative byte ceiling. AsyncBufRead::read_line
/// alone can allocate an unbounded line before the caller checks its length.
struct Frames<R>{reader:R,pending:Vec<u8>,observed:usize}
impl<R:AsyncRead+Unpin> Frames<R>{
    fn new(reader:R)->Self{Self{reader,pending:Vec::new(),observed:0}}
    async fn next(&mut self)->Result<Value,String>{
        loop{
            if let Some(end)=self.pending.iter().position(|&b|b==b'\n'){
                let line=self.pending.drain(..=end).collect::<Vec<_>>();
                let event=serde_json::from_slice::<Value>(&line)
                    .map_err(|_|"invalid Codex App Server JSONL frame".to_string())?;
                return Ok(event);
            }
            let mut chunk=[0u8;4096];
            let n=self.reader.read(&mut chunk).await
                .map_err(|_|"Codex App Server stdout read failed")?;
            if n==0{return Err("Codex App Server stdout closed before turn completion".into());}
            self.observed=self.observed.saturating_add(n);
            if self.observed>MAX_EVENT_BYTES{
                return Err("Codex App Server exceeded total output byte cap".into());
            }
            self.pending.extend_from_slice(&chunk[..n]);
        }
    }
}
async fn send<W:AsyncWrite+Unpin>(writer:&mut W,payload:Value)->Result<(),String>{
    let mut encoded=serde_json::to_vec(&payload).map_err(|_|"invalid App Server request")?;
    encoded.push(b'\n');
    writer.write_all(&encoded).await
        .map_err(|_|"Codex App Server stdin write failed".to_string())?;
    writer.flush().await.map_err(|_|"Codex App Server stdin flush failed".to_string())
}
fn guard_server_request(value:&Value)->Result<(),String>{
    if value.get("method").and_then(Value::as_str).is_some()
        && value.get("id").is_some(){
        // Especially item/commandExecution/requestApproval and
        // item/fileChange/requestApproval: this bridge does NOT possess
        // authority to accept, escalate, or respond on the user's behalf.
        return Err("Codex App Server requested unsupported interactive approval".into());
    }
    Ok(())
}
async fn response<R:AsyncRead+Unpin>(frames:&mut Frames<R>,id:u64)->Result<Value,String>{
    loop{
        let value=frames.next().await?;
        guard_server_request(&value)?;
        if value.get("id").and_then(Value::as_u64)==Some(id){
            if value.get("error").is_some(){
                return Err("Codex App Server rejected a protocol request".into());
            }
            return value.get("result").cloned()
                .ok_or_else(||"Codex App Server response missing result".into());
        }
    }
}
pub(super) async fn consult<R,W>(stdout:R,mut stdin:W,cwd:&Path,prompt:&str)
    ->Result<String,String>
where R:AsyncRead+Unpin,W:AsyncWrite+Unpin{
    let cwd=cwd.to_str().ok_or("Codex App Server scratch path is not UTF-8")?;
    let mut frames=Frames::new(stdout);
    send(&mut stdin,json!({"id":1,"method":"initialize","params":{
        "clientInfo":{"name":"dominion","title":"Dominion Agent Bridge","version":"0.1.0"}
    }})).await?;
    response(&mut frames,1).await?;
    send(&mut stdin,json!({"method":"initialized","params":{}})).await?;
    // An ephemeral thread avoids filling the operator's Codex history. The
    // advisory bridge never grants workspace writes or interactive approvals.
    send(&mut stdin,json!({"id":2,"method":"thread/start","params":{
        "cwd":cwd,"sandbox":"readOnly","approvalPolicy":"never",
        "ephemeral":true
    }})).await?;
    let started=response(&mut frames,2).await?;
    let thread=started.pointer("/thread/id").and_then(Value::as_str)
        .filter(|id|!id.is_empty()).ok_or("App Server omitted thread id")?;
    let thread=thread.to_string();
    send(&mut stdin,json!({"id":3,"method":"turn/start","params":{
        "threadId":thread,"input":[{"type":"text","text":prompt}]
    }})).await?;
    let response=response(&mut frames,3).await?;
    let turn=response.pointer("/turn/id").and_then(Value::as_str)
        .filter(|id|!id.is_empty()).ok_or("App Server omitted turn id")?;
    let turn=turn.to_string();
    let mut last=None;
    loop{
        let event=frames.next().await?;
        guard_server_request(&event)?;
        if event.get("id").is_some(){continue;}
        if event.pointer("/params/threadId").and_then(Value::as_str)
            .is_some_and(|id|id!=thread){continue;}
        match event["method"].as_str(){
            Some("item/completed") if event["params"]["item"]["type"]=="agentMessage"=>{
                if let Some(text)=event["params"]["item"]["text"].as_str(){
                    last=Some(text.to_string());
                }
            }
            Some("turn/completed")=>{
                let finished=&event["params"]["turn"];
                if finished["id"]!=turn{continue;}
                if finished["status"]!="completed"{
                    return Err("Codex App Server turn failed or interrupted".into());
                }
                if last.is_none(){
                    last=finished["items"].as_array().and_then(|items|items.iter().rev()
                        .find(|item|item["type"]=="agentMessage"))
                        .and_then(|item|item["text"].as_str()).map(str::to_owned);
                }
                return bounded_answer(last.as_deref().unwrap_or(""));
            }
            _=>{}
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use tokio::io::{AsyncBufReadExt,BufReader};
    #[tokio::test]
    async fn codex_app_server_handshake_is_ephemeral_read_only_and_turn_verified(){
        let (mut server_write,client_read)=tokio::io::duplex(8192);
        let (client_write,server_read)=tokio::io::duplex(8192);
        let server=tokio::spawn(async move{
            let mut input=BufReader::new(server_read);
            let mut line=String::new();
            input.read_line(&mut line).await.unwrap();
            let initialized:Value=serde_json::from_str(&line).unwrap();
            assert_eq!(initialized["method"],"initialize");
            server_write.write_all(b"{\"id\":1,\"result\":{}}\n").await.unwrap();
            line.clear();input.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["method"],"initialized");
            line.clear();input.read_line(&mut line).await.unwrap();
            let created:Value=serde_json::from_str(&line).unwrap();
            assert_eq!(created["method"],"thread/start");
            assert_eq!(created["params"]["sandbox"],"readOnly");
            assert_eq!(created["params"]["approvalPolicy"],"never");
            assert_eq!(created["params"]["ephemeral"],true);
            server_write.write_all(b"{\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-test\"}}}\n").await.unwrap();
            line.clear();input.read_line(&mut line).await.unwrap();
            let turn:Value=serde_json::from_str(&line).unwrap();
            assert_eq!(turn["method"],"turn/start");
            assert_eq!(turn["params"]["threadId"],"thread-test");
            assert_eq!(turn["params"]["input"][0]["text"],"Review exactly this diff");
            server_write.write_all(b"{\"id\":3,\"result\":{\"turn\":{\"id\":\"turn-test\"}}}\n").await.unwrap();
            server_write.write_all(b"{\"method\":\"item/completed\",\"params\":{\"threadId\":\"thread-test\",\"item\":{\"type\":\"agentMessage\",\"text\":\"Advisory only\"}}}\n").await.unwrap();
            server_write.write_all(b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-test\",\"turn\":{\"id\":\"turn-test\",\"status\":\"completed\"}}}\n").await.unwrap();
        });
        let answer=consult(client_read,client_write,Path::new("/tmp/sub4-scratch"),
            "Review exactly this diff").await.unwrap();
        assert_eq!(answer,"Advisory only");
        server.await.unwrap();
    }
    #[test]
    fn server_initiated_approval_never_auto_accepts(){
        assert!(guard_server_request(&json!({
            "id":42,"method":"item/fileChange/requestApproval",
            "params":{"reason":"write outside scratch"}})).is_err());
        assert!(guard_server_request(&json!({
            "id":43,"method":"item/commandExecution/requestApproval"})).is_err());
        assert!(guard_server_request(&json!({
            "method":"turn/completed","params":{}})).is_ok());
    }
    #[tokio::test]
    async fn framing_rejects_oversized_output_before_unbounded_allocation(){
        let (mut write,read)=tokio::io::duplex(8192);
        let data=vec![b'a';MAX_EVENT_BYTES+1];
        let task=tokio::spawn(async move{let _=write.write_all(&data).await;});
        let mut frames=Frames::new(read);
        let failure=frames.next().await.unwrap_err();
        assert!(failure.contains("byte cap"));
        task.abort();
    }
}
