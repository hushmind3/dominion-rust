//! GitHub / Google / YouTube OAuth authorization-code flow for the unchanged UI.
//! Access and refresh tokens are encrypted at rest; never returned through API.
//! Fixed provider endpoints, random single-use state and Google PKCE are required.
use crate::{config::Settings,domain::now_ms,engine::Engine};
use axum::{extract::{Path,Query,State},http::{header,StatusCode},routing::{get,post},
 response::{IntoResponse,Response},Json,Router};
use base64::{engine::general_purpose::{STANDARD,URL_SAFE_NO_PAD},Engine as _};
use chacha20poly1305::{aead::{Aead,KeyInit,OsRng,rand_core::RngCore},Key,XChaCha20Poly1305,XNonce};
use serde::{Serialize,Deserialize};
use serde_json::{json,Value};
use sha2::{Sha256,Digest};
use std::{collections::HashMap,fs,io::Write,path::{Path as FilePath,PathBuf},
 sync::{Arc,Mutex,OnceLock},time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
fn bad(msg:impl Into<String>)->Fault{(StatusCode::BAD_REQUEST,Json(json!({"error":msg.into()})))}
fn fail(msg:impl Into<String>)->Fault{(StatusCode::BAD_GATEWAY,Json(json!({"error":msg.into()})))}
fn conflict(msg:impl Into<String>)->Fault{(StatusCode::CONFLICT,Json(json!({"error":msg.into()})))}
const MAGIC:&[u8]=b"DOMINION-OAUTH-V1\0";
static LOCK:OnceLock<Mutex<()>>=OnceLock::new();
static PENDING:OnceLock<Mutex<HashMap<String,Pending>>>=OnceLock::new();
fn lock()->std::sync::MutexGuard<'static,()>{
 LOCK.get_or_init(||Mutex::new(())).lock().unwrap_or_else(|e|e.into_inner())
}
fn pending()->&'static Mutex<HashMap<String,Pending>>{
 PENDING.get_or_init(||Mutex::new(HashMap::new()))
}
fn random()->String{
 let mut bytes=[0u8;32];OsRng.fill_bytes(&mut bytes);URL_SAFE_NO_PAD.encode(bytes)
}
fn valid_provider(id:&str)->bool{matches!(id,"github"|"google"|"youtube")}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct App{client_id:String,client_secret:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Account{
 display_name:String,access_token:String,
 #[serde(default)]refresh_token:String,
 #[serde(default)]scopes:String,
 #[serde(default)]expires_at_ms:u64
}
#[derive(Default,Serialize,Deserialize)]
struct Snapshot{
 #[serde(default)]apps:HashMap<String,App>,
 #[serde(default)]accounts:HashMap<String,Account>
}
struct Pending{provider:String,verifier:String,at_ms:u64,redirect:String}
fn base(settings:&Settings)->PathBuf{settings.data_dir.join("oauth-vault")}
fn atomic(path:&FilePath,bytes:&[u8])->Result<(),String>{
 let dir=path.parent().ok_or("OAuth storage parent missing")?;
 fs::create_dir_all(dir).map_err(|e|e.to_string())?;
 let temp=dir.join(format!(".oauth-{}.tmp",Uuid::new_v4()));
 let result=(||->Result<(),String>{
  let mut opts=fs::OpenOptions::new();opts.write(true).create_new(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
  let mut out=opts.open(&temp).map_err(|e|e.to_string())?;
  out.write_all(bytes).map_err(|e|e.to_string())?;
  out.sync_all().map_err(|e|e.to_string())?;
  drop(out);
  fs::rename(&temp,path).map_err(|e|e.to_string())?;
  #[cfg(unix)]fs::File::open(dir).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
  Ok(())
 })();
 if result.is_err(){let _=fs::remove_file(temp);}
 result
}
fn owner_only(path:&FilePath)->Result<(),String>{
 #[cfg(unix)]{
  use std::os::unix::fs::PermissionsExt;
  if fs::metadata(path).map_err(|e|e.to_string())?.permissions().mode()&0o077!=0{
   return Err("OAuth secrets permissions must be owner-only (chmod 600)".into());
  }
 }
 Ok(())
}
fn key(settings:&Settings)->Result<[u8;32],String>{
 let file=base(settings).join("key.bin");
 if !file.is_file(){
  let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
  atomic(&file,&key)?;
  return Ok(key);
 }
 owner_only(&file)?;
 let raw=fs::read(file).map_err(|e|e.to_string())?;
 raw.try_into().map_err(|_|"OAuth encryption key must be 32 bytes".into())
}
fn read(settings:&Settings)->Result<Snapshot,String>{
 let file=base(settings).join("state.enc");
 if !file.exists(){return Ok(Snapshot::default());}
 owner_only(&file)?;
 let bytes=fs::read(file).map_err(|e|e.to_string())?;
 if bytes.len()>2_000_000||bytes.len()<MAGIC.len()+24+16||!bytes.starts_with(MAGIC){
  return Err("OAuth vault container invalid".into());
 }
 let key=key(settings)?;
 let at=MAGIC.len();
 let cipher=XChaCha20Poly1305::new(Key::from_slice(&key));
 let plain=cipher.decrypt(XNonce::from_slice(&bytes[at..at+24]),&bytes[at+24..])
  .map_err(|_|"OAuth vault authentication failed")?;
 serde_json::from_slice(&plain).map_err(|e|e.to_string())
}
fn write(settings:&Settings,snapshot:&Snapshot)->Result<(),String>{
 let plain=serde_json::to_vec(snapshot).map_err(|e|e.to_string())?;
 if plain.len()>1_000_000{return Err("OAuth vault exceeds 1 MiB".into());}
 let key=key(settings)?;
 let cipher=XChaCha20Poly1305::new(Key::from_slice(&key));
 let mut nonce=[0u8;24];OsRng.fill_bytes(&mut nonce);
 let encrypted=cipher.encrypt(XNonce::from_slice(&nonce),plain.as_ref())
  .map_err(|_|"OAuth vault encryption failed".to_owned())?;
 let mut data=MAGIC.to_vec();data.extend_from_slice(&nonce);data.extend_from_slice(&encrypted);
 atomic(&base(settings).join("state.enc"),&data)
}
fn callback_url(settings:&Settings,id:&str)->Result<String,String>{
 let source=std::env::var("DOMINION_PUBLIC_URL")
  .unwrap_or_else(|_|format!("http://{}",settings.bind));
 let mut url=reqwest::Url::parse(&source).map_err(|e|e.to_string())?;
 if url.scheme()!="https" && !(url.scheme()=="http"&&
   url.host_str().is_some_and(|h|h=="localhost"||h.parse::<std::net::IpAddr>()
    .is_ok_and(|ip|ip.is_loopback()))){
  return Err("OAuth callback requires HTTPS or a loopback HTTP address".into());
 }
 if !url.username().is_empty()||url.password().is_some()
  ||url.fragment().is_some()||url.query().is_some(){return Err("invalid OAuth public URL".into());}
 url.set_path(&format!("/api/oauth/{id}/callback"));
 Ok(url.to_string())
}
fn client()->Result<reqwest::Client,String>{
 reqwest::Client::builder().timeout(Duration::from_secs(18))
  .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())
}
fn html(message:&str)->Response{
 // Only static local text is rendered, never untrusted provider parameters.
 ([(header::CONTENT_TYPE,"text/html; charset=utf-8"),
   (header::CACHE_CONTROL,"no-store")],
   format!("<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><title>DOMINION</title><body><p>{message}</p><button onclick=\"window.close()\">닫기</button><script>window.opener?.postMessage({{type:'dominion-oauth-completed'}},window.location.origin);</script></body></html>")
 ).into_response()
}
pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/oauth/providers",get(providers))
  .route("/oauth/providers/{id}",post(save_app))
  .route("/oauth/connections",get(connections))
  .route("/oauth/{id}/start",get(start))
  .route("/oauth/{id}/callback",get(callback))
  .route("/oauth/{id}/open",post(open_unavailable))
}
async fn providers(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let store=read(&engine.settings).map_err(bad)?;
 let rows=["github","google","youtube"].iter().map(|id|{
  json!({"id":id,"configured":store.apps.contains_key(*id),
   "callback":callback_url(&engine.settings,id).ok()})
 }).collect::<Vec<_>>();
 Ok(Json(json!(rows)))
}
async fn connections(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
 let store=read(&engine.settings).map_err(bad)?;
 Ok(Json(json!(store.accounts.iter().map(|(provider,account)|json!({
  "id":provider,"provider":provider,"displayName":account.display_name,
  "hasToken":!account.access_token.is_empty()||!account.refresh_token.is_empty(),
  "scopes":account.scopes
 })).collect::<Vec<_>>())))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct ClientDraft{client_id:String,client_secret:String}
async fn save_app(State(engine):State<Arc<Engine>>,Path(id):Path<String>,
 Json(draft):Json<ClientDraft>)->Result<Json<Value>,Fault>{
 if !valid_provider(&id){return Err(bad("unsupported OAuth provider"));}
 if draft.client_id.trim().is_empty()||draft.client_id.len()>512||
  draft.client_secret.is_empty()||draft.client_secret.len()>2048||
  draft.client_id.chars().any(char::is_control)||
  draft.client_secret.chars().any(char::is_control){
  return Err(bad("invalid OAuth client credentials"));
 }
 let _guard=lock();
 let mut state=read(&engine.settings).map_err(bad)?;
 state.apps.insert(id.clone(),App{client_id:draft.client_id,client_secret:draft.client_secret});
 write(&engine.settings,&state).map_err(bad)?;
 Ok(Json(json!({"id":id,"configured":true,"callback":callback_url(&engine.settings,&id).ok()})))
}
fn authorize_url(id:&str,app:&App,state:&str,verifier:&str,callback:&str)->Result<String,String>{
 let target=if id=="github"{"https://github.com/login/oauth/authorize"}
  else{"https://accounts.google.com/o/oauth2/v2/auth"};
 let mut url=reqwest::Url::parse(target).map_err(|e|e.to_string())?;
 {
  let mut p=url.query_pairs_mut();
  p.append_pair("client_id",&app.client_id)
   .append_pair("redirect_uri",callback)
   .append_pair("response_type","code")
   .append_pair("state",state);
  if id=="github"{
   p.append_pair("scope","read:user user:email");
  }else{
   let scopes=if id=="youtube"{
    "openid email profile https://www.googleapis.com/auth/youtube.upload"
   }else{"openid email profile"};
   p.append_pair("scope",scopes)
    .append_pair("access_type","offline")
    .append_pair("prompt","consent")
    .append_pair("code_challenge",&URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())))
    .append_pair("code_challenge_method","S256");
  }
 }
 Ok(url.to_string())
}
async fn start(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
 ->Result<Json<Value>,Fault>{
 if !valid_provider(&id){return Err(bad("unsupported OAuth provider"));}
 let app=read(&engine.settings).map_err(bad)?.apps.remove(&id)
  .ok_or_else(||bad("configure OAuth Client ID and Client Secret first"))?;
 let callback=callback_url(&engine.settings,&id).map_err(bad)?;
 let state=random();let verifier=random();
 let url=authorize_url(&id,&app,&state,&verifier,&callback).map_err(bad)?;
 {
  let mut guard=pending().lock().unwrap_or_else(|e|e.into_inner());
  guard.retain(|_,entry|now_ms().saturating_sub(entry.at_ms)<600_000);
  if guard.len()>=64{return Err(conflict("too many concurrent OAuth logins"));}
  guard.insert(state,Pending{provider:id,verifier,at_ms:now_ms(),redirect:callback});
 }
 Ok(Json(json!({"url":url})))
}
#[derive(Deserialize)]
struct Callback{code:Option<String>,state:Option<String>,error:Option<String>}
async fn callback(State(engine):State<Arc<Engine>>,Path(id):Path<String>,
 Query(input):Query<Callback>)->Result<Response,Fault>{
 if !valid_provider(&id){return Err(bad("unsupported OAuth provider"));}
 if input.error.is_some(){return Ok(html("로그인이 취소됐습니다. DOMINION으로 돌아가 다시 시도할 수 있습니다."));}
 let code=input.code.as_deref().filter(|s|!s.is_empty()&&s.len()<4096)
  .ok_or_else(||bad("OAuth authorization code missing"))?;
 let state=input.state.as_deref().ok_or_else(||bad("OAuth state missing"))?;
 let saved={
  pending().lock().unwrap_or_else(|e|e.into_inner()).remove(state)
 }.ok_or_else(||bad("OAuth state expired or already used"))?;
 if saved.provider!=id||now_ms().saturating_sub(saved.at_ms)>600_000{
  return Err(bad("OAuth callback state or lifetime mismatch"));
 }
 let app=read(&engine.settings).map_err(bad)?.apps.remove(&id)
  .ok_or_else(||bad("OAuth app configuration missing"))?;
 let http=client().map_err(bad)?;
 let response=if id=="github"{
  http.post("https://github.com/login/oauth/access_token")
   .header("Accept","application/json")
   .form(&[("client_id",app.client_id.as_str()),("client_secret",app.client_secret.as_str()),
    ("code",code),("redirect_uri",saved.redirect.as_str())])
   .send().await
 }else{
  http.post("https://oauth2.googleapis.com/token")
   .form(&[("client_id",app.client_id.as_str()),("client_secret",app.client_secret.as_str()),
    ("code",code),("redirect_uri",saved.redirect.as_str()),
    ("grant_type","authorization_code"),("code_verifier",saved.verifier.as_str())])
   .send().await
 }.map_err(|e|fail(format!("OAuth token exchange transport: {e}")))?;
 if !response.status().is_success(){return Err(fail(format!("OAuth token exchange HTTP {}",response.status())));}
 let token:Value=response.json().await.map_err(|_|fail("OAuth provider did not return JSON"))?;
 let access=token["access_token"].as_str().filter(|s|!s.is_empty())
  .ok_or_else(||fail("OAuth provider did not grant an access token"))?;
 let (who,display)=if id=="github"{
  let response=http.get("https://api.github.com/user")
   .header("User-Agent","Dominion-Rust-OAuth").bearer_auth(access)
   .send().await.map_err(|e|fail(e.to_string()))?;
  let ok=response.status().is_success();
  let json:Value=response.json().await.map_err(|_|fail("GitHub user identity unavailable"))?;
  (ok,json["login"].as_str().map(str::to_owned))
 }else{
  let response=http.get("https://openidconnect.googleapis.com/v1/userinfo")
   .bearer_auth(access).send().await.map_err(|e|fail(e.to_string()))?;
  let ok=response.status().is_success();
  let json:Value=response.json().await.map_err(|_|fail("Google user identity unavailable"))?;
  (ok,json["email"].as_str().or_else(||json["name"].as_str()).map(str::to_owned))
 };
 if !who{return Err(fail("OAuth account identity request was rejected"));}
 let display=display.ok_or_else(||fail("OAuth account identity was not returned"))?;
 let record=Account{
  display_name:display,access_token:access.to_owned(),
  refresh_token:token["refresh_token"].as_str().unwrap_or("").to_owned(),
  scopes:token["scope"].as_str().unwrap_or("").to_owned(),
  expires_at_ms:now_ms().saturating_add(token["expires_in"].as_u64().unwrap_or(3600).saturating_mul(1000))
 };
 {
  let _held=lock();
  let mut store=read(&engine.settings).map_err(bad)?;
  // Do not lose a previously granted refresh token when the provider omits
  // one from a subsequent authorization-code exchange.
  let mut record=record;
  if record.refresh_token.is_empty(){
   if let Some(old)=store.accounts.get(&id){record.refresh_token=old.refresh_token.clone();}
  }
  store.accounts.insert(id.clone(),record);
  write(&engine.settings,&store).map_err(bad)?;
 }
 Ok(html("계정 연결이 완료됐습니다. DOMINION 창으로 돌아가 연결 목록을 새로고침하세요."))
}
async fn open_unavailable()->Result<Json<Value>,Fault>{
 Err((StatusCode::NOT_IMPLEMENTED,Json(json!({"error":"Native desktop popup requires an installed desktop bridge. Use browser OAuth login."}))))
}
/// For authenticated native video integrations. A Google refresh is persisted
/// before returning; an expired access token is never silently reused.
pub async fn youtube_token(settings:&Settings)->Result<String,String>{
 let record=read(settings)?.accounts.remove("youtube").ok_or("YouTube account is not connected")?;
 if !record.access_token.is_empty()&&record.expires_at_ms>now_ms().saturating_add(60_000){
  return Ok(record.access_token);
 }
 if record.refresh_token.is_empty(){return Err("YouTube access expired; reconnect account".into());}
 let app=read(settings)?.apps.remove("youtube").ok_or("YouTube OAuth app not configured")?;
 let http=client()?;
 let response=http.post("https://oauth2.googleapis.com/token")
  .form(&[("client_id",app.client_id.as_str()),("client_secret",app.client_secret.as_str()),
   ("refresh_token",record.refresh_token.as_str()),("grant_type","refresh_token")])
  .send().await.map_err(|e|e.to_string())?;
 if !response.status().is_success(){return Err(format!("YouTube refresh HTTP {}",response.status()));}
 let data:Value=response.json().await.map_err(|e|e.to_string())?;
 let access=data["access_token"].as_str().ok_or("Google refresh token response missing access_token")?;
 let _held=lock();
 let mut state=read(settings)?;
 let row=state.accounts.get_mut("youtube").ok_or("YouTube connection was removed")?;
 row.access_token=access.to_owned();
 row.expires_at_ms=now_ms().saturating_add(data["expires_in"].as_u64().unwrap_or(3600)*1000);
 write(settings,&state)?;
 Ok(access.to_owned())
}
pub fn youtube_public(settings:&Settings)->Option<Value>{
 let state=read(settings).ok()?;
 let user=state.accounts.get("youtube")?;
 Some(json!({"displayName":user.display_name,"scopes":user.scopes}))
}
pub fn youtube_connection(settings:&Settings)->bool{
 read(settings).ok().is_some_and(|s|s.accounts.get("youtube")
  .is_some_and(|row|!row.access_token.is_empty()||!row.refresh_token.is_empty()))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn oauth_state_and_pkce_not_leaked_and_vault_encrypted(){
  let mut settings=Settings::from_env().unwrap();
  let dir=std::env::temp_dir().join(format!("dominion-oauth-{}",Uuid::new_v4()));
  settings.data_dir=dir.clone();
  let app=App{client_id:"public".into(),client_secret:"very-secret".into()};
  let url=authorize_url("google",&app,"one-time-state","private-verifier","http://localhost/api/oauth/google/callback").unwrap();
  assert!(url.contains("code_challenge="));
  assert!(!url.contains("private-verifier"));
  let mut state=Snapshot::default();state.apps.insert("google".into(),app);
  write(&settings,&state).unwrap();
  let bytes=fs::read(base(&settings).join("state.enc")).unwrap();
  assert!(!String::from_utf8_lossy(&bytes).contains("very-secret"));
  assert_eq!(read(&settings).unwrap().apps["google"].client_secret,"very-secret");
  fs::remove_dir_all(dir).unwrap();
 }
}
