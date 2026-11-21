//! P3 opt-in remote worker gateway. This is a transport for the existing
//! native Harness, never an independent policy, scheduler or model executor.
use crate::{domain::{ToolCall,ToolEffectOutcome},ports::RunStorePort};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{sync::Arc,time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

pub const TOOL:&str="remote_worker_consult";
pub const EDIT_TOOL:&str="remote_worker_edit";
const AUTH_HEADER:&str="x-dominion-worker-token";
const MAX_REMOTE_BODY:u64=256*1024;

#[derive(Clone,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct RegistryNode{
    id:String,
    url:String,
    token:String,
    #[serde(default)]writer_token:Option<String>,
    #[serde(default="default_weight")]weight:u8,
}
fn default_weight()->u8{1}
pub struct RemoteWorker{
    endpoint:String,
    token:String,
    writer_token:Option<String>,
    nodes:Vec<RegistryNode>,
    client:reqwest::Client,
}
impl RemoteWorker{
    pub fn from_env()->Result<Option<Self>,String>{
        if let Ok(raw)=std::env::var("DOMINION_REMOTE_WORKERS_JSON"){
            if !raw.trim().is_empty(){
                let mut entries=serde_json::from_str::<Vec<RegistryNode>>(&raw)
                    .map_err(|e|format!("invalid remote worker registry: {e}"))?;
                if entries.is_empty()||entries.len()>16{
                    return Err("remote worker registry needs 1..16 entries".into());
                }
                let mut unique=std::collections::HashSet::new();
                for entry in &entries{
                    if entry.id.is_empty()||entry.id.len()>64||
                        !entry.id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')||
                        entry.weight==0||entry.weight>16||
                        !unique.insert(entry.id.clone()){
                        return Err("remote worker IDs/weights must be unique and bounded".into());
                    }
                    let node=Self::configured(&entry.url,entry.token.clone())?;
                    if entry.writer_token.as_ref().is_some_and(|v|
                        v.len()<32||v.len()>2048||v.bytes().any(|b|b==10||b==13)){
                        return Err("remote Writer token must be 32..2048 bytes".into());
                    }
                    if !unique.insert(node.endpoint.clone()){
                        return Err("duplicate remote worker endpoint".into());
                    }
                }
                entries.sort_by(|a,b|a.id.cmp(&b.id));
                let first=Self::configured(&entries[0].url,entries[0].token.clone())?;
                return Ok(Some(Self{nodes:entries,..first}));
            }
        }
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
        Ok(Self{endpoint:base.trim_end_matches('/').to_owned(),token,
            writer_token:std::env::var("DOMINION_REMOTE_WORKER_WRITER_TOKEN")
                .ok().filter(|s|s.len()>=32),
            nodes:Vec::new(),client})
    }
    /// Owner-only, credential-redacted topology. A passive GET never probes
    /// remote networks; an explicit probe runs bounded authenticated checks.
    /// The UI cannot create a remote worker or edit stored capability tokens.
    pub async fn operator_topology(&self,probe:bool)->Value{
        let entries=if self.nodes.is_empty(){
            vec![RegistryNode{id:"legacy".into(),url:self.endpoint.clone(),
                token:self.token.clone(),writer_token:self.writer_token.clone(),
                weight:1}]
        }else{self.nodes.clone()};
        if !probe{
            let rows=entries.into_iter().map(|node|json!({
                "id":node.id,"url":node.url,"weight":node.weight,
                "writerConfigured":node.writer_token.is_some(),
                "state":"not_probed"
            })).collect::<Vec<_>>();
            return json!({"configured":true,"probed":false,"nodes":rows});
        }
        let mut pending=tokio::task::JoinSet::new();
        for node in entries{
            let client=self.client.clone();
            pending.spawn(async move{
                let mut row=json!({"id":node.id,"url":node.url,"weight":node.weight,
                    "writerConfigured":node.writer_token.is_some(),
                    "state":"unavailable"});
                let probe=async{
                    let reply=client.get(format!("{}/api/v1/remote-worker/capabilities",
                        node.url.trim_end_matches('/')))
                        .header(AUTH_HEADER,&node.token).send().await
                        .map_err(|error|error.to_string())?;
                    Self::response_json(reply).await
                };
                match tokio::time::timeout(Duration::from_secs(4),probe).await{
                    Ok(Ok(payload)) if payload["schemaVersion"]==1
                        &&payload["readOnly"]==true=>{
                        let slots=payload["slots"].as_u64().unwrap_or(0);
                        let active=payload["active"].as_u64().unwrap_or(u64::MAX);
                        row["state"]=json!(if slots>active{"ready"}else{"at_capacity"});
                        row["slots"]=json!(slots);row["active"]=json!(active);
                        row["writerAvailable"]=json!(payload["writer"]==true
                            &&node.writer_token.is_some());
                        // Repositories are exposed only to this owner-authenticated
                        // dashboard, never as an unauthenticated discovery endpoint.
                        row["writerRepositories"]=payload["writerRepositories"].clone();
                    }
                    Ok(Ok(_))=>{row["state"]=json!("invalid_protocol");}
                    Ok(Err(_))=>{row["state"]=json!("unreachable_or_unauthorized");}
                    Err(_)=>{row["state"]=json!("timeout");}
                }
                row
            });
        }
        let mut nodes=Vec::new();
        while let Some(result)=pending.join_next().await{
            if let Ok(node)=result{nodes.push(node);}
        }
        nodes.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));
        json!({"configured":true,"probed":true,"nodes":nodes})
    }
    pub fn writer_definition(&self)->Option<Value>{
        if self.writer_token.is_none() &&
            !self.nodes.iter().any(|n|n.writer_token.is_some()){return None;}
        Some(json!({"type":"function","function":{
            "name":EDIT_TOOL,
            "description":"Owner-approved remote Team Writer edits an isolated, exact-revision Git worktree. The reviewed SHA-256 patch returns to the local Team child and still requires separate Team merge approval. External billing and unsafe remote outcomes are journalled.",
            "parameters":{"type":"object","properties":{
                "instruction":{"type":"string","minLength":1,"maxLength":12000}
            },"required":["instruction"],"additionalProperties":false}
        }}))
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
        self.remote_id_for(&self.endpoint,origin,call_id)
    }
    fn remote_id_for(&self,endpoint:&str,origin:Uuid,call_id:&str)->Uuid{
        let mut digest=Sha256::new();
        digest.update(b"dominion-remote-readonly-v1\\0");
        digest.update(endpoint.as_bytes());
        digest.update(origin.as_bytes());
        digest.update(call_id.as_bytes());
        let digest=digest.finalize();
        let mut bytes=[0u8;16];bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(bytes)
    }
    fn url_for(endpoint:&str,run_id:Option<Uuid>)->String{
        let path=run_id.map(|id|format!("/{id}")).unwrap_or_default();
        format!("{endpoint}/api/v1/remote-worker/runs{path}")
    }
    /// Operator-approved registry + authenticated live capability discovery.
    /// Nodes that fail health/capacity checks are excluded BEFORE any POST.
    /// Once chosen the binding is journalled; transport errors never fan-out
    /// the same potentially billable instruction to a second machine.
    async fn select_node(&self,origin:Uuid,call_id:&str,writer:bool,
        repository:Option<&str>)
        ->Result<(String,String,String,Option<String>),String>{
        if self.nodes.is_empty(){
            if writer&&self.writer_token.is_none(){
                return Err("remote writer token is not configured".into());
            }
            return Ok(("legacy".into(),self.endpoint.clone(),self.token.clone(),
                self.writer_token.clone()));
        }
        let mut candidates=Vec::<(f64,&RegistryNode)>::new();
        for entry in &self.nodes{
            if writer&&entry.writer_token.is_none(){continue;}
            let status=self.client.get(format!("{}/api/v1/remote-worker/capabilities",
                entry.url.trim_end_matches('/')))
                .header(AUTH_HEADER,&entry.token).send().await;
            let Ok(reply)=status else{continue};
            let Ok(capabilities)=Self::response_json(reply).await else{continue};
            if capabilities["schemaVersion"]!=1 || capabilities["readOnly"]!=true
                ||(writer&&capabilities["writer"]!=true){continue;}
            if writer&&!capabilities["writerRepositories"].as_array()
                .is_some_and(|entries|entries.iter().any(|v|
                    v.as_str()==repository)){continue;}
            let active=capabilities["active"].as_u64().unwrap_or(u64::MAX);
            let slots=capabilities["slots"].as_u64().unwrap_or(0);
            if slots==0||active>=slots{continue;}
            let mut digest=Sha256::new();
            digest.update(origin.as_bytes());digest.update(call_id.as_bytes());
            digest.update(entry.id.as_bytes());
            let hash=digest.finalize();
            let mut bytes=[0u8;8];bytes.copy_from_slice(&hash[..8]);
            let u=(u64::from_be_bytes(bytes) as f64+1.0)/(u64::MAX as f64+2.0);
            // Weighted rendezvous hashing is insensitive to JSON ordering.
            // Discount near-capacity nodes before admitting a new Run.
            let score=-(u.ln())/(entry.weight as f64)
                *(slots as f64)/(slots.saturating_sub(active) as f64);
            candidates.push((score,entry));
        }
        candidates.sort_by(|a,b|a.0.total_cmp(&b.0));
        candidates.first().map(|(_,node)|(
            node.id.clone(),node.url.trim_end_matches('/').to_owned(),
            node.token.clone(),node.writer_token.clone()))
            .ok_or("no authenticated remote workers have capacity".into())
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
    /// Record the highest provider-observed remote invoice, not merely
    /// successful answers. Failed/cancelled/approval-held workers may have
    /// already consumed billable tokens. Later observations add only deltas.
    async fn record_remote_charge(store:&Arc<RunStorePort>,origin:Uuid,
        call:&ToolCall,remote_id:Uuid,view:&Value)->Result<f64,String>{
        let billed=view["costKrw"].as_f64()
            .filter(|v|v.is_finite()&&*v>=0.0)
            .ok_or("remote worker omitted valid KRW usage")?;
        let observed=store.get(origin).await.ok_or("origin Run unavailable")?;
        let prior=|run:&crate::domain::Run|run.events.iter().rev()
            .find(|event|event.kind=="remote_worker_metered"
                &&event.detail.as_ref().is_some_and(|detail|
                    detail["remoteRunId"]==remote_id.to_string()))
            .and_then(|event|event.detail.as_ref())
            .and_then(|detail|detail["observedCostKrw"].as_f64()
                .or_else(||detail["costKrw"].as_f64()))
            .unwrap_or(0.0);
        if billed+0.000001<prior(&observed){
            return Err("remote provider usage decreased after durable receipt".into());
        }
        if billed<=prior(&observed){return Ok(billed);}
        let input=view["inputTokens"].as_u64().unwrap_or(0);
        let output=view["outputTokens"].as_u64().unwrap_or(0);
        store.mutate(origin,|run|{
            let already=prior(run);
            if billed+0.000001<already{
                return Err("remote usage changed during reconciliation".into());
            }
            let delta=(billed-already).max(0.0);
            if delta==0.0{return Ok(());}
            run.remote_cost_krw+=delta;
            run.event("remote_worker_metered",
                "Verified remote model usage reserved in KRW hierarchy",
                Some(json!({"remoteRunId":remote_id,"callId":call.id,
                    "costKrw":delta,"observedCostKrw":billed,
                    "inputTokens":input,"outputTokens":output,
                    "budgetExhausted":
                        run.remote_cost_krw+run.cost_krw>run.budget_krw})));
            Ok(())
        }).await?;
        Ok(billed)
    }
    pub async fn execute(&self,settings:&crate::config::Settings,
        store:&Arc<RunStorePort>,id:Uuid,
        call:&ToolCall,mut cancellation:watch::Receiver<bool>)
        ->Result<ToolEffectOutcome,String>{
        let editing=call.name==EDIT_TOOL;
        if call.name!=TOOL&&!editing{return Ok(ToolEffectOutcome::Settled(Err(
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
        // A remote Writer must originate from an owner-created Task Team.
        // It can change only the Writer's detached child worktree. The
        // coordinator root still requires a separate SHA-pinned human merge.
        let writer_context=if editing{
            let parent_id=origin.parent_run_id
                .ok_or("remote Writer requires a Task Team parent")?;
            let parent=store.get(parent_id).await
                .ok_or("remote Writer coordinator missing")?;
            let approved=parent.team.as_ref().is_some_and(|team|
                team.tasks.iter().any(|task|task.run_id==id &&
                    task.spec.role==crate::domain::team::TeamRole::Writer));
            if !approved||origin.read_only||origin.room_binding.is_some()
                ||origin.delegation_profile!="implementation"
                ||origin.repository.is_none()||origin.repository!=parent.repository
                ||origin.base_commit.is_none()||parent.base_commit.is_none(){
                return Ok(ToolEffectOutcome::Settled(Err(
                    "remote Writer requires an isolated, owner-authored Team lease".into())));
            }
            let parent_sha=parent.base_commit.as_deref().unwrap();
            let child_sha=origin.base_commit.as_deref().unwrap();
            crate::workspace::verified_external_writer_worktree(
                settings,parent_id,id,parent_sha,child_sha).await?;
            let (seed,seed_hash)=crate::workspace::remote_team_seed(
                settings,parent_id,id,parent_sha,child_sha).await?;
            Some((parent_id,parent_sha.to_owned(),child_sha.to_owned(),
                origin.repository.clone().unwrap(),seed,seed_hash))
        }else{None};
        let remaining=(crate::policy::effective_budget_krw(&origin)-origin.cost_krw)
            .max(0.0).min(10000.0);
        if remaining==0.0{
            return Ok(ToolEffectOutcome::Settled(Err(
                "remote worker KRW reservation exhausted before dispatch".into())));
        }
        let (node_id,endpoint,worker_token,writer_token)=match self.select_node(
            id,&call.id,editing,
            writer_context.as_ref().map(|w|w.3.as_str())).await{
            Ok(node)=>node,Err(error)=>return Ok(ToolEffectOutcome::Settled(Err(error))),
        };
        let remote_id=self.remote_id_for(&endpoint,id,&call.id);
        store.mutate(id,|run|{
            run.event("remote_worker_unmetered",
                "Remote model charges are metered by the remote worker, not by the local Run",
                Some(json!({"remoteRunId":remote_id,"callId":call.id,
                    "nodeId":node_id,"endpoint":endpoint,
                    "billing":"remote_separately_metered","readOnly":!editing,
                    "writer":editing})));
            Ok(())
        }).await?;
        let execute=async {
            let mut request=json!({"runId":remote_id,"instruction":instruction,
                "budgetKrw":remaining,
                "maxSteps":origin.max_steps.min(20).max(1),
                "writer":editing});
            if let Some((_,ref parent_sha,_,ref repository,ref seed,ref seed_hash))
                =writer_context{
                request["repository"]=json!(repository);
                request["baseCommit"]=json!(parent_sha);
                request["seedPatch"]=json!(seed);
                request["seedSha256"]=json!(seed_hash);
            }
            let mut post=self.client.post(Self::url_for(&endpoint,None))
                .header(AUTH_HEADER,&worker_token);
            if let Some(secret)=writer_token.as_ref(){
                if editing{post=post.header("x-dominion-worker-writer-token",secret);}
            }
            let response=post.json(&request).send().await;
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
                let mut inspect=self.client.get(Self::url_for(&endpoint,Some(remote_id)))
                    .header(AUTH_HEADER,&worker_token);
                if editing{
                    if let Some(secret)=writer_token.as_ref(){
                        inspect=inspect.header("x-dominion-worker-writer-token",secret);
                    }
                }
                let response=inspect.send().await;
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
                // Reconcile positive spend even if the remote model fails,
                // pauses for approval, or is cancelled before producing text.
                if let Err(error)=Self::record_remote_charge(store,id,call,
                    remote_id,&view).await{
                    return ToolEffectOutcome::Uncertain(format!(
                        "remote model cost reconciliation failed: {error}"));
                }
                match view["status"].as_str(){
                    Some("completed")=>{
                        let Some(answer)=view["answer"].as_str().filter(|v|!v.trim().is_empty())else{
                            return ToolEffectOutcome::Uncertain("remote worker completed without an answer receipt".into());
                        };
                        let mut patch_meta=Value::Null;
                        if let Some((parent_id,ref parent_sha,ref child_sha,
                            _,_,_))=writer_context{
                            if view["writer"]!=true{
                                return ToolEffectOutcome::Uncertain(
                                    "remote worker did not attest Writer mode".into());
                            }
                            let Some(patch)=view["patch"].as_str() else{
                                return ToolEffectOutcome::Uncertain(
                                    "remote writer omitted the exact Git patch".into());
                            };
                            let Some(digest)=view["patchSha256"].as_str() else{
                                return ToolEffectOutcome::Uncertain(
                                    "remote writer omitted patch digest".into());
                            };
                            if patch.len()>131_072||patch.is_empty()
                                ||format!("{:x}",Sha256::digest(patch.as_bytes()))!=digest{
                                return ToolEffectOutcome::Uncertain(
                                    "remote writer patch digest or length invalid".into());
                            }
                            let intent=store.mutate(id,|run|{
                                if run.in_flight.as_ref().is_none_or(|pending|
                                    pending.id!=call.id||pending.name!=EDIT_TOOL){
                                    return Err("remote Writer lost approved tool intent".into());
                                }
                                run.event("remote_writer_patch_apply_intent",
                                    "Approved isolated Team Writer patch SHA journalled before Git apply",
                                    Some(json!({"remoteRunId":remote_id,
                                        "sha256":digest,"parentId":parent_id,
                                        "workerId":id})));
                                Ok(())
                            }).await;
                            if let Err(error)=intent{
                                return ToolEffectOutcome::Uncertain(format!(
                                    "remote patch intent not durable: {error}"));
                            }
                            let installed=crate::workspace::apply_remote_writer_patch(
                                settings,parent_id,id,parent_sha,child_sha,patch,digest).await;
                            let changed=match installed{
                                Ok(paths)=>paths,
                                Err(error)=>return ToolEffectOutcome::Uncertain(format!(
                                    "remote Writer Git apply outcome requires reconciliation: {error}"))
                            };
                            let receipt=store.mutate(id,|run|{
                                run.event("remote_writer_patch_installed",
                                    "Remote Writer patch applied only to isolated Team child",
                                    Some(json!({"remoteRunId":remote_id,"sha256":digest,
                                        "changedPaths":changed,
                                        "requiresTeamMergeApproval":true})));
                                Ok(())
                            }).await;
                            if receipt.is_err(){
                                return ToolEffectOutcome::Uncertain(
                                    "remote patch applied but durable receipt failed; reconcile".into());
                            }
                            patch_meta=json!({"sha256":digest,
                                "changedPaths":changed,
                                "installedInIsolatedWorker":true,
                                "requiresTeamMergeApproval":true});
                        }
                        return ToolEffectOutcome::Settled(Ok(json!({
                            "source":"dominion_remote_worker","remoteRunId":remote_id,
                            "nodeId":node_id,"remoteWriterPatch":patch_meta,
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
                let mut request=self.client.post(format!("{}/cancel",
                    Self::url_for(&endpoint,Some(remote_id))))
                    .header(AUTH_HEADER,&worker_token);
                if editing {
                    if let Some(secret)=writer_token.as_ref(){
                        request=request.header("x-dominion-worker-writer-token",secret);
                    }
                }
                let outcome=tokio::time::timeout(Duration::from_secs(3),
                    request.send()).await;
                ToolEffectOutcome::Uncertain(format!(
                    "remote worker {remote_id} cancellation or channel loss after dispatch: {signal:?}; remote cancel acknowledgement: {}",
                    outcome.as_ref().ok().and_then(|v|v.as_ref().ok())
                        .is_some_and(|v|v.status().is_success())))
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
        let mut settings=crate::config::Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        let output=worker.execute(&settings,&store,origin_id,&call,cancel_rx).await.unwrap();
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
    #[tokio::test]
    async fn remote_failure_usage_is_metered_incrementally_without_double_charge(){
        use crate::{domain::{CreateRun,Run},store::Store};
        let root=std::env::temp_dir().join(format!(
            "dominion-remote-usage-{}",Uuid::new_v4()));
        let store=Arc::new(RunStorePort::new(Arc::new(
            Store::open(root.clone()).await.unwrap())));
        let mut run=Run::new(CreateRun{
            instruction:"Remote usage ledger".into(),
            budget_krw:Some(15.0),max_steps:Some(3)},3).unwrap();
        let id=run.id;
        let call=ToolCall{id:"billing-1".into(),name:TOOL.into(),
            arguments:json!({"instruction":"inspect"})};
        run.in_flight=Some(call.clone());
        store.put(run).await.unwrap();
        let remote_id=Uuid::new_v4();
        for cost in [1.0,3.5,3.5]{
            RemoteWorker::record_remote_charge(&store,id,&call,remote_id,
                &json!({"status":"failed","costKrw":cost,
                    "inputTokens":50,"outputTokens":9})).await.unwrap();
        }
        let end=store.get(id).await.unwrap();
        assert_eq!(end.remote_cost_krw,3.5);
        assert_eq!(crate::policy::effective_budget_krw(&end),11.5);
        assert_eq!(end.events.iter().filter(|e|e.kind=="remote_worker_metered").count(),2);
        assert!(RemoteWorker::record_remote_charge(&store,id,&call,remote_id,
            &json!({"status":"failed","costKrw":2.0})).await.is_err());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn authenticated_pool_rejects_unhealthy_node_and_routes_to_healthy_peer(){
        use axum::{extract::State,http::StatusCode,routing::get,Json,Router};
        async fn capabilities(State(healthy):State<bool>)
            ->(StatusCode,Json<Value>){
            if !healthy{return (StatusCode::SERVICE_UNAVAILABLE,Json(json!({
                "schemaVersion":1,"active":0,"slots":4,"readOnly":true})))}
            (StatusCode::OK,Json(json!({
                "schemaVersion":1,"active":0,"slots":4,"readOnly":true,
                "writer":true,
                "writerRepositories":["projects/repo"]})))
        }
        let unhealthy_socket=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let healthy_socket=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a=unhealthy_socket.local_addr().unwrap();
        let b=healthy_socket.local_addr().unwrap();
        let up=tokio::spawn(async move{
            axum::serve(healthy_socket,Router::new()
                .route("/api/v1/remote-worker/capabilities",get(capabilities))
                .with_state(true)).await.unwrap();
        });
        let down=tokio::spawn(async move{
            axum::serve(unhealthy_socket,Router::new()
                .route("/api/v1/remote-worker/capabilities",get(capabilities))
                .with_state(false)).await.unwrap();
        });
        let mut worker=RemoteWorker::configured(&format!("http://{b}"),
            "token-with-at-least-thirty-two-characters".into()).unwrap();
        worker.nodes=vec![
            RegistryNode{id:"down".into(),url:format!("http://{a}"),
                token:"token-with-at-least-thirty-two-characters".into(),
                writer_token:None,weight:16},
            RegistryNode{id:"up".into(),url:format!("http://{b}"),
                token:"token-with-at-least-thirty-two-characters".into(),
                writer_token:Some("writer-token-at-least-thirty-two-characters".into()),
                weight:1}
        ];
        let selected=worker.select_node(Uuid::new_v4(),"call",false,None).await.unwrap();
        assert_eq!(selected.0,"up");
        let writing=worker.select_node(Uuid::new_v4(),"writer",true,
            Some("projects/repo")).await.unwrap();
        assert_eq!(writing.0,"up");
        down.abort();up.abort();
    }
    #[tokio::test]
    async fn passive_remote_topology_never_discloses_scoped_secrets(){
        let token="redacted-operator-credential-".repeat(2);
        let mut worker=RemoteWorker::configured(
            "http://127.0.0.1:65530",token.clone()).unwrap();
        worker.nodes=vec![RegistryNode{
            id:"node_a".into(),url:"http://127.0.0.1:65530".into(),
            token:token.clone(),writer_token:Some("writer-private-".repeat(4)),
            weight:3
        }];
        let view=worker.operator_topology(false).await;
        assert_eq!(view["configured"],true);
        assert_eq!(view["probed"],false);
        assert_eq!(view["nodes"][0]["state"],"not_probed");
        assert_eq!(view["nodes"][0]["writerConfigured"],true);
        assert_eq!(view["nodes"][0]["weight"],3);
        assert!(!view.to_string().contains(&token));
        assert!(!view.to_string().contains("writer-private-"));
        // An explicit connection probe must be read-only: an unreachable
        // worker does not cause model calls, retries or credential disclosure.
        let inspected=worker.operator_topology(true).await;
        assert_ne!(inspected["nodes"][0]["state"],"ready");
        assert!(!inspected.to_string().contains(&token));
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
