//! Isolated repository workspaces: original Git checkout remains untouched.
//! The native SDK can run coding agents on existing multi-file repositories.
use crate::config::Settings;
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
use std::path::{Component,Path};
use tokio::{fs,io::AsyncWriteExt,process::Command,time::{timeout,Duration}};
use std::process::Stdio;
use uuid::Uuid;

/// Git for Windows does not accept Rust's canonical verbatim path prefix
/// (\\\\?\\C:\\...) as a worktree argument. Validate with canonical paths,
/// then convert only the process argument to the regular drive/UNC spelling.
fn git_path_argument(path:&Path)->Result<String,String>{
    let raw=path.to_str().ok_or("workspace path is not UTF-8")?;
    #[cfg(windows)]
    {
        if let Some(rest)=raw.strip_prefix(r"\\?\"){
            if let Some(unc)=rest.strip_prefix(r"UNC\"){
                return Ok(format!(r"\\{}",unc));
            }
            return Ok(rest.to_owned());
        }
    }
    Ok(raw.to_owned())
}
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
    let target_string=git_path_argument(&target)?;
    git(&["worktree","add","--detach",&target_string,&baseline],&source).await?;
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

/// An exact, bounded review of a completed Team worker's Git delta.
/// The caller owns authorization. This preview does not write the parent.
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct TeamPatchPreview{
    pub source_run_id:Uuid,
    pub sha256:String,
    pub changed_paths:Vec<String>,
    pub patch_bytes:usize,
    pub diff:String,
    pub can_apply:bool,
    pub conflict:Option<String>,
}
/// Verify the owner/Writer detached worktrees and immutable Git revisions
/// before allowing an external coding CLI to enter the child's directory.
pub async fn verified_external_writer_worktree(settings:&Settings,parent:Uuid,
    child:Uuid,parent_base:&str,child_base:&str)->Result<std::path::PathBuf,String>{
    let (_,writer)=verified_team_worktrees(settings,parent,child,
        parent_base,child_base).await?;
    Ok(writer)
}
async fn verified_team_worktrees(settings:&Settings,parent:Uuid,child:Uuid,
    parent_base:&str,child_base:&str)->Result<(std::path::PathBuf,std::path::PathBuf),String>{
    let root=settings.workspace_dir.canonicalize().map_err(|e|e.to_string())?;
    let parent_dir=root.join(parent.to_string()).canonicalize().map_err(|e|e.to_string())?;
    let child_dir=root.join(child.to_string()).canonicalize().map_err(|e|e.to_string())?;
    if parent==child || !parent_dir.starts_with(&root)||!child_dir.starts_with(&root)||
        !parent_dir.is_dir()||!child_dir.is_dir(){
        return Err("invalid isolated Team worktree lineage".into());
    }
    for (dir,revision) in [(&parent_dir,parent_base),(&child_dir,child_base)]{
        if revision.len()!=40||!revision.bytes().all(|b|b.is_ascii_hexdigit()){
            return Err("invalid Team worktree revision".into());
        }
        if git(&["rev-parse","HEAD"],dir).await?!=revision{
            return Err("Team worktree revision changed since the owner-scoped snapshot".into());
        }
    }
    Ok((parent_dir,child_dir))
}
async fn changes_with_intent(dir:&Path)->Result<(String,Vec<String>),String>{
    // Intent-to-add exposes untracked files in the patch without creating a
    // commit or staging their content. The source checkout is never used.
    git(&["add","-N","--","."],dir).await?;
    let patch=git_patch(dir,&["diff","--binary","--no-ext-diff","HEAD","--"]).await?;
    let names=timeout(Duration::from_secs(30),Command::new("git")
        .args(["diff","--name-only","-z","HEAD","--"])
        .current_dir(dir).kill_on_drop(true).output()).await
        .map_err(|_|"Team changed path enumeration timed out".to_string())?
        .map_err(|e|e.to_string())?;
    if !names.status.success(){return Err("cannot enumerate Team changed paths".into());}
    let mut paths=Vec::new();
    for raw in names.stdout.split(|b|*b==0).filter(|p|!p.is_empty()){
        let path=String::from_utf8(raw.to_vec())
            .map_err(|_|"non-UTF8 changed path in Team patch".to_string())?;
        if path.len()>512||path.contains('\\')||path==".git"||path.starts_with(".git/")||
            !Path::new(&path).components().all(|c|matches!(c,Component::Normal(_))){
            return Err("invalid or unsafe changed path in Team patch".into());
        }
        paths.push(path);
        if paths.len()>200{return Err("Team patch exceeds 200 changed files".into());}
    }
    if patch.lines().any(|line|line.trim().ends_with("mode 120000")||
        line.trim().ends_with("mode 160000")){
        return Err("symlink and submodule changes require separate review".into());
    }
    Ok((patch,paths))
}
/// Seed a fresh child worktree with the team's already-approved changes,
/// then freeze that snapshot in the CHILD'S DETACHED HEAD. Later child edits
/// are diffed against this seed, not replayed as stale changes from the
/// original source commit. No source branch or checkout is committed to.
pub async fn seed_team_worker(settings:&Settings,parent:Uuid,child:Uuid,
    original_sha:&str)->Result<String,String>{
    let (parent_dir,child_dir)=verified_team_worktrees(settings,parent,child,
        original_sha,original_sha).await?;
    let (patch,_)=changes_with_intent(&parent_dir).await?;
    if patch.trim().is_empty(){return Ok(original_sha.to_string());}
    apply_patch_checked(&child_dir,&patch,true).await?;
    apply_patch_checked(&child_dir,&patch,false).await?;
    git(&["add","-A","--","."],&child_dir).await?;
    // The new commit exists only on the detached child worktree. No hooks,
    // signing or model-authored commit message is run.
    git(&["-c","core.hooksPath=/dev/null",
        "-c","commit.gpgsign=false",
        "-c","user.name=Dominion Team",
        "-c","user.email=team@dominion.invalid",
        "commit","--no-verify","-qm","Team approved workspace snapshot"],&child_dir).await?;
    let seeded=git(&["rev-parse","HEAD"],&child_dir).await?;
    if seeded==original_sha{return Err("Team snapshot was not committed".into());}
    Ok(seeded)
}
/// Read-only conflict detection, stable patch digest and exact diff.
pub async fn preview_team_patch(settings:&Settings,parent:Uuid,child:Uuid,
    parent_base:&str,child_base:&str)->Result<TeamPatchPreview,String>{
    let (target,source)=verified_team_worktrees(settings,parent,child,
        parent_base,child_base).await?;
    let (diff,changed_paths)=changes_with_intent(&source).await?;
    if diff.trim().is_empty(){return Err("completed Writer produced no new source patch".into());}
    let digest=format!("{:x}",Sha256::digest(diff.as_bytes()));
    let conflict=apply_patch_checked(&target,&diff,true).await.err();
    Ok(TeamPatchPreview{
        source_run_id:child,sha256:digest,changed_paths,patch_bytes:diff.len(),
        diff,can_apply:conflict.is_none(),conflict,
    })
}
/// Only the owner-authorized, journaled Engine approval path may call this.
/// Fail closed on changed snapshots or patch bytes, and never edit the
/// original user checkout. At-most-once dispatch is ensured by the caller's
/// persisted, non-replayable intent.
pub async fn merge_team_patch_verified(settings:&Settings,parent:Uuid,child:Uuid,
    parent_base:&str,child_base:&str,approved_sha256:&str)->Result<String,String>{
    let preview=preview_team_patch(settings,parent,child,parent_base,child_base).await?;
    if preview.sha256!=approved_sha256{
        return Err("Team patch changed after owner preview; new approval required".into());
    }
    if let Some(conflict)=preview.conflict{
        return Err(format!("Team patch cannot be safely integrated: {conflict}"));
    }
    let target=settings.workspace_dir.join(parent.to_string());
    apply_patch_checked(&target,&preview.diff,true).await?;
    apply_patch_checked(&target,&preview.diff,false).await?;
    Ok(format!("Approved Team patch {} from worker {} integrated into isolated coordinator {} ({} files)",
        approved_sha256,child,parent,preview.changed_paths.len()))
}


