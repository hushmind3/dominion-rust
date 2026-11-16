//! Historical main-product data is copied into a separate, read-only-from-the-
//! agent SQLite archive. Never translate old jobs into executable native Runs.
//! Only an explicit owner request may promote an archived memory into Knowledge.
use crate::legacy_import::open_legacy;
use rusqlite::{params,Connection,OpenFlags,OptionalExtension};
use serde::Serialize;
use serde_json::{json,Value};
use std::{collections::HashSet,path::{Path,PathBuf},time::Duration};
use tokio::task;

const ARCHIVE_FILE:&str="legacy-history.sqlite";
const KINDS:&[&str]=&[
    "memory","room","job","execution","operation","workflow",
    "experience","strategy","session","message"
];
fn allowed(kind:&str)->bool{KINDS.contains(&kind)}
fn archive_path(root:&Path)->PathBuf{root.join(ARCHIVE_FILE)}

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ArchiveStats{
    pub inspected:usize,
    pub stored_or_updated:usize,
    pub unchanged:usize,
    pub by_kind:std::collections::BTreeMap<String,usize>,
    pub mode:&'static str,
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ArchiveRow{
    pub kind:String,
    pub id:String,
    pub title:String,
    pub status:String,
    pub updated_at:String,
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ArchivePage{
    pub total:usize,
    pub rows:Vec<ArchiveRow>,
    pub limit:usize,
    pub offset:usize,
}
fn open_archive(root:&Path,write:bool)->Result<Connection,String>{
    let path=archive_path(root);
    if write {
        std::fs::create_dir_all(root).map_err(|e|e.to_string())?;
        let conn=Connection::open(path).map_err(|e|e.to_string())?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS legacy_records(
               kind TEXT NOT NULL,id TEXT NOT NULL,data TEXT NOT NULL,
               created_at TEXT NOT NULL,updated_at TEXT NOT NULL,
               PRIMARY KEY(kind,id)
             );
             CREATE INDEX IF NOT EXISTS idx_legacy_kind_time
               ON legacy_records(kind,updated_at DESC,id);
            ").map_err(|e|e.to_string())?;
        Ok(conn)
    }else{
        let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e|e.to_string())?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(|e|e.to_string())?;
        Ok(conn)
    }
}
fn record(tx:&rusqlite::Transaction<'_>,kind:&str,id:&str,raw:&str,
    created:&str,updated:&str)->Result<usize,String>{
    if !allowed(kind){return Err("unsupported legacy archive kind".into());}
    if id.trim().is_empty()||id.len()>512{return Err("legacy record ID is invalid".into());}
    // Preserve even malformed historical JSON as inert data. None of this is
    // treated as instructions or model-call/tool-call input.
    let data=serde_json::from_str::<Value>(raw)
        .unwrap_or_else(|_|json!({"legacyRawData":raw})).to_string();
    tx.execute(
        "INSERT INTO legacy_records(kind,id,data,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(kind,id) DO UPDATE SET
           data=excluded.data,created_at=excluded.created_at,
           updated_at=excluded.updated_at
         WHERE legacy_records.data!=excluded.data OR
           legacy_records.created_at!=excluded.created_at OR
           legacy_records.updated_at!=excluded.updated_at",
        params![kind,id,data,created,updated]
    ).map_err(|e|e.to_string())
}
fn message_columns(db:&Connection)->Result<HashSet<String>,String>{
    let mut schema=db.prepare("PRAGMA table_info(messages)").map_err(|e|e.to_string())?;
    let rows=schema.query_map([],|r|r.get::<_,String>(1))
        .map_err(|e|e.to_string())?;
    rows.collect::<Result<HashSet<_>,_>>().map_err(|e|e.to_string())
}
fn import_blocking(source:&Path,root:&Path)->Result<ArchiveStats,String>{
    // The source is already pinned to a read-only WAL transaction.
    let db=open_legacy(source)?;
    let mut target=open_archive(root,true)?;
    let tx=target.transaction().map_err(|e|e.to_string())?;
    let mut stats=ArchiveStats{
        inspected:0,stored_or_updated:0,unchanged:0,
        by_kind:Default::default(),mode:"read_only_source; inert_archive"
    };
    let mut statement=db.prepare(
        "SELECT kind,id,data,created_at,updated_at FROM records WHERE
          kind IN ('memory','room','job','execution','operation','workflow',
                   'experience','strategy','session')
          ORDER BY kind,id"
    ).map_err(|e|format!("legacy records: {e}"))?;
    let rows=statement.query_map([],|row|Ok((
        row.get::<_,String>(0)?,row.get::<_,String>(1)?,
        row.get::<_,String>(2)?,row.get::<_,String>(3)?,
        row.get::<_,String>(4)?
    ))).map_err(|e|e.to_string())?;
    for row in rows{
        let (kind,id,data,created,updated)=row.map_err(|e|e.to_string())?;
        let modified=record(&tx,&kind,&id,&data,&created,&updated)?;
        stats.inspected+=1;
        stats.stored_or_updated+=modified;
        stats.unchanged+=usize::from(modified==0);
        *stats.by_kind.entry(kind).or_insert(0)+=1;
    }
    drop(statement);
    let cols=message_columns(&db)?;
    if ["id","session_id","role","content","created_at"].iter().any(|k|!cols.contains(*k)){
        return Err("legacy message table missing required columns".into());
    }
    let optional=|name:&'static str,missing:&'static str|if cols.contains(name){name}else{missing};
    let sql=format!(
        "SELECT id,session_id,role,content,created_at,{},{},{},{},{} \
         FROM messages ORDER BY created_at ASC,rowid ASC",
        optional("reasoning","''"),optional("agent_id","NULL"),
        optional("tool_calls","NULL"),optional("tool_call_id","NULL"),
        optional("metadata","'{}'")
    );
    let mut stmt=db.prepare(&sql).map_err(|e|e.to_string())?;
    let rows=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,
        r.get::<_,String>(2)?,r.get::<_,String>(3)?,
        r.get::<_,String>(4)?,r.get::<_,String>(5)?,
        r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?,
        r.get::<_,Option<String>>(8)?,r.get::<_,String>(9)?
    ))).map_err(|e|e.to_string())?;
    for row in rows{
        let (id,session_id,role,content,created,reasoning,agent_id,tool_calls,tool_call_id,metadata)
            =row.map_err(|e|e.to_string())?;
        let value=json!({
            "id":id,"sessionId":session_id,"role":role,"content":content,
            "createdAt":created,"reasoning":reasoning,"agentId":agent_id,
            "toolCallsJson":tool_calls,"toolCallId":tool_call_id,
            "metadataJson":metadata,
            "legacyOnly":true
        });
        let modified=record(&tx,"message",&id,&value.to_string(),&created,&created)?;
        stats.inspected+=1;
        stats.stored_or_updated+=modified;
        stats.unchanged+=usize::from(modified==0);
        *stats.by_kind.entry("message".into()).or_insert(0)+=1;
    }
    drop(stmt);
    tx.commit().map_err(|e|e.to_string())?;
    db.execute_batch("COMMIT").map_err(|e|e.to_string())?;
    Ok(stats)
}
pub async fn import(source:PathBuf,root:PathBuf)->Result<ArchiveStats,String>{
    task::spawn_blocking(move||import_blocking(&source,&root))
        .await.map_err(|e|format!("legacy archival worker: {e}"))?
}
fn load_page(root:&Path,kind:&str,offset:usize,limit:usize)->Result<ArchivePage,String>{
    if !allowed(kind){return Err("unknown legacy archive category".into());}
    if limit==0||limit>50||offset>100_000{return Err("legacy archive pagination out of range".into());}
    if !archive_path(root).exists(){return Ok(ArchivePage{total:0,rows:Vec::new(),limit,offset});}
    let db=open_archive(root,false)?;
    let total=db.query_row(
        "SELECT COUNT(*) FROM legacy_records WHERE kind=?1",
        params![kind],|r|r.get::<_,usize>(0)
    ).map_err(|e|e.to_string())?;
    let mut query=db.prepare(
        "SELECT id,data,updated_at FROM legacy_records WHERE kind=?1
         ORDER BY updated_at DESC,id LIMIT ?2 OFFSET ?3"
    ).map_err(|e|e.to_string())?;
    let rows=query.query_map(params![kind,limit,offset],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?
    ))).map_err(|e|e.to_string())?;
    let mut entries=Vec::new();
    for row in rows {
        let (id,text,updated_at)=row.map_err(|e|e.to_string())?;
        let value:Value=serde_json::from_str(&text).map_err(|e|e.to_string())?;
        let label=["title","name","text","content","description"].iter()
            .find_map(|k|value.get(*k).and_then(Value::as_str))
            .or_else(||value.pointer("/input/prompt").and_then(Value::as_str))
            .unwrap_or("");
        entries.push(ArchiveRow{
            kind:kind.into(),id,
            title:label.chars().take(140).collect(),
            status:value.get("status").and_then(Value::as_str).unwrap_or("").into(),
            updated_at
        });
    }
    Ok(ArchivePage{total,rows:entries,limit,offset})
}
pub async fn list(root:PathBuf,kind:String,offset:usize,limit:usize)->Result<ArchivePage,String>{
    task::spawn_blocking(move||load_page(&root,&kind,offset,limit))
        .await.map_err(|e|e.to_string())?
}
fn load_item(root:&Path,kind:&str,id:&str)->Result<Option<Value>,String>{
    if !allowed(kind){return Err("unknown legacy archive category".into());}
    if id.is_empty()||id.len()>512{return Err("invalid legacy archive record ID".into());}
    if !archive_path(root).exists(){return Ok(None);}
    let db=open_archive(root,false)?;
    let record=db.query_row(
        "SELECT data FROM legacy_records WHERE kind=?1 AND id=?2",
        params![kind,id],|r|r.get::<_,String>(0)
    ).optional().map_err(|e|e.to_string())?;
    record.map(|text|serde_json::from_str::<Value>(&text)
        .map_err(|e|e.to_string())).transpose()
}
pub async fn get(root:PathBuf,kind:String,id:String)->Result<Option<Value>,String>{
    task::spawn_blocking(move||load_item(&root,&kind,&id))
        .await.map_err(|e|e.to_string())?
}

