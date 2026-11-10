//! Optional official CLI-backed inference adapter.
//! DOMINION owns sessions, memory, tools and approvals; a logged-in CLI is
//! only a text generation transport, never the executor of DOMINION actions.
use serde_json::{json,Value};
use std::{path::PathBuf,process::Stdio,time::Duration};
use tokio::process::Command;

fn executable(name:&str)->Option<PathBuf>{
 let mut dirs=Vec::new();
 if let Some(h)=std::env::var_os("HOME").map(PathBuf::from){
  dirs.extend([h.join(".local/bin"),h.join(".bun/bin"),h.join(".npm-global/bin")]);
 }
 dirs.extend(["/opt/homebrew/bin","/usr/local/bin","/usr/bin"].into_iter().map(PathBuf::from));
 if let Some(path)=std::env::var_os("PATH"){dirs.extend(std::env::split_paths(&path));}
 dirs.into_iter().flat_map(|dir|["",".exe",".cmd"].into_iter().map(move |ext|dir.join(format!("{name}{ext}"))))
  .find(|path|path.is_file())
}

fn prep(command:&mut Command) {
 let mut paths=vec![PathBuf::from("/opt/homebrew/bin"),PathBuf::from("/usr/local/bin")];
 if let Some(h)=std::env::var_os("HOME").map(PathBuf::from){
  paths.insert(0,h.join(".local/bin"));paths.insert(0,h.join(".bun/bin"));
 }
 if let Some(path)=std::env::var_os("PATH"){paths.extend(std::env::split_paths(&path));}
 if let Ok(path)=std::env::join_paths(paths){command.env("PATH",path);}
 // No project files are exposed to a third-party CLI. The DOMINION harness
 // remains the only authority for file/process/tool mutations.
 let work=std::env::temp_dir().join("dominion-inference");
 let _=std::fs::create_dir_all(&work);
 command.current_dir(work).stdin(Stdio::null()).kill_on_drop(true);
}

fn binary(source:&str)->Result<PathBuf,String>{
 if !matches!(source,"claude"|"gemini"|"opencode"){return Err("지원하지 않는 CLI 모델입니다.".into());}
 executable(source).ok_or_else(||format!("{source} CLI를 찾지 못했습니다. 계정 연결에서 설치 상태를 확인하세요."))
}

fn read_text(message:&Value)->String{
 if let Some(text)=message["content"].as_str(){return text.to_string();}
 if let Some(blocks)=message["content"].as_array(){
  return blocks.iter().filter_map(|block|block["text"].as_str()).collect::<Vec<_>>().join("\n");
 }
 String::new()
}

fn prompt(messages:&[Value])->Result<String,String>{
 let mut prompt=String::new();
 for m in messages {
  let text=read_text(m);
  if text.trim().is_empty(){continue;}
  let role=m["role"].as_str().unwrap_or("user");
  prompt.push_str(&format!("[{role}]\n{text}\n\n"));
 }
 if prompt.len()>60000{return Err("CLI 모델의 문맥 입력 제한을 초과했습니다. 대화를 압축하세요.".into());}
 if prompt.trim().is_empty(){return Err("입력 내용이 없습니다.".into());}
 Ok(prompt)
}

fn output_text(source:&str,output:&str)->Result<String,String>{
 if source=="claude"{
  let value:Value=serde_json::from_str(output.trim()).map_err(|_|"Claude CLI 응답 JSON을 해석하지 못했습니다.".to_string())?;
  if value["is_error"]==true{return Err(value["result"].as_str().unwrap_or("Claude CLI 모델 오류").into());}
  return value["result"].as_str().filter(|s|!s.trim().is_empty()).map(str::to_string)
   .ok_or_else(||"Claude CLI가 빈 응답을 반환했습니다.".into());
 }
 if source=="gemini"{
  let value:Value=serde_json::from_str(output.trim()).map_err(|_|"Gemini CLI 응답 JSON을 해석하지 못했습니다.".to_string())?;
  if !value["error"].is_null(){return Err(format!("Gemini CLI 오류: {}",value["error"]));}
  return value["response"].as_str().filter(|s|!s.trim().is_empty()).map(str::to_string)
   .ok_or_else(||"Gemini CLI가 빈 응답을 반환했습니다.".into());
 }
 let mut text=String::new();
 for line in output.lines(){
  let Ok(event)=serde_json::from_str::<Value>(line) else {continue};
  if event["type"]=="error"{return Err(format!("OpenCode CLI 오류: {}",event["error"]));}
  if event["type"]=="text" {
   if let Some(part)=event["part"]["text"].as_str().or_else(||event["text"].as_str()){text.push_str(part);}
  }
 }
 if text.trim().is_empty(){return Err("OpenCode CLI가 텍스트 응답을 반환하지 않았습니다.".into());}
 Ok(text)
}

