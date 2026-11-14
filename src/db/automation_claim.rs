//! Durable automation dispatcher: the due-slot and its runnable job commit
//! atomically. Repeated scheduler ticks cannot enqueue duplicate work.
use super::*;

impl Store {
    /// Explicit owner review unblocks a schedule WITHOUT rewriting a failed
    /// job as successful. The old result and tool effect receipts remain for
    /// audit. A future slot starts after one full interval, never immediately.
    pub fn acknowledge_automation_review(
        &self,auto_id:&str,job_id:&str,note:&str,
    )->Result<Option<(Value,Value)>,String>{
        if note.trim().chars().count()<10||note.chars().count()>2000{
            return Err("외부 작업을 확인한 처리 기록은 10~2000자로 작성해야 합니다.".into());
        }
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let get=|kind:&str,id:&str|->Result<Option<Value>,String>{
            tx.query_row(
                "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                params![kind,id],record_from_row
            ).optional().map_err(|e|e.to_string())
        };
        let Some(mut automation)=get("automation",auto_id)? else{return Ok(None)};
        let Some(mut job)=get("job",job_id)? else{return Ok(None)};
        if automation["lastJobId"]!=job_id||job["automationId"]!=auto_id
            ||job["status"]!="needs_review" || automation["enabled"]!=true {
            return Ok(None);
        }
        let interval=automation["intervalMinutes"].as_i64().unwrap_or(0);
        if !(1..=525600).contains(&interval){
            return Err("자동화 반복 간격이 유효하지 않습니다.".into());
        }
        let now=chrono::Utc::now();
        job["ownerReview"]=json!({
            "originalStatus":"needs_review","at":now.to_rfc3339(),
            "resolutionNote":note.trim(),
            "authorizedFutureRuns":true,"verifiedCompletion":false,
        });
        job["status"]=json!("cancelled");
        automation["lastReviewedJobId"]=json!(job_id);
        automation["lastReviewAt"]=json!(now.to_rfc3339());
        automation["nextAt"]=json!((now+chrono::Duration::minutes(interval)).to_rfc3339());
        upsert_tx(&tx,"job",&job)?;
        upsert_tx(&tx,"automation",&automation)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some((automation,job)))
    }

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
    fn explicit_owner_review_can_rearm_an_automation_without_fabricating_success(){
        let (store,dir)=setup();
        let now=chrono::Utc::now();
        let due=(now-chrono::Duration::minutes(10)).to_rfc3339();
        store.put("workflow",&json!({"id":"flow","steps":[{"id":"s"}]})).unwrap();
        store.put("automation",&json!({"id":"auto","enabled":true,
            "kind":"workflow","intervalMinutes":60,"nextAt":due,
            "input":{"workflowId":"flow"}})).unwrap();
        let (_,queued)=store.claim_due_automation("auto",&due,now).unwrap().unwrap();
        let job_id=queued["id"].as_str().unwrap();
        store.patch_if("job",job_id,"status",&json!("queued"),
            &json!({"status":"needs_review","result":{
                "status":"needs_review","verificationGaps":["external effect unknown"]
            }})).unwrap();
        assert!(store.acknowledge_automation_review("auto",job_id,"short").is_err());
        assert!(store.acknowledge_automation_review("auto","wrong",
            "확인 기록을 충분히 입력했습니다.").unwrap().is_none());
        assert_eq!(store.get("job",job_id).unwrap()["status"],"needs_review");
        let (auto,reviewed)=store.acknowledge_automation_review(
            "auto",job_id,"외부 결과를 직접 확인했고 차기 일정 재실행을 승인했습니다.")
            .unwrap().unwrap();
        assert_eq!(reviewed["status"],"cancelled");
        assert_eq!(reviewed["ownerReview"]["verifiedCompletion"],false);
        assert_eq!(reviewed["result"]["verificationGaps"][0],"external effect unknown");
        assert_eq!(auto["lastReviewedJobId"],job_id);
        assert!(chrono::DateTime::parse_from_rfc3339(auto["nextAt"].as_str().unwrap())
            .unwrap().with_timezone(&chrono::Utc)>now);
        assert!(store.acknowledge_automation_review("auto",job_id,
            "다시 승인한 기록은 중복입니다.").unwrap().is_none());
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