/// Non-mutating reconciliation witness. A successful reverse Git check
/// proves the exact approved delta is already present in the parent
/// coordinator worktree, without replaying the apply operation.
pub async fn team_patch_is_applied(settings:&Settings,parent:Uuid,child:Uuid,
    parent_base:&str,child_base:&str,approved_sha256:&str)->Result<bool,String>{
    let (target,source)=verified_team_worktrees(settings,parent,child,
        parent_base,child_base).await?;
    let (diff,_)=changes_with_intent(&source).await?;
    if diff.trim().is_empty() ||
        format!("{:x}",Sha256::digest(diff.as_bytes()))!=approved_sha256{
        return Err("approved patch bytes changed before reconciliation".into());
    }
    let mut cmd=Command::new("git");
    cmd.args(["apply","--reverse","--check","--binary","--"])
        .current_dir(target).env_clear().env("HOME",settings.workspace_dir.as_os_str());
    #[cfg(windows)]
    cmd.env("PATH",std::env::var_os("PATH").unwrap_or_default())
        .env("SystemRoot",std::env::var_os("SystemRoot").unwrap_or_default());
    #[cfg(not(windows))]
    cmd.env("PATH","/usr/local/bin:/usr/bin:/bin");
    let mut child=cmd.stdin(Stdio::piped()).stdout(Stdio::null())
        .stderr(Stdio::null()).kill_on_drop(true).spawn()
        .map_err(|e|format!("Git reconciliation probe failed: {e}"))?;
    let status=timeout(Duration::from_secs(30),async{
        let mut stdin=child.stdin.take().ok_or("Git reverse-check stdin missing")?;
        stdin.write_all(diff.as_bytes()).await.map_err(|e|e.to_string())?;
        drop(stdin);
        child.wait().await.map_err(|e|e.to_string())
    }).await.map_err(|_|"Git reverse-check timed out".to_string())??;
    Ok(status.success())
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
    command.arg("--").current_dir(dir).env_clear().env("HOME",dir);
    #[cfg(windows)]
    {
        // Keep process lookup and Windows DLL resolution while isolating
        // Git's HOME/config. A Unix-only PATH breaks Git on Windows.
        command.env("PATH",std::env::var_os("PATH").unwrap_or_default())
            .env("SystemRoot",std::env::var_os("SystemRoot").unwrap_or_default());
    }
    #[cfg(not(windows))]
    command.env("PATH","/usr/local/bin:/usr/bin:/bin");
    let mut child=command.stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).kill_on_drop(true).spawn()
        .map_err(|e|format!("git apply launch: {e}"))?;
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

