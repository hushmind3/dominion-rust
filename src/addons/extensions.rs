//! Original main Extensions UI contracts: durable managed SKILL.md records
//! and live MCP Streamable HTTP registration. Never run untrusted installer
//! commands or pretend that an unsupported stdio server was installed.
use crate::{engine::Engine,mcp::{Mcp,Server}};
use axum::{
    extract::{Path,State},
    http::{header,StatusCode},response::IntoResponse,routing::get,Json,Router
};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::Write,path::PathBuf,sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
fn bad(message:impl Into<String>)->Fault{(StatusCode::BAD_REQUEST,Json(json!({"error":message.into()})))}
fn missing(message:&str)->Fault{(StatusCode::NOT_FOUND,Json(json!({"error":message})))}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ManagedMcp {
    pub id:String,pub name:String,pub url:String,pub enabled:bool,
    #[serde(default)]pub bearer_token:String,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ManagedSkill {
    pub id:String,pub name:String,pub instructions:String,pub enabled:bool,
    #[serde(default)]pub source:String,
}
#[derive(Clone,Default,Serialize,Deserialize)]
struct Snapshot {
    #[serde(default)]mcp:HashMap<String,ManagedMcp>,
    #[serde(default)]skills:HashMap<String,ManagedSkill>,
}
pub struct Extensions{
    file:PathBuf,state:Mutex<Snapshot>,initial:Vec<Server>
}
fn server_key(id:&str)->String{
    if id.len()==36&&Uuid::parse_str(id).is_ok(){
        format!("mcp_{}",id.replace('-',""))
    }else{id.to_owned()}
}
fn servers(snapshot:&Snapshot)->Vec<Server>{
    snapshot.mcp.values().filter(|m|m.enabled).map(|m|Server{
        name:server_key(&m.id),url:m.url.clone(),bearer_token:m.bearer_token.clone()
    }).collect()
}
fn persist(file:&std::path::Path,next:&Snapshot)->Result<(),String>{
    let data=serde_json::to_vec(next).map_err(|e|e.to_string())?;
    if data.len()>4*1024*1024{return Err("extensions state exceeds 4 MiB".into());}
    let dir=file.parent().ok_or("invalid extensions state directory")?;
    let tmp=dir.join(format!(".extensions-{}.tmp",Uuid::new_v4()));
    let result=(||->Result<(),String>{
        let mut opts=fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]{
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut stage=opts.open(&tmp).map_err(|e|e.to_string())?;
        stage.write_all(&data).map_err(|e|e.to_string())?;
        stage.sync_all().map_err(|e|e.to_string())?;
        drop(stage);
        fs::rename(&tmp,file).map_err(|e|e.to_string())?;
        #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
        Ok(())
    })();
    if result.is_err(){let _=fs::remove_file(&tmp);}
    result
}

