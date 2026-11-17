//! Connected Google Drive, Docs and Sheets. No arbitrary host, token in model
//! payload, automatic mutations, or unverified external write retries.
use crate::{config::Settings,domain::ToolCall,mcp::McpCallOutcome};
use reqwest::{Client,RequestBuilder,StatusCode,Url};
use serde_json::{json,Value};
use std::time::Duration;
const DRIVE:&str="https://www.googleapis.com";
const DOCS:&str="https://docs.googleapis.com";
const SHEETS:&str="https://sheets.googleapis.com";
const MAX_TEXT:usize=128*1024;
const MAX_ROWS:usize=250;
#[derive(Clone)]
struct Endpoints{drive:String,docs:String,sheets:String}
impl Default for Endpoints{
 fn default()->Self{Self{drive:DRIVE.into(),docs:DOCS.into(),sheets:SHEETS.into()}}
}
fn def(name:&str,about:&str,properties:Value,required:&[&str])->Value{
 json!({"type":"function","function":{"name":name,"description":about,
 "parameters":{"type":"object","properties":properties,"required":required}}})
}
pub fn definitions()->Vec<Value>{
 let id=json!({"file_id":{"type":"string"}});
 let doc=json!({"document_id":{"type":"string"}});
 let mut edit=doc.clone();edit["text"]=json!({"type":"string"});edit["expected_revision"]=json!({"type":"string"});
 let sheet=json!({"spreadsheet_id":{"type":"string"},"range":{"type":"string"}});
 let mut write=sheet.clone();write["values"]=json!({"type":"array","items":{"type":"array"}});
 write["expected_values"]=json!({"type":"array","items":{"type":"array"}});
 vec![
 def("google_drive_search","Search connected Google Drive metadata by file name; returns names and file IDs.",json!({"query":{"type":"string"}}),&[]),
 def("google_drive_read_text","Read plain-text Drive file or export Google Doc to text, using explicit file ID.",id,&["file_id"]),
 def("google_drive_upload_text","Create a UTF-8 text file in Google Drive; human approval and read-back required.",json!({"name":{"type":"string"},"text":{"type":"string"},"folder_id":{"type":"string"}}),&["name","text"]),
 def("google_drive_create_folder","Create a named Google Drive folder after approval and verify the folder ID.",json!({"name":{"type":"string"},"parent_id":{"type":"string"}}),&["name"]),
 def("google_docs_get","Read a Google document, its revision ID and bounded text.",doc.clone(),&["document_id"]),
 def("google_docs_create","Create a Google document by title after approval; verify it by re-reading.",json!({"title":{"type":"string"}}),&["title"]),
 def("google_docs_append","Append to a Google document with an observed requiredRevisionId, approval and read-back.",edit,&["document_id","text","expected_revision"]),
 def("google_sheets_create","Create a Google spreadsheet with a title after human approval and confirm its ID.",json!({"title":{"type":"string"}}),&["title"]),
 def("google_sheets_get","Read a Google Sheets A1 range and cell values.",sheet,&["spreadsheet_id","range"]),
 def("google_sheets_update","Update exact cells after pre-reading expected values and approval; confirm new values by re-reading.",write.clone(),&["spreadsheet_id","range","expected_values","values"]),
 def("google_sheets_append","Append rows to a selected sheet after approval; verify the updated range.",write,&["spreadsheet_id","range","values"])
 ]
}
pub fn supports(name:&str)->bool{matches!(name,
 "google_drive_search"|"google_drive_read_text"|"google_drive_upload_text"|"google_drive_create_folder"|
 "google_docs_get"|"google_docs_create"|"google_docs_append"|"google_sheets_create"|
 "google_sheets_get"|"google_sheets_update"|"google_sheets_append")}
pub fn mutates(name:&str)->bool{matches!(name,
 "google_drive_upload_text"|"google_drive_create_folder"|"google_docs_create"|
 "google_docs_append"|"google_sheets_create"|"google_sheets_update"|"google_sheets_append")}