#[cfg(test)]
mod tests{
    use super::*;
    #[tokio::test]
    async fn archive_keeps_job_room_memory_and_messages_inert_and_retriable(){
        let root=std::env::temp_dir().join(format!("dominion-history-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source=root.join("dominion.sqlite");
        let db=Connection::open(&source).unwrap();
        db.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT,created_at TEXT,updated_at TEXT);
            CREATE TABLE messages(id TEXT,session_id TEXT,role TEXT,content TEXT,created_at TEXT);
            INSERT INTO records VALUES('memory','m1','{\"text\":\"Owner corrected the command\",\"status\":\"active\"}','2026','2026');
            INSERT INTO records VALUES('room','r1','{\"name\":\"The team\"}','2026','2026');
            INSERT INTO records VALUES('job','j1','{\"status\":\"completed\",\"input\":{\"prompt\":\"fix tests\"}}','2026','2026');
            INSERT INTO records VALUES('oauth_config','secret','{\"token\":\"must not copy\"}','2026','2026');
            INSERT INTO messages VALUES('msg','s1','user','inspect the tests','2026');").unwrap();
        drop(db);
        let first=import(source.clone(),root.join("native")).await.unwrap();
        assert_eq!((first.inspected,first.stored_or_updated),(4,4));
        let second=import(source.clone(),root.join("native")).await.unwrap();
        assert_eq!((second.inspected,second.stored_or_updated),(4,0));
        let page=list(root.join("native"),"job".into(),0,20).await.unwrap();
        assert_eq!(page.total,1);
        assert_eq!(page.rows[0].id,"j1");
        assert_eq!(get(root.join("native"),"job".into(),"j1".into()).await.unwrap().unwrap()["status"],"completed");
        assert!(get(root.join("native"),"oauth_config".into(),"secret".into()).await.is_err());
        assert!(get(root.join("native"),"job".into(),"nonexistent".into()).await.unwrap().is_none());
        assert!(list(root.join("native"),"job".into(),0,51).await.is_err());
        let count=Connection::open_with_flags(&source,OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
            .query_row("SELECT COUNT(*) FROM records",[],|r|r.get::<_,i64>(0)).unwrap();
        assert_eq!(count,4);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn legacy_record_updates_are_captured_without_duplication(){
        let root=std::env::temp_dir().join(format!("dominion-history-updates-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source=root.join("dominion.sqlite");
        let db=Connection::open(&source).unwrap();
        db.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT,created_at TEXT,updated_at TEXT);
            CREATE TABLE messages(id TEXT,session_id TEXT,role TEXT,content TEXT,created_at TEXT);").unwrap();
        db.execute("INSERT INTO records VALUES('job','j','{\"status\":\"running\"}','t1','t1')",[]).unwrap();
        import(source.clone(),root.join("native")).await.unwrap();
        db.execute("UPDATE records SET data='{\"status\":\"completed\"}',updated_at='t2' WHERE id='j'",[]).unwrap();
        let updated=import(source.clone(),root.join("native")).await.unwrap();
        assert_eq!(updated.stored_or_updated,1);
        assert_eq!(get(root.join("native"),"job".into(),"j".into()).await.unwrap().unwrap()["status"],"completed");
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
