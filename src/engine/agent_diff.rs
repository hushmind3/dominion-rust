//! Bounded read-only Git inspection for coding goals and tool execution.
//! This observes the working tree. It never runs shell commands or edits files.
use serde_json::{json,Value};
use std::{path::{Component,Path},time::Duration};
use tokio::process::Command;

fn checked_path(input:&str)->Result<String,String>{
    if input.is_empty()||input.len()>4096||input.chars().any(char::is_control){
        return Err("diff path is empty or invalid".into());
    }
    let p=Path::new(input);
    if p.is_absolute()||p.components().any(|c|matches!(c,Component::ParentDir|Component::Prefix(_))){
        return Err("diff path must be inside workspace".into());
    }
    Ok(input.to_string())
}
async fn call(root:&Path,action:&str,paths:&[String])->Result<Value,String>{
    let mut cmd=Command::new("git");
    cmd.arg("-C").arg(root);
    match action {
        "unstaged"=>{cmd.args(["diff","--no-ext-diff","--no-textconv"]);}
        "staged"=>{cmd.args(["diff","--cached","--no-ext-diff","--no-textconv"]);}
        _=>{cmd.args(["status","--short","--untracked-files=normal"]);}
    }
    cmd.arg("--");
    for path in paths{cmd.arg(path);}
    let output=tokio::time::timeout(Duration::from_secs(12),cmd.output())
        .await.map_err(|_|"git inspection timed out".to_string())?
        .map_err(|error|format!("git inspection: {error}"))?;
    let cut=|bytes:&[u8]|->String{
        let all=String::from_utf8_lossy(bytes);
        let mut out=all.chars().take(12_000).collect::<String>();
        if all.chars().count()>12_000{out.push_str("\n[diff clipped]");}
        out
    };
    Ok(json!({"exitCode":output.status.code(),"text":cut(&output.stdout),
        "stderr":cut(&output.stderr)}))
}
pub async fn inspect(root:&Path,paths:&[String])->Result<Value,String>{
    if paths.len()>20{return Err("Up to 20 file paths per diff request".into());}
    let selected=paths.iter().map(|p|checked_path(p))
        .collect::<Result<Vec<_>,_>>()?;
    if !root.is_dir(){return Err("Workspace does not exist".into());}
    let status=call(root,"status",&selected).await?;
    if status["exitCode"]!=0{return Err(status["stderr"].as_str().unwrap_or("git unavailable").into());}
    let unstaged=call(root,"unstaged",&selected).await?;
    let staged=call(root,"staged",&selected).await?;
    if unstaged["exitCode"]!=0||staged["exitCode"]!=0{
        return Err("git diff failed; inspect the workspace repository status".into());
    }
    Ok(json!({"paths":selected,"status":status["text"],"unstaged":unstaged["text"],
        "staged":staged["text"],"observed":true,"readOnly":true}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_paths_outside_checkout(){
        assert!(checked_path("../secret").is_err());
        assert!(checked_path("/etc/hosts").is_err());
        assert!(checked_path("src/main.rs").is_ok());
    }
}
