//! Operator-owned provider registry for the unmodified main Models page.
//! Credentials never enter frontend responses, Run transcripts, or SDK events.
//! Provider identifiers are bound to the actual model used by native workers.
use crate::config::ModelSettings;
use reqwest::Url;
use serde::{Deserialize,Serialize};
use std::{collections::HashMap,fs,io::Write,path::{Path,PathBuf},sync::Mutex,time::Duration};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Provider {
    pub id:String,
    pub name:String,
    pub base_url:String,
    pub kind:String,
    pub model:String,
    pub enabled:bool,
    pub local:bool,
    #[serde(default)]pub api_key:String,
    #[serde(default)]pub reflection:bool,
    #[serde(default)]pub reflection_model:String,
    #[serde(default)]pub requests_per_minute:u32,
}
#[derive(Default,Clone,Serialize,Deserialize)]
struct Snapshot{entries:HashMap<String,Provider>}
pub struct Registry{
    file:PathBuf,entries:Mutex<Snapshot>,
    next_request:tokio::sync::Mutex<HashMap<String,tokio::time::Instant>>,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Draft{
    pub id:Option<String>,
    pub name:String,
    pub base_url:String,
    #[serde(default="default_kind")] pub kind:String,
    #[serde(default)]pub model:String,
    #[serde(default)]pub api_key:Option<String>,
    #[serde(default="enabled")]pub enabled:bool,
    #[serde(default)]pub local:bool,
    #[serde(default)]pub reflection:bool,
    #[serde(default)]pub reflection_model:Option<String>,
    #[serde(default)]pub requests_per_minute:u32,
}
fn default_kind()->String{"openai".into()}
fn enabled()->bool{true}
fn store(file:&Path,snapshot:&Snapshot)->Result<(),String>{
    let data=serde_json::to_vec(snapshot).map_err(|e|e.to_string())?;
    if data.len()>2*1024*1024{return Err("provider registry exceeds 2 MiB".into());}
    let dir=file.parent().ok_or("provider state path has no parent")?;
    let staged=dir.join(format!(".providers-{}.tmp",Uuid::new_v4()));
    let result=(||->Result<(),String>{
        let mut opts=fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]{
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f=opts.open(&staged).map_err(|e|e.to_string())?;
        f.write_all(&data).map_err(|e|e.to_string())?;
        f.sync_all().map_err(|e|e.to_string())?;
        drop(f);
        fs::rename(&staged,file).map_err(|e|e.to_string())?;
        #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
        Ok(())
    })();
    if result.is_err(){let _=fs::remove_file(staged);}
    result
}
pub fn normalized_url(raw:&str)->Result<(String,bool),String>{
    let mut url=Url::parse(raw.trim()).map_err(|e|format!("provider URL: {e}"))?;
    let host=url.host_str().ok_or("missing provider host")?.to_ascii_lowercase();
    if !url.username().is_empty()||url.password().is_some()||url.fragment().is_some()||
       url.query().is_some(){return Err("provider URL cannot contain secrets, query, or fragments".into());}
    let loopback=host=="localhost"||host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip|ip.is_loopback());
    if url.scheme()!="https" && !(url.scheme()=="http"&&loopback){
        return Err("provider URL requires HTTPS, except loopback HTTP".into());
    }
    if matches!(host.as_str(),"169.254.169.254"|"metadata.google.internal"|"metadata"|"0.0.0.0"){
        return Err("provider metadata endpoints are blocked".into());
    }
    if host.ends_with(".local")&&!loopback{return Err("local-network provider requires explicit loopback address".into());}
    if let Ok(ip)=host.parse::<std::net::IpAddr>(){
        let blocked=match ip {
            std::net::IpAddr::V4(v)=>{
                let oct=v.octets();
                v.is_unspecified()||v.is_broadcast()||v.is_multicast()||
                oct[0]==10||(oct[0]==172&&(16..=31).contains(&oct[1]))||
                (oct[0]==192&&oct[1]==168)||(oct[0]==169&&oct[1]==254)||
                oct[0]>=240
            },
            std::net::IpAddr::V6(v)=>v.is_unspecified()||v.is_multicast()||
                (v.segments()[0]&0xfe00)==0xfc00||
                (v.segments()[0]&0xffc0)==0xfe80
        };
        if blocked&&!loopback{
            return Err("private, link-local and metadata IP endpoints are blocked".into());
        }
    }
    url.set_fragment(None);
    Ok((url.to_string().trim_end_matches('/').to_owned(),loopback))
}
impl Registry{
    pub fn open(root:PathBuf)->Result<Self,String>{
        fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("providers.json");
        let state=if file.exists(){
            #[cfg(unix)]{
                use std::os::unix::fs::PermissionsExt;
                let metadata=fs::metadata(&file).map_err(|e|e.to_string())?;
                if metadata.permissions().mode()&0o077!=0{
                    return Err("providers.json permissions expose API credentials: chmod 600 required".into());
                }
            }
            serde_json::from_slice::<Snapshot>(&fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("invalid provider registry: {e}"))?
        }else{Snapshot::default()};
        Ok(Self{file,entries:Mutex::new(state),
    next_request:tokio::sync::Mutex::new(HashMap::new())})
    }
    pub fn list(&self)->Vec<Provider>{
        let mut v=self.entries.lock().unwrap_or_else(|e|e.into_inner()).entries.values().cloned().collect::<Vec<_>>();
        v.sort_by(|a,b|a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }
    pub fn get(&self,id:&str)->Option<Provider>{
        self.entries.lock().unwrap_or_else(|e|e.into_inner()).entries.get(id).cloned()
    }
    pub fn upsert(&self,draft:Draft)->Result<Provider,String>{
        if draft.name.trim().is_empty()||draft.name.len()>120||draft.model.len()>240
            ||draft.requests_per_minute>100_000{return Err("invalid provider name/model/rate".into());}
        if !matches!(draft.kind.as_str(),"openai"|"ollama"){
            return Err("native model client currently supports OpenAI-compatible APIs and Ollama; other protocols require a native adapter".into());
        }
        let (base_url,loopback)=normalized_url(&draft.base_url)?;
        if draft.local&&!loopback{
            return Err("a local provider must use an actual loopback endpoint".into());
        }
        let mut guard=self.entries.lock().unwrap_or_else(|e|e.into_inner());
        let mut next=guard.clone();
        let old=draft.id.as_deref().and_then(|id|next.entries.get(id).cloned());
        if draft.id.is_some()&&old.is_none(){return Err("provider to edit was not found".into());}
        if old.is_none()&&next.entries.len()>=128{return Err("maximum 128 providers".into());}
        let id=draft.id.unwrap_or_else(||Uuid::new_v4().to_string());
        if id.len()>120{return Err("invalid provider ID".into());}
        let key=draft.api_key.filter(|key|!key.is_empty())
            .or_else(||old.as_ref().map(|r|r.api_key.clone()))
            .unwrap_or_default();
        if key.len()>8192||key.contains('\r')||key.contains('\n'){
            return Err("invalid provider credential".into());
        }
        let record=Provider{
            id:id.clone(),name:draft.name.trim().into(),base_url,
            kind:draft.kind,model:draft.model.trim().into(),api_key:key,
            enabled:draft.enabled,local:loopback,reflection:draft.reflection,
            reflection_model:draft.reflection_model.unwrap_or_default(),
            requests_per_minute:draft.requests_per_minute,
        };
        next.entries.insert(id,record.clone());
        store(&self.file,&next)?;
        *guard=next;
        Ok(record)
    }
    pub fn delete(&self,id:&str)->Result<(),String>{
        let mut guard=self.entries.lock().unwrap_or_else(|e|e.into_inner());
        let mut next=guard.clone();
        next.entries.remove(id).ok_or("provider does not exist")?;
        store(&self.file,&next)?;*guard=next;Ok(())
    }
    pub async fn throttle(&self,id:&str){
        let rpm=self.get(id).map_or(0,|provider|provider.requests_per_minute);
        if rpm==0{return;}
        let interval=Duration::from_millis((60_000u64/u64::from(rpm)).max(1));
        let mut slots=self.next_request.lock().await;
        let now=tokio::time::Instant::now();
        let start=slots.get(id).copied().unwrap_or(now).max(now);
        slots.insert(id.to_owned(),start+interval);
        drop(slots);
        tokio::time::sleep_until(start).await;
    }
    pub fn resolve(&self,id:&str,model:&str,fallback:&ModelSettings)->Result<ModelSettings,String>{
        if id==""||id=="dominion-native"{
            if model.is_empty()||model==fallback.name{return Ok(fallback.clone());}
            return Err("selected model does not match the configured native model".into());
        }
        let record=self.get(id).ok_or("provider is not registered")?;
        if !record.enabled{return Err("provider is disabled".into());}
        if model.is_empty()||model!=record.model||record.model.is_empty(){
            return Err("selected model is not the provider's configured live model".into());
        }
        // Prices are not fabricated from provider names. They need the owner's
        // explicit cost table before any financial accounting claim.
        Ok(ModelSettings{
            name:record.model,base_url:record.base_url,api_key:record.api_key,
            input_krw_per_million:0.0,cached_input_krw_per_million:0.0,
            output_krw_per_million:0.0,cache_write_krw_per_million:0.0,
            cache_ttl:None,cache_key:None,parallel_tool_calls:fallback.parallel_tool_calls,
        })
    }
}
pub fn public(record:&Provider)->serde_json::Value{
    serde_json::json!({
        "id":record.id,"name":record.name,"baseUrl":record.base_url,
        "kind":record.kind,"model":record.model,"enabled":record.enabled,
        "local":record.local,"hasKey":!record.api_key.is_empty()||record.local,
        "reflection":record.reflection,"reflectionModel":record.reflection_model,
        "requestsPerMinute":record.requests_per_minute,
        "pricing":"unconfigured"
    })
}
