//! Isolated repository workspaces: original Git checkout remains untouched.
//! The native SDK can run coding agents on existing multi-file repositories.
use crate::config::Settings;
use std::path::{Component,Path};
use tokio::{fs,process::Command,time::{timeout,Duration}};
use uuid::Uuid;

fn validate_relative(path:&str)->Result<(),String>{
    if path.is_empty()||path.len()>512||Path::new(path).components().any(|c|!matches!(c,Component::Normal(_))){
        return Err("repository must be a relative path inside Dominion's workspace".into());
    }
    Ok(())
}
async fn git(args:&[&str],dir:&Path)->Result<String,String>{
    let result=timeout(Duration::from_secs(30),Command::new("git")
        .args(args).current_dir(dir).kill_on_drop(true).output())
        .await.map_err(|_|"git operation timed out".to_string())?
        .map_err(|e|format!("git unavailable: {e}"))?;
    if !result.status.success(){
        return Err(format!("git failed: {}",String::from_utf8_lossy(&result.stderr).chars().take(1200).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
}
/// Attach a repository by creating an independent Git worktree for this run.
/// No command, test, or source edit is executed during attachment.
pub async fn attach(settings:&Settings,id:Uuid,relative:&str)->Result<String,String>{
    attach_at(settings,id,relative,None).await
}
/// Attach an exact recorded revision for reproducible delegated checkouts.
pub async fn attach_at(settings:&Settings,id:Uuid,relative:&str,revision:Option<&str>)->Result<String,String>{
    validate_relative(relative)?;
    if revision.is_some_and(|r|r.len()!=40||!r.bytes().all(|b|b.is_ascii_hexdigit())){
        return Err("invalid source commit; expected a 40-character Git SHA".into());
    }
    fs::create_dir_all(&settings.workspace_dir).await.map_err(|e|e.to_string())?;
    let root=settings.workspace_dir.canonicalize().map_err(|e|e.to_string())?;
    let source=root.join(relative).canonicalize().map_err(|e|format!("repository unavailable: {e}"))?;
    if !source.starts_with(&root)||!source.is_dir(){
        return Err("repository must be inside the configured workspace".into());
    }
    let toplevel=git(&["rev-parse","--show-toplevel"],&source).await?;
    let origin=Path::new(&toplevel).canonicalize().map_err(|e|e.to_string())?;
    if origin!=source{
        return Err("repository must refer to a Git checkout root".into());
    }
    let baseline=git(&["rev-parse",revision.unwrap_or("HEAD")],&source).await?;
    let target=root.join(id.to_string());
    if fs::symlink_metadata(&target).await.is_ok(){
        return Err("run workspace already exists; refusing to overwrite".into());
    }
    let target_string=target.to_str().ok_or("workspace path is not UTF-8")?;
    git(&["worktree","add","--detach",target_string,&baseline],&source).await?;
    let restored=git(&["rev-parse","HEAD"],&target).await?;
    if restored!=baseline {
        return Err("worktree commit differs from selected source".into());
    }
    Ok(baseline)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn forbids_traversal_and_absolute_repositories(){
        assert!(validate_relative("../other").is_err());
        assert!(validate_relative("/root/project").is_err());
        assert!(validate_relative("repo/project").is_ok());
    }
    #[tokio::test]
    async fn isolated_checkout_does_not_mutate_parent(){
        let root=std::env::temp_dir().join(format!("dominion-worktree-{}",Uuid::new_v4()));
        let mut config=Settings::from_env().unwrap();
        config.workspace_dir=root.clone();
        let source=root.join("repos").join("demo");
        fs::create_dir_all(&source).await.unwrap();
        git(&["init","-q"],&source).await.unwrap();
        fs::write(source.join("a.txt"),"original").await.unwrap();
        git(&["add","a.txt"],&source).await.unwrap();
        git(&["-c","user.name=Dominion","-c","user.email=dominion@localhost","commit","-qm","seed"],&source).await.unwrap();
        let id=Uuid::new_v4();
        let baseline=attach(&config,id,"repos/demo").await.unwrap();
        assert_eq!(baseline,git(&["rev-parse","HEAD"],&source).await.unwrap());
        fs::write(root.join(id.to_string()).join("a.txt"),"changed").await.unwrap();
        assert_eq!(fs::read_to_string(source.join("a.txt")).await.unwrap(),"original");
        let target=root.join(id.to_string()).to_string_lossy().to_string();
        git(&["worktree","remove","--force",&target],&source).await.unwrap();
        fs::remove_dir_all(root).await.unwrap();
    }
}
