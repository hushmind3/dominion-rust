//! Authenticated outbound channel bridges for the original Connections UI.
//! Telegram and Discord are supported only as outbound room sinks. No
//! invented incoming polling/webhook support is advertised.
use crate::{engine::Engine,group::Room};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},fs,io::Write,path::{Path as FilePath,PathBuf},sync::{Arc,Mutex,OnceLock},time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
static WRITE_LOCK:OnceLock<Mutex<()>>=OnceLock::new();
// A process-local gate distinguishes an active HTTP send from a crash-unknown
// delivery. Only the latter can be reconciled by a user.
static ACTIVE_SENDS:OnceLock<Mutex<HashSet<String>>>=OnceLock::new();
fn active_sends()->&'static Mutex<HashSet<String>>{
 ACTIVE_SENDS.get_or_init(||Mutex::new(HashSet::new()))
}
struct ActiveSend(String);
impl Drop for ActiveSend {
 fn drop(&mut self){
  active_sends().lock().unwrap_or_else(|e|e.into_inner()).remove(&self.0);
 }
}

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
 #[serde(default)]verified:bool,
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
 let sending=channel.pending_index.is_some()&&active_sends()
  .lock().unwrap_or_else(|e|e.into_inner()).contains(&channel.id);
 json!({
  "id":channel.id,"name":channel.name,"kind":channel.kind,
  "roomId":channel.room_id,"enabled":channel.enabled,
  "status":if sending{"sending"}
   else if !channel.enabled{"disabled"}
   else if channel.pending_index.is_some(){"needs_review"}
   else if channel.runtime_error.is_some(){"error"}
   else if !channel.verified{"not_verified"}
   else if !channel.sync_room_messages{"sync_disabled"}
   else{"outbound_ready"},
  "runtimeError":channel.runtime_error,
  "configured":!channel.token.is_empty(),
  "credentialsPresent":{"token":!channel.token.is_empty()},
  "chatId":channel.chat_id,"channelId":channel.channel_id,
  "syncRoomMessages":channel.sync_room_messages,
  "delivery":{"pending":usize::from(channel.pending_index.is_some()),
   "sending":usize::from(sending),
   "needsReview":usize::from(channel.pending_index.is_some()&&!sending),
   "pendingIndex":channel.pending_index,"cursor":channel.cursor}
 })
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/channels/catalog",get(catalog))
  .route("/channels",get(list).post(upsert))
  .route("/channels/{id}",axum::routing::delete(remove))
  .route("/channels/{id}/diagnose",post(diagnose))
  .route("/channels/{id}/delivery/reconcile",post(reconcile_delivery))
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
 // A pending external effect cannot be silently transferred to different
 // credentials or destination. Resolve its observed outcome first.
 if previous.is_some_and(|row|row.pending_index.is_some() &&
     (row.room_id!=room_id || row.token!=new_token && !new_token.is_empty() ||
      (if kind=="telegram"{row.chat_id!=destination}else{row.channel_id!=destination}))){
  return Err(err(StatusCode::CONFLICT,
   "review the pending delivery before changing credentials or destination"));
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
  verified:previous.is_some_and(|row|row.verified && row.token==token &&
   row.room_id==room_id && (if kind=="telegram"{row.chat_id==destination}
    else{row.channel_id==destination})),
  token,sync_room_messages:input["syncRoomMessages"].as_bool().unwrap_or(true),
  cursor,pending_index:previous.and_then(|row|row.pending_index),
  runtime_error:previous.filter(|row|row.pending_index.is_some())
   .and_then(|row|row.runtime_error.clone()),
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
 let checked=probe(&record).await;
 // Reject stale verification: a user may have rotated the token or target
 // while the remote probe was in flight. Such a result is not authorization
 // for a newly configured destination.
 let file=file(&engine);
 let _held=guard();
 let mut state=read(&file).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 let row=state.channels.get_mut(&id)
  .ok_or_else(||err(StatusCode::NOT_FOUND,"channel not found"))?;
 if row.token!=record.token || row.room_id!=record.room_id ||
     row.chat_id!=record.chat_id || row.channel_id!=record.channel_id {
  return Err(err(StatusCode::CONFLICT,
   "channel changed during verification; run the check again"));
 }
 row.verified=checked.is_ok();
 // Never overwrite an unresolved outbound receipt with a diagnostic result.
 if row.pending_index.is_none(){row.runtime_error=checked.as_ref().err().cloned();}
 save(&file,&state).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 match checked{
  Ok(())=>Ok(Json(json!({"verified":true,"message":"Bot and destination verified by live API"}))),
  Err(error)=>Ok(Json(json!({"verified":false,"message":error})))
 }
}
/// A human must identify the observed outcome of an uncertain external send.
/// "confirmed_delivered" skips the exact already-sent message; "confirmed_not_delivered"
/// allows retry on the next room event. Neither outcome is invented by the server.
async fn reconcile_delivery(State(engine):State<Arc<Engine>>,
 Path(id):Path<String>,Json(input):Json<Value>)->Result<Json<Value>,Fault>{
 let resolution=input["resolution"].as_str().unwrap_or("");
 if !matches!(resolution,"confirmed_delivered"|"confirmed_not_delivered"){
  return Err(err(StatusCode::BAD_REQUEST,"select an observed delivery outcome"));
 }
 let _held=guard();
 if active_sends().lock().unwrap_or_else(|e|e.into_inner()).contains(&id){
  return Err(err(StatusCode::CONFLICT,
   "delivery is still in progress; check again when it finishes"));
 }
 let file=file(&engine);
 let mut state=read(&file).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 let row=state.channels.get_mut(&id)
  .ok_or_else(||err(StatusCode::NOT_FOUND,"channel not found"))?;
 let pending=row.pending_index.ok_or_else(||err(StatusCode::CONFLICT,
  "there is no uncertain delivery to reconcile"))?;
 if resolution=="confirmed_delivered"{row.cursor=pending.saturating_add(1);}
 row.pending_index=None;
 row.runtime_error=None;
 let response=public(row);
 save(&file,&state).map_err(|e|err(StatusCode::INTERNAL_SERVER_ERROR,e))?;
 Ok(Json(response))
}