/// Transfer the exact accepted Team seed to an independently cloned remote
/// repository. The remote starts at the original owner revision, commits this
/// bounded seed in an isolated worktree, then edits against the same tree.
pub async fn remote_team_seed(settings:&Settings,parent:Uuid,child:Uuid,
    original_sha:&str,child_seed:&str)->Result<(String,String),String>{
    let (_,writer)=verified_team_worktrees(settings,parent,child,
        original_sha,child_seed).await?;
    let patch=git_patch(&writer,&["diff","--binary","--no-ext-diff",
        original_sha,child_seed,"--"]).await?;
    if patch.len()>131_072{return Err("remote Team seed exceeds 128 KiB".into());}
    let digest=format!("{:x}",Sha256::digest(patch.as_bytes()));
    Ok((patch,digest))
}
/// Install an owner-approved seed only in a detached remote worker worktree.
pub async fn install_remote_seed(settings:&Settings,worker:Uuid,
    original_sha:&str,patch:&str,digest:&str)->Result<String,String>{
    if patch.len()>131_072||format!("{:x}",Sha256::digest(patch.as_bytes()))!=digest{
        return Err("remote seed digest or size mismatch".into());
    }
    let root=settings.workspace_dir.canonicalize().map_err(|e|e.to_string())?;
    let target=root.join(worker.to_string()).canonicalize().map_err(|e|e.to_string())?;
    if !target.starts_with(&root)||git(&["rev-parse","HEAD"],&target).await?!=original_sha{
        return Err("remote worker seed checkout changed".into());
    }
    if patch.is_empty(){return Ok(original_sha.to_string());}
    apply_patch_checked(&target,patch,true).await?;
    apply_patch_checked(&target,patch,false).await?;
    git(&["add","-A","--","."],&target).await?;
    git(&["-c","core.hooksPath=/dev/null","-c","commit.gpgsign=false",
        "-c","user.name=Dominion Remote",
        "-c","user.email=remote@dominion.invalid",
        "commit","--no-verify","-qm","Approved remote Team seed"],&target).await?;
    git(&["rev-parse","HEAD"],&target).await
}
/// Immutable exported patch from a remote isolated Writer. Never reads or
/// returns the source Git checkout, and refuses symlinks/submodules and huge
/// changes before sending bytes to the coordinating machine.
pub async fn export_remote_writer_patch(settings:&Settings,worker:Uuid,
    expected_seed:&str)->Result<(String,String,Vec<String>),String>{
    let root=settings.workspace_dir.canonicalize().map_err(|e|e.to_string())?;
    let target=root.join(worker.to_string()).canonicalize().map_err(|e|e.to_string())?;
    if !target.starts_with(&root)||git(&["rev-parse","HEAD"],&target).await?!=expected_seed{
        return Err("remote Writer head changed since admitted seed".into());
    }
    let (diff,paths)=changes_with_intent(&target).await?;
    if diff.trim().is_empty()||diff.len()>131_072{
        return Err("remote Writer patch is missing or exceeds 128 KiB".into());
    }
    let hash=format!("{:x}",Sha256::digest(diff.as_bytes()));
    Ok((diff,hash,paths))
}
/// Apply an authenticated remote Writer result ONLY to the original
/// owner-authorized Team child worktree; the main checkout is untouched.
/// The caller journals the approved in-flight external tool BEFORE invoking
/// this method. A crash with unknown outcome must be manually reconciled.
pub async fn apply_remote_writer_patch(settings:&Settings,parent:Uuid,child:Uuid,
    parent_sha:&str,child_seed:&str,patch:&str,expected_sha256:&str)
    ->Result<Vec<String>,String>{
    if patch.is_empty()||patch.len()>131_072||
        format!("{:x}",Sha256::digest(patch.as_bytes()))!=expected_sha256{
        return Err("remote Writer patch digest or size mismatch".into());
    }
    let (_,child_dir)=verified_team_worktrees(settings,parent,child,
        parent_sha,child_seed).await?;
    if patch.lines().any(|line|line.trim().ends_with("mode 120000")
        ||line.trim().ends_with("mode 160000")){
        return Err("remote Writer cannot install symlinks or submodules".into());
    }
    // Accept only regular file paths; git apply also validates path escapes.
    let mut paths=Vec::new();
    for line in patch.lines().filter(|line|line.starts_with("diff --git ")){
        let pieces=line.split_whitespace().collect::<Vec<_>>();
        if pieces.len()!=4||!pieces[2].starts_with("a/")||
            pieces[3]!=format!("b/{}",&pieces[2][2..]){
            return Err("invalid remote Writer patch headers".into());
        }
        let path=&pieces[2][2..];
        validate_relative(path)?;
        if path.starts_with(".git/")||path==".git"{
            return Err("remote Writer patch cannot edit .git internals".into());
        }
        paths.push(path.to_owned());
        if paths.len()>200{return Err("remote Writer patch changes too many files".into());}
    }
    if paths.is_empty(){return Err("remote Writer patch has no file deltas".into());}
    apply_patch_checked(&child_dir,patch,true).await?;
    apply_patch_checked(&child_dir,patch,false).await?;
    Ok(paths)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[cfg(windows)]
    #[test]
    fn canonical_verbatim_paths_are_compatible_with_git(){
        assert_eq!(git_path_argument(Path::new(r"\\?\C:\temp\repo")).unwrap(),r"C:\temp\repo");
        assert_eq!(git_path_argument(Path::new(r"\\?\UNC\server\share\repo")).unwrap(),r"\\server\share\repo");
    }
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
    async fn reviewed_team_patch_uses_exact_digest_and_seed_without_touching_source(){
        let root=std::env::temp_dir().join(format!("dominion-team-patch-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.workspace_dir=root.clone();
        let source=root.join("projects").join("demo");
        fs::create_dir_all(&source).await.unwrap();
        git(&["init","-q"],&source).await.unwrap();
        fs::write(source.join("app.txt"),"base\n").await.unwrap();
        git(&["add","app.txt"],&source).await.unwrap();
        git(&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid",
            "commit","-qm","seed"],&source).await.unwrap();
        let parent=Uuid::new_v4();
        let first=Uuid::new_v4();
        let first_sha=attach(&settings,parent,"projects/demo").await.unwrap();
        attach_at(&settings,first,"projects/demo",Some(&first_sha)).await.unwrap();
        fs::write(root.join(first.to_string()).join("app.txt"),"first\n").await.unwrap();
        let preview=preview_team_patch(&settings,parent,first,&first_sha,&first_sha).await.unwrap();
        assert!(preview.can_apply);
        assert_eq!(preview.changed_paths,vec!["app.txt"]);
        assert!(merge_team_patch_verified(&settings,parent,first,&first_sha,&first_sha,
            "wrong").await.is_err());
        assert_eq!(fs::read_to_string(root.join(parent.to_string()).join("app.txt"))
            .await.unwrap(),"base\n");
        merge_team_patch_verified(&settings,parent,first,&first_sha,&first_sha,
            &preview.sha256).await.unwrap();
        assert!(team_patch_is_applied(&settings,parent,first,&first_sha,&first_sha,
            &preview.sha256).await.unwrap());
        assert_eq!(fs::read_to_string(root.join(parent.to_string()).join("app.txt"))
            .await.unwrap(),"first\n");
        assert_eq!(fs::read_to_string(source.join("app.txt")).await.unwrap(),"base\n");

        // The second child sees the accepted first patch, yet only its NEW
        // delta is proposed on review; the first edit is never double-applied.
        let second=Uuid::new_v4();
        attach_at(&settings,second,"projects/demo",Some(&first_sha)).await.unwrap();
        let seeded=seed_team_worker(&settings,parent,second,&first_sha).await.unwrap();
        assert_ne!(seeded,first_sha);
        assert_eq!(fs::read_to_string(root.join(second.to_string()).join("app.txt"))
            .await.unwrap(),"first\n");
        fs::write(root.join(second.to_string()).join("more.txt"),"second\n").await.unwrap();
        let v2=preview_team_patch(&settings,parent,second,&first_sha,&seeded).await.unwrap();
        assert_eq!(v2.changed_paths,vec!["more.txt"]);
        merge_team_patch_verified(&settings,parent,second,&first_sha,&seeded,
            &v2.sha256).await.unwrap();
        assert_eq!(fs::read_to_string(root.join(parent.to_string()).join("more.txt"))
            .await.unwrap(),"second\n");

        // A stale, independently edited worktree cannot clobber an accepted file.
        let stale=Uuid::new_v4();
        attach_at(&settings,stale,"projects/demo",Some(&first_sha)).await.unwrap();
        fs::write(root.join(stale.to_string()).join("app.txt"),"conflict\n").await.unwrap();
        assert!(!preview_team_patch(&settings,parent,stale,&first_sha,&first_sha)
            .await.unwrap().can_apply);
        for id in [parent,first,second,stale] {
            git(&["worktree","remove","--force",
                &root.join(id.to_string()).to_string_lossy()],&source).await.unwrap();
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
