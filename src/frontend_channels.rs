//! Authenticated outbound channel bridges for the original Connections UI.
//! Telegram and Discord are supported only as outbound room sinks. No
//! invented incoming polling/webhook support is advertised.
use crate::{engine::Engine,group::Room};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::Write,path::{Path as FilePath,PathBuf},sync::{Arc,Mutex,OnceLock},time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
static WRITE_LOCK:OnceLock<Mutex<()>>=OnceLock::new();
fn guard()->std::sync::MutexGuard<'static,()>{
 WRITE_LOCK.get_or_init(||Mutex::new(())).lock().unwrap_or_else(|e|e.into_inner())
}
fn err(code:StatusCode,message:impl Into<String>)->Fault{
 (code,Json(json!({"error":message.into()})))
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Channel {
 id:String,name:String,kind:String,room_id:String,enabled:bool,
 chat_id:String,channel_id:String,token:String,
 #[serde(default)]sync_room_messages:bool,
 #[serde(default)]cursor:usize,
 #[serde(default)]pending_index:Option<usize>,
 #[serde(default)]runtime_error:Option<String>,
}
#[derive(Default,Serialize,Deserialize)]
struct Snapshot { channels:HashMap<String,Channel> }
fn file(engine:&Engine)->PathBuf{engine.settings.data_dir.join("channels.json")}
fn read(file:&FilePath)->Result<Snapshot,String>{
 if !file.exists(){return Ok(Snapshot::default());}
 #[cfg(unix)]{
  use std::os::unix::fs::PermissionsExt;
  if fs::metadata(file).map_err(|e|e.to_string())?.permissions().mode()&0o077!=0{
   return Err("channel configuration permissions must be owner-only".into());
  }
 }
 serde_json::from_slice(&fs::read(file).map_err(|e|e.to_string())?)
  .map_err(|e|format!("invalid channel configuration: {e}"))
}
fn save(file:&FilePath,state:&Snapshot)->Result<(),String>{
 let bytes=serde_json::to_vec(state).map_err(|e|e.to_string())?;
 if bytes.len()>2*1024*1024{return Err("channel configuration exceeds 2 MiB".into());}
 let dir=file.parent().ok_or("channel data path has no parent")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let temp=dir.join(format!(".channels-{}",Uuid::new_v4()));
 let result=(||->Result<(),String>{
  let mut opts=fs::OpenOptions::new();opts.write(true).create_new(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
  let mut output=opts.open(&temp).map_err(|e|e.to_string())?;
  output.write_all(&bytes).map_err(|e|e.to_string())?;
  output.sync_all().map_err(|e|e.to_string())?;
  drop(output);fs::rename(&temp,file).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if result.is_err(){let _=fs::remove_file(temp);}
 result
}
fn public(channel:&Channel)->Value{
 json!({
  "id":channel.id,"name":channel.name,"kind":channel.kind,
  "roomId":channel.room_id,"enabled":channel.enabled,
  "status":if !channel.enabled{"disabled"}
   else if channel.pending_index.is_some(){"needs_review"}
   else if channel.runtime_error.is_some(){"error"}else{"outbound_ready"},
  "runtimeError":channel.runtime_error,
  "configured":!channel.token.is_empty(),
  "credentialsPresent":{"token":!channel.token.is_empty()},
  "chatId":channel.chat_id,"channelId":channel.channel_id,
  "syncRoomMessages":channel.sync_room_messages,
  "delivery":{"pending":usize::from(channel.pending_index.is_some()),"cursor":channel.cursor}
 })
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/channels/catalog",get(catalog))
  .route("/channels",get(list).post(upsert))
  .route("/channels/{id}",axum::routing::delete(remove))
  .route("/channels/{id}/diagnose",post(diagnose))
}
async fn catalog()->Json<Value>{
 Json(json!([
  {"id":"telegram","name":"Telegram","category":"messaging","mode":"outbound",
   "available":true,"status":"available","requiresRoom":true,"requiresPublicUrl":false,
   "description":"실제 Telegram Bot API로 DOMINION 회의실의 새 메시지를 지정한 채팅에 보냅니다. 수신은 지원하지 않습니다.",
   "docs":"https://core.telegram.org/bots/api",
   "features":["메시지 보내기","전송 기록"],
   "fields":[
    {"key":"token","label":"Bot Token","type":"secret","required":true},
    {"key":"chatId","label":"Chat ID","type":"text","required":true}
   ]},
  {"id":"discord","name":"Discord","category":"community","mode":"outbound",
   "available":true,"status":"available","requiresRoom":true,"requiresPublicUrl":false,
   "description":"실제 Discord Bot API로 DOMINION 회의실의 새 메시지를 지정 채널에 보냅니다. 수신은 지원하지 않습니다.",
   "docs":"https://discord.com/developers/docs/resources/message",
   "features":["메시지 보내기","전송 기록"],
   "fields":[
    {"key":"token","label":"Bot Token","type":"secret","required":true},
    {"key":"channelId","label":"Channel ID","type":"text","required":true}
   ]}
 ]))
}
async fn list(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let records=read(&file(&engine)).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 let mut entries=records.channels.values().map(public).collect::<Vec<_>>();
 entries.sort_by(|a,b|a["name"].as_str().cmp(&b["name"].as_str()));
 Ok(Json(Value::Array(entries)))
}
fn safe_token(value:&str)->bool{
 !value.is_empty()&&value.len()<=512&&!value.chars().any(|c|c.is_control()||c.is_whitespace())
}
async fn upsert(State(engine):State<Arc<Engine>>,Json(input):Json<Value>)
 ->Result<Json<Value>,Fault>{
 let kind=input["kind"].as_str().ok_or_else(||err(StatusCode::BAD_REQUEST,"channel kind required"))?;
 if !matches!(kind,"telegram"|"discord"){
  return Err(err(StatusCode::NOT_IMPLEMENTED,"native outbound driver is not implemented for this channel"));
 }
 let name=input["name"].as_str().unwrap_or(kind).trim();
 if name.is_empty()||name.len()>120{return Err(err(StatusCode::BAD_REQUEST,"invalid channel name"));}
 let room_id=input["roomId"].as_str().unwrap_or("");
 let room_uuid=Uuid::parse_str(room_id).map_err(|_|err(StatusCode::BAD_REQUEST,"select a native room"))?;
 let room=engine.groups.get(room_uuid).await.ok_or_else(||err(StatusCode::NOT_FOUND,"channel room not found"))?;
 let destination=if kind=="telegram"{input["chatId"].as_str()}else{input["channelId"].as_str()}
  .ok_or_else(||err(StatusCode::BAD_REQUEST,"channel destination is required"))?;
 if destination.is_empty()||destination.len()>128||!destination.chars().all(|c|c.is_ascii_digit()||c=='-'){
  return Err(err(StatusCode::BAD_REQUEST,"invalid chat/channel ID"));
 }
 let id=input["id"].as_str().map(str::to_owned).unwrap_or_else(||Uuid::new_v4().to_string());
 if id.len()>120||id.is_empty(){return Err(err(StatusCode::BAD_REQUEST,"invalid channel ID"));}
 let new_token=input["token"].as_str().unwrap_or("");
 let _held=guard();
 let file=file(&engine);
 let mut state=read(&file).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 let previous=state.channels.get(&id);
 if previous.is_none()&&state.channels.len()>=64{
  return Err(err(StatusCode::CONFLICT,"maximum 64 channel configurations"));
 }
 if previous.is_some_and(|row|row.kind!=kind){
  return Err(err(StatusCode::CONFLICT,"channel driver cannot be changed after creation"));
 }
 let token=if new_token.is_empty(){
  previous.map(|row|row.token.clone()).unwrap_or_default()
 }else{new_token.to_owned()};
 if !safe_token(&token){return Err(err(StatusCode::BAD_REQUEST,"missing or malformed channel token"));}
 let cursor=if let Some(previous)=previous{
  if previous.room_id==room_id{previous.cursor}else{room.messages.len()}
 }else{room.messages.len()};
 let enabled=input["enabled"].as_bool().unwrap_or(false);
 let mut record=Channel{
  id:id.clone(),name:name.into(),kind:kind.into(),room_id:room_id.into(),enabled,
  chat_id:if kind=="telegram"{destination.to_owned()}else{String::new()},
  channel_id:if kind=="discord"{destination.to_owned()}else{String::new()},
  token,sync_room_messages:input["syncRoomMessages"].as_bool().unwrap_or(true),
  cursor,pending_index:previous.and_then(|row|row.pending_index),
  runtime_error:None,
 };
 if previous.is_some_and(|p|p.room_id!=room_id){record.pending_index=None;}
 state.channels.insert(id,record.clone());
 save(&file,&state).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 Ok(Json(public(&record)))
}
async fn remove(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
 ->Result<Json<Value>,Fault>{
 let _held=guard();let file=file(&engine);
 let mut state=read(&file).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 state.channels.remove(&id).ok_or_else(||err(StatusCode::NOT_FOUND,"channel not found"))?;
 save(&file,&state).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 Ok(Json(json!({"ok":true})))
}
fn http()->Result<reqwest::Client,String>{
 reqwest::Client::builder().timeout(Duration::from_secs(16))
  .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())
}
async fn probe(row:&Channel)->Result<(),String>{
 let client=http()?;
 let response=if row.kind=="telegram"{
  let me=client.get(format!("https://api.telegram.org/bot{}/getMe",row.token))
   .send().await.map_err(|_|"Telegram credential check connection failed".to_owned())?;
  let actual=me.json::<Value>().await.map_err(|_|"Telegram bot check returned invalid JSON")?;
  if actual["ok"]!=true{return Err("Telegram Bot Token verification failed".into());}
  client.post(format!("https://api.telegram.org/bot{}/getChat",row.token))
   .form(&[("chat_id",row.chat_id.as_str())])
   .send().await.map_err(|_|"Telegram chat check connection failed".to_owned())?
 }else{
  let auth=format!("Bot {}",row.token);
  let me=client.get("https://discord.com/api/v10/users/@me").header("Authorization",&auth)
   .send().await.map_err(|_|"Discord bot check connection failed".to_owned())?;
  if !me.status().is_success(){return Err("Discord Bot Token verification failed".into());}
  client.get(format!("https://discord.com/api/v10/channels/{}",row.channel_id))
   .header("Authorization",auth)
   .send().await.map_err(|_|"Discord destination check failed".to_owned())?
 };
 if response.status().is_success(){Ok(())}
 else{Err(format!("channel destination rejected access: HTTP {}",response.status()))}
}
async fn diagnose(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
 ->Result<Json<Value>,Fault>{
 let record=read(&file(&engine)).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?
  .channels.remove(&id).ok_or_else(||err(StatusCode::NOT_FOUND,"channel not found"))?;
 match probe(&record).await{
  Ok(())=>Ok(Json(json!({"verified":true,"message":"Bot and destination verified by live API"}))),
  Err(error)=>Ok(Json(json!({"verified":false,"message":error})))
 }
}
async fn transmit(row:&Channel,content:&str)->Result<(),String>{
 let client=http()?;
 let response=if row.kind=="telegram"{
  client.post(format!("https://api.telegram.org/bot{}/sendMessage",row.token))
   .form(&[("chat_id",row.chat_id.as_str()),("text",content)])
   .send().await.map_err(|_|"Telegram message delivery transport failed".to_owned())?
 }else{
  client.post(format!("https://discord.com/api/v10/channels/{}/messages",row.channel_id))
   .header("Authorization",format!("Bot {}",row.token))
   .json(&json!({"content":content}))
   .send().await.map_err(|_|"Discord message delivery transport failed".to_owned())?
 };
 if response.status().is_success(){Ok(())}
 else{Err(format!("channel message delivery HTTP {}",response.status()))}
}
fn update(file:&FilePath,id:&str,edit:impl FnOnce(&mut Channel))->Result<(),String>{
 let _held=guard();let mut state=read(file)?;
 let record=state.channels.get_mut(id).ok_or("channel missing")?;
 edit(record);
 save(file,&state)
}
/// At-most-once intent: mark a delivery uncertain BEFORE outbound HTTP. A
/// crash cannot silently replay an already delivered external message.
pub async fn dispatch_room(engine:&Arc<Engine>,room:&Room){
 let file=file(engine);
 let entries=match read(&file){
  Ok(state)=>state.channels.into_values().filter(|row|
   row.enabled&&row.sync_room_messages&&row.room_id==room.id.to_string()&&
   row.pending_index.is_none()).collect::<Vec<_>>(),
  Err(error)=>{tracing::warn!(%error,"cannot read outbound room bridges");return;}
 };
 for entry in entries {
  let start=entry.cursor.min(room.messages.len());
  let until=room.messages.len().min(start.saturating_add(30));
  for index in start..until {
   let message=&room.messages[index];
   let limit=if entry.kind=="telegram"{3000}else{1700};
   let content=format!("{}: {}",message.speaker,
    message.content.chars().take(limit).collect::<String>());
   let claimed=update(&file,&entry.id,|row|{
    if row.pending_index.is_none()&&row.cursor==index{
     row.pending_index=Some(index);
     row.runtime_error=Some("delivery may be in-flight; review before retrying".into());
    }
   });
   if claimed.is_err(){break;}
   let current=match read(&file).ok().and_then(|s|s.channels.get(&entry.id).cloned()){
    Some(record) if record.pending_index==Some(index)=>record,
    _=>break
   };
   let outcome=transmit(&current,&content).await;
   if let Err(error)=update(&file,&entry.id,|row|{
    if row.pending_index==Some(index){
     if let Err(ref failure)=outcome {
      row.runtime_error=Some(failure.clone());
     }else{
      row.cursor=index+1;row.pending_index=None;row.runtime_error=None;
     }
    }
   }){tracing::warn!(%error,"outbound receipt could not be persisted");break;}
   if outcome.is_err(){break;}
  }
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn channel_public_projection_never_contains_bot_secret(){
  let row=Channel{id:"id".into(),name:"Bot".into(),kind:"telegram".into(),
   room_id:"room".into(),enabled:true,chat_id:"123".into(),channel_id:String::new(),
   token:"sensitive-token".into(),sync_room_messages:true,cursor:0,
   pending_index:None,runtime_error:None};
  assert!(!public(&row).to_string().contains("sensitive-token"));
  assert!(safe_token("123456:ABC"));
  assert!(!safe_token("bad\nkey"));
 }
}
