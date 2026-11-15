//! Backup and maintenance adapter for the original Operations > Backup panel.
//! Snapshots are authenticated-encrypted with locally stored versioned keys.
//! Restore is staged and applied before native stores open on the next restart.
use crate::{config::Settings,domain::{now_ms,RunStatus},engine::Engine};
use axum::{extract::State,http::StatusCode,routing::{get,post},Json,Router};
use base64::{engine::general_purpose::STANDARD,Engine as _};
use chacha20poly1305::{XChaCha20Poly1305,XNonce,Key,aead::{Aead,KeyInit,OsRng,rand_core::RngCore}};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::BTreeMap,fs,io::Write,path::{Component,Path,PathBuf},sync::{Arc,Mutex,OnceLock}};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
const MAGIC:&[u8]=b"DOMINION-BACKUP-V1\0";
const MAX_FILE:usize=128*1024*1024;
const MAX_TOTAL:usize=256*1024*1024;
static LOCK:OnceLock<Mutex<()>>=OnceLock::new();
fn guard()->std::sync::MutexGuard<'static,()>{
 LOCK.get_or_init(||Mutex::new(())).lock().unwrap_or_else(|e|e.into_inner())
}
fn fault(status:StatusCode,reason:impl Into<String>)->Fault{
 (status,Json(json!({"error":reason.into()})))
}
fn bad(reason:impl Into<String>)->Fault{fault(StatusCode::BAD_REQUEST,reason)}
fn root(settings:&Settings)->PathBuf{settings.data_dir.join("system-backups")}
fn keyfile(settings:&Settings)->PathBuf{root(settings).join("keys.json")}
fn flagfile(settings:&Settings)->PathBuf{root(settings).join("maintenance.json")}
pub fn maintenance(settings:&Settings)->bool{
 fs::read(flagfile(settings)).ok().and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok())
  .and_then(|v|v["maintenance"].as_bool()).unwrap_or(false)
}
fn atomic(file:&Path,bytes:&[u8])->Result<(),String>{
 let dir=file.parent().ok_or("missing snapshot parent")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let stage=dir.join(format!(".stage-{}",Uuid::new_v4()));
 let result=(||->Result<(),String>{
  let mut opt=fs::OpenOptions::new();opt.create_new(true).write(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opt.mode(0o600);}
  let mut out=opt.open(&stage).map_err(|e|e.to_string())?;
  out.write_all(bytes).map_err(|e|e.to_string())?;
  out.sync_all().map_err(|e|e.to_string())?;
  drop(out);
  fs::rename(&stage,file).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if result.is_err(){let _=fs::remove_file(stage);}
 result
}
#[derive(Clone,Serialize,Deserialize)]
struct Keys{active:u32,versions:BTreeMap<u32,String>}
fn load_keys(settings:&Settings,create:bool)->Result<Keys,String>{
 let path=keyfile(settings);
 if path.is_file(){
  #[cfg(unix)]{
   use std::os::unix::fs::PermissionsExt;
   if fs::metadata(&path).map_err(|e|e.to_string())?.permissions().mode()&0o077!=0{
    return Err("backup key permissions must be owner-only (chmod 600)".into());
   }
  }
  return serde_json::from_slice(&fs::read(&path).map_err(|e|e.to_string())?)
   .map_err(|e|format!("backup key store invalid: {e}"));
 }
 if !create{return Err("backup key store does not exist".into());}
 let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
 let ring=Keys{active:1,versions:BTreeMap::from([(1,STANDARD.encode(key))])};
 atomic(&path,&serde_json::to_vec(&ring).map_err(|e|e.to_string())?)?;
 Ok(ring)
}
fn encrypt(ring:&Keys,plaintext:&[u8])->Result<Vec<u8>,String>{
 let key_bytes=STANDARD.decode(ring.versions.get(&ring.active).ok_or("active backup key missing")?)
  .map_err(|e|e.to_string())?;
 let cipher=XChaCha20Poly1305::new(Key::from_slice(&key_bytes));
 let mut nonce=[0u8;24];OsRng.fill_bytes(&mut nonce);
 let ciphertext=cipher.encrypt(XNonce::from_slice(&nonce),plaintext)
  .map_err(|_|"backup encryption failed".to_owned())?;
 let mut output=Vec::with_capacity(MAGIC.len()+4+nonce.len()+ciphertext.len());
 output.extend_from_slice(MAGIC);output.extend_from_slice(&ring.active.to_le_bytes());
 output.extend_from_slice(&nonce);output.extend_from_slice(&ciphertext);
 Ok(output)
}
fn decrypt(ring:&Keys,ciphertext:&[u8])->Result<Vec<u8>,String>{
 let offset=MAGIC.len();
 if ciphertext.len()<offset+4+24+16||!ciphertext.starts_with(MAGIC){
  return Err("invalid backup container signature".into());
 }
 let version=u32::from_le_bytes(ciphertext[offset..offset+4].try_into().map_err(|_|"invalid key version")?);
 let secret=STANDARD.decode(ring.versions.get(&version).ok_or("backup decryption key unavailable")?)
  .map_err(|e|e.to_string())?;
 let cipher=XChaCha20Poly1305::new(Key::from_slice(&secret));
 cipher.decrypt(XNonce::from_slice(&ciphertext[offset+4..offset+28]),
  &ciphertext[offset+28..]).map_err(|_|"backup integrity check failed".into())
}
#[derive(Serialize,Deserialize)]
struct Entry{path:String,content:String}
#[derive(Serialize,Deserialize)]
struct Bundle{version:u32,created_ms:u64,files:Vec<Entry>}
fn collect(base:&Path,dir:&Path,prefix:&str,files:&mut Vec<Entry>,size:&mut usize)->Result<(),String>{
 for item in fs::read_dir(dir).map_err(|e|e.to_string())?{
  let item=item.map_err(|e|e.to_string())?;
  let path=item.path();
  let relative=path.strip_prefix(base).map_err(|e|e.to_string())?;
  if prefix=="data"&&relative.components().next()
   .is_some_and(|c|c.as_os_str()=="system-backups"){continue;}
  let kind=item.file_type().map_err(|e|e.to_string())?;
  if kind.is_symlink(){return Err(format!("backup cannot follow symlink: {}",path.display()));}
  if kind.is_dir(){collect(base,&path,prefix,files,size)?;continue;}
  if !kind.is_file(){return Err("backup encountered unsupported filesystem object".into());}
  let metadata=item.metadata().map_err(|e|e.to_string())?;
  if metadata.len()>MAX_FILE as u64{return Err(format!("backup file too large: {}",path.display()));}
  *size=size.saturating_add(metadata.len() as usize);
  if *size>MAX_TOTAL{return Err("backup exceeds the 256 MiB snapshot bound".into());}
  let bytes=fs::read(&path).map_err(|e|e.to_string())?;
  files.push(Entry{path:format!("{prefix}/{}",relative.to_string_lossy().replace('\\',"/")),
   content:STANDARD.encode(bytes)});
 }
 Ok(())
}
fn snapshot(settings:&Settings)->Result<Bundle,String>{
 let mut files=Vec::new();let mut size=0;
 for (prefix,path) in [("data",&settings.data_dir),("workspace",&settings.workspace_dir)]{
  if path.is_dir(){
   let base=path.canonicalize().map_err(|e|e.to_string())?;
   collect(&base,&base,prefix,&mut files,&mut size)?;
  }
 }
 files.sort_by(|a,b|a.path.cmp(&b.path));
 Ok(Bundle{version:1,created_ms:now_ms(),files})
}
fn validate_relative(path:&str)->Result<(&str,&Path),String>{
 if path.len()>2048||path.contains('\0'){return Err("unsafe backup entry path".into());}
 let (scope,rest)=path.split_once('/').ok_or("missing backup entry scope")?;
 if !matches!(scope,"data"|"workspace") || rest.is_empty(){
  return Err("backup entry scope is invalid".into());
 }
 let relative=Path::new(rest);
 if relative.components().any(|c|!matches!(c,Component::Normal(_))){
  return Err("backup entry path traversal rejected".into());
 }
 if scope=="data"&&relative.components().next().is_some_and(|c|c.as_os_str()=="system-backups"){
  return Err("backup entry attempts to overwrite keys".into());
 }
 Ok((scope,relative))
}
fn decode_bundle(plaintext:&[u8])->Result<Bundle,String>{
 let bundle:Bundle=serde_json::from_slice(plaintext).map_err(|e|e.to_string())?;
 if bundle.version!=1||bundle.files.len()>100_000{return Err("unsupported backup bundle".into());}
 let mut seen=std::collections::HashSet::new();let mut total=0;
 for entry in &bundle.files{
  validate_relative(&entry.path)?;
  if !seen.insert(entry.path.as_str()){return Err("duplicate backup entry".into());}
  let bytes=STANDARD.decode(&entry.content).map_err(|e|e.to_string())?;
  if bytes.len()>MAX_FILE{return Err("backup entry exceeds file bound".into());}
  total+=bytes.len();
  if total>MAX_TOTAL{return Err("backup restore exceeds snapshot bound".into());}
 }
 Ok(bundle)
}
pub fn apply_pending(settings:&Settings)->Result<(),String>{
 let _held=guard();
 let pending=root(settings).join("restore-pending.dbackup");
 if !pending.exists(){return Ok(());}
 let ring=load_keys(settings,false)?;
 let bytes=fs::read(&pending).map_err(|e|e.to_string())?;
 let bundle=decode_bundle(&decrypt(&ring,&bytes)?)?;
 // Validate every destination before writing a single restored entry.
 for entry in &bundle.files{
  let (scope,relative)=validate_relative(&entry.path)?;
  let base=if scope=="data"{&settings.data_dir}else{&settings.workspace_dir};
  fs::create_dir_all(base).map_err(|e|e.to_string())?;
  let canonical=base.canonicalize().map_err(|e|e.to_string())?;
  let target=canonical.join(relative);
  if target.exists(){
   if target.symlink_metadata().map_err(|e|e.to_string())?.file_type().is_symlink(){
    return Err("restore target is a symlink".into());
   }
  }
  let mut parent=target.parent();
  while let Some(path)=parent{
   if path==canonical{break;}
   if path.exists()&&path.symlink_metadata().map_err(|e|e.to_string())?.file_type().is_symlink(){
    return Err("restore parent is a symlink".into());
   }
   parent=path.parent();
  }
 }
 for entry in &bundle.files{
  let (scope,relative)=validate_relative(&entry.path)?;
  let base=if scope=="data"{&settings.data_dir}else{&settings.workspace_dir};
  let bytes=STANDARD.decode(&entry.content).map_err(|e|e.to_string())?;
  atomic(&base.join(relative),&bytes)?;
 }
 fs::remove_file(pending).map_err(|e|e.to_string())?;
 Ok(())
}
fn require_maintenance(engine:&Engine)->Result<(),Fault>{
 if !maintenance(&engine.settings){return Err(fault(StatusCode::CONFLICT,
  "enable maintenance mode before modifying backups or restoring state"));}
 Ok(())
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/system/backups",get(backups))
  .route("/system/backup",post(create_backup))
  .route("/system/restore",post(restore))
  .route("/system/maintenance",get(maintenance_status).post(maintenance_set))
  .route("/system/backup-keys",get(backup_keys))
  .route("/system/backup-keys/rotate",post(rotate_backup_keys))
  .route("/system/secret-key",get(secret_status))
  .route("/system/secret-key/rotate",post(rotate_secret_unavailable))
}
async fn backups(State(engine):State<Arc<Engine>>)->Json<Value>{
 let dir=root(&engine.settings);
 let mut list=Vec::new();
 if let Ok(entries)=fs::read_dir(dir){
  for entry in entries.flatten(){
   let name=entry.file_name().to_string_lossy().into_owned();
   if !name.starts_with("dominion-")||!name.ends_with(".dbackup"){continue;}
   if let Ok(meta)=entry.metadata(){
    if !meta.is_file(){continue;}
    let modified=meta.modified().ok().and_then(|time|time.duration_since(std::time::UNIX_EPOCH).ok())
     .map(|time|crate::frontend_state::timestamp(time.as_millis() as u64));
    list.push(json!({"name":name,"path":entry.path(),"bytes":meta.len(),
     "encrypted":true,"modifiedAt":modified}));
   }
  }
 }
 list.sort_by(|a,b|b["name"].as_str().cmp(&a["name"].as_str()));
 Json(Value::Array(list))
}
async fn create_backup(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 require_maintenance(&engine)?;
 let settings=engine.settings.clone();
 let result=tokio::task::spawn_blocking(move||{
  let _held=guard();
  let ring=load_keys(&settings,true)?;
  let bytes=serde_json::to_vec(&snapshot(&settings)?).map_err(|e|e.to_string())?;
  let encrypted=encrypt(&ring,&bytes)?;
  let file=root(&settings).join(format!("dominion-{}-{}.dbackup",now_ms(),Uuid::new_v4()));
  atomic(&file,&encrypted)?;
  Ok::<Value,String>(json!({"ok":true,"path":file,"bytes":encrypted.len(),
   "remote":{"attempted":false}}))
 }).await.map_err(|e|bad(e.to_string()))?.map_err(bad)?;
 Ok(Json(result))
}
#[derive(Deserialize)]
struct MaintenanceUpdate{enabled:bool}
async fn maintenance_status(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(json!({"maintenance":maintenance(&engine.settings)}))
}
async fn maintenance_set(State(engine):State<Arc<Engine>>,Json(input):Json<MaintenanceUpdate>)
 ->Result<Json<Value>,Fault>{
 let settings=engine.settings.clone();
 if input.enabled{
  let live=engine.store.list().await.iter().any(|run|!run.terminal()&&
   matches!(run.status,RunStatus::Queued|RunStatus::Running|RunStatus::AwaitingApproval));
  let active=engine.campaigns.list().await.iter().any(|c|matches!(c.status.as_str(),"queued"|"running"));
  if live||active{return Err(fault(StatusCode::CONFLICT,"finish active jobs and RSI campaigns before maintenance"));}
 }
 tokio::task::spawn_blocking(move||{
  let _held=guard();
  atomic(&flagfile(&settings),json!({"maintenance":input.enabled}).to_string().as_bytes())
 }).await.map_err(|e|bad(e.to_string()))?.map_err(bad)?;
 Ok(Json(json!({"maintenance":input.enabled})))
}
#[derive(Deserialize)]
struct Restore{path:String,confirm:bool}
async fn restore(State(engine):State<Arc<Engine>>,Json(input):Json<Restore>)
 ->Result<Json<Value>,Fault>{
 require_maintenance(&engine)?;
 if !input.confirm{return Err(bad("restore requires explicit confirmation"));}
 let name=Path::new(&input.path).file_name().and_then(|v|v.to_str()).unwrap_or("");
 if !name.starts_with("dominion-")||!name.ends_with(".dbackup")||
  (input.path!=name && input.path!=root(&engine.settings).join(name).to_string_lossy()){
  return Err(bad("restore accepts only a listed local backup path"));
 }
 let settings=engine.settings.clone();let name=name.to_owned();
 let staged=tokio::task::spawn_blocking(move||{
  let _held=guard();
  let file=root(&settings).join(&name);
  let bytes=fs::read(file).map_err(|e|e.to_string())?;
  let key=load_keys(&settings,false)?;
  decode_bundle(&decrypt(&key,&bytes)?)?;
  atomic(&root(&settings).join("restore-pending.dbackup"),&bytes)?;
  Ok::<(),String>(())
 }).await.map_err(|e|bad(e.to_string()))?.map_err(bad)?;
 let _=staged;
 Ok(Json(json!({"ok":true,"staged":true,"requiresRestart":true})))
}
async fn backup_keys(State(engine):State<Arc<Engine>>)->Json<Value>{
 let ring=load_keys(&engine.settings,false);
 match ring{
  Ok(ring)=>Json(json!({"activeVersion":ring.active,
   "versions":ring.versions.keys().copied().collect::<Vec<_>>() })),
  Err(_)=>Json(json!({"activeVersion":0,"versions":[]}))
 }
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Rotate{#[serde(default)]keep_old:bool}
async fn rotate_backup_keys(State(engine):State<Arc<Engine>>,Json(input):Json<Rotate>)
 ->Result<Json<Value>,Fault>{
 require_maintenance(&engine)?;
 let settings=engine.settings.clone();
 let result=tokio::task::spawn_blocking(move||{
  let _held=guard();
  let mut ring=load_keys(&settings,true)?;
  let old=ring.clone();
  let next=ring.active.checked_add(1).ok_or("backup key version exhausted")?;
  let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
  ring.versions.insert(next,STANDARD.encode(key));ring.active=next;
  // Persist both keys before migrating anything, so a crash can never
  // make an existing encrypted backup unreadable.
  atomic(&keyfile(&settings),&serde_json::to_vec(&ring).map_err(|e|e.to_string())?)?;
  let mut changed=0;
  for item in fs::read_dir(root(&settings)).map_err(|e|e.to_string())?{
   let item=item.map_err(|e|e.to_string())?;
   let name=item.file_name().to_string_lossy().into_owned();
   if !name.starts_with("dominion-")||!name.ends_with(".dbackup"){continue;}
   let original=fs::read(item.path()).map_err(|e|e.to_string())?;
   let contents=decrypt(&old,&original)?;
   decode_bundle(&contents)?;
   let encrypted=encrypt(&ring,&contents)?;
   atomic(&item.path(),&encrypted)?;
   changed+=1;
  }
  if !input.keep_old{
   ring.versions.retain(|version,_|*version==next);
   atomic(&keyfile(&settings),&serde_json::to_vec(&ring).map_err(|e|e.to_string())?)?;
  }
  Ok::<Value,String>(json!({"activeVersion":ring.active,
   "retainedVersions":ring.versions.keys().copied().collect::<Vec<_>>(),
   "reencrypted":changed}))
 }).await.map_err(|e|bad(e.to_string()))?.map_err(bad)?;
 Ok(Json(result))
}
async fn secret_status()->Json<Value>{
 Json(json!({"previousKeys":0,"oldKeyFiles":[],
  "error":"Provider credentials use their own file permissions; separate secret rotation is not configured."}))
}
async fn rotate_secret_unavailable()->Result<Json<Value>,Fault>{
 Err(fault(StatusCode::NOT_IMPLEMENTED,
  "Provider secret rotation needs a transactional encrypted registry migration; it has not been configured."))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn backup_crypto_rejects_tampering_and_carries_key_versions(){
  let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
  let ring=Keys{active:2,versions:BTreeMap::from([(2,STANDARD.encode(key))])};
  let data=b"private rust state";
  let sealed=encrypt(&ring,data).unwrap();
  assert_eq!(decrypt(&ring,&sealed).unwrap(),data);
  let mut altered=sealed.clone();*altered.last_mut().unwrap()^=1;
  assert!(decrypt(&ring,&altered).is_err());
  assert!(validate_relative("data/../secrets").is_err());
  assert!(validate_relative("data/system-backups/keys.json").is_err());
  assert!(validate_relative("workspace/src/main.rs").is_ok());
 }
}
