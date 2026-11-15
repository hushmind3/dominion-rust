//! Isolated repository workspaces: original Git checkout remains untouched.
//! The native SDK can run coding agents on existing multi-file repositories.
use crate::config::Settings;
use std::path::{Component,Path};
use tokio::{fs,io::AsyncWriteExt,process::Command,time::{timeout,Duration}};
use std::process::Stdio;
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

/// Integrate a completed child's tracked and newly-created files into its parent
/// worktree, never the user's source repository. Git checks the entire patch
/// against current parent files before it may change them. The caller must
/// require explicit approval and persist tool intent before invoking this.
pub async fn merge_child(settings:&Settings,parent:Uuid,child:Uuid,expected_sha:&str)->Result<String,String>{
    if expected_sha.len()!=40 || !expected_sha.bytes().all(|c|c.is_ascii_hexdigit()){
        return Err("missing or invalid immutable source revision".into());
    }
    let source=settings.workspace_dir.join(parent.to_string());
    let from=settings.workspace_dir.join(child.to_string());
    let root=settings.workspace_dir.canonicalize().map_err(|e|e.to_string())?;
    for path in [&source,&from]{
        let checked=path.canonicalize().map_err(|e|e.to_string())?;
        if !checked.starts_with(&root)||!checked.is_dir(){return Err("worktree escaped configured workspace".into());}
        let head=git(&["rev-parse","HEAD"],&checked).await?;
        if head!=expected_sha{return Err("worktree base revision no longer matches delegation".into());}
    }
    // Intent-to-add exposes new untracked files to git diff without staging
    // their contents or ever committing them. The child is already completed.
    git(&["add","-N","--","."],&from).await?;
    let patch=git_patch(&from,&["diff","--binary","--no-ext-diff","HEAD","--"]).await?;
    if patch.trim().is_empty(){return Ok("No child edits to integrate".into());}
    if patch.len()>524_288{return Err("child changes exceed 512 KB integration limit; inspect manually".into());}
    // Stop on conflicts. Never run git merge on the user's source checkout.
    apply_patch_checked(&source,&patch,true).await?;
    apply_patch_checked(&source,&patch,false).await?;
    let summary=git(&["status","--short"],&source).await?;
    Ok(format!("Child {child} changes integrated into parent {parent} isolated worktree.\n{summary}"))
}
async fn git_patch(dir:&Path,args:&[&str])->Result<String,String>{
    let result=timeout(Duration::from_secs(30),Command::new("git")
        .args(args).current_dir(dir).kill_on_drop(true).output())
        .await.map_err(|_|"git patch timed out".to_string())?
        .map_err(|e|e.to_string())?;
    if !result.status.success(){return Err(format!("git diff failed: {}",String::from_utf8_lossy(&result.stderr)));}
    if result.stdout.len()>524_288 {return Err("child changes exceed 512 KB integration limit".into());}
    String::from_utf8(result.stdout).map_err(|_|"patch path is not valid UTF-8".into())
}
async fn apply_patch_checked(dir:&Path,patch:&str,check_only:bool)->Result<(),String>{
    let mut command=Command::new("git");
    command.arg("apply").arg("--binary");
    if check_only{command.arg("--check");}
    let mut child=command.arg("--").current_dir(dir)
        .env_clear().env("PATH","/usr/local/bin:/usr/bin:/bin").env("HOME",dir)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e|format!("git apply launch: {e}"))?;
    let result=timeout(Duration::from_secs(30),async{
        let mut stdin=child.stdin.take().ok_or("git patch stdin unavailable")?;
        stdin.write_all(patch.as_bytes()).await.map_err(|e|e.to_string())?;
        drop(stdin);
        child.wait_with_output().await.map_err(|e|e.to_string())
    }).await.map_err(|_|"git patch timed out".to_string())??;
    if !result.status.success(){
        return Err(format!("{}: {}",if check_only{"child patch conflict"}else{"child patch apply failed"},
            String::from_utf8_lossy(&result.stderr).chars().take(1500).collect::<String>()));
    }
    Ok(())
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
    async fn child_patch_integration_checks_conflicts_and_preserves_original(){
        let root=std::env::temp_dir().join(format!("dominion-merge-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.workspace_dir=root.clone();
        let source=root.join("projects").join("demo");
        fs::create_dir_all(&source).await.unwrap();
        git(&["init","-q"],&source).await.unwrap();
        fs::write(source.join("tracked.txt"),"base\n").await.unwrap();
        git(&["add","tracked.txt"],&source).await.unwrap();
        git(&["-c","user.name=Dominion","-c","user.email=local@example.com","commit","-qm","seed"],&source).await.unwrap();
        let parent=Uuid::new_v4();let child=Uuid::new_v4();
        let sha=attach(&settings,parent,"projects/demo").await.unwrap();
        attach_at(&settings,child,"projects/demo",Some(&sha)).await.unwrap();
        let parent_dir=root.join(parent.to_string());
        let child_dir=root.join(child.to_string());
        fs::write(child_dir.join("tracked.txt"),"child edit\n").await.unwrap();
        fs::write(child_dir.join("new.txt"),"new file\n").await.unwrap();
        merge_child(&settings,parent,child,&sha).await.unwrap();
        assert_eq!(fs::read_to_string(parent_dir.join("tracked.txt")).await.unwrap(),"child edit\n");
        assert_eq!(fs::read_to_string(parent_dir.join("new.txt")).await.unwrap(),"new file\n");
        assert_eq!(fs::read_to_string(source.join("tracked.txt")).await.unwrap(),"base\n");

        let competing=Uuid::new_v4();
        attach_at(&settings,competing,"projects/demo",Some(&sha)).await.unwrap();
        fs::write(root.join(competing.to_string()).join("tracked.txt"),"conflicting change\n").await.unwrap();
        assert!(merge_child(&settings,parent,competing,&sha).await.is_err());
        assert_eq!(fs::read_to_string(parent_dir.join("tracked.txt")).await.unwrap(),"child edit\n");
        for id in [parent,child,competing]{
            let path=root.join(id.to_string()).to_string_lossy().to_string();
            git(&["worktree","remove","--force",&path],&source).await.unwrap();
        }
        fs::remove_dir_all(root).await.unwrap();
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
