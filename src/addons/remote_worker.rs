//! P3 opt-in remote worker gateway. This is a transport for the existing
//! native Harness, never an independent policy, scheduler or model executor.
use crate::{domain::{ToolCall,ToolEffectOutcome},ports::RunStorePort};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{sync::Arc,time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

pub const TOOL:&str="remote_worker_consult";
const AUTH_HEADER:&str="x-dominion-worker-token";
const MAX_REMOTE_BODY:u64=256*1024;

pub struct RemoteWorker{
    endpoint:String,
    token:String,
    client:reqwest::Client,
}
impl RemoteWorker{
    pub fn from_env()->Result<Option<Self>,String>{
        let Some(url)=std::env::var("DOMINION_REMOTE_WORKER_URL")
            .ok().filter(|v|!v.trim().is_empty()) else{return Ok(None)};
        let token=std::env::var("DOMINION_REMOTE_WORKER_TOKEN")
            .map_err(|_|"remote worker URL requires its dedicated scoped token")?;
        Self::configured(&url,token).map(Some)
    }
    pub fn configured(base:&str,token:String)->Result<Self,String>{
        if token.len()<32||token.len()>2048||token.bytes().any(|byte|byte==10||byte==13){
            return Err("remote worker capability token must be 32..2048 bytes".into());
        }
        let url=reqwest::Url::parse(base).map_err(|e|format!("invalid remote worker URL: {e}"))?;
        let loopback=url.host_str().is_some_and(|host|host=="localhost"||
            host.parse::<std::net::IpAddr>().is_ok_and(|ip|ip.is_loopback()));
        if url.scheme()!="https"&&!(url.scheme()=="http"&&loopback){
            return Err("remote worker requires HTTPS except exact loopback".into());
        }
        if !url.username().is_empty()||url.password().is_some()
            ||url.query().is_some()||url.fragment().is_some()
            ||url.path()!="/"{
            return Err("remote worker URL must contain only an origin, without userinfo/path/query".into());
        }
        let client=reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build().map_err(|e|e.to_string())?;
        Ok(Self{endpoint:base.trim_end_matches('/').to_owned(),token,client})
    }
    pub fn definition(&self)->Value{
        json!({"type":"function","function":{
            "name":TOOL,
            "description":"Delegate ONLY owner-approved, read-only investigation to a separately authenticated Dominion Rust native Harness. This tool does not modify remote files and does not hide remote model costs. Do not retry an uncertain remote call without reconciliation.",
            "parameters":{"type":"object","properties":{
                "instruction":{"type":"string","minLength":1,"maxLength":12000}
            },"required":["instruction"],"additionalProperties":false}
        }})
    }
    fn remote_id(&self,origin:Uuid,call_id:&str)->Uuid{
        let mut digest=Sha256::new();
        digest.update(b"dominion-remote-readonly-v1\\0");
        digest.update(self.endpoint.as_bytes());
        digest.update(origin.as_bytes());
        digest.update(call_id.as_bytes());
        let digest=digest.finalize();
        let mut bytes=[0u8;16];bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(bytes)
    }
    fn url(&self,run_id:Option<Uuid>)->String{
        let path=run_id.map(|id|format!("/{id}")).unwrap_or_default();
        format!("{}/api/v1/remote-worker/runs{path}",self.endpoint)
    }
    async fn response_json(response:reqwest::Response)->Result<Value,String>{
        if response.content_length().is_some_and(|n|n>MAX_REMOTE_BODY){
            return Err("remote worker response exceeded byte cap".into());
        }
        let status=response.status();
        if !status.is_success(){return Err(format!("remote worker HTTP {status}"));}
        let bytes=response.bytes().await.map_err(|e|format!("remote worker body: {e}"))?;
        if bytes.len() as u64>MAX_REMOTE_BODY{return Err("remote worker response too large".into());}
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|e|format!("invalid remote worker protocol: {e}"))
    }
    pub async fn execute(&self,store:&Arc<RunStorePort>,id:Uuid,
        call:&ToolCall,mut cancellation:watch::Receiver<bool>)
        ->Result<ToolEffectOutcome,String>{
        if call.name!=TOOL{return Ok(ToolEffectOutcome::Settled(Err(
            "wrong remote worker tool".into())));}
        let Some(instruction)=call.arguments.as_object().filter(|v|v.len()==1)
            .and_then(|v|v.get("instruction")).and_then(Value::as_str)
            .filter(|v|!v.trim().is_empty()&&v.len()<=12000)else{
            return Ok(ToolEffectOutcome::Settled(Err(
                "remote worker requires one bounded instruction".into())));
        };
        if *cancellation.borrow(){return Ok(ToolEffectOutcome::Settled(
            Err("remote worker cancelled before dispatch".into())));}
        let Some(origin)=store.get(id).await else{return Err("original Run not found".into())};
        if origin.in_flight.as_ref().is_none_or(|pending|
            pending.id!=call.id||pending.name!=call.name||pending.arguments!=call.arguments){
            return Ok(ToolEffectOutcome::Settled(Err(
                "remote worker requires a journalled, approved tool intent".into())));
        }
        if origin.room_binding.is_some(){
            return Ok(ToolEffectOutcome::Settled(Err(
                "a Room peer cannot authorize a remote worker".into())));
        }
        let remaining=(crate::policy::effective_budget_krw(&origin)-origin.cost_krw)
            .max(0.0).min(10000.0);
        if remaining==0.0{
            return Ok(ToolEffectOutcome::Settled(Err(
                "remote worker KRW reservation exhausted before dispatch".into())));
        }
        let remote_id=self.remote_id(id,&call.id);
        store.mutate(id,|run|{
            run.event("remote_worker_unmetered",
                "Remote model charges are metered by the remote worker, not by the local Run",
                Some(json!({"remoteRunId":remote_id,"callId":call.id,
                    "billing":"remote_separately_metered","readOnly":true})));
            Ok(())
        }).await?;
        let execute=async {
            let response=self.client.post(self.url(None))
                .header(AUTH_HEADER,&self.token)
                .json(&json!({"runId":remote_id,"instruction":instruction,
                    "budgetKrw":remaining,
                    "maxSteps":origin.max_steps.min(20).max(1)}))
                .send().await;
            let response=match response{
                Ok(resp)=>resp,
                Err(error)=>return ToolEffectOutcome::Uncertain(format!(
                    "remote worker submission transport uncertain for {remote_id}: {error}"))
            };
            if response.status().is_client_error(){
                return ToolEffectOutcome::Settled(Err(format!(
                    "remote worker rejected request: {}",response.status())));
            }
            let first=match Self::response_json(response).await{
                Ok(value)=>value,
                Err(error)=>return ToolEffectOutcome::Uncertain(format!(
                    "remote worker submit uncertain for {remote_id}: {error}")),
            };
            if first["id"]!=remote_id.to_string(){
                return ToolEffectOutcome::Uncertain("remote worker returned a mismatched Run ID".into());
            }
            for _ in 0..180 {
                let response=self.client.get(self.url(Some(remote_id)))
                    .header(AUTH_HEADER,&self.token).send().await;
                let result=match response{
                    Ok(value)=>value,
                    Err(error)=>return ToolEffectOutcome::Uncertain(format!(
                        "remote worker observation uncertain for {remote_id}: {error}")),
                };
                let view=match Self::response_json(result).await{
                    Ok(value)=>value,
                    Err(error)=>return ToolEffectOutcome::Uncertain(format!(
                        "remote worker status unavailable for {remote_id}: {error}")),
                };
                if view["id"]!=remote_id.to_string(){
                    return ToolEffectOutcome::Uncertain("remote worker status identity mismatch".into());
                }
                match view["status"].as_str(){
                    Some("completed")=>{
                        let Some(answer)=view["answer"].as_str().filter(|v|!v.trim().is_empty())else{
                            return ToolEffectOutcome::Uncertain("remote worker completed without an answer receipt".into());
                        };
                        let Some(charged)=view["costKrw"].as_f64()
                            .filter(|v|v.is_finite()&&*v>=0.0)else{
                            return ToolEffectOutcome::Uncertain(
                                "remote worker omitted verified KRW usage".into());
                        };
                        let input_tokens=view["inputTokens"].as_u64().unwrap_or(0);
                        let output_tokens=view["outputTokens"].as_u64().unwrap_or(0);
                        // The provider invoice belongs to the remote node,
                        // but the coordinator reserves the measured amount in
                        // its own hierarchical budget once per remote Run.
                        let settled=store.mutate(id,|run|{
                            if run.events.iter().any(|event|
                                event.kind=="remote_worker_metered"&&
                                event.detail.as_ref().is_some_and(|detail|
                                    detail["remoteRunId"]==remote_id.to_string())){
                                return Ok(());
                            }
                            run.remote_cost_krw+=charged;
                            run.event("remote_worker_metered",
                                "Remote native worker provider usage reconciled",
                                Some(json!({"remoteRunId":remote_id,"callId":call.id,
                                    "costKrw":charged,"inputTokens":input_tokens,
                                    "outputTokens":output_tokens,"budgetExhausted":
                                        run.remote_cost_krw+run.cost_krw>run.budget_krw})));
                            Ok(())
                        }).await;
                        if let Err(error)=settled{
                            return ToolEffectOutcome::Uncertain(format!(
                                "remote cost receipt could not be journalled: {error}"));
                        }
                        return ToolEffectOutcome::Settled(Ok(json!({
                            "source":"dominion_remote_worker","remoteRunId":remote_id,
                            "answer":answer.chars().take(20000).collect::<String>(),
                            "remoteCostKrw":view["costKrw"],
                            "remoteInputTokens":view["inputTokens"],
                            "remoteOutputTokens":view["outputTokens"],
                            "billing":"remote_separately_metered","readOnly":true
                        }).to_string()));
                    }
                    Some("failed"|"cancelled")=>return ToolEffectOutcome::Settled(Err(format!(
                        "remote worker {remote_id} ended with status {}",view["status"]))),
                    Some("awaiting_approval"|"paused")=>return ToolEffectOutcome::Uncertain(
                        format!("remote worker {remote_id} requires remote owner reconciliation")),
                    Some("queued"|"running")=>tokio::time::sleep(Duration::from_millis(500)).await,
                    _=>return ToolEffectOutcome::Uncertain("unknown remote worker status".into())
                }
            }
            ToolEffectOutcome::Uncertain(format!(
                "remote worker {remote_id} exceeded bounded observation window"))
        };
        Ok(tokio::select!{
            biased;
            signal=cancellation.changed()=>{
                ToolEffectOutcome::Uncertain(format!(
                    "remote worker {remote_id} cancellation or channel loss after dispatch: {signal:?}"))
            }
            outcome=execute=>outcome
        })
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[tokio::test]
    async fn remote_worker_executes_one_authorized_native_result_over_http(){
        use axum::{extract::{State,Path},http::StatusCode,
            routing::{get,post},Json,Router};
        use crate::{domain::{CreateRun,Run},store::Store};
        use std::sync::atomic::{AtomicUsize,Ordering};
        async fn submit(State(count):State<Arc<AtomicUsize>>,
            Json(request):Json<Value>)->(StatusCode,Json<Value>){
            count.fetch_add(1,Ordering::SeqCst);
            (StatusCode::CREATED,Json(json!({
                "id":request["runId"],"status":"running"
            })))
        }
        async fn inspect(Path(id):Path<Uuid>)->Json<Value>{
            Json(json!({"id":id,"status":"completed","answer":"observed native evidence",
                "costKrw":2.5,"inputTokens":50,"outputTokens":8}))
        }
        let server_socket=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=server_socket.local_addr().unwrap();
        let count=Arc::new(AtomicUsize::new(0));
        let router=Router::new()
            .route("/api/v1/remote-worker/runs",post(submit))
            .route("/api/v1/remote-worker/runs/{id}",get(inspect))
            .with_state(Arc::clone(&count));
        let server=tokio::spawn(async move{
            axum::serve(server_socket,router).await.unwrap();
        });
        let root=std::env::temp_dir().join(format!(
            "dominion-remote-gateway-test-{}",Uuid::new_v4()));
        let backend=Arc::new(Store::open(root.clone()).await.unwrap());
        let store=Arc::new(RunStorePort::new(backend));
        let call=ToolCall{id:"one-authorized-call".into(),
            name:TOOL.into(),arguments:json!({"instruction":"inspect test evidence"})};
        let mut origin=Run::new(CreateRun{
            instruction:"Ask remote investigator".into(),budget_krw:Some(100.0),
            max_steps:Some(4)},4).unwrap();
        origin.in_flight=Some(call.clone());
        let origin_id=origin.id;
        store.put(origin).await.unwrap();
        let worker=RemoteWorker::configured(&format!("http://{addr}"),"x".repeat(40)).unwrap();
        let (_cancel_tx,cancel_rx)=watch::channel(false);
        let output=worker.execute(&store,origin_id,&call,cancel_rx).await.unwrap();
        match output{
            ToolEffectOutcome::Settled(Ok(content))=>{
                let receipt:Value=serde_json::from_str(&content).unwrap();
                assert_eq!(receipt["answer"],"observed native evidence");
                assert_eq!(receipt["remoteCostKrw"],2.5);
                assert_eq!(receipt["readOnly"],true);
            }
            other=>panic!("remote native inspection was not settled: {other:?}")
        }
        assert_eq!(count.load(Ordering::SeqCst),1,
            "one journalled call must emit only one remote submission");
        assert_eq!(store.get(origin_id).await.unwrap().remote_cost_krw,2.5);
        assert!(store.get(origin_id).await.unwrap().events.iter()
            .any(|e|e.kind=="remote_worker_unmetered"));
        server.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[test]
    fn remote_worker_url_is_operator_scoped_and_rejects_unsafe_targets(){
        let token="a".repeat(40);
        assert!(RemoteWorker::configured("https://worker.example.com",token.clone()).is_ok());
        assert!(RemoteWorker::configured("http://127.0.0.1:4317",token.clone()).is_ok());
        for invalid in ["http://worker.example.com","https://worker.example.com/extra",
            "https://user:pass@worker.example.com","https://worker.example.com/?x=1"]{
            assert!(RemoteWorker::configured(invalid,token.clone()).is_err());
        }
        let worker=RemoteWorker::configured("https://worker.example.com",token).unwrap();
        assert_eq!(worker.remote_id(Uuid::nil(),"call"),worker.remote_id(Uuid::nil(),"call"));
        assert_ne!(worker.remote_id(Uuid::nil(),"call"),worker.remote_id(Uuid::nil(),"other"));
    }
}
