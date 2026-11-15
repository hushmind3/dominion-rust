//! Durable, non-weight memory scoped to a source repository or the owner.
//! Explicitly stored notes are searchable without an embedding model or API.
use crate::domain::now_ms;
use serde::{Serialize,Deserialize};
use std::{path::PathBuf,collections::HashSet};
use tokio::{fs,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct MemoryEntry{
    pub id:Uuid,
    pub scope:String,
    pub title:String,
    pub content:String,
    pub created_at_ms:u64,
    pub updated_at_ms:u64,
}
pub struct Knowledge{
    file:PathBuf,
    entries:Mutex<Vec<MemoryEntry>>,
}
fn valid(scope:&str,title:&str,content:&str)->Result<(),String>{
    if scope.is_empty()||scope.len()>512||scope.bytes().any(|b|b==0||b==b'\n'){
        return Err("memory scope must be a short repository identifier or global".into());
    }
    if title.trim().is_empty()||title.len()>120||content.trim().is_empty()||content.len()>4000{
        return Err("memory title must be 1..120 bytes and content 1..4000 bytes".into());
    }
    Ok(())
}
impl Knowledge{
    pub fn open(data_dir:PathBuf)->Result<Self,String>{
        std::fs::create_dir_all(&data_dir).map_err(|e|e.to_string())?;
        let file=data_dir.join("knowledge.json");
        let entries=if file.exists(){
            serde_json::from_slice::<Vec<MemoryEntry>>(&std::fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("corrupt durable knowledge file: {e}"))?
        }else{Vec::new()};
        Ok(Self{file,entries:Mutex::new(entries)})
    }
    pub async fn remember(&self,scope:&str,title:&str,content:&str)->Result<MemoryEntry,String>{
        valid(scope,title,content)?;
        let mut locked=self.entries.lock().await;
        // Clone before mutating: a failed filesystem write must never expose a
        // durable memory that disappears silently after restart.
        let mut entries=locked.clone();
        let now=now_ms();
        let updated=if let Some(old)=entries.iter_mut().find(|m|m.scope==scope&&m.title.eq_ignore_ascii_case(title)){
            old.content=content.to_owned();old.updated_at_ms=now;old.clone()
        }else{
            if entries.len()>=1024{return Err("memory capacity reached (1024 notes); consolidate existing notes".into());}
            let value=MemoryEntry{id:Uuid::new_v4(),scope:scope.to_owned(),title:title.to_owned(),content:content.to_owned(),created_at_ms:now,updated_at_ms:now};
            entries.push(value.clone());value
        };
        let temp=self.file.with_extension("json.tmp");
        fs::write(&temp,serde_json::to_vec(&*entries).map_err(|e|e.to_string())?).await.map_err(|e|e.to_string())?;
        fs::rename(temp,&self.file).await.map_err(|e|e.to_string())?;
        *locked=entries;
        Ok(updated)
    }
    pub async fn search(&self,scope:&str,query:&str,max:usize)->Vec<MemoryEntry>{
        let query=query.to_lowercase();
        let terms=query.split(|c:char|!c.is_alphanumeric())
            .filter(|s|s.chars().count()>=2).take(20)
            .map(str::to_owned).collect::<HashSet<_>>();
        let entries=self.entries.lock().await;
        let mut ranked=entries.iter().filter(|entry|entry.scope==scope||scope!="global"&&entry.scope=="global")
            .filter_map(|entry|{
                let title=entry.title.to_lowercase();
                let content=entry.content.to_lowercase();
                let score=if terms.is_empty(){1usize}else{terms.iter().map(|term|{
                    if title.contains(term){4}else if content.contains(term){1}else{0}
                }).sum::<usize>()};
                if score==0{None}else{Some((score,entry))}
            }).collect::<Vec<_>>();
        ranked.sort_by(|a,b|b.0.cmp(&a.0).then_with(||b.1.updated_at_ms.cmp(&a.1.updated_at_ms)));
        ranked.into_iter().take(max.min(20)).map(|(_,entry)|entry.clone()).collect()
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[tokio::test]
    async fn memory_persists_with_scope_and_idempotent_revisions(){
        let root=std::env::temp_dir().join(format!("dominion-memory-{}",Uuid::new_v4()));
        let k=Knowledge::open(root.clone()).unwrap();
        let first=k.remember("repo-a","build command","cargo test").await.unwrap();
        let revised=k.remember("repo-a","build command","cargo test --lib").await.unwrap();
        assert_eq!(first.id,revised.id);
        k.remember("repo-b","secret note","other project only").await.unwrap();
        k.remember("global","user preference","Korean interface").await.unwrap();
        drop(k);
        let restored=Knowledge::open(root.clone()).unwrap();
        assert_eq!(restored.search("repo-a","build",8).await.len(),1);
        assert!(restored.search("repo-a","secret",8).await.is_empty());
        assert_eq!(restored.search("repo-a","Korean",8).await.len(),1);
        fs::remove_dir_all(root).await.unwrap();
    }
}