pub async fn generate(source:&str,model:&str,messages:&[Value])->Result<String,String>{
 let exe=binary(source)?;
 let prompt=prompt(messages)?;
 let mut command=Command::new(exe);
 match source {
  "claude"=>{
   command.args(["-p","--output-format","json","--tools",""]);
   if !model.is_empty()&&model!="default"{command.args(["--model",model]);}
   command.arg(&prompt);
  },
  "gemini"=>{
   command.args(["--output-format","json","--approval-mode","plan"]);
   if !model.is_empty()&&model!="auto"{command.args(["--model",model]);}
   command.args(["--prompt",&prompt]);
  },
  "opencode"=>{
   command.args(["run","--format","json","--agent","plan","--archived"]);
   if !model.is_empty()&&model!="default"{command.args(["--model",model]);}
   command.arg(&prompt);
  },
  _=>return Err("지원하지 않는 CLI 모델입니다.".into())
 }
 prep(&mut command);
 let output=tokio::time::timeout(Duration::from_secs(240),command.output()).await
  .map_err(|_|format!("{source} CLI 모델 호출 시간이 초과됐습니다."))?
  .map_err(|error|format!("{source} CLI 실행 실패: {error}"))?;
 if !output.status.success(){
  let err=String::from_utf8_lossy(&output.stderr);
  let detail=err.chars().take(360).collect::<String>();
  return Err(format!("{source} 모델 호출 실패 (코드 {:?}): {detail}",output.status.code()));
 }
 let stdout=String::from_utf8_lossy(&output.stdout);
 output_text(source,&stdout)
}

pub async fn models(source:&str)->Result<Vec<Value>,String>{
 if source!="opencode" {
  let name=if source=="claude"{"CLI 기본 모델"}else if source=="gemini"{"Gemini 자동 모델"}else {return Err("지원하지 않는 CLI입니다.".into())};
  let id=if source=="gemini"{"auto"}else{"default"};
  return Ok(vec![json!({"id":id,"name":name,"freeStatus":"unknown","freeSource":"cli-session"})]);
 }
 let exe=binary("opencode")?;
 let mut command=Command::new(exe);
 command.arg("models");prep(&mut command);
 let output=tokio::time::timeout(Duration::from_secs(20),command.output()).await
  .map_err(|_|"OpenCode 모델 조회 시간 초과".to_string())?.map_err(|e|e.to_string())?;
 if !output.status.success(){return Err(String::from_utf8_lossy(&output.stderr).chars().take(220).collect());}
 let stdout=String::from_utf8_lossy(&output.stdout);
 let rows=stdout.lines().map(str::trim)
  .filter(|v|v.contains('/')&&!v.contains(' ')&&v.len()<160)
  .take(180).map(|id|json!({"id":id,"name":id,"freeStatus":"unknown","freeSource":"cli-catalog"})).collect();
 Ok(rows)
}

#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn cli_json_response_parsers(){
  assert_eq!(output_text("claude",r#"{"result":"hello","is_error":false}"#).unwrap(),"hello");
  assert_eq!(output_text("gemini",r#"{"response":"world","stats":{}}"#).unwrap(),"world");
  assert_eq!(output_text("opencode",r#"{"type":"text","part":{"text":"A"}}"#).unwrap(),"A");
 }
 #[test] fn reject_unknown_models_and_empty_prompts(){
  assert!(binary("sh").is_err());
  assert!(prompt(&[]).is_err());
 }
}
