//! Explicit, read-only import of the 2026 main application's SQLite chat data.
//! The source remains owned by the main app. Never execute old jobs or move
//! encrypted credentials across incompatible vaults.
use crate::frontend_state::{FrontendStore,UiAgent,UiMessage,UiSession};
use crate::provider_registry::{Provider,Registry,normalized_url};
use rusqlite::{Connection,OpenFlags};
use serde::Serialize;
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},path::{Path,PathBuf},time::Duration};

struct Source {
    agents:HashMap<String,UiAgent>,
    sessions:HashMap<String,UiSession>,
    providers:Vec<Provider>,
    skipped_providers:usize,
    skipped_bound:usize,
    skipped_archived:usize,
    messages:usize,
}

#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct LegacyPreview {
    pub source_agents:usize,
    pub source_providers:usize,
    pub importable_providers:usize,
    pub already_present_providers:usize,
    pub skipped_provider_records:usize,
    pub source_sessions:usize,
    pub source_messages:usize,
    pub importable_agents:usize,
    pub importable_sessions:usize,
    pub already_present_agents:usize,
    pub already_present_sessions:usize,
    pub skipped_bound_sessions:usize,
    pub skipped_archived_sessions:usize,
    pub note:&'static str,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct LegacyImportResult {
    pub imported_agents:usize,
    pub imported_providers:usize,
    pub existing_providers:usize,
    pub skipped_provider_records:usize,
    pub imported_sessions:usize,
    pub imported_messages:usize,
    pub existing_agents:usize,
    pub existing_sessions:usize,
    pub skipped_bound_sessions:usize,
    pub skipped_archived_sessions:usize,
}

