//! Single-owner profile data for the canonical main frontend.
//! The profile is a presentation preference, never an instruction to the agent.
use crate::domain::now_ms;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize,Serialize};
use std::path::{Path,PathBuf};
use tokio::{fs,io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct OwnerProfile {
    pub name:String,
    pub avatar:String,
    #[serde(default)]
    pub updated_at_ms:u64,
}

#[derive(Deserialize)]
pub struct NewOwnerProfile {
    pub name:String,
    #[serde(default)]
    pub avatar:String,
}
pub struct OwnerProfileStore {
    file:PathBuf,
    profile:Mutex<OwnerProfile>,
}
fn validate(input:&NewOwnerProfile)->Result<(),String>{
    if input.name.trim().is_empty() || input.name.chars().count()>60 ||
        input.name.chars().any(|c|c.is_control()){
        return Err("profile name must be 1-60 printable characters".into());
    }
    if input.avatar.is_empty(){return Ok(());}
    let (mime,data)=input.avatar.split_once(";base64,")
        .ok_or("avatar must be a PNG/JPEG/WebP data URL")?;
    if !matches!(mime,"data:image/png"|"data:image/jpeg"|"data:image/webp"){
        return Err("avatar must be a PNG/JPEG/WebP image".into());
    }
    if data.len()>1_400_000{return Err("avatar exceeds size limit".into());}
    let decoded=STANDARD.decode(data).map_err(|_|"avatar is not valid base64")?;
    if decoded.len()>1_048_576{return Err("avatar exceeds 1 MiB".into());}
    if decoded.is_empty(){return Err("empty image is not valid".into());}
    let signature_ok=match mime{
        "data:image/png"=>decoded.starts_with(&[0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a]),
        "data:image/jpeg"=>decoded.starts_with(&[0xff,0xd8,0xff]),
        "data:image/webp"=>decoded.len()>=12 && &decoded[..4]==b"RIFF" && &decoded[8..12]==b"WEBP",
        _=>false,
    };
    if !signature_ok{return Err("avatar image signature does not match its MIME type".into());}
    Ok(())
}
async fn persist(path:&Path,profile:&OwnerProfile)->Result<(),String>{
    let dir=path.parent().ok_or("invalid profile directory")?;
    let stage=dir.join(format!(".owner-profile-{}.tmp",Uuid::new_v4()));
    let data=serde_json::to_vec(profile).map_err(|e|e.to_string())?;
    let result=async{
        let mut file=fs::OpenOptions::new().create_new(true).write(true)
            .open(&stage).await.map_err(|e|e.to_string())?;
        file.write_all(&data).await.map_err(|e|e.to_string())?;
        file.sync_all().await.map_err(|e|e.to_string())?;
        fs::rename(&stage,path).await.map_err(|e|e.to_string())?;
        #[cfg(unix)]
        {
            let directory=dir.to_path_buf();
            tokio::task::spawn_blocking(move||std::fs::File::open(directory)
                .and_then(|handle|handle.sync_all())).await
                .map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
        }
        Ok::<(),String>(())
    }.await;
    if result.is_err(){let _=fs::remove_file(&stage).await;}
    result
}
impl OwnerProfileStore{
    pub fn open(root:PathBuf)->Result<Self,String>{
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("owner-profile.json");
        let profile=match std::fs::read(&file){
            Ok(bytes)=>{
                let previous:OwnerProfile=serde_json::from_slice(&bytes)
                    .map_err(|e|format!("invalid owner profile: {e}"))?;
                validate(&NewOwnerProfile{name:previous.name.clone(),avatar:previous.avatar.clone()})?;
                previous
            },
            Err(error) if error.kind()==std::io::ErrorKind::NotFound=>
                OwnerProfile{name:"나".into(),avatar:String::new(),updated_at_ms:0},
            Err(error)=>return Err(error.to_string()),
        };
        Ok(Self{file,profile:Mutex::new(profile)})
    }
    pub async fn get(&self)->OwnerProfile{self.profile.lock().await.clone()}
    pub async fn save(&self,input:NewOwnerProfile)->Result<OwnerProfile,String>{
        validate(&input)?;
        let mut lock=self.profile.lock().await;
        let revised=OwnerProfile{
            name:input.name.trim().to_owned(),
            avatar:input.avatar,
            updated_at_ms:now_ms(),
        };
        persist(&self.file,&revised).await?;
        *lock=revised.clone();
        Ok(revised)
    }
}