fn required<'a>(a:&'a Value,key:&str)->Result<&'a str,String>{
 a.get(key).and_then(Value::as_str).filter(|x|!x.trim().is_empty()&&x.len()<=MAX_TEXT)
 .ok_or_else(||format!("Google {key} required"))
}
fn id(s:&str)->Result<&str,String>{
 if s.is_empty()||s.len()>256||!s.bytes().all(|c|c.is_ascii_alphanumeric()||b"_-/".contains(&c)){
  return Err("invalid Google file ID".into());}
 Ok(s)
}
fn a1(s:&str)->Result<&str,String>{
 if s.is_empty()||s.len()>250||s.bytes().any(|b|b<32||b==127||b==b'%'||b==b'#'||b==b'?'){
  return Err("invalid Google Sheets A1 range".into());}Ok(s)
}
fn api_url(base:&str,segments:&[&str],query:&[(&str,&str)])->Result<Url,String>{
 let mut u=Url::parse(base).map_err(|e|e.to_string())?;
 {let mut p=u.path_segments_mut().map_err(|_|"invalid Workspace API host")?;
  p.clear();p.extend(segments.iter().copied());}
 u.query_pairs_mut().extend_pairs(query.iter().copied());Ok(u)
}
fn http()->Result<Client,String>{
 Client::builder().timeout(Duration::from_secs(25))
  .redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())
}
fn auth(req:RequestBuilder,token:&str)->RequestBuilder{req.bearer_auth(token)}
async fn get(client:&Client,token:&str,url:Url)->Result<Value,String>{
 let r=auth(client.get(url),token).send().await.map_err(|e|e.to_string())?;
 if !r.status().is_success(){return Err(format!("Google Workspace read HTTP {}",r.status()));}
 r.json().await.map_err(|e|format!("Google Workspace JSON: {e}"))
}
async fn text_response(client:&Client,token:&str,url:Url)->Result<String,String>{
 let r=auth(client.get(url),token).send().await.map_err(|e|e.to_string())?;
 if !r.status().is_success(){return Err(format!("Google Drive download HTTP {}",r.status()));}
 if r.content_length().is_some_and(|n|n>MAX_TEXT as u64){
  return Err("Google Drive text exceeds 128 KiB limit".into());
 }
 let bytes=r.bytes().await.map_err(|e|e.to_string())?;
 if bytes.len()>MAX_TEXT{return Err("Drive download exceeds 128 KiB".into());}
 String::from_utf8(bytes.to_vec()).map_err(|_|"Drive content is not UTF-8".into())
}
async fn write(req:RequestBuilder)->McpCallOutcome{
 let r=req.send().await;
 let resp=match r{Ok(v)=>v,Err(_)=>return McpCallOutcome::Uncertain(
  "Google Workspace write outcome unknown; inspect remote file before reconciling".into())};
 let status=resp.status();
 if !status.is_success(){
  if matches!(status.as_u16(),400|401|403|404|409|412|422){
   return McpCallOutcome::Settled(Err(format!("Google Workspace write HTTP {status}")));
  }
  return McpCallOutcome::Uncertain(format!("Google Workspace write HTTP {status}; inspect remote state"));
 }
 match resp.json::<Value>().await{
  Ok(v)=>McpCallOutcome::Settled(Ok(v.to_string())),
  Err(_)=>McpCallOutcome::Uncertain("Google Workspace write acknowledged without verifiable JSON receipt".into())
 }
}
fn accepted(value:McpCallOutcome)->Result<Value,McpCallOutcome>{
 match value{McpCallOutcome::Settled(Ok(s))=>serde_json::from_str(&s)
  .map_err(|_|McpCallOutcome::Uncertain("Workspace write receipt invalid".into())),
  other=>Err(other)}
}
fn doc_text(value:&Value)->String{
 fn collect(value:&Value,out:&mut String){
  match value{
   Value::Object(map)=>{
    if let Some(t)=map.get("textRun").and_then(|v|v.get("content")).and_then(Value::as_str){
     if out.len()<MAX_TEXT{out.push_str(&t.chars().take(MAX_TEXT-out.len()).collect::<String>());}
    }else{for v in map.values(){collect(v,out);}}
   },
   Value::Array(a)=>for v in a{collect(v,out);},
   _=>{}
  }
 }
 let mut result=String::new();
 if let Some(body)=value.get("body"){collect(body,&mut result);}
 if let Some(tabs)=value.get("tabs"){collect(tabs,&mut result);}
 result.chars().take(12_000).collect()
}
fn limit_cells(v:&Value)->Result<(),String>{
 let rows=v.as_array().ok_or("Google Sheets values must be arrays of rows")?;
 if rows.len()>MAX_ROWS||rows.iter().any(|r|r.as_array().is_none_or(|cells|
  cells.len()>50 || cells.iter().any(|v|!v.is_string()&&!v.is_number()&&!v.is_boolean()&& !v.is_null()))){
  return Err("Google Sheets limited to 250 rows, 50 scalar cells per row".into());
 }
 if v.to_string().len()>MAX_TEXT{return Err("Google Sheets write exceeds 128 KiB".into());}
 Ok(())
}
async fn sheets_read(client:&Client,token:&str,base:&str,spread:&str,range:&str)->Result<Value,String>{
 let u=api_url(base,&["v4","spreadsheets",spread,"values",range],
   &[("valueRenderOption","UNFORMATTED_VALUE")])?;
 get(client,token,u).await
}
async fn execute_with(token:&str,endpoints:&Endpoints,call:&ToolCall)->McpCallOutcome{
 let client=match http(){Ok(c)=>c,Err(e)=>return McpCallOutcome::Settled(Err(e))};
 let a=&call.arguments;
 match call.name.as_str(){
  "google_drive_search"=>{
   let q=a["query"].as_str().unwrap_or("").trim();
   if q.len()>120{return McpCallOutcome::Settled(Err("Drive query too long".into()));}
   let escaped=q.replace('\\',"\\\\").replace('\'',"\\'");
   let filter=if q.is_empty(){"trashed = false".to_owned()}
      else{format!("trashed = false and name contains '{escaped}'")};
   let u=api_url(&endpoints.drive,&["drive","v3","files"],&[
    ("pageSize","50"),("fields","nextPageToken,files(id,name,mimeType,webViewLink,modifiedTime,parents)"),
    ("q",&filter),("orderBy","modifiedTime desc")]);
   McpCallOutcome::Settled(match u{Ok(u)=>get(&client,token,u).await.map(|v|v.to_string()),Err(e)=>Err(e)})
  },
  "google_drive_read_text"=>{
   let file=match required(a,"file_id").and_then(id){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let u=match api_url(&endpoints.drive,&["drive","v3","files",file],
    &[("fields","id,name,mimeType,webViewLink")]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let metadata=match get(&client,token,u).await{Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let mime=metadata["mimeType"].as_str().unwrap_or("");
   let url=if mime=="application/vnd.google-apps.document"{
    api_url(&endpoints.drive,&["drive","v3","files",file,"export"],&[("mimeType","text/plain")])
   }else if mime.starts_with("text/")||mime=="application/json"||mime=="application/xml"{
    api_url(&endpoints.drive,&["drive","v3","files",file],&[("alt","media")])
   }else{return McpCallOutcome::Settled(Err("This Drive file is not a text file; use Sheets tools for spreadsheets".into()))};
   McpCallOutcome::Settled(match url{
    Ok(url)=>text_response(&client,token,url).await.map(|text|json!({"file":metadata,"content":text}).to_string()),
    Err(e)=>Err(e)
   })
  },
  "google_docs_get"|"google_docs_append"=>{
   let document=match required(a,"document_id").and_then(id){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let u=match api_url(&endpoints.docs,&["v1","documents",document],&[]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let current=match get(&client,token,u).await{Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   if call.name=="google_docs_get"{
    return McpCallOutcome::Settled(Ok(json!({"documentId":current["documentId"],
      "title":current["title"],"revisionId":current["revisionId"],"text":doc_text(&current)}).to_string()));
   }
   let added=match required(a,"text"){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let revision=match required(a,"expected_revision"){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   if current["revisionId"]!=revision{return McpCallOutcome::Settled(Err("Google Doc revision changed; re-read before editing".into()));}
   let u=match api_url(&endpoints.docs,&["v1","documents",&format!("{document}:batchUpdate")],&[]){
    Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let payload=json!({"requests":[{"insertText":{"endOfSegmentLocation":{},"text":added}}],
    "writeControl":{"requiredRevisionId":revision}});
   let result=write(auth(client.post(u),token).json(&payload)).await;
   if let Err(e)=accepted(result){return e;}
   let u=match api_url(&endpoints.docs,&["v1","documents",document],&[]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Uncertain(e)};
   match get(&client,token,u).await{
    Ok(v) if doc_text(&v).contains(added)=>McpCallOutcome::Settled(Ok(json!({
     "verified":true,"documentId":document,"revisionId":v["revisionId"]}).to_string())),
    _=>McpCallOutcome::Uncertain("Doc append returned success, but post-write text was not verified".into())
   }
  },
  "google_docs_create"|"google_sheets_create"=>{
   let title=match required(a,"title"){Ok(v) if v.len()<=200=>v,_=>return McpCallOutcome::Settled(Err("title required (max 200 bytes)".into()))};
   let docs=call.name=="google_docs_create";
   let (base,parts,payload,key)=if docs{(&endpoints.docs,vec!["v1","documents"],json!({"title":title}),"documentId")}
      else{(&endpoints.sheets,vec!["v4","spreadsheets"],json!({"properties":{"title":title}}),"spreadsheetId")};
   let u=match api_url(base,&parts,&[]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let created=match accepted(write(auth(client.post(u),token).json(&payload)).await){Ok(v)=>v,Err(e)=>return e};
   let doc=match created[key].as_str().and_then(|s|id(s).ok()){Some(v)=>v, None=>
    return McpCallOutcome::Uncertain("Workspace object created without file ID".into())};
   let mut target=parts.clone();target.push(doc);
   let u=match api_url(base,&target,&[]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Uncertain(e)};
   match get(&client,token,u).await{
    Ok(v) if v[key]==doc && (docs&&v["title"]==title||!docs&&v["properties"]["title"]==title)=>
      McpCallOutcome::Settled(Ok(json!({"verified":true,"id":doc,"title":title}).to_string())),
    _=>McpCallOutcome::Uncertain("Workspace file created but read-back failed".into())
   }
  },
  "google_sheets_get"|"google_sheets_update"|"google_sheets_append"=>{
   let spread=match required(a,"spreadsheet_id").and_then(id){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let range=match required(a,"range").and_then(a1){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let prior=match sheets_read(&client,token,&endpoints.sheets,spread,range).await{
    Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   if call.name=="google_sheets_get"{
    return McpCallOutcome::Settled(Ok(json!({"spreadsheetId":spread,"range":range,
      "values":prior.get("values").cloned().unwrap_or(json!([]))}).to_string()));
   }
   let rows=a.get("values").unwrap_or(&Value::Null);
   if let Err(e)=limit_cells(rows){return McpCallOutcome::Settled(Err(e));}
   let old=prior.get("values").cloned().unwrap_or(json!([]));
   if call.name=="google_sheets_update" && a.get("expected_values")!=Some(&old){
    return McpCallOutcome::Settled(Err("Sheets cells changed; re-read and supply exact expected_values".into()));
   }
   let append=call.name=="google_sheets_append";
   let mut parts=vec!["v4","spreadsheets",spread,"values",range];
   if append{parts.push(":append");}
   let endpoint=if append{
     // Sheets append uses :append on the range itself (not a separate segment).
     parts.pop();parts.pop();
     let ranged=format!("{range}:append");parts.push(&ranged);
     match api_url(&endpoints.sheets,&parts,&[("valueInputOption","RAW")]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))}
   }else{
     match api_url(&endpoints.sheets,&parts,&[("valueInputOption","RAW")]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))}
   };
   let payload=json!({"range":range,"majorDimension":"ROWS","values":rows});
   let request=if append{client.post(endpoint)}else{client.put(endpoint)};
   let done=match accepted(write(auth(request,token).json(&payload)).await){Ok(v)=>v,Err(e)=>return e};
   let actual_range=if append{
     match done.pointer("/updates/updatedRange").and_then(Value::as_str){
      Some(v)=>v.to_owned(),None=>return McpCallOutcome::Uncertain("Sheets append missing updated range".into())
     }
   }else{range.to_owned()};
   match sheets_read(&client,token,&endpoints.sheets,spread,&actual_range).await{
    Ok(v) if v.get("values")==Some(rows)=>McpCallOutcome::Settled(Ok(json!({
      "verified":true,"spreadsheetId":spread,"updatedRange":actual_range,
      "updatedCells":done["updatedCells"]}).to_string())),
    _=>McpCallOutcome::Uncertain("Sheets write accepted but re-read did not match exact cells".into())
   }
  },
  "google_drive_create_folder"|"google_drive_upload_text"=>{
   let name=match required(a,"name"){Ok(v) if v.len()<=200=>v,_=>return McpCallOutcome::Settled(Err("file name required (max 200 bytes)".into()))};
   let folder=call.name=="google_drive_create_folder";
   let mut metadata=json!({"name":name,"mimeType":if folder{"application/vnd.google-apps.folder"}else{"text/plain"}});
   let parent=a["parent_id"].as_str().or_else(||a["folder_id"].as_str());
   if let Some(parent)=parent{
    let parent=match id(parent){Ok(v)=>v,Err(e)=>return McpCallOutcome::Settled(Err(e))};
    metadata["parents"]=json!([parent]);
   }
   let raw=if folder{None}else{
    match required(a,"text"){Ok(v)=>Some(v),Err(e)=>return McpCallOutcome::Settled(Err(e))}
   };
   let u=if folder{
     api_url(&endpoints.drive,&["drive","v3","files"],&[("fields","id,name,mimeType,webViewLink")])
   }else{
     api_url(&endpoints.drive,&["upload","drive","v3","files"],&[
       ("uploadType","multipart"),("fields","id,name,mimeType,webViewLink")])
   };
   let u=match u{Ok(u)=>u,Err(e)=>return McpCallOutcome::Settled(Err(e))};
   let req=if let Some(text)=raw{
    let boundary=format!("dominion-{}",uuid::Uuid::new_v4());
    let multipart=format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{}\r\n--{boundary}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{text}\r\n--{boundary}--\r\n",metadata);
    auth(client.post(u),token).header("Content-Type",format!("multipart/related; boundary={boundary}")).body(multipart)
   }else{auth(client.post(u),token).json(&metadata)};
   let created=match accepted(write(req).await){Ok(v)=>v,Err(e)=>return e};
   let file=match created["id"].as_str().and_then(|s|id(s).ok()){
    Some(v)=>v,None=>return McpCallOutcome::Uncertain("Drive file created without ID".into())
   };
   let u=match api_url(&endpoints.drive,&["drive","v3","files",file],&[
    ("fields","id,name,mimeType,webViewLink")]){Ok(u)=>u,Err(e)=>return McpCallOutcome::Uncertain(e)};
   let metadata=match get(&client,token,u).await{Ok(v)=>v,Err(_)=>
    return McpCallOutcome::Uncertain("Drive metadata could not be read after create".into())};
   if metadata["name"]!=name{return McpCallOutcome::Uncertain("Drive file name changed during creation".into());}
   if let Some(text)=raw{
    let u=match api_url(&endpoints.drive,&["drive","v3","files",file],&[("alt","media")]){
     Ok(u)=>u,Err(e)=>return McpCallOutcome::Uncertain(e)};
    match text_response(&client,token,u).await{
     Ok(actual) if actual==text=>{},
     _=>return McpCallOutcome::Uncertain("Drive text upload succeeded but bytes did not verify".into())
    }
   }
   McpCallOutcome::Settled(Ok(json!({"verified":true,"id":file,"name":name,
     "webViewLink":metadata["webViewLink"]}).to_string()))
  },
  _=>McpCallOutcome::Settled(Err("unknown Google Workspace tool".into()))
 }
}
pub async fn execute(settings:&Settings,call:&ToolCall)->McpCallOutcome{
 let token=match crate::frontend_oauth::google_workspace_access_token(settings).await{
  Ok(t)=>t,Err(e)=>return McpCallOutcome::Settled(Err(e))
 };
 execute_with(&token,&Endpoints::default(),call).await
}
#[cfg(test)]
mod tests{
 use super::*;
 use axum::{Router,Json,routing::{get,post}};
 #[test]fn capability_flags_and_limits(){
  assert!(supports("google_sheets_update")&&mutates("google_sheets_update"));
  assert!(supports("google_docs_get")&&!mutates("google_docs_get"));
  assert!(!supports("google_drive_delete"));
  assert!(id("../evil").is_err());
  assert!(a1("Sheet1!A1:B2").is_ok());
  assert!(limit_cells(&json!([["one",2,true]])).is_ok());
  assert!(limit_cells(&json!({"danger":true})).is_err());
 }
 async fn create_doc(Json(v):Json<Value>)->Json<Value>{
  assert_eq!(v["title"],"Dominion note");Json(json!({"documentId":"doc123"}))
 }
 async fn read_doc()->Json<Value>{
  Json(json!({"documentId":"doc123","title":"Dominion note","revisionId":"R1",
    "body":{"content":[{"paragraph":{"elements":[{"textRun":{"content":"Hello"}}]}}]}}))
 }
 #[tokio::test]async fn create_doc_checks_remote_document(){
  let router=Router::new().route("/v1/documents",post(create_doc))
    .route("/v1/documents/doc123",get(read_doc));
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr=listener.local_addr().unwrap();
  tokio::spawn(async move{axum::serve(listener,router).await.unwrap();});
  let base=format!("http://{addr}");
  let endpoints=Endpoints{drive:base.clone(),docs:base.clone(),sheets:base};
  let call=ToolCall{id:"doc-create".into(),name:"google_docs_create".into(),
    arguments:json!({"title":"Dominion note"})};
  match execute_with("fixture-token",&endpoints,&call).await{
   McpCallOutcome::Settled(Ok(body))=>assert!(body.contains("\"verified\":true")),
   _=>panic!("Google document create verification failed"),
  }
 }
}
