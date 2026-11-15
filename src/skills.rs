//! Lazy discovery of project-local SKILL.md instructions.
//! Skills are untrusted procedural references; loading never runs code.
use crate::{config::Settings,domain::ToolCall,tools};
use serde::{Serialize,Deserialize};
use serde_json::json;
use tokio::fs;
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct SkillInfo { pub name:String,pub path:String }
const ROOTS:[&str;3]=[".agents/skills",".dominion/skills","skills"];
fn safe_name(name:&str)->bool{
    !name.is_empty()&&name.len()<=64&&name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')
}
pub async fn list(settings:&Settings,run:Uuid)->Result<Vec<SkillInfo>,String>{
    let base=settings.workspace_dir.join(run.to_string());
    let mut result=Vec::new();
    for root in ROOTS{
        let mut dir=base.clone();
        let mut safe=true;
        for part in root.split('/'){
            dir.push(part);
            match fs::symlink_metadata(&dir).await{
                Ok(meta) if meta.file_type().is_symlink()||!meta.is_dir()=>{safe=false;break;}
                Ok(_)=>{}
                Err(_)=>{safe=false;break;}
            }
        }
        if !safe{continue;}
        let mut entries=fs::read_dir(&dir).await.map_err(|e|e.to_string())?;
        while let Some(entry)=entries.next_entry().await.map_err(|e|e.to_string())?{
            let name=entry.file_name().to_string_lossy().to_string();
            if !safe_name(&name){continue;}
            if !entry.file_type().await.map_err(|e|e.to_string())?.is_dir(){continue;}
            let skill=entry.path().join("SKILL.md");
            if fs::symlink_metadata(&skill).await.is_ok_and(|m|m.is_file()&&!m.file_type().is_symlink()&&m.len()<=32768){
                result.push(SkillInfo{name:name.clone(),path:format!("{root}/{name}/SKILL.md")});
            }
            if result.len()>=64{break;}
        }
        if result.len()>=64{break;}
    }
    result.sort_by(|a,b|a.name.cmp(&b.name));
    result.dedup_by(|a,b|a.name==b.name);
    Ok(result)
}
pub async fn load(settings:&Settings,run:Uuid,name:&str)->Result<String,String>{
    if !safe_name(name){return Err("invalid skill name".into());}
    let skill=list(settings,run).await?.into_iter().find(|s|s.name==name).ok_or("skill not found")?;
    tools::execute(settings,run,&ToolCall{id:"skill-read".into(),name:"read_file".into(),arguments:json!({"path":skill.path})}).await
}
#[cfg(test)]
mod tests{
    use super::*;
    #[tokio::test]
    async fn list_and_lazy_load_project_skill(){
        let mut settings=Settings::from_env().unwrap();
        settings.workspace_dir=std::env::temp_dir().join(format!("dominion-skill-{}",Uuid::new_v4()));
        let id=Uuid::new_v4();
        let folder=settings.workspace_dir.join(id.to_string()).join(".agents/skills/review");
        fs::create_dir_all(&folder).await.unwrap();
        fs::write(folder.join("SKILL.md"),"Review the diff.").await.unwrap();
        assert_eq!(list(&settings,id).await.unwrap().len(),1);
        assert!(load(&settings,id,"review").await.unwrap().contains("Review"));
        assert!(load(&settings,id,"../review").await.is_err());
        fs::remove_dir_all(settings.workspace_dir).await.unwrap();
    }
}
