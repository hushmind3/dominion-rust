//! Persistent Money evidence corpus. Independent of the native Run journal:
 //! market observations are not task/tool receipts or verified transactions.
pub mod intelligence;
pub mod mechanism;
pub mod operators;
pub mod sources;
pub mod api;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json,Value};
use std::path::Path;

fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn open(root:&Path)->Result<Connection,String>{
    std::fs::create_dir_all(root).map_err(err)?;
    let conn=Connection::open(root.join("money.sqlite")).map_err(err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS money_signals(
            id TEXT PRIMARY KEY,
            captured_ms INTEGER NOT NULL,
            publisher TEXT NOT NULL,
            title TEXT NOT NULL,
            url TEXT NOT NULL,
            record TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS money_signals_recent ON money_signals(captured_ms DESC);
        CREATE TABLE IF NOT EXISTS money_candidates(
            id TEXT PRIMARY KEY,
            updated_ms INTEGER NOT NULL,
            record TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS money_candidates_recent ON money_candidates(updated_ms DESC);
        CREATE TABLE IF NOT EXISTS money_state(key TEXT PRIMARY KEY, record TEXT NOT NULL);
    ").map_err(err)?;
    Ok(conn)
}
fn stored(conn:&Connection,key:&str)->Result<Option<Value>,String>{
    let mut stmt=conn.prepare("SELECT record FROM money_state WHERE key=?1").map_err(err)?;
    let raw:Option<String>=stmt.query_row(params![key],|row|row.get(0)).optional().map_err(err)?;
    raw.map(|s|serde_json::from_str(&s).map_err(err)).transpose()
}
fn set(conn:&Connection,key:&str,record:&Value)->Result<(),String>{
    conn.execute("INSERT INTO money_state(key,record) VALUES(?1,?2)
        ON CONFLICT(key) DO UPDATE SET record=excluded.record",
        params![key,record.to_string()]).map_err(err)?;
    Ok(())
}
pub fn state(root:&Path,key:&str)->Result<Option<Value>,String>{
    stored(&open(root)?,key)
}
pub fn set_state(root:&Path,key:&str,record:&Value)->Result<(),String>{
    set(&open(root)?,key,record)
}
pub fn signals(root:&Path,limit:usize)->Result<Vec<Value>,String>{
    let conn=open(root)?;
    let mut stmt=conn.prepare("SELECT record FROM money_signals ORDER BY captured_ms DESC LIMIT ?1").map_err(err)?;
    let rows=stmt.query_map(params![limit.min(240) as i64],|row|row.get::<_,String>(0)).map_err(err)?;
    rows.map(|x|x.map_err(err).and_then(|raw|serde_json::from_str(&raw).map_err(err))).collect()
}
pub fn save_signals(root:&Path,items:&[Value])->Result<usize,String>{
    let mut conn=open(root)?;
    let tx=conn.transaction().map_err(err)?;
    let mut added=0;
    {
        let mut stmt=tx.prepare("INSERT OR IGNORE INTO money_signals(id,captured_ms,publisher,title,url,record)
             VALUES (?1,?2,?3,?4,?5,?6)").map_err(err)?;
        for row in items.iter().take(150){
            let Some(id)=row["id"].as_str().filter(|s|!s.is_empty())else{continue};
            let Some(url)=row["url"].as_str().filter(|s|s.starts_with("https://"))else{continue};
            added+=stmt.execute(params![id,crate::domain::now_ms() as i64,
                row["sourceName"].as_str().unwrap_or("unknown"),
                row["title"].as_str().unwrap_or(""),url,row.to_string()]).map_err(err)?;
        }
    }
    tx.commit().map_err(err)?;
    Ok(added)
}
pub fn candidates(root:&Path,limit:usize)->Result<Vec<Value>,String>{
    let conn=open(root)?;
    let mut stmt=conn.prepare("SELECT record FROM money_candidates ORDER BY updated_ms DESC LIMIT ?1").map_err(err)?;
    let rows=stmt.query_map(params![limit.min(100) as i64],|row|row.get::<_,String>(0)).map_err(err)?;
    rows.map(|x|x.map_err(err).and_then(|raw|serde_json::from_str(&raw).map_err(err))).collect()
}
pub fn candidate(root:&Path,id:&str)->Result<Option<Value>,String>{
    let conn=open(root)?;
    let raw:Option<String>=conn.query_row("SELECT record FROM money_candidates WHERE id=?1",
        params![id],|row|row.get(0)).optional().map_err(err)?;
    raw.map(|s|serde_json::from_str(&s).map_err(err)).transpose()
}
/// Store shortlisted leads and report in one SQLite transaction. Previously
/// rejected items retain their original user decision on regeneration.
pub fn save_report(root:&Path,items:&[Value],report:&Value)->Result<(),String>{
    let mut conn=open(root)?;
    let tx=conn.transaction().map_err(err)?;
    {
        let mut stmt=tx.prepare("INSERT OR IGNORE INTO money_candidates(id,updated_ms,record)
            VALUES (?1,?2,?3)").map_err(err)?;
        for row in items.iter().take(24){
            let Some(id)=row["id"].as_str().filter(|s|!s.is_empty())else{continue};
            stmt.execute(params![id,crate::domain::now_ms() as i64,row.to_string()]).map_err(err)?;
        }
    }
    set(&tx,"find",report)?;
    tx.commit().map_err(err)?;
    Ok(())
}
pub fn replace_candidate(root:&Path,record:&Value)->Result<(),String>{
    let id=record["id"].as_str().filter(|s|!s.is_empty()).ok_or("candidate id missing")?;
    let conn=open(root)?;
    conn.execute("UPDATE money_candidates SET record=?1,updated_ms=?2 WHERE id=?3",
        params![record.to_string(),crate::domain::now_ms() as i64,id]).map_err(err)?;
    Ok(())
}
pub fn latest(root:&Path)->Result<Value,String>{
    let conn=open(root)?;
    let mut report=stored(&conn,"find")?.unwrap_or_else(||json!({
        "candidateIds":[],"shownCandidateIds":[],"modelCalls":0,
        "message":"수집된 시장 근거가 아직 없습니다. 공개 자료를 수집해 주세요."
    }));
    let ids=report["candidateIds"].as_array().cloned().unwrap_or_default();
    let mut drafts=Vec::new();
    let mut stmt=conn.prepare("SELECT record FROM money_candidates WHERE id=?1").map_err(err)?;
    for id in ids.iter().take(24).filter_map(Value::as_str){
        let raw:Option<String>=stmt.query_row(params![id],|row|row.get(0))
            .optional().map_err(err)?;
        if let Some(raw)=raw{
            let item:Value=serde_json::from_str(&raw).map_err(err)?;
            if item["status"]!="rejected"{drafts.push(item);}
        }
    }
    report["drafts"]=json!(drafts);
    Ok(report)
}
pub fn stats(root:&Path)->Result<Value,String>{
    let conn=open(root)?;
    let signals:i64=conn.query_row("SELECT COUNT(*) FROM money_signals",[],|row|row.get(0)).map_err(err)?;
    let candidates:i64=conn.query_row("SELECT COUNT(*) FROM money_candidates",[],|row|row.get(0)).map_err(err)?;
    Ok(json!({"signals":signals,"candidates":candidates,
        "lastIngest":stored(&conn,"scan")?,
        "automation":stored(&conn,"automation")?.unwrap_or(json!({"enabled":false}))}))
}

#[cfg(test)]
mod tests{
 use super::*;
 use uuid::Uuid;
 #[test] fn evidence_deduplicates_and_shortlist_commit_survives_reopen(){
    let root=std::env::temp_dir().join(format!("dominion-money-db-{}",Uuid::new_v4()));
    let row=json!({"id":"one","url":"https://example.org/p","title":"Buyer request",
        "sourceName":"Public source","changeTags":["buyer_request"]});
    assert_eq!(save_signals(&root,&[row.clone(),row]).unwrap(),1);
    assert_eq!(signals(&root,20).unwrap().len(),1);
    let item=json!({"id":"lead1","title":"Buyer lead","status":"draft"});
    save_report(&root,&[item],&json!({"candidateIds":["lead1"],"modelCalls":0})).unwrap();
    assert_eq!(latest(&root).unwrap()["drafts"][0]["id"],"lead1");
    assert_eq!(stats(&root).unwrap()["signals"],1);
    std::fs::remove_dir_all(root).unwrap();
 }
}