fn field<'a>(value:&'a Value,name:&str)->&'a str{
    value.get(name).and_then(Value::as_str).unwrap_or("")
}
fn date(value:&str)->String {
    if value.is_empty(){return "1970-01-01T00:00:00Z".into();}
    if value.contains('T'){return value.into();}
    if value.len()==19 && value.as_bytes()[10]==b' '{
        return format!("{}T{}Z",&value[..10],&value[11..]);
    }
    value.into()
}
pub(crate) fn open_legacy(path:&Path)->Result<Connection,String>{
    if path.file_name().and_then(|s|s.to_str())!=Some("dominion.sqlite"){
        return Err("select the main application's dominion.sqlite file".into());
    }
    let canonical=path.canonicalize().map_err(|e|format!("legacy SQLite path: {e}"))?;
    if !canonical.is_file(){return Err("legacy SQLite source must be a regular file".into());}
    // No CREATE, journal mode change, schema migration, or credentials read.
    // Normal read-only WAL access sees a transactionally consistent main DB.
    let db=Connection::open_with_flags(&canonical,OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e|format!("open read-only legacy DB: {e}"))?;
    db.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
    db.execute_batch("PRAGMA query_only=ON; BEGIN DEFERRED TRANSACTION;")
        .map_err(|e|format!("begin legacy read snapshot: {e}"))?;
    Ok(db)
}
fn source(path:&Path)->Result<Source,String>{
    let db=open_legacy(path)?;
    let mut agents=HashMap::new();
    let mut agent_query=db.prepare("SELECT id,data FROM records WHERE kind='agent' ORDER BY id")
        .map_err(|e|format!("legacy records schema: {e}"))?;
    let agent_rows=agent_query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))
        .map_err(|e|e.to_string())?;
    for item in agent_rows {
        let (id,raw)=item.map_err(|e|e.to_string())?;
        if id.trim().is_empty(){return Err("legacy agent has an empty ID".into());}
        let value:Value=serde_json::from_str(&raw)
            .map_err(|e|format!("legacy agent {id} JSON: {e}"))?;
        let agent=UiAgent{
            id:id.clone(),name:field(&value,"name").to_string(),
            description:field(&value,"description").to_string(),
            system_prompt:field(&value,"systemPrompt").to_string(),
            provider_id:field(&value,"providerId").to_string(),
            model:field(&value,"model").to_string(),
            color:field(&value,"color").to_string(),
            role:field(&value,"role").to_string(),
            reasoning_effort:value.get("reasoningEffort").and_then(Value::as_str).map(str::to_string),
            thinking:value.get("thinking").and_then(Value::as_bool),
            aliases:value.get("aliases").and_then(Value::as_array).map(|a|
                a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
            avatar_url:field(&value,"avatarUrl").to_string(),
            avatar:field(&value,"avatar").to_string(),
        };
        if agent.name.trim().is_empty(){return Err(format!("legacy agent {id} missing name"));}
        agents.insert(id,agent);
    }
    drop(agent_query);

    // Configurations only: no API key, credential metadata or OAuth tokens
    // are copied. A disabled native provider can be reviewed and reconnected.
    let mut providers=Vec::new();
    let mut skipped_providers=0;
    let mut provider_query=db.prepare("SELECT id,data FROM records WHERE kind='provider' ORDER BY id")
        .map_err(|e|format!("legacy provider schema: {e}"))?;
    let rows=provider_query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))
        .map_err(|e|e.to_string())?;
    for row in rows{
        let (id,raw)=row.map_err(|e|e.to_string())?;
        let value:Value=serde_json::from_str(&raw)
            .map_err(|e|format!("legacy provider {id} JSON: {e}"))?;
        let kind=field(&value,"kind");
        if id.is_empty()||id.len()>120||
           !matches!(kind,"openai"|"ollama"|"anthropic"){
            skipped_providers+=1;continue;
        }
        let Ok((url,local))=normalized_url(field(&value,"baseUrl")) else {
            skipped_providers+=1;continue;
        };
        let name=field(&value,"name");
        let model=field(&value,"model");
        if name.trim().is_empty()||name.len()>120||model.len()>240{
            skipped_providers+=1;continue;
        }
        providers.push(Provider{
            id,name:name.to_owned(),base_url:url,kind:kind.to_owned(),model:model.to_owned(),
            enabled:false,local,api_key:String::new(),
            reflection:false,reflection_model:String::new(),requests_per_minute:0
        });
    }
    drop(provider_query);

    let mut sessions=HashMap::new();
    let mut skipped_bound=0;
    let mut skipped_archived=0;
    let mut query=db.prepare(
        "SELECT id,data,created_at,updated_at FROM records WHERE kind='session' ORDER BY id")
        .map_err(|e|format!("legacy session schema: {e}"))?;
    let rows=query.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,
        r.get::<_,String>(2)?,r.get::<_,String>(3)?
    ))).map_err(|e|e.to_string())?;
    for item in rows {
        let (id,raw,created,updated)=item.map_err(|e|e.to_string())?;
        if id.trim().is_empty(){return Err("legacy session has an empty ID".into());}
        let value:Value=serde_json::from_str(&raw)
            .map_err(|e|format!("legacy session {id} JSON: {e}"))?;
        if value["archived"]==true{skipped_archived+=1;continue;}
        // Main's room and channel sessions require their own participant and
        // routing state. Never misrepresent them as ordinary private chats.
        if value["roomId"].as_str().is_some_and(|v|!v.is_empty())
            || value["channelContext"].is_object(){
            skipped_bound+=1;continue;
        }
        sessions.insert(id.clone(),UiSession{
            id,title:field(&value,"title").to_string(),
            agent_id:field(&value,"agentId").to_string(),
            provider_id:field(&value,"providerId").to_string(),
            model:field(&value,"model").to_string(),
            created_at:date(value["createdAt"].as_str().unwrap_or(&created)),
            updated_at:date(value["updatedAt"].as_str().unwrap_or(&updated)),
            room_id:None,pending_run:None,messages:Vec::new(),
        });
    }
    drop(query);

    // Earlier main databases may not have the later reasoning/tool columns.
    // Detect optional fields without altering the source schema.
    let mut schema=db.prepare("PRAGMA table_info(messages)")
        .map_err(|e|format!("legacy message table schema: {e}"))?;
    let cols=schema.query_map([],|row|row.get::<_,String>(1))
        .map_err(|e|e.to_string())?
        .collect::<Result<HashSet<_>,_>>().map_err(|e|e.to_string())?;
    drop(schema);
    if ["id","session_id","role","content","created_at"]
        .iter().any(|name|!cols.contains(*name)){
        return Err("legacy messages missing required core columns".into());
    }
    let optional=|name:&'static str,fallback:&'static str|{
        if cols.contains(name){name}else{fallback}
    };
    let sql=format!(
        "SELECT id,session_id,role,content,{},{},{},{},created_at,{} \
         FROM messages ORDER BY created_at ASC,rowid ASC",
        optional("reasoning","''"),optional("agent_id","NULL"),
        optional("tool_calls","NULL"),optional("tool_call_id","NULL"),
        optional("metadata","'{}'")
    );
    let mut message_query=db.prepare(&sql)
        .map_err(|e|format!("legacy messages schema: {e}"))?;
    let rows=message_query.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
        r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,
        r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?,
        r.get::<_,String>(8)?,r.get::<_,String>(9)?
    ))).map_err(|e|e.to_string())?;
    let mut messages=0usize;
    for item in rows {
        let (id,session_id,role,content,reasoning,agent_id,calls,call_id,created_at,raw_metadata)
            =item.map_err(|e|e.to_string())?;
        let Some(session)=sessions.get_mut(&session_id)else{continue};
        let mut metadata=serde_json::from_str::<Value>(&raw_metadata)
            .unwrap_or_else(|_|json!({"legacyRawMetadata":raw_metadata}));
        if !metadata.is_object(){metadata=json!({"originalMetadata":metadata});}
        // Keep the original model/tool diagnostics without promoting old
        // tool calls into live, executable Run intents.
        metadata["legacyImport"]=json!({
            "reasoning":reasoning,"toolCalls":calls.as_deref().map(|s|
                serde_json::from_str::<Value>(s)
                    .unwrap_or_else(|_|json!({"legacyRawToolCalls":s}))),
            "toolCallId":call_id,
        });
        session.messages.push(UiMessage{
            id,session_id,role,content,created_at:date(&created_at),
            agent_id,metadata:Some(metadata),
        });
        messages+=1;
    }
    db.execute_batch("COMMIT").map_err(|e|format!("finish legacy read snapshot: {e}"))?;
    Ok(Source{agents,sessions,providers,skipped_providers,skipped_bound,skipped_archived,messages})
}
fn counts(source:&Source,current:&crate::frontend_state::Snapshot,registry:&Registry)->LegacyPreview{
    let existing_agents=source.agents.keys().filter(|k|current.agents.contains_key(*k)).count();
    let existing_sessions=source.sessions.keys().filter(|k|current.sessions.contains_key(*k)).count();
    let present_providers=source.providers.iter().filter(|entry|registry.get(&entry.id).is_some()).count();
    LegacyPreview{
        source_agents:source.agents.len(),
        source_providers:source.providers.len()+source.skipped_providers,
        importable_providers:source.providers.len()-present_providers,
        already_present_providers:present_providers,
        skipped_provider_records:source.skipped_providers,
        source_sessions:source.sessions.len()+source.skipped_bound+source.skipped_archived,
        source_messages:source.messages,
        importable_agents:source.agents.len()-existing_agents,
        importable_sessions:source.sessions.len()-existing_sessions,
        already_present_agents:existing_agents,already_present_sessions:existing_sessions,
        skipped_bound_sessions:source.skipped_bound,
        skipped_archived_sessions:source.skipped_archived,
        note:"Only active private chats, agents and compatible disabled provider configurations are imported. Bound rooms, archived chats, encrypted secrets, jobs and execution history remain unchanged in the source SQLite database.",
    }
}
pub async fn preview(frontend:&FrontendStore,registry:&Registry,path:PathBuf)->Result<LegacyPreview,String>{
    let loaded=tokio::task::spawn_blocking(move||source(&path))
        .await.map_err(|e|format!("legacy import worker: {e}"))??;
    Ok(counts(&loaded,&frontend.snapshot().await,registry))
}
pub async fn apply(frontend:&FrontendStore,registry:&Registry,path:PathBuf)->Result<LegacyImportResult,String>{
    let mut loaded=tokio::task::spawn_blocking(move||source(&path))
        .await.map_err(|e|format!("legacy import worker: {e}"))??;
    let providers=std::mem::take(&mut loaded.providers);
    let mut result=frontend.mutate(move |current|{
        let mut result=LegacyImportResult{
            imported_agents:0,imported_sessions:0,imported_messages:0,
            imported_providers:0,existing_providers:0,
            skipped_provider_records:loaded.skipped_providers,
            existing_agents:0,existing_sessions:0,
            skipped_bound_sessions:loaded.skipped_bound,
            skipped_archived_sessions:loaded.skipped_archived,
        };
        for (id,agent) in loaded.agents{
            if current.agents.contains_key(&id){result.existing_agents+=1;}
            else{current.agents.insert(id,agent);result.imported_agents+=1;}
        }
        for (id,session) in loaded.sessions{
            if current.sessions.contains_key(&id){result.existing_sessions+=1;}
            else{
                result.imported_messages+=session.messages.len();
                current.sessions.insert(id,session);
                result.imported_sessions+=1;
            }
        }
        Ok(result)
    }).await?;
    // Separate stores are intentionally retriable: on a provider write error,
    // a repeated import skips already-copied sessions without overwriting them.
    let (added,existing)=registry.import_disabled(providers)?;
    result.imported_providers=added;
    result.existing_providers=existing;
    Ok(result)
}

