//! Durable automation dispatcher: the due-slot and its runnable job commit
//! atomically. Repeated scheduler ticks cannot enqueue duplicate work.
use super::*;

impl Store {
    pub fn claim_due_automation(
        &self, id:&str, expected_next_at:&str, now:chrono::DateTime<chrono::Utc>
    )->Result<Option<(Value,Value)>,String>{
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let current:Option<Value>=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='automation' AND id=?1",
            [id],record_from_row
        ).optional().map_err(|e|e.to_string())?;
        let Some(mut automation)=current else{return Ok(None)};
        if automation["enabled"]!=true
            || automation["nextAt"].as_str()!=Some(expected_next_at){
            return Ok(None);
        }
        let due=chrono::DateTime::parse_from_rfc3339(expected_next_at)
            .map_err(|e|e.to_string())?.with_timezone(&chrono::Utc);
        if due>now{return Ok(None);}
        let kind=automation["kind"].as_str().unwrap_or("prompt");
        let job_kind=if kind=="meeting"{"room"}else{kind};
        if !matches!(job_kind,"prompt"|"workflow"|"room"|"learn"|"goal"){
            return Err("등록되지 않은 자동화 작업 유형입니다.".into());
        }
        // Do not overlap identical recurring runs while their last attempt
        // is still queued/running (a common source of repeated external bills).
        if let Some(last)=automation["lastJobId"].as_str(){
            let prior:Option<Value>=tx.query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind='job' AND id=?1",
                [last],record_from_row
            ).optional().map_err(|e|e.to_string())?;
            if prior.is_some_and(|j|matches!(j["status"].as_str(),Some("queued"|"running"|"needs_review"))){
                return Ok(None);
            }
        }
        let input=automation.get("input").cloned().unwrap_or_else(||json!({}));
        if job_kind=="workflow"{
            let flow_id=input["workflowId"].as_str().filter(|v|!v.is_empty())
                .ok_or("자동화의 워크플로 ID가 비어 있습니다.")?;
            let exists:Option<String>=tx.query_row(
                "SELECT data FROM records WHERE kind='workflow' AND id=?1",
                [flow_id],|r|r.get(0)
            ).optional().map_err(|e|e.to_string())?;
            if exists.is_none(){return Err("자동화에 연결된 워크플로가 삭제되었습니다.".into());}
        }
        let minutes=automation["intervalMinutes"].as_i64().unwrap_or(0);
        if !(1..=525600).contains(&minutes){
            return Err("자동화 반복 간격이 허용 범위를 벗어납니다.".into());
        }
        let job_id=uuid::Uuid::new_v4().to_string();
        let job=json!({
            "id":job_id,"kind":job_kind,"input":input,
            "automationId":id,"status":"queued",
            "queuedAt":now.to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
            "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
        });
        tx.execute(
            "INSERT INTO records(kind,id,data) VALUES('job',?1,?2)",
            params![job_id,job.to_string()]
        ).map_err(|e|e.to_string())?;
        automation["lastAt"]=json!(now.to_rfc3339());
        automation["lastJobId"]=json!(job_id);
        automation["nextAt"]=json!((now+chrono::Duration::minutes(minutes)).to_rfc3339());
        upsert_tx(&tx,"automation",&automation)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some((automation,job)))
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn setup()->(Store,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-automation-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (Store::open(&dir.join("database.sqlite")).unwrap(),dir)
    }
    #[test]
    fn workflow_schedule_is_one_claim_one_job_with_no_overlap(){
        let (store,dir)=setup();
        let now=chrono::Utc::now();
        let due=(now-chrono::Duration::minutes(2)).to_rfc3339();
        store.put("workflow",&json!({"id":"flow","steps":[{"agentId":"agent"}]})).unwrap();
        store.put("automation",&json!({"id":"auto","kind":"workflow","enabled":true,
            "intervalMinutes":1,"nextAt":due,
            "input":{"workflowId":"flow","input":"artifact task"}})).unwrap();
        let (saved,job)=store.claim_due_automation("auto",&due,now).unwrap().unwrap();
        assert_eq!(job["kind"],"workflow");
        assert_eq!(job["input"]["workflowId"],"flow");
        assert_eq!(job["automationId"],"auto");
        assert_eq!(saved["lastJobId"],job["id"]);
        assert_eq!(store.count("job"),1);
        assert!(store.claim_due_automation("auto",&due,now).unwrap().is_none());
        let next=saved["nextAt"].as_str().unwrap();
        assert!(store.claim_due_automation("auto",next,now+chrono::Duration::minutes(2)).unwrap().is_none(),
            "never overlap queued work");
        store.patch_if("job",job["id"].as_str().unwrap(),"status",
            &json!("queued"),&json!({"status":"needs_review"})).unwrap();
        assert!(store.claim_due_automation("auto",next,
            now+chrono::Duration::minutes(2)).unwrap().is_none(),
            "a job requiring owner review must never silently rerun next period");
        store.patch_if("job",job["id"].as_str().unwrap(),"status",
            &json!("needs_review"),&json!({"status":"completed"})).unwrap();
        assert!(store.claim_due_automation("auto",next,now+chrono::Duration::minutes(2)).unwrap().is_some());
        assert_eq!(store.count("job"),2);
        drop(store);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn injected_automation_state_failure_rolls_back_new_job(){
        let (store,dir)=setup();
        let now=chrono::Utc::now();
        let due=(now-chrono::Duration::seconds(2)).to_rfc3339();
        store.put("automation",&json!({"id":"auto","enabled":true,"kind":"prompt",
            "intervalMinutes":60,"nextAt":due,"input":{"prompt":"test"}})).unwrap();
        let raw=rusqlite::Connection::open(dir.join("database.sqlite")).unwrap();
        raw.execute_batch("CREATE TRIGGER fail_schedule_update BEFORE UPDATE ON records
          WHEN NEW.kind='automation' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        assert!(store.claim_due_automation("auto",&due,now).is_err());
        assert_eq!(store.count("job"),0);
        assert_eq!(store.get("automation","auto").unwrap()["nextAt"],due);
        raw.execute_batch("DROP TRIGGER fail_schedule_update").unwrap();
        drop(raw);drop(store);std::fs::remove_dir_all(dir).unwrap();
    }
}
