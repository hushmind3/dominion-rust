//! Capability broker: expose only connected OAuth-backed tools to the model.
use crate::{config::Settings,domain::ToolCall,mcp::McpCallOutcome};
use serde_json::Value;

pub fn definitions(settings:&Settings)->Vec<Value>{
 let mut out=Vec::new();
 if crate::frontend_oauth::github_repository_connected(settings){
  out.extend(crate::github_work::definitions());
  out.extend(crate::github_collab::definitions());
 }
 if crate::frontend_oauth::google_workspace_connected(settings){
  out.extend(crate::google_workspace::definitions());
 }
 out
}
pub fn mutates(name:&str)->bool{
 name=="github_commit_file"||crate::github_collab::mutates(name)||
 crate::google_workspace::mutates(name)
}
pub fn supports(name:&str)->bool{
 matches!(name,"github_list_repositories"|"github_read_file"|"github_commit_file")
 ||crate::github_collab::supports(name)||crate::google_workspace::supports(name)
}
pub fn replay_safe(name:&str)->bool{
 matches!(name,
  "github_list_repositories"|"github_read_file"|"github_list_branches"|
  "github_list_issues"|"github_get_issue"|"github_list_pull_requests"|"github_get_pull_request"|
  "google_drive_search"|"google_drive_read_text"|"google_docs_get"|"google_sheets_get")
}
pub async fn execute(settings:&Settings,call:&ToolCall)->McpCallOutcome{
 if crate::google_workspace::supports(&call.name){
  return crate::google_workspace::execute(settings,call).await;
 }
 if crate::github_collab::supports(&call.name){
  return crate::github_collab::execute(settings,call).await;
 }
 match call.name.as_str(){
  "github_list_repositories"|"github_read_file"|"github_commit_file"=>
    crate::github_work::execute(settings,call).await,
  _=>McpCallOutcome::Settled(Err("connected tool is not registered".into()))
 }
}
pub fn names_for(settings:&Settings,provider:&str)->Vec<String>{
 definitions(settings).into_iter().filter_map(|tool|{
  let name=tool.pointer("/function/name").and_then(Value::as_str)?;
  if provider=="github"&&name.starts_with("github_")||
    provider=="google"&&name.starts_with("google_"){Some(name.to_owned())}
  else{None}
 }).collect()
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn only_expected_remote_mutations_need_human_approval(){
  assert!(mutates("github_create_pull_request"));
  assert!(mutates("google_sheets_update"));
  assert!(mutates("google_docs_append"));
  assert!(!mutates("google_docs_get"));
  assert!(!mutates("github_list_issues"));
  assert!(!supports("google_drive_delete"));
  assert!(replay_safe("google_sheets_get"));
  assert!(!replay_safe("google_docs_append"));
 }
}
