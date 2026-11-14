//! Atomic native sub-agent admission. The parent goal owns every queued job;
 //! no partial session/job records or race-prone count-then-create admission.
use super::*;

impl Store {
    /// Reserve one child session and its queued job with a single SQLite
    /// IMMEDIATE transaction, including the active and lifetime limits.
    pub fn reserve_delegation(
        &self,
        parent_session_id: &str,
        session: &Value,
        job: &Value,
        max_total: usize,
        max_active: usize,
    ) -> Result<(), String> {
        let parent = parent_session_id.trim();
        let session_id = session["id"].as_str().filter(|id| !id.is_empty())
            .ok_or("delegation session id missing")?;
        let job_id = job["id"].as_str().filter(|id| !id.is_empty())
            .ok_or("delegation job id missing")?;
        if parent.is_empty() || max_total == 0 || max_active == 0
            || session["delegated"] != true
            || session["parentSessionId"].as_str() != Some(parent)
            || job["kind"] != "prompt" || job["status"] != "queued"
            || job["input"]["delegation"] != true
            || job["input"]["sessionId"].as_str() != Some(session_id)
            || job["input"]["parentSessionId"].as_str() != Some(parent)
        {
            return Err("invalid delegated session/job reservation".into());
        }
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let cancelled: i64 = tx.query_row(
            "SELECT COUNT(*) FROM records
             WHERE kind='session_cancel' AND id=?1
               AND json_extract(data,'$.cancelled')=1",
            [parent], |row| row.get(0)
        ).map_err(|error| error.to_string())?;
        if cancelled != 0 {
            return Err("상위 세션이 중단되어 새 위임을 생성할 수 없습니다.".into());
        }
        let (total, active): (i64, i64) = tx.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN json_extract(data,'$.status')
                          IN ('queued','running') THEN 1 ELSE 0 END),0)
               FROM records
              WHERE kind='job'
                AND json_extract(data,'$.kind')='prompt'
                AND json_extract(data,'$.input.delegation')=1
                AND json_extract(data,'$.input.parentSessionId')=?1",
            [parent], |row| Ok((row.get(0)?, row.get(1)?))
        ).map_err(|error| error.to_string())?;
        if total >= max_total as i64 {
            return Err("이 세션의 누적 위임 작업 한도에 도달했습니다.".into());
        }
        if active >= max_active as i64 {
            return Err("동시 위임 한도에 도달했습니다. 완료된 하위 작업을 먼저 수집하세요.".into());
        }
        tx.execute(
            "INSERT INTO records(kind,id,data) VALUES('session',?1,?2)",
            params![session_id,session.to_string()],
        ).map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO records(kind,id,data) VALUES('job',?1,?2)",
            params![job_id,job.to_string()],
        ).map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pair(parent:&str,index:u32)->(Value,Value){
        let session_id=format!("delegated-session-{index}");
        let job_id=format!("delegated-job-{index}");
        let session=json!({"id":session_id,"parentSessionId":parent,
            "delegated":true,"delegatedReadOnly":true});
        let job=json!({"id":job_id,"kind":"prompt","status":"queued",
            "input":{"delegation":true,"delegatedReadOnly":true,
                "sessionId":session_id,"parentSessionId":parent}});
        (session,job)
    }
    #[test]
    fn lifetime_and_active_limits_are_atomic_under_one_sqlite_transaction(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("session",&json!({"id":"root"})).unwrap();
        for index in 0..4 {
            let (s,j)=pair("root",index);
            store.reserve_delegation("root",&s,&j,6,4).unwrap();
        }
        let (s,j)=pair("root",4);
        assert!(store.reserve_delegation("root",&s,&j,6,4).is_err());
        assert!(store.get("session","delegated-session-4").is_none());
        assert!(store.get("job","delegated-job-4").is_none());
        let mut first=store.get("job","delegated-job-0").unwrap();
        first["status"]=json!("completed");
        store.put("job",&first).unwrap();
        store.reserve_delegation("root",&s,&j,6,4).unwrap();
        let mut second=store.get("job","delegated-job-1").unwrap();
        second["status"]=json!("completed");
        store.put("job",&second).unwrap();
        let (s,j)=pair("root",5);
        store.reserve_delegation("root",&s,&j,6,4).unwrap();
        let (s,j)=pair("root",6);
        assert!(store.reserve_delegation("root",&s,&j,6,4).is_err());
        assert!(store.get("job","delegated-job-6").is_none());
    }
    #[test]
    fn forged_child_cancelled_parent_and_duplicate_session_never_leave_orphan_job(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("session",&json!({"id":"root"})).unwrap();
        let (s,mut j)=pair("root",1);
        j["input"]["parentSessionId"]=json!("other");
        assert!(store.reserve_delegation("root",&s,&j,24,4).is_err());
        assert!(store.get("job","delegated-job-1").is_none());
        j["input"]["parentSessionId"]=json!("root");
        store.put("session",&s).unwrap();
        assert!(store.reserve_delegation("root",&s,&j,24,4).is_err());
        assert!(store.get("job","delegated-job-1").is_none());
        let (s,j)=pair("root",2);
        store.cancel_session_epoch("root").unwrap();
        assert!(store.reserve_delegation("root",&s,&j,24,4).is_err());
        assert!(store.get("session","delegated-session-2").is_none());
        assert!(store.get("job","delegated-job-2").is_none());
    }
}