#[cfg(test)]
mod tests{
    use super::*;
    use rusqlite::params;
    #[tokio::test]
    async fn main_sqlite_import_is_read_only_idempotent_and_preserves_legacy_receipts(){
        let dir=std::env::temp_dir().join(format!("dominion-legacy-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file=dir.join("dominion.sqlite");
        let db=Connection::open(&file).unwrap();
        db.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT,created_at TEXT,updated_at TEXT);
            CREATE TABLE messages(id TEXT,session_id TEXT,role TEXT,content TEXT,reasoning TEXT,
            agent_id TEXT,tool_calls TEXT,tool_call_id TEXT,created_at TEXT,metadata TEXT);").unwrap();
        db.execute("INSERT INTO records VALUES('agent','a',?1,'2026-11-17 09:00:00','2026-11-17 09:00:00')",
            params![json!({"name":"Researcher","systemPrompt":"Search carefully","providerId":"p","model":"m"}).to_string()]).unwrap();
        db.execute("INSERT INTO records VALUES('session','s',?1,'2026-11-17 09:00:00','2026-11-17 09:10:00')",
            params![json!({"title":"Original","agentId":"a","providerId":"p","model":"m"}).to_string()]).unwrap();
        db.execute("INSERT INTO records VALUES('session','room',?1,'2026-11-17 09:00:00','2026-11-17 09:10:00')",
            params![json!({"title":"Shared room","roomId":"room-1"}).to_string()]).unwrap();
        db.execute("INSERT INTO messages VALUES('m1','s','assistant','Test failed','inspect error','a',?1,'call-1','2026-11-17 09:01:00',?2)",
            params![r#"[{"name":"read_file"}]"#,r#"{"usage":{"totalTokens":32}}"#]).unwrap();
        db.execute("INSERT INTO records VALUES('provider','p',?1,'2026-11-17 09:00:00','2026-11-17 09:00:00')",
            params![json!({"name":"Legacy OpenAI","kind":"openai","baseUrl":"https://api.openai.com/v1","model":"m","apiKey":"DO-NOT-COPY"}).to_string()]).unwrap();
        drop(db);

        let frontend=FrontendStore::open(dir.join("native"),"configured-model").unwrap();
        let registry=Registry::open(dir.join("native")).unwrap();
        let preview=preview(&frontend,&registry,file.clone()).await.unwrap();
        assert_eq!((preview.source_agents,preview.source_sessions,preview.source_messages),(1,2,1));
        assert_eq!(preview.skipped_bound_sessions,1);
        assert_eq!(preview.importable_providers,1);
        let first=apply(&frontend,&registry,file.clone()).await.unwrap();
        assert_eq!((first.imported_agents,first.imported_sessions,first.imported_messages),(1,1,1));
        assert_eq!(first.imported_providers,1);
        let provider=registry.get("p").unwrap();
        assert!(!provider.enabled);
        assert!(provider.api_key.is_empty());
        let session=frontend.session("s").await.unwrap();
        assert_eq!(session.messages[0].metadata.as_ref().unwrap()["legacyImport"]["toolCallId"],"call-1");
        assert_eq!(session.messages[0].created_at,"2026-11-17T09:01:00Z");
        assert!(frontend.session("room").await.is_none());
        let repeated=apply(&frontend,&registry,file.clone()).await.unwrap();
        assert_eq!((repeated.imported_agents,repeated.imported_sessions,repeated.imported_messages),(0,0,0));
        assert_eq!(repeated.imported_providers,0);
        let check=Connection::open_with_flags(&file,OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let unchanged:i64=check.query_row("SELECT COUNT(*) FROM messages",[],|r|r.get(0)).unwrap();
        assert_eq!(unchanged,1);
        drop(check);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn older_message_schema_and_bad_json_preserve_raw_history(){
        let root=std::env::temp_dir().join(format!("dominion-legacy-old-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file=root.join("dominion.sqlite");
        let db=Connection::open(&file).unwrap();
        db.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT,created_at TEXT,updated_at TEXT);
            CREATE TABLE messages(id TEXT,session_id TEXT,role TEXT,content TEXT,created_at TEXT);").unwrap();
        db.execute("INSERT INTO records VALUES('session','old',?1,'2026-11-17 01:00:00','2026-11-17 01:00:00')",
            params![json!({"title":"old history"}).to_string()]).unwrap();
        db.execute("INSERT INTO messages VALUES('m','old','user','preserve this text','2026-11-17 01:00:00')",[]).unwrap();
        drop(db);
        let frontend=FrontendStore::open(root.join("native"),"test").unwrap();
        let registry=Registry::open(root.join("native")).unwrap();
        let copied=apply(&frontend,&registry,file.clone()).await.unwrap();
        assert_eq!(copied.imported_messages,1);
        let message=frontend.session("old").await.unwrap().messages.remove(0);
        assert_eq!(message.content,"preserve this text");
        assert_eq!(message.metadata.unwrap()["legacyImport"]["reasoning"],"");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn invalid_source_cannot_mutate_native_state(){
        let dir=std::env::temp_dir().join(format!("dominion-legacy-bad-{}",uuid::Uuid::new_v4()));
        let frontend=FrontendStore::open(dir.join("native"),"m").unwrap();
        let registry=Registry::open(dir.join("native")).unwrap();
        assert!(apply(&frontend,&registry,dir.join("missing.sqlite")).await.is_err());
        assert!(frontend.sessions().await.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
