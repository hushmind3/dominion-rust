//! Isolated transaction boundary for resuming autonomous goals.
use super::*;

impl Store {
    /// A resumed goal is a single atomic transition: terminal goal -> queued
    /// goal, one fresh runnable job, and owner-reviewed unknown effects.
    /// Unknown outcomes remain unverified evidence after acknowledgement.
    pub fn resume_goal_atomic(
        &self, goal_id:&str, expected_status:&str, expected_job_id:&str,
        patch:&Value, new_job:&Value, reconciliation:Option<(&str,&str)>,
    )->Result<Option<Value>,String>{
        let job_id=new_job["id"].as_str().filter(|s|!s.is_empty())
            .ok_or("새 작업의 ID가 없습니다.")?;
        if new_job["kind"]!="goal"||new_job["status"]!="queued"
            ||new_job["input"]["goalId"]!=goal_id{
            return Err("다른 목표의 실행 작업은 등록할 수 없습니다.".into());
        }
        let changes=patch.as_object().ok_or("변경 내역은 객체여야 합니다.")?;
        if changes.contains_key("id"){return Err("목표 ID는 바꿀 수 없습니다.".into());}
        let mut conn=self.conn.lock().map_err(|e|e.to_string())?;
        let tx=conn.transaction().map_err(|e|e.to_string())?;
        let current:Option<Value>=tx.query_row(
            "SELECT data,id,created_at,updated_at FROM records WHERE kind='goal' AND id=?1",
            [goal_id], record_from_row
        ).optional().map_err(|e|e.to_string())?;
        let Some(mut goal)=current else{return Ok(None)};
        if goal["status"]!=expected_status || goal["jobId"]!=expected_job_id{
            return Ok(None);
        }
        for (key,value) in changes{goal[key]=value.clone();}
        goal["jobId"]=json!(job_id);
        if let Some((session,note))=reconciliation{
            if session.trim().is_empty() || note.trim().chars().count()<10{
                return Err("수동 복구에는 세션과 충분한 확인 기록이 필요합니다.".into());
            }
            let rows={
                let mut stmt=tx.prepare(
                    "WITH RECURSIVE owned(id,depth) AS (
                        SELECT ?1,0
                        UNION ALL SELECT s.id,o.depth+1
                        FROM records s JOIN owned o
                        ON s.kind='session'
                          AND json_extract(s.data,'$.parentSessionId')=o.id
                        WHERE o.depth<16
                    )
                    SELECT DISTINCT op.data FROM records op
                    JOIN records ex
                      ON ex.kind='execution'
                      AND ex.id=json_extract(op.data,'$.runId')
                    WHERE op.kind='execution_operation'
                      AND json_extract(ex.data,'$.sessionId') IN
                          (SELECT id FROM owned)
                      AND (json_extract(op.data,'$.outcomeUnknown')=1
                        OR json_extract(op.data,'$.status')
                           IN ('running','interrupted'))"
                ).map_err(|e|e.to_string())?;
                let results=stmt.query_map([session],|row|row.get::<_,String>(0))
                    .map_err(|e|e.to_string())?;
                let mut collected=Vec::new();
                for row in results{
                    let text=row.map_err(|e|e.to_string())?;
                    collected.push(serde_json::from_str::<Value>(&text)
                        .map_err(|e|e.to_string())?);
                }
                collected
            };
            // A parent goal's job may be terminal while a delegated child
            // is still actively executing a remote operation. Owner review
            // cannot cancel an actively-running execution by relabelling it.
            if rows.len()>256{
                return Err("재개할 미확정 작업이 256건을 초과합니다. 개별 점검이 필요합니다.".into());
            }
            for operation in &rows {
                let run_id=operation["runId"].as_str().ok_or(
                    "미확정 도구 기록에 실행 ID가 없습니다.")?;
                let run:Option<String>=tx.query_row(
                    "SELECT data FROM records WHERE kind='execution' AND id=?1",
                    [run_id],|r|r.get(0)
                ).optional().map_err(|e|e.to_string())?;
                let value:Value=serde_json::from_str(
                    run.as_deref().ok_or("미확정 도구의 실행 기록을 찾을 수 없습니다.")?
                ).map_err(|e|e.to_string())?;
                if value["status"]=="running"||value["status"]=="queued"{
                    return Err("위임 실행이 아직 활성 상태입니다. 종료 후 재개하세요.".into());
                }
            }
            let now=chrono::Utc::now().to_rfc3339();
            for mut operation in rows{
                operation["ownerReconciliation"]=json!({
                    "goalId":goal_id,"reviewedAt":now,"note":note,
                    "originalStatus":operation["status"],
                    "originalOutcomeUnknown":operation["outcomeUnknown"],
                    "originalEffectState":operation["effectState"],
                    "newResultVerified":false
                });
                operation["outcomeUnknown"]=json!(false);
                operation["status"]=json!("reviewed");
                operation["effectState"]=json!("inspected");
                if !operation["toolReceipt"].is_object(){
                    operation["toolReceipt"]=json!({});
                }
                operation["toolReceipt"]["status"]=json!("inspected");
                operation["toolReceipt"]["verified"]=json!(false);
                operation["toolReceipt"]["inspectedBy"]=json!(format!("owner:goal:{goal_id}"));
                operation["toolReceipt"]["inspectedAt"]=json!(now);
                upsert_tx(&tx,"execution_operation",&operation)?;
            }
        }
        // INSERT fails instead of overwriting an already-registered job.
        tx.execute("INSERT INTO records(kind,id,data) VALUES('job',?1,?2)",
            params![job_id,new_job.to_string()]).map_err(|e|e.to_string())?;
        upsert_tx(&tx,"goal",&goal)?;
        tx.commit().map_err(|e|e.to_string())?;
        Ok(Some(goal))
    }

}