/// Atomically claim a specific unsent room message. No stale concurrent
/// fanout may send the same message merely because the earlier caller already
/// wrote its pending_index. The active marker is installed before any await.
fn claim_delivery(file:&FilePath,entry:&Channel,index:usize)
 ->Result<Option<ActiveSend>,String>{
 let _held=guard();
 let mut state=read(file)?;
 let Some(row)=state.channels.get_mut(&entry.id)else{return Ok(None)};
 if !row.enabled || !row.verified || !row.sync_room_messages ||
     row.pending_index.is_some() || row.cursor!=index ||
     row.room_id!=entry.room_id || row.token!=entry.token ||
     row.chat_id!=entry.chat_id || row.channel_id!=entry.channel_id {
  return Ok(None);
 }
 row.pending_index=Some(index);
 row.runtime_error=Some("delivery outcome has not been confirmed".into());
 save(file,&state)?;
 active_sends().lock().unwrap_or_else(|e|e.into_inner())
  .insert(entry.id.clone());
 Ok(Some(ActiveSend(entry.id.clone())))
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
   row.enabled&&row.verified&&row.sync_room_messages&&row.room_id==room.id.to_string()&&
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
   let _active=match claim_delivery(&file,&entry,index){
    Ok(Some(guard))=>guard,
    Ok(None)=>break,
    Err(error)=>{tracing::warn!(%error,"outbound claim could not be persisted");break;}
   };
   let current=match read(&file).ok().and_then(|s|s.channels.get(&entry.id).cloned()){
    Some(record) if record.pending_index==Some(index)=>record,
    _=>break
   };
   // If the operator disabled/suspended this channel during the claim,
   // do not send a new external message. Retain the unknown receipt for
   // deliberate reconciliation rather than silently advancing the cursor.
   if !current.enabled || !current.sync_room_messages{
    // No HTTP send began. Release this locally known-unsent claim rather
    // than asking the operator to reconcile an effect that never happened.
    let _=update(&file,&entry.id,|row|{
     if row.pending_index==Some(index){
      row.pending_index=None;row.runtime_error=None;
     }
    });
    break;
   }
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
 fn concurrent_outbound_claims_never_dispatch_the_same_message_twice(){
  let base=std::env::temp_dir().join(format!("dominion-channel-{}",Uuid::new_v4()));
  let file=base.join("channels.json");
  let row=Channel{id:"claim-1".into(),name:"Demo".into(),kind:"telegram".into(),
   room_id:"room".into(),enabled:true,chat_id:"123".into(),channel_id:String::new(),
   token:"fixture-token".into(),verified:true,sync_room_messages:true,
   cursor:0,pending_index:None,runtime_error:None};
  let mut state=Snapshot::default();
  state.channels.insert(row.id.clone(),row.clone());
  save(&file,&state).expect("persist a clean channel fixture");
  let hold=claim_delivery(&file,&row,0).expect("first claim").expect("owns send");
  assert_eq!(public(&read(&file).unwrap().channels[&row.id])["status"],"sending");
  assert!(claim_delivery(&file,&row,0).unwrap().is_none(),
   "a concurrent fanout must not call the remote API twice");
  assert_eq!(read(&file).unwrap().channels[&row.id].pending_index,Some(0));
  drop(hold);
  assert_eq!(public(&read(&file).unwrap().channels[&row.id])["status"],"needs_review",
   "a process exit or uncertain result requires manual confirmation");
  assert_eq!(public(&read(&file).unwrap().channels[&row.id])["delivery"]["sending"],0);
  std::fs::remove_dir_all(&base).expect("clean isolated fixture");
 }
 #[test]
 fn projected_delivery_status_never_misreports_disabled_sync_as_ready(){
  let mut row=Channel{id:"c".into(),name:"Channel".into(),kind:"discord".into(),
   room_id:"room".into(),enabled:true,chat_id:String::new(),channel_id:"123".into(),
   token:"secret".into(),verified:true,sync_room_messages:false,cursor:5,
   pending_index:None,runtime_error:None};
  assert_eq!(public(&row)["status"],"sync_disabled");
  row.sync_room_messages=true;
  assert_eq!(public(&row)["status"],"outbound_ready");
  row.pending_index=Some(5);
  assert_eq!(public(&row)["status"],"needs_review");
  assert_eq!(public(&row)["delivery"]["pendingIndex"],5);
  assert!(!public(&row).to_string().contains("secret"));
 }
 #[test]
 fn channel_public_projection_never_contains_bot_secret(){
  let row=Channel{id:"id".into(),name:"Bot".into(),kind:"telegram".into(),
   room_id:"room".into(),enabled:true,chat_id:"123".into(),channel_id:String::new(),
   token:"sensitive-token".into(),verified:false,sync_room_messages:true,cursor:0,
   pending_index:None,runtime_error:None};
  assert!(!public(&row).to_string().contains("sensitive-token"));
  assert!(safe_token("123456:ABC"));
  assert!(!safe_token("bad\nkey"));
 }
}
