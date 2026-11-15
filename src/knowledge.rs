//! Durable, non-weight memory scoped to a source repository or the owner.
//! Explicitly stored notes are searchable without an embedding model or API.
use crate::domain::now_ms;
use serde::{Serialize,Deserialize};
use std::{path::PathBuf,collections::HashSet};
use tokio::{fs,io::AsyncWriteExt,sync::Mutex};
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
async fn persist(file:&std::path::Path,entries:&[MemoryEntry])->Result<(),String>{
    let parent=file.parent().ok_or("memory path has no parent")?;
    let temp=parent.join(format!(".knowledge-{}.tmp",Uuid::new_v4()));
    let bytes=serde_json::to_vec(entries).map_err(|e|e.to_string())?;
    let write=async{
        let mut out=fs::OpenOptions::new().write(true).create_new(true)
            .open(&temp).await.map_err(|e|e.to_string())?;
        out.write_all(&bytes).await.map_err(|e|e.to_string())?;
        out.sync_all().await.map_err(|e|e.to_string())?;
        fs::rename(&temp,file).await.map_err(|e|e.to_string())?;
        #[cfg(unix)]
        {
            let parent=parent.to_owned();
            tokio::task::spawn_blocking(move||{
                std::fs::File::open(parent).and_then(|dir|dir.sync_all())
            }).await.map_err(|e|e.to_string())?
                .map_err(|e|e.to_string())?;
        }
        Ok::<_,String>(())
    }.await;
    if write.is_err(){let _=fs::remove_file(&temp).await;}
    write
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
        persist(&self.file,&entries).await?;
        *locked=entries;
        Ok(updated)
    }
    /// Exact-scope inventory: unlike search this never includes inherited
    /// global notes. A user must explicitly select a project to manage it.
    pub async fn list(&self,scope:&str)->Vec<MemoryEntry>{
        let entries=self.entries.lock().await;
        let mut result=entries.iter().filter(|entry|entry.scope==scope).cloned().collect::<Vec<_>>();
        result.sort_by_key(|entry|std::cmp::Reverse(entry.updated_at_ms));
        result
    }
    pub async fn scopes(&self)->Vec<String>{
        let entries=self.entries.lock().await;
        let mut scopes=entries.iter().map(|e|e.scope.clone()).collect::<HashSet<_>>();
        scopes.insert("global".into());
        let mut result=scopes.into_iter().collect::<Vec<_>>();
        result.sort();
        result
    }
    /// An ID is immutable. Editing never silently changes project scope.
    pub async fn revise(&self,scope:&str,id:Uuid,title:&str,content:&str)
        ->Result<MemoryEntry,String>{
        valid(scope,title,content)?;
        let mut locked=self.entries.lock().await;
        let mut next=locked.clone();
        let target=next.iter().position(|e|e.id==id&&e.scope==scope)
            .ok_or("memory not found in selected scope")?;
        if next.iter().any(|e|e.scope==scope&&e.id!=id&&e.title.eq_ignore_ascii_case(title)){
            return Err("another memory already has that title in this project".into());
        }
        let entry=&mut next[target];
        entry.title=title.to_owned();
        entry.content=content.to_owned();
        entry.updated_at_ms=now_ms();
        let result=entry.clone();
        persist(&self.file,&next).await?;
        *locked=next;
        Ok(result)
    }
    pub async fn forget(&self,scope:&str,id:Uuid)->Result<(),String>{
        if scope.is_empty(){return Err("memory scope required".into());}
        let mut locked=self.entries.lock().await;
        let mut next=locked.clone();
        let before=next.len();
        next.retain(|e|e.id!=id||e.scope!=scope);
        if next.len()==before{return Err("memory not found in selected scope".into());}
        persist(&self.file,&next).await?;
        *locked=next;
        Ok(())
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
    async fn failed_persist_does_not_publish_non_durable_memory(){
        let dir=std::env::temp_dir().join(format!("dominion-memory-failure-{}",Uuid::new_v4()));
        let knowledge=Knowledge::open(dir.clone()).unwrap();
        // Force a rename failure by placing a directory at the destination.
        fs::create_dir(dir.join("knowledge.json")).await.unwrap();
        assert!(knowledge.remember("global","important","must persist").await.is_err());
        assert!(knowledge.search("global","important",5).await.is_empty());
        fs::remove_dir_all(dir).await.unwrap();
    }
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
