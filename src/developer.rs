//! Native developer workspace service for the unchanged main DeveloperPage.
//! Git subprocesses always receive structured arguments (never a shell) and
//! operate only inside an explicitly connected, verified Git worktree.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,path::{Path,PathBuf},process::Stdio,sync::Arc,time::Duration};
use tokio::{fs,io::AsyncWriteExt,process::{Child,Command},sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SourceConfig{pub source_dir:String,pub repo_url:String,pub branch:String,pub preview_port:u16}
impl Default for SourceConfig{
 fn default()->Self{Self{source_dir:String::new(),repo_url:String::new(),branch:"main".into(),preview_port:5174}}
}
#[derive(Clone,Serialize,Deserialize)]
pub struct DevJob{
 pub id:Uuid,pub kind:String,pub status:String,pub logs:Vec<String>,
 pub error:Option<String>,pub result:Option<Value>
}
pub struct Developer{
 root:PathBuf,file:PathBuf,
 config:Mutex<SourceConfig>,jobs:Mutex<HashMap<Uuid,DevJob>>,
 preview:Mutex<Option<Child>>,
}
async fn snapshot(path:&Path,cfg:&SourceConfig)->Result<(),String>{
 let data=serde_json::to_vec(cfg).map_err(|e|e.to_string())?;
 let temp=path.with_file_name(format!(".developer-{}.tmp",Uuid::new_v4()));
 let write=async{
  let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&temp).await.map_err(|e|e.to_string())?;
  file.write_all(&data).await.map_err(|e|e.to_string())?;
  file.sync_all().await.map_err(|e|e.to_string())?;
  fs::rename(&temp,path).await.map_err(|e|e.to_string())?;
  #[cfg(unix)]{
   let parent=path.parent().ok_or("developer configuration directory missing")?.to_owned();
   tokio::task::spawn_blocking(move||std::fs::File::open(parent).and_then(|f|f.sync_all()))
    .await.map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
  }
  Ok::<(),String>(())
 }.await;
 if write.is_err(){let _=fs::remove_file(temp).await;}
 write
}
async fn execute(dir:&Path,tool:&str,args:&[&str],timeout:u64)->Result<String,String>{
 let out=tokio::time::timeout(Duration::from_secs(timeout),
  Command::new(tool).current_dir(dir).args(args).env("GIT_TERMINAL_PROMPT","0")
   .stdin(Stdio::null()).kill_on_drop(true).output()
 ).await.map_err(|_|format!("{tool} timed out"))?.map_err(|e|e.to_string())?;
 let mut text=String::from_utf8_lossy(&out.stdout).into_owned();
 text.push_str(&String::from_utf8_lossy(&out.stderr));
 if text.len()>256_000{
   let mut end=256_000;
   while !text.is_char_boundary(end){end-=1;}
   text.truncate(end);
 }
 if !out.status.success(){return Err(format!("{tool} exited {:?}: {text}",out.status.code()));}
 Ok(text.trim().to_owned())
}
impl Developer{
 pub fn open(data_dir:PathBuf,workspace:PathBuf)->Result<Arc<Self>,String>{
  std::fs::create_dir_all(&data_dir).map_err(|e|e.to_string())?;
  std::fs::create_dir_all(&workspace).map_err(|e|e.to_string())?;
  let root=workspace.canonicalize().map_err(|e|e.to_string())?;
  let file=data_dir.join("developer-config.json");
  let config=if file.is_file(){
   serde_json::from_slice(&std::fs::read(&file).map_err(|e|e.to_string())?)
    .map_err(|e|format!("developer config corrupt: {e}"))?
  }else{SourceConfig::default()};
  Ok(Arc::new(Self{root,file,config:Mutex::new(config),jobs:Mutex::new(HashMap::new()),preview:Mutex::new(None)}))
 }
 pub async fn get_config(&self)->SourceConfig{self.config.lock().await.clone()}
 pub async fn save_config(&self,input:SourceConfig)->Result<SourceConfig,String>{
  if input.branch.is_empty()||input.branch.len()>120||!input.branch.bytes().all(|b|b.is_ascii_alphanumeric()||b"/-._".contains(&b)){
   return Err("invalid Git branch".into());
  }
  if input.repo_url.len()>1000||input.source_dir.len()>2000||input.preview_port<1024{return Err("invalid developer configuration".into());}
  if !input.source_dir.trim().is_empty(){self.rooted(&input.source_dir).await?;}
  let mut state=self.config.lock().await;
  snapshot(&self.file,&input).await?;
  *state=input.clone();
  Ok(input)
 }
 async fn rooted(&self,raw:&str)->Result<PathBuf,String>{
  let path=PathBuf::from(raw);
  let resolved=fs::canonicalize(path).await.map_err(|e|format!("source folder inaccessible: {e}"))?;
  if !resolved.starts_with(&self.root){return Err("source directory must be inside DOMINION_WORKSPACE_DIR".into());}
  if !fs::metadata(&resolved).await.map_err(|e|e.to_string())?.is_dir(){return Err("source must be a directory".into());}
  Ok(resolved)
 }
 pub async fn repository(&self)->Result<PathBuf,String>{
  let config=self.get_config().await;
  if config.source_dir.is_empty(){return Err("connect a Git checkout in the developer settings".into());}
  let path=self.rooted(&config.source_dir).await?;
  let git=execute(&path,"git",&["rev-parse","--show-toplevel"],8).await?;
  let actual=fs::canonicalize(&git).await.map_err(|e|e.to_string())?;
  if actual!=path{return Err("source path must be the Git checkout root".into());}
  Ok(path)
 }
 pub async fn status(&self)->Value{
  let config=self.get_config().await;
  let raw=config.source_dir.clone();
  let exists=!raw.is_empty() && self.rooted(&raw).await.is_ok();
  let root=self.repository().await.ok();
  let git=root.is_some();
  let node_modules=root.as_ref().is_some_and(|p|p.join("web/node_modules").is_dir());
  let package_dir=self.root.join("packages");
  let mut installer=None;
  if let Ok(mut listing)=fs::read_dir(&package_dir).await{
   while let Ok(Some(entry))=listing.next_entry().await{
    if !entry.file_name().to_string_lossy().starts_with("dominion-") ||
      !entry.file_name().to_string_lossy().ends_with(".tar.gz"){continue;}
    if let Ok(meta)=entry.metadata().await{
     let newer=installer.as_ref().and_then(|v:&Value|v["mtimeUnix"].as_u64()).unwrap_or(0);
     let stamp=meta.modified().ok().and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok())
       .map(|d|d.as_secs()).unwrap_or(0);
     if stamp>=newer{
      installer=Some(json!({"name":entry.file_name().to_string_lossy(),
       "path":entry.path(),"bytes":meta.len(),
       "mtime":crate::frontend_state::timestamp(stamp.saturating_mul(1000)),
       "mtimeUnix":stamp}));
     }
    }
   }
  }
  let mut branch=String::new();let mut commit=String::new();let mut remote=String::new();let mut dirty=0usize;
  if let Some(path)=&root{
   branch=execute(path,"git",&["branch","--show-current"],8).await.unwrap_or_default();
   commit=execute(path,"git",&["rev-parse","HEAD"],8).await.unwrap_or_default();
   remote=execute(path,"git",&["remote","get-url","origin"],8).await.unwrap_or_default();
   if let Ok(mut parsed)=reqwest::Url::parse(&remote){
     let _=parsed.set_username("");let _=parsed.set_password(None);
     parsed.set_query(None);parsed.set_fragment(None);
     remote=parsed.to_string();
   }else if remote.contains('@'){
     // SSH remotes may legitimately contain user@host, which is not a
     // secret. Never return unparsed URLs with embedded credentials.
     remote=String::from("[ssh remote configured]");
   }
   dirty=execute(path,"git",&["status","--porcelain","--untracked-files=normal"],8)
    .await.unwrap_or_default().lines().count();
  }
  let mut preview=self.preview.lock().await;
  let running=if let Some(child)=preview.as_mut(){
   matches!(child.try_wait(),Ok(None))
  }else{false};
  if !running{*preview=None;}
  let url=if running{format!("http://127.0.0.1:{}",config.preview_port)}else{String::new()};
  json!({
   "config":config,"exists":exists,"git":git,"nodeModules":node_modules,
   "branch":branch,"commit":commit,"remote":remote,"dirty":dirty,
   "version":env!("CARGO_PKG_VERSION"),"name":"dominion",
   "preview":{"status":if running{"running"}else{"stopped"},"url":url,"logs":[]},
   "platform":std::env::consts::OS,"arch":std::env::consts::ARCH,
   "installedExe":std::env::current_exe().ok().map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),
   "installer":installer
  })
 }
 pub async fn diff(&self)->Result<Value,String>{
  let repo=self.repository().await?;
  let tracked=execute(&repo,"git",&["status","--porcelain","--untracked-files=normal"],8).await?;
  let mut changes=Vec::new();let mut total=0usize;
  for entry in tracked.lines(){
   if entry.len()<4{continue;}
   total+=1;
   if changes.len()>=80{continue;}
   let status=&entry[..2];let name=&entry[3..];
   // git status defaults to quoting unusual paths. Never reinterpret a
   // quoted path as executable arguments; only show a bounded patch.
   let patch=if status=="??"{"Untracked file (no staged diff)".to_string()}
    else{execute(&repo,"git",&["diff","--no-ext-diff","HEAD","--",name],8).await.unwrap_or_else(|e|format!("Diff unavailable: {e}"))};
   changes.push(json!({"path":name,"status":status,"patch":patch.chars().take(10_000).collect::<String>()}));
  }
  Ok(json!({"changes":changes,"count":total,"truncated":total>80}))
 }
 pub async fn start_preview(&self)->Result<Value,String>{
  let repo=self.repository().await?;
  let web=repo.join("web");
  if !web.join("package.json").is_file()||!web.join("node_modules").is_dir(){
   return Err("install the original main frontend dependencies first".into());
  }
  let config=self.get_config().await;
  let mut guard=self.preview.lock().await;
  if let Some(child)=guard.as_mut(){
   if child.try_wait().map_err(|e|e.to_string())?.is_none(){
    return Ok(json!({"status":"running","url":format!("http://127.0.0.1:{}",config.preview_port)}));
   }
  }
  let child=Command::new("npm").args(["run","dev","--","--host","127.0.0.1","--port",&config.preview_port.to_string(),"--strictPort"])
    .current_dir(web).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true)
    .spawn().map_err(|e|e.to_string())?;
  *guard=Some(child);
  Ok(json!({"status":"running","url":format!("http://127.0.0.1:{}",config.preview_port)}))
 }
 pub async fn stop_preview(&self)->Result<Value,String>{
  let mut guard=self.preview.lock().await;
  if let Some(mut child)=guard.take(){child.kill().await.map_err(|e|e.to_string())?;}
  Ok(json!({"status":"stopped","url":""}))
 }
 pub async fn job(&self,id:Uuid)->Option<DevJob>{self.jobs.lock().await.get(&id).cloned()}
 async fn update_job(&self,id:Uuid,edit:impl FnOnce(&mut DevJob)){
  if let Some(job)=self.jobs.lock().await.get_mut(&id){edit(job);}
 }
 pub async fn start_job(self:&Arc<Self>,kind:String,body:Value)->Result<DevJob,String>{
  if !matches!(kind.as_str(),"setup"|"test"|"build"|"package"|"commit"){
   return Err("unsupported developer operation".into());
  }
  let root=self.repository().await?;
  let mut jobs_guard=self.jobs.lock().await;
  if jobs_guard.values().any(|job|job.status=="running"){
   return Err("another developer job is already running".into());
  }
  let message=body["message"].as_str().unwrap_or("").trim().to_owned();
  if kind=="commit"&&(message.is_empty()||message.len()>300||message.chars().any(|c|c=='\n'||c=='\r')){
   return Err("Git commit requires a nonempty one-line message (up to 300 bytes)".into());
  }
  let id=Uuid::new_v4();
  let job=DevJob{id,kind:kind.clone(),status:"running".into(),logs:Vec::new(),error:None,result:None};
  jobs_guard.insert(id,job.clone());
  drop(jobs_guard);
  let service=Arc::clone(self);
  tokio::spawn(async move{
   let task=async{
    let result=match kind.as_str(){
     "setup"=>execute(&root.join("web"),"npm",&["ci","--ignore-scripts"],600).await.map(|log|json!({"log":log})),
     "test"=>execute(&root,"cargo",&["test","--locked"],900).await.map(|log|json!({"log":log})),
     "build"=>execute(&root,"cargo",&["build","--release","--locked"],900).await.map(|log|json!({"log":log})),
     "package"=>{
       let frontend=root.join("web");
       if !frontend.join("node_modules").is_dir(){
        return Err("install the main web dependencies before packaging".into());
       }
       execute(&frontend,"npm",&["run","build"],600).await?;
       execute(&root,"cargo",&["build","--release","--locked"],900).await?;
       let binary=if cfg!(windows){"target/release/dominion.exe"}
         else{"target/release/dominion"};
       if !root.join(binary).is_file() || !frontend.join("dist/index.html").is_file(){
        return Err("built Rust binary or main UI dist is missing".into());
       }
       let destination=service.root.join("packages");
       fs::create_dir_all(&destination).await.map_err(|e|e.to_string())?;
       let archive=destination.join(format!("dominion-{}.tar.gz",Uuid::new_v4()));
       let result=async{
        let archive_name=archive.to_str().ok_or("invalid package output path")?;
        execute(&root,"tar",&["-czf",archive_name,"-C",root.to_str()
          .ok_or("invalid source root")?,binary,"web/dist"],120).await?;
        let bytes=fs::metadata(&archive).await.map_err(|e|e.to_string())?.len();
        if bytes==0{return Err("package is unexpectedly empty".into());}
        Ok::<Value,String>(json!({"path":archive,"bytes":bytes,"format":"tar.gz"}))
       }.await;
       if result.is_err(){let _=fs::remove_file(&archive).await;}
       result
     },
     "commit"=>{
       // Explicit original UI commit action. Reject ignored and untracked
       // symlinks that could lead to unexpected binary or credential capture.
       execute(&root,"git",&["status","--porcelain"],8).await?;
       execute(&root,"git",&["add","--all","--", "."],30).await?;
       execute(&root,"git",&["-c","user.name=Dominion","-c","user.email=dominion@localhost",
           "commit","-m",&message],60).await?;
       execute(&root,"git",&["rev-parse","HEAD"],8).await.map(|oid|json!({"oid":oid}))
     },
     _=>Err("unsupported developer job".into())
    };
    Ok::<Value,String>(result?)
   }.await;
   service.update_job(id,|job|{
    match task{
     Ok(out)=>{job.status="completed".into();
       job.logs.push(out["log"].as_str().unwrap_or("Operation completed").chars().take(60000).collect());
       job.result=Some(out);
     },
     Err(error)=>{job.status="failed".into();job.error=Some(error.clone());job.logs.push(error);}
    }
   }).await;
  });
  Ok(job)
 }
}
