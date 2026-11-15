//! Durable schedules for the unchanged main Operations/Automations page.
//! Dispatch is at-most-once per claimed occurrence, never a simulated job.
use crate::frontend_state::timestamp;
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,path::PathBuf};
use tokio::{fs,io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Automation {
    pub id:String,
    pub kind:String,
    pub input:Value,
    pub interval_minutes:u64,
    pub next_at:String,
    pub next_at_ms:u64,
    pub enabled:bool,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ScheduledJob {
    pub id:Uuid,
    pub automation_id:String,
    pub kind:String,
    pub input:Value,
    pub status:String,
    pub created_at:String,
    pub updated_at:String,
    pub native_run_id:Option<Uuid>,
    pub result:Option<Value>,
    pub error:Option<String>,
}
#[derive(Clone,Default,Serialize,Deserialize)]
struct Snapshot {
    #[serde(default)] automations:HashMap<String,Automation>,
    #[serde(default)] jobs:HashMap<Uuid,ScheduledJob>,
}
pub struct Automations {
    file:PathBuf,
    state:Mutex<Snapshot>,
}
fn date_to_days(y:i64,m:i64,d:i64)->i64{
    let y=y-i64::from(m<=2);
    let era=y.div_euclid(400);
    let yoe=y-era*400;
    let mp=m+if m>2{-3}else{9};
    let doy=(153*mp+2)/5+d-1;
    let doe=yoe*365+yoe/4-yoe/100+doy;
    era*146097+doe-719468
}
pub fn iso_millis(text:&str)->Result<u64,String>{
    // Main frontend always serializes a datetime-local input via toISOString()
    // so a strict UTC timestamp is sufficient and avoids timezone guessing.
    let raw=text.strip_suffix('Z').ok_or("nextAt must be an ISO UTC timestamp ending Z")?;
    let (date,time)=raw.split_once('T').ok_or("nextAt needs date and time")?;
    let mut parts=date.split('-');
    let year:i64=parts.next().ok_or("year missing")?.parse().map_err(|_|"invalid year")?;
    let month:i64=parts.next().ok_or("month missing")?.parse().map_err(|_|"invalid month")?;
    let day:i64=parts.next().ok_or("day missing")?.parse().map_err(|_|"invalid day")?;
    if parts.next().is_some(){return Err("invalid ISO date".into());}
    if !(1970..=9999).contains(&year)||!(1..=12).contains(&month){
        return Err("nextAt date is outside allowed range".into());
    }
    let days_this_month=match month {
        2=>if year%4==0&&(year%100!=0||year%400==0){29}else{28},
        4|6|9|11=>30,_=>31
    };
    if day<1||day>days_this_month{return Err("invalid calendar day".into());}
    let mut clock=time.split(':');
    let hh:u64=clock.next().ok_or("missing hour")?.parse().map_err(|_|"invalid hour")?;
    let mm:u64=clock.next().ok_or("missing minute")?.parse().map_err(|_|"invalid minute")?;
    let sec=clock.next().ok_or("missing seconds")?;
    if clock.next().is_some(){return Err("invalid ISO time".into());}
    let (ss,frac)=sec.split_once('.').unwrap_or((sec,""));
    let secs:u64=ss.parse().map_err(|_|"invalid seconds")?;
    if hh>23||mm>59||secs>59||frac.len()>3||!frac.bytes().all(|c|c.is_ascii_digit()){
        return Err("invalid ISO time value".into());
    }
    let mut millis=frac.to_owned();
    while millis.len()<3{millis.push('0');}
    let sub:u64=millis.parse().map_err(|_|"invalid fractional seconds")?;
    let days=date_to_days(year,month,day);
    if days<0{return Err("nextAt cannot be before 1970".into());}
    Ok((days as u64*86_400+hh*3600+mm*60+secs)*1000+sub)
}
impl Automations {
    pub fn open(root:PathBuf)->Result<Self,String>{
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("automations.json");
        let mut state=if file.exists(){
            serde_json::from_slice::<Snapshot>(&std::fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("corrupt automations file: {e}"))?
        }else{Snapshot::default()};
        for job in state.jobs.values_mut(){
            if matches!(job.status.as_str(),"claimed"|"running"){
                job.status="needs_review".into();
                job.error=Some("Server restarted; reconcile the original execution before retrying".into());
                job.updated_at=timestamp(crate::domain::now_ms());
            }
        }
        // Recovery markers are written before jobs are served to the frontend.
        if file.exists(){
            let bytes=serde_json::to_vec(&state).map_err(|e|e.to_string())?;
            let tmp=root.join(format!(".automations-recovery-{}.tmp",Uuid::new_v4()));
            let result=(||->Result<(),String>{
                use std::io::Write;
                let mut f=std::fs::OpenOptions::new().write(true).create_new(true)
                    .open(&tmp).map_err(|e|e.to_string())?;
                f.write_all(&bytes).map_err(|e|e.to_string())?;
                f.sync_all().map_err(|e|e.to_string())?;
                std::fs::rename(&tmp,&file).map_err(|e|e.to_string())?;
                #[cfg(unix)]std::fs::File::open(&root)
                    .and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
                Ok(())
            })();
            if result.is_err(){let _=std::fs::remove_file(tmp);}
            result?;
        }
        Ok(Self{file,state:Mutex::new(state)})
    }
    async fn persist(&self,state:&Snapshot)->Result<(),String>{
        let bytes=serde_json::to_vec(state).map_err(|e|e.to_string())?;
        if bytes.len()>16*1024*1024{return Err("automations state exceeds 16 MiB".into());}
        let dir=self.file.parent().ok_or("invalid automation data directory")?;
        let tmp=dir.join(format!(".automations-{}.tmp",Uuid::new_v4()));
        let result=async{
            let mut f=fs::OpenOptions::new().create_new(true).write(true).open(&tmp)
                .await.map_err(|e|e.to_string())?;
            f.write_all(&bytes).await.map_err(|e|e.to_string())?;
            f.sync_all().await.map_err(|e|e.to_string())?;
            drop(f);
            fs::rename(&tmp,&self.file).await.map_err(|e|e.to_string())?;
            #[cfg(unix)]{
                let root=dir.to_owned();
                tokio::task::spawn_blocking(move||std::fs::File::open(root)
                    .and_then(|f|f.sync_all())).await
                    .map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
            }
            Ok::<(),String>(())
        }.await;
        if result.is_err(){let _=fs::remove_file(&tmp).await;}
        result
    }
    async fn change<T,F>(&self,f:F)->Result<T,String>
    where F:FnOnce(&mut Snapshot)->Result<T,String>{
        let mut state=self.state.lock().await;
        let mut next=state.clone();
        let out=f(&mut next)?;
        self.persist(&next).await?;
        *state=next;
        Ok(out)
    }
    pub async fn list(&self)->Vec<Automation>{
        let mut data=self.state.lock().await.automations.values().cloned().collect::<Vec<_>>();
        data.sort_by(|a,b|a.next_at_ms.cmp(&b.next_at_ms));data
    }
    pub async fn jobs(&self)->Vec<ScheduledJob>{
        let mut data=self.state.lock().await.jobs.values().cloned().collect::<Vec<_>>();
        data.sort_by(|a,b|b.created_at.cmp(&a.created_at));data
    }
    pub async fn job(&self,id:Uuid)->Option<ScheduledJob>{
        self.state.lock().await.jobs.get(&id).cloned()
    }
    pub async fn save(&self,entry:Automation,update:bool)->Result<Automation,String>{
        self.change(|state|{
            if update&&!state.automations.contains_key(&entry.id){
                return Err("automation to edit not found".into());
            }
            if !update&&state.automations.len()>=128{
                return Err("maximum 128 automations".into());
            }
            state.automations.insert(entry.id.clone(),entry.clone());
            Ok(entry)
        }).await
    }
    pub async fn delete(&self,id:&str)->Result<(),String>{
        self.change(|state|{
            state.automations.remove(id).ok_or("automation not found")?;
            Ok(())
        }).await
    }
    /// Persistent claim advances the next schedule *before* launching an
    /// external action. Restart cannot silently repeat a mutating side effect.
    pub async fn claim_due(&self,now:u64)->Result<Vec<ScheduledJob>,String>{
        self.change(|state|{
            let mut ready=state.automations.values_mut()
                .filter(|a|a.enabled&&a.next_at_ms<=now)
                .collect::<Vec<_>>();
            ready.sort_by_key(|a|a.next_at_ms);
            let mut new=Vec::new();
            for task in ready.into_iter().take(8){
                let id=Uuid::new_v4();
                if state.jobs.len()>=2048{return Err("automation job retention full; archive before scheduling more".into());}
                let interval=task.interval_minutes.saturating_mul(60_000);
                let missed=now.saturating_sub(task.next_at_ms)/interval+1;
                task.next_at_ms=task.next_at_ms.saturating_add(interval.saturating_mul(missed));
                task.next_at=timestamp(task.next_at_ms);
                let at=timestamp(now);
                new.push(ScheduledJob{id,automation_id:task.id.clone(),
                    kind:task.kind.clone(),input:task.input.clone(),
                    status:"claimed".into(),created_at:at.clone(),updated_at:at,
                    native_run_id:None,result:None,error:None});
            }
            for job in &new{state.jobs.insert(job.id,job.clone());}
            Ok(new)
        }).await
    }
    pub async fn bind_run(&self,id:Uuid,run:Uuid)->Result<ScheduledJob,String>{
        self.change(|state|{
            let job=state.jobs.get_mut(&id).ok_or("scheduled job missing")?;
            if job.status!="claimed"||job.native_run_id.is_some(){
                return Err("scheduled execution was already bound".into());
            }
            job.native_run_id=Some(run);
            job.status="running".into();
            job.updated_at=timestamp(crate::domain::now_ms());
            Ok(job.clone())
        }).await
    }
    pub async fn finish(&self,id:Uuid,status:&str,result:Option<Value>,error:Option<String>)
        ->Result<ScheduledJob,String>{
        self.change(|state|{
            let job=state.jobs.get_mut(&id).ok_or("scheduled job missing")?;
            if matches!(job.status.as_str(),"completed"|"cancelled"|"failed") {
                return Ok(job.clone());
            }
            job.status=status.into();
            job.result=result;
            job.error=error;
            job.updated_at=timestamp(crate::domain::now_ms());
            Ok(job.clone())
        }).await
    }
}
