//! Source-of-truth SQLite backend: Run snapshot and immutable event receipts
//! commit in the SAME transaction. Legacy JSON import is a one-shot migration.
use crate::domain::Run;
use rusqlite::{params,Connection,OptionalExtension,Transaction,TransactionBehavior};
use std::{path::{Path,PathBuf},time::Duration};
use uuid::Uuid;

fn open(path:&Path)->Result<Connection,String>{
    if let Some(parent)=path.parent(){
        std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    }
    let connection=Connection::open(path).map_err(|e|e.to_string())?;
    connection.busy_timeout(Duration::from_secs(30)).map_err(|e|e.to_string())?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS metadata(
           key TEXT PRIMARY KEY,value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS runs(
           id TEXT PRIMARY KEY,snapshot_json BLOB NOT NULL,updated_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS run_events(
           run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
           seq INTEGER NOT NULL,event_json BLOB NOT NULL,
           PRIMARY KEY(run_id,seq)
         );"
    ).map_err(|e|e.to_string())?;
    Ok(connection)
}
fn in_transaction(tx:&Transaction<'_>,run:&Run)->Result<(),String>{
    let id=run.id.to_string();
    let json=serde_json::to_vec(run).map_err(|e|e.to_string())?;
    tx.execute(
        "INSERT INTO runs(id,snapshot_json,updated_at_ms) VALUES(?1,?2,?3)
         ON CONFLICT(id) DO UPDATE SET snapshot_json=excluded.snapshot_json,
             updated_at_ms=excluded.updated_at_ms",
        params![id,json,run.updated_at_ms as i64]
    ).map_err(|e|e.to_string())?;
    let old_count:i64=tx.query_row(
        "SELECT COUNT(*) FROM run_events WHERE run_id=?1",
        params![id],|row|row.get(0)
    ).map_err(|e|e.to_string())?;
    if old_count>run.events.len() as i64{
        return Err("SQLite event journal cannot be truncated".into());
    }
    for event in &run.events{
        let value=serde_json::to_vec(event).map_err(|e|e.to_string())?;
        let old:Option<Vec<u8>>=tx.query_row(
            "SELECT event_json FROM run_events WHERE run_id=?1 AND seq=?2",
            params![id,event.seq as i64],|row|row.get(0)
        ).optional().map_err(|e|e.to_string())?;
        match old{
            Some(old) if old!=value=>{
                return Err(format!("immutable Event Journal mismatch {id}:{}",event.seq));
            }
            Some(_)=>{}
            None=>{
                tx.execute(
                    "INSERT INTO run_events(run_id,seq,event_json) VALUES(?1,?2,?3)",
                    params![id,event.seq as i64,value]
                ).map_err(|e|e.to_string())?;
            }
        }
    }
    Ok(())
}
pub async fn import_or_read(path:PathBuf,legacy:PathBuf)->Result<Vec<Run>,String>{
    tokio::task::spawn_blocking(move||{
        let mut db=open(&path)?;
        let imported:Option<String>=db.query_row(
            "SELECT value FROM metadata WHERE key='legacy_import_complete'",
            [],|row|row.get(0)
        ).optional().map_err(|e|e.to_string())?;
        if imported.is_none(){
            let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e|e.to_string())?;
            if legacy.is_dir(){
                for file in std::fs::read_dir(&legacy).map_err(|e|e.to_string())?{
                    let file=file.map_err(|e|e.to_string())?;
                    if file.path().extension().and_then(|x|x.to_str())!=Some("json"){continue;}
                    if !file.file_type().map_err(|e|e.to_string())?.is_file(){
                        return Err("legacy Run import forbids symlinks".into());
                    }
                    let raw=std::fs::read(file.path()).map_err(|e|e.to_string())?;
                    let run:Run=serde_json::from_slice(&raw).map_err(|e|e.to_string())?;
                    let stem=file.path().file_stem().and_then(|s|s.to_str())
                        .ok_or("invalid legacy Run file name")?.to_owned();
                    if Uuid::parse_str(&stem).map_err(|e|e.to_string())?!=run.id{
                        return Err("legacy filename does not match Run id".into());
                    }
                    in_transaction(&tx,&run)?;
                }
            }
            tx.execute("INSERT INTO metadata(key,value) VALUES('schema_version','1')",[])
                .map_err(|e|e.to_string())?;
            tx.execute("INSERT INTO metadata(key,value) VALUES('legacy_import_complete','1')",[])
                .map_err(|e|e.to_string())?;
            tx.commit().map_err(|e|e.to_string())?;
        }
        let version:String=db.query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",[],
            |row|row.get(0)
        ).map_err(|e|e.to_string())?;
        if version!="1"{return Err("unsupported SQLite storage version".into());}
        let mut stmt=db.prepare("SELECT id,snapshot_json FROM runs ORDER BY id")
            .map_err(|e|e.to_string())?;
        let mut cursor=stmt.query([]).map_err(|e|e.to_string())?;
        let mut runs=Vec::new();
        while let Some(row)=cursor.next().map_err(|e|e.to_string())?{
            let key:String=row.get(0).map_err(|e|e.to_string())?;
            let raw:Vec<u8>=row.get(1).map_err(|e|e.to_string())?;
            let run:Run=serde_json::from_slice(&raw).map_err(|e|
                format!("SQLite Run snapshot {key} is corrupt: {e}"))?;
            if key!=run.id.to_string(){return Err("SQLite Run ID mismatch".into());}
            runs.push(run);
        }
        Ok(runs)
    }).await.map_err(|e|e.to_string())?
}
pub async fn upsert(path:PathBuf,run:Run)->Result<(),String>{
    tokio::task::spawn_blocking(move||{
        let mut db=open(&path)?;
        let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e|e.to_string())?;
        in_transaction(&tx,&run)?;
        tx.commit().map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}
pub fn checkpoint(path:&Path)->Result<(),String>{
    if !path.is_file(){return Ok(());}
    let db=open(path)?;
    let busy:i64=db.query_row("PRAGMA wal_checkpoint(TRUNCATE)",
        [],|row|row.get(0)).map_err(|e|e.to_string())?;
    if busy!=0{return Err("SQLite WAL checkpoint busy: backup deferred".into());}
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::CreateRun;
    #[tokio::test]
    async fn migrates_once_and_guards_immutable_event_receipts(){
        let root=std::env::temp_dir().join(format!("dominion-sql-{}",Uuid::new_v4()));
        let legacy=root.join("runs");std::fs::create_dir_all(&legacy).unwrap();
        let mut run=Run::new(CreateRun{instruction:"test".into(),
            budget_krw:Some(10.0),max_steps:Some(2)},2).unwrap();
        run.event("created","owner",None);
        std::fs::write(legacy.join(format!("{}.json",run.id)),
            serde_json::to_vec(&run).unwrap()).unwrap();
        let db=root.join("dominion.sqlite3");
        assert_eq!(import_or_read(db.clone(),legacy.clone()).await.unwrap().len(),1);
        let mut newer=run.clone();newer.event("done","confirmed",None);
        upsert(db.clone(),newer.clone()).await.unwrap();
        let opened=import_or_read(db.clone(),legacy).await.unwrap();
        assert_eq!(opened[0].events.len(),2,"stale JSON must never overwrite SQLite");
        let mut fake=newer;fake.events[0].message="tampered".into();
        assert!(upsert(db,fake).await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