impl Extensions{
    /// Inactive integration snapshot: do not read managed credentials, MCP
    /// endpoints or persisted extensions when the operator disables add-ons.
    pub fn inactive(root:PathBuf)->Self{
        Self{file:root.join("extensions.json"),
            state:Mutex::new(Snapshot::default()),initial:Vec::new()}
    }
    pub fn open(root:PathBuf,initial:Vec<Server>)->Result<Self,String>{
        fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("extensions.json");
        let state=if file.exists(){
            #[cfg(unix)]{
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(&file).map_err(|e|e.to_string())?
                    .permissions().mode()&0o077!=0{
                    return Err("extensions.json permissions expose MCP credentials; chmod 600 required".into());
                }
            }
            serde_json::from_slice::<Snapshot>(&fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("corrupt extensions state: {e}"))?
        }else{
            let mut snapshot=Snapshot::default();
            for server in initial{
                snapshot.mcp.insert(server.name.clone(),ManagedMcp{
                    id:server.name.clone(),name:server.name,url:server.url,enabled:true,
                    bearer_token:server.bearer_token
                });
            }
            snapshot
        };
        let initial=servers(&state);
        Mcp::new(initial.clone())?; // Validate before the server starts.
        Ok(Self{file,state:Mutex::new(state),initial})
    }
    pub fn initial_servers(&self)->Vec<Server>{self.initial.clone()}
    pub async fn mcp_list(&self)->Vec<ManagedMcp>{
        let mut v=self.state.lock().await.mcp.values().cloned().collect::<Vec<_>>();
        v.sort_by(|a,b|a.name.cmp(&b.name));v
    }
    pub async fn skill_list(&self)->Vec<ManagedSkill>{
        let mut v=self.state.lock().await.skills.values().cloned().collect::<Vec<_>>();
        v.sort_by(|a,b|a.name.cmp(&b.name));v
    }
    pub async fn skill(&self,id:&str)->Option<ManagedSkill>{
        self.state.lock().await.skills.get(id).cloned()
    }
    pub async fn upsert_mcp(&self,mcp:&Mcp,item:ManagedMcp)->Result<ManagedMcp,String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        if next.mcp.len()>=8 && !next.mcp.contains_key(&item.id){
            return Err("maximum 8 MCP servers".into());
        }
        next.mcp.insert(item.id.clone(),item.clone());
        let enabled=servers(&next);
        Mcp::new(enabled.clone())?; // Endpoint and name validation before commit.
        persist(&self.file,&next)?;
        mcp.replace_servers(enabled).await?;
        *guard=next;
        Ok(item)
    }
    pub async fn delete_mcp(&self,mcp:&Mcp,id:&str)->Result<(),String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        next.mcp.remove(id).ok_or("MCP server not found")?;
        let enabled=servers(&next);
        Mcp::new(enabled.clone())?;
        persist(&self.file,&next)?;
        mcp.replace_servers(enabled).await?;
        *guard=next;Ok(())
    }
    pub async fn upsert_skill(&self,item:ManagedSkill,update:bool)->Result<ManagedSkill,String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        if update&&!next.skills.contains_key(&item.id){return Err("skill to update not found".into());}
        if next.skills.len()>=64&&!next.skills.contains_key(&item.id){
            return Err("maximum 64 managed skills".into());
        }
        next.skills.insert(item.id.clone(),item.clone());
        persist(&self.file,&next)?;
        *guard=next;
        Ok(item)
    }
    pub async fn delete_skill(&self,id:&str)->Result<(),String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        next.skills.remove(id).ok_or("skill not found")?;
        persist(&self.file,&next)?;
        *guard=next;Ok(())
    }
}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/mcp",get(mcp_list).post(mcp_save))
        .route("/mcp/discover",get(mcp_discover))
        .route("/mcp/{id}",axum::routing::delete(mcp_delete))
        .route("/skills",get(skill_list).post(skill_save))
        .route("/skills/{id}",axum::routing::delete(skill_delete))
        .route("/skills/{id}/export",get(skill_export))
        // GitHub imports are handled by the pinned-source native importer.
}
async fn mcp_list(State(engine):State<Arc<Engine>>)->Json<Value>{
    Json(json!(engine.addons.extensions.mcp_list().await.iter().map(|v|json!({
        "id":v.id,"name":v.name,"transport":"http",
        "url":v.url,"enabled":v.enabled
    })).collect::<Vec<_>>()))
}
fn managed_mcp_owner<'a>(name:&str,registered:&'a [(String,String)])->Option<&'a str>{
    registered.iter().find(|(prefix,_)|name.starts_with(prefix))
        .map(|(_,id)|id.as_str())
}
async fn mcp_discover(State(engine):State<Arc<Engine>>)->Json<Value>{
    // The model uses mcp__<server_key>__<tool>, while the UI stores a UUID.
    // Join the real native tool catalog with the owner's active MCP registry.
    let registered=engine.addons.extensions.mcp_list().await.into_iter()
        .filter(|server|server.enabled)
        .map(|server|(format!("mcp__{}__",server_key(&server.id)),server.id))
        .collect::<Vec<_>>();
    let definitions=engine.addons.mcp.definitions().await;
    Json(json!(definitions.iter().filter_map(|d|{
        let name=d.pointer("/function/name").and_then(Value::as_str)?;
        let description=d.pointer("/function/description")
            .and_then(Value::as_str).unwrap_or("");
        Some(json!({"name":name,"description":description,
            "mcpId":managed_mcp_owner(name,&registered)}))
    }).collect::<Vec<_>>()))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct McpDraft{
    id:Option<String>,name:String,
    #[serde(default)]url:String,
    #[serde(default)]enabled:Option<bool>,
    #[serde(default)]bearer_token:Option<String>,
}
async fn mcp_save(State(engine):State<Arc<Engine>>,Json(input):Json<McpDraft>)
    ->Result<Json<Value>,Fault>{
    if input.name.trim().is_empty()||input.name.len()>120||
        input.url.len()>1024||input.bearer_token.as_deref().is_some_and(|s|s.len()>8192){
        return Err(bad("invalid MCP server name, URL or credential size"));
    }
    let id=input.id.as_deref().filter(|s|!s.is_empty())
        .map(str::to_owned).unwrap_or_else(||Uuid::new_v4().to_string());
    if id.len()>120{return Err(bad("MCP ID is too long"));}
    let old=engine.addons.extensions.mcp_list().await.into_iter().find(|x|x.id==id);
    if input.id.as_deref().is_some_and(|s|!s.is_empty())&&old.is_none(){
        return Err(bad("MCP server to update does not exist"));
    }
    let record=ManagedMcp{
        id,name:input.name.trim().into(),url:input.url,
        enabled:input.enabled.unwrap_or(true),
        bearer_token:input.bearer_token.filter(|s|!s.is_empty())
            .or_else(||old.map(|x|x.bearer_token)).unwrap_or_default(),
    };
    let saved=engine.addons.extensions.upsert_mcp(&engine.addons.mcp,record).await.map_err(bad)?;
    Ok(Json(json!({"id":saved.id,"name":saved.name,"transport":"http",
        "url":saved.url,"enabled":saved.enabled})))
}
async fn mcp_delete(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    engine.addons.extensions.delete_mcp(&engine.addons.mcp,&id).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn skill_list(State(engine):State<Arc<Engine>>)->Json<Vec<Value>>{
    Json(engine.addons.extensions.skill_list().await.iter().map(|item|json!({
        "id":item.id,"name":item.name,"instructions":item.instructions,
        "enabled":item.enabled,"status":if item.enabled{"active"}else{"disabled"},
        "source":item.source
    })).collect())
}
#[derive(Deserialize)]
struct SkillDraft{
    id:Option<String>,name:String,instructions:String,
    #[serde(default="enabled")]enabled:bool,
    #[serde(default)]source:String
}
fn enabled()->bool{true}
async fn skill_save(State(engine):State<Arc<Engine>>,Json(input):Json<SkillDraft>)
    ->Result<Json<Value>,Fault>{
    if input.name.trim().is_empty()||input.name.len()>160||
        input.instructions.trim().is_empty()||input.instructions.len()>32_768{
        return Err(bad("skill name/instructions invalid (max 32 KiB)"));
    }
    let update=input.id.as_ref().is_some_and(|id|!id.is_empty());
    if let Some(ref id)=input.id{
        if !id.is_empty()&&Uuid::parse_str(id).is_err(){
            return Err(bad("managed skill id is not a valid UUID"));
        }
    }
    let record=ManagedSkill{
        id:input.id.filter(|s|!s.is_empty()).unwrap_or_else(||Uuid::new_v4().to_string()),
        name:input.name.trim().into(),
        instructions:input.instructions,
        enabled:input.enabled,
        source:if input.source.is_empty(){"user".into()}else{input.source}
    };
    let record=engine.addons.extensions.upsert_skill(record,update).await.map_err(bad)?;
    Ok(Json(json!({"id":record.id,"name":record.name,
        "instructions":record.instructions,"enabled":record.enabled,
        "status":if record.enabled{"active"}else{"disabled"},
        "source":record.source})))
}
async fn skill_delete(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    engine.addons.extensions.delete_skill(&id).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn skill_export(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<impl IntoResponse,Fault>{
    let record=engine.addons.extensions.skill(&id).await.ok_or_else(||missing("skill not found"))?;
    let body=format!("# {}\n\n{}\n",record.name,record.instructions);
    Ok(([(header::CONTENT_TYPE,"text/markdown; charset=utf-8"),
        (header::CONTENT_DISPOSITION,"attachment; filename=SKILL.md"),
        (header::CACHE_CONTROL,"no-store")],body))
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    #[test]
    fn mcp_bearer_field_matches_live_browser_contract(){
        let input: McpDraft=serde_json::from_value(json!({
            "name":"Remote MCP", "url":"https://example.org/mcp",
            "bearerToken":"test-token", "enabled":true
        })).unwrap();
        assert_eq!(input.bearer_token.as_deref(),Some("test-token"));
    }
    #[test]
    fn native_mcp_alias_resolves_to_persisted_managed_id() {
        let uuid="2b35e369-1c79-498f-b850-439af3cdb024";
        let registered=vec![(format!("mcp__{}__",server_key(uuid)),uuid.into())];
        assert_eq!(managed_mcp_owner(
            "mcp__mcp_2b35e3691c79498fb850439af3cdb024__list_tools",
            &registered),Some(uuid));
        assert_eq!(managed_mcp_owner("mcp__unregistered__list_tools",&registered),None);
        assert_eq!(managed_mcp_owner("mcp__mcp_2b35e3691c79498fb850439af3cdb025__list_tools",&registered),None);
    }
}
