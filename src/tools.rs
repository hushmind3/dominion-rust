use crate::{config::Settings, domain::ToolCall};
use serde_json::{json, Value};
use std::{path::{Component, Path, PathBuf}, time::Duration};
use tokio::{fs, io::{AsyncRead, AsyncReadExt, AsyncWriteExt}, process::Command};

pub fn definitions(shell_enabled: bool) -> Vec<Value> {
    let mut tools = vec![
        tool("list_files", "List workspace files (sorted), with offset/limit pagination for repositories larger than 200 files", json!({"type":"object","properties":{"path":{"type":"string","description":"Relative directory; empty for root"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":400}},"required":["path"]})),
        tool("read_file", "Read a UTF-8 source file, optionally selecting 1-based start_line and end_line; response is capped at 128 KB", json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},"required":["path"]})),
        tool("write_file", "Write a UTF-8 file inside workspace; replaces existing content", json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})),
        tool("search_files", "Search text across up to 10000 repository files, returning bounded matching lines; path scopes to a subtree", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string","description":"Relative search subtree, default workspace root"}},"required":["query"]})),
        tool("edit_file", "Edit one exact occurrence in a file; fails when the expected text differs or appears more than once", json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["path","old_text","new_text"]})),
        tool("git_status", "Inspect changed files in an attached Git worktree, without modifying anything", json!({"type":"object","properties":{}})),
        tool("git_diff", "Inspect uncommitted changes in an attached Git worktree, without modifying anything", json!({"type":"object","properties":{}})),
        tool("spawn_subagent", "Delegate an independent task to an isolated Rust worker with reserved budget; use check_subagent to read results", json!({"type":"object","properties":{"instruction":{"type":"string"},"budget_krw":{"type":"number"},"max_steps":{"type":"integer"},"read_only":{"type":"boolean","description":"True for read-only investigators: no writes, shell, external MCP mutations or patch adoption."}},"required":["instruction"]})),
        tool("check_subagent", "Read a child agent status, final answer and measured cost", json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("read_subagent_file", "Inspect a file created or changed by one of your isolated subagents", json!({"type":"object","properties":{"id":{"type":"string"},"path":{"type":"string"}},"required":["id","path"]})),
        tool("merge_subagent", "Propose adopting a completed child's Git patch into the parent worktree. This requires explicit human approval and checks conflicts.", json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("memory_search", "Retrieve project-scoped persistent notes and reusable lessons without any model API call",json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]})),
        tool("memory_store", "Store or update one reusable non-weight project note, keyed by title; visible to future runs in the same repository",json!({"type":"object","properties":{"title":{"type":"string"},"content":{"type":"string"}},"required":["title","content"]})),
        tool("list_skills","List project SKILL.md procedures; procedure content is loaded on demand",json!({"type":"object","properties":{}})),
        tool("load_skill","Read one project-local SKILL.md by name, without executing scripts",json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]})),
    ];
    if shell_enabled {
        tools.push(tool("shell", "Execute an approved shell command in the workspace; user approval is always required", json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]})));
    }
    tools
}
fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}
fn validated_path(root: &Path, raw: &str) -> Result<PathBuf, String> {
    if raw.len() > 1024 || raw.as_bytes().contains(&0) { return Err("invalid path".into()); }
    let path = Path::new(raw);
    if path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err("absolute or parent path forbidden".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut output = root.clone();
    for component in path.components() {
        if let Component::Normal(part) = component {
            output.push(part);
            match std::fs::symlink_metadata(&output) {
                Ok(metadata) if metadata.file_type().is_symlink() => return Err("symbolic links forbidden".into()),
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    if !output.starts_with(&root) { return Err("path outside workspace".into()); }
    Ok(output)
}
fn required<'a>(v: &'a Value, name: &str) -> Result<&'a str, String> {
    v.get(name).and_then(Value::as_str).ok_or_else(|| format!("missing {name}"))
}

/**
 * Replace a source file without exposing an intermediate truncated version.
 * The tool journal already records the mutation intent before this function
 * runs. Staging also keeps prior file contents intact on failed writes.
 * Preserve executable/read-only permissions on existing source files.
 */
async fn replace_atomically(path:&Path, contents:&[u8])->Result<(),String>{
    let parent=path.parent().ok_or("file path has no parent")?;
    let name=path.file_name().ok_or("file path has no name")?.to_string_lossy();
    let temporary=parent.join(format!(".{name}.dominion-{}.tmp",uuid::Uuid::new_v4()));
    let permissions=match fs::symlink_metadata(path).await{
        Ok(meta) if meta.file_type().is_file()=>Some(meta.permissions()),
        Ok(_)=>return Err("write target is not a regular file".into()),
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>None,
        Err(error)=>return Err(error.to_string()),
    };
    let result=async {
        let mut file=fs::OpenOptions::new().write(true).create_new(true)
            .open(&temporary).await.map_err(|e|format!("staging write failed: {e}"))?;
        file.write_all(contents).await.map_err(|e|format!("staging content failed: {e}"))?;
        if let Some(permissions)=permissions {
            fs::set_permissions(&temporary,permissions).await.map_err(|e|format!("preserving source permissions failed: {e}"))?;
        }
        file.sync_all().await.map_err(|e|format!("staging fsync failed: {e}"))?;
        drop(file);
        fs::rename(&temporary,path).await.map_err(|e|format!("atomic source replacement failed: {e}"))?;
        // The file contents are synced before rename. On Unix, sync the
        // containing directory as well so a successful rename is durable.
        #[cfg(unix)]
        std::fs::File::open(parent).and_then(|directory|directory.sync_all())
            .map_err(|e|format!("directory fsync failed after rename; inspect the destination before retrying: {e}"))?;
        Ok(())
    }.await;
    if result.is_err(){let _=fs::remove_file(&temporary).await;}
    result
}
fn shell_receipt(success:bool,status:&str,stdout:&str,stderr:&str)->Result<String,String>{
    let report=format!("exit={status}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    if success {Ok(report)} else {Err(format!("shell command failed:\n{report}"))}
}

pub async fn execute(settings: &Settings, run_id: uuid::Uuid, call: &ToolCall) -> Result<String, String> {
    let root = settings.workspace_dir.join(run_id.to_string());
    fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
    match call.name.as_str() {
        "read_file" => {
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            let meta = fs::metadata(&path).await.map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.len() > 4_194_304 { return Err("file is not regular or exceeds 4 MB".into()); }
            let content=fs::read_to_string(path).await.map_err(|e|e.to_string())?;
            if call.arguments.get("start_line").is_none() && call.arguments.get("end_line").is_none() && content.len()<=131_072 {
                return Ok(content); // Preserve existing SDK/read_file semantics for small files.
            }
            let start=call.arguments.get("start_line").and_then(Value::as_u64).unwrap_or(1).clamp(1,1_000_000) as usize;
            let end=call.arguments.get("end_line").and_then(Value::as_u64).unwrap_or(start.saturating_add(2999) as u64).clamp(start as u64,(start+2999) as u64) as usize;
            let mut out=String::new();
            let mut truncated=false;
            let mut seen=false;
            for (index,line) in content.lines().enumerate() {
                let number=index+1;
                if number<start{continue;}
                if number>end{break;}
                seen=true;
                let item=format!("{number}: {line}\n");
                if out.len()+item.len()>128_000{truncated=true;break;}
                out.push_str(&item);
            }
            if !seen{return Err("start_line is beyond the end of this file".into());}
            if truncated{out.push_str("[truncated; request a smaller line range or advance start_line]\n");}
            Ok(out)
        }
        "write_file" => {
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            let content = required(&call.arguments,"content")?;
            if content.len() > 1_048_576 { return Err("write exceeds 1 MB".into()); }
            if path == root.canonicalize().map_err(|e| e.to_string())? { return Err("path must name a file".into()); }
            if let Some(parent) = path.parent() { fs::create_dir_all(parent).await.map_err(|e| e.to_string())?; }
            // Recheck all components after directory creation.
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            replace_atomically(&path, content.as_bytes()).await?;
            Ok(format!("wrote {} bytes to {}", content.len(), path.strip_prefix(root.canonicalize().map_err(|e| e.to_string())?).unwrap_or(&path).display()))
        }
        "edit_file" => {
            let raw_path=required(&call.arguments,"path")?;
            let old=required(&call.arguments,"old_text")?;
            let new=required(&call.arguments,"new_text")?;
            if old.is_empty() || old.len()>65_536 || new.len()>262_144 {
                return Err("invalid edit sizes; expected a bounded nonempty old_text".into());
            }
            let path=validated_path(&root,raw_path)?;
            let metadata=fs::metadata(&path).await.map_err(|e|e.to_string())?;
            if !metadata.is_file()||metadata.len()>4_194_304 {return Err("file exceeds 4 MB or is not regular".into());}
            let content=fs::read_to_string(&path).await.map_err(|e|e.to_string())?;
            let matches=content.match_indices(old).count();
            if matches!=1 {return Err(format!("edit_file requires exactly one exact match, found {matches}")); }
            let updated=content.replacen(old,new,1);
            if updated.len()>4_194_304 {return Err("edited file exceeds 4 MB".into());}
            let checked=validated_path(&root,raw_path)?;
            replace_atomically(&checked,updated.as_bytes()).await?;
            Ok(format!("edited {}: one exact match replaced",raw_path))
        }
        "git_status" => git_read(&root,&["status","--short"]).await,
        "git_diff" => git_read(&root,&["diff","--no-ext-diff","--"]).await,
        "list_files" => {
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            let paths = list_relative(&root, &path,10_000).await?;
            let offset=call.arguments.get("offset").and_then(Value::as_u64).unwrap_or(0).min(10_000) as usize;
            let limit=call.arguments.get("limit").and_then(Value::as_u64).unwrap_or(200).clamp(1,400) as usize;
            let selected=paths.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
            Ok(serde_json::to_string(&selected).map_err(|e|e.to_string())?)
        }
        "search_files" => {
            let query = required(&call.arguments,"query")?;
            if query.is_empty() || query.len() > 200 { return Err("query must be 1..200 bytes".into()); }
            let subtree=call.arguments.get("path").and_then(Value::as_str).unwrap_or("");
            let starting=validated_path(&root,subtree)?;
            let files = list_relative(&root, &starting,10_000).await?;
            let mut hits = vec![];
            for file in &files {
                let path = validated_path(&root, file)?;
                if let Ok(meta) = fs::metadata(&path).await {
                    if meta.is_file() && meta.len() <= 131_072 {
                        if let Ok(content) = fs::read_to_string(path).await {
                            for (i, line) in content.lines().enumerate() {
                                if line.contains(query) {
                                    hits.push(json!({"file":file,"line":i+1,"preview":line.chars().take(240).collect::<String>()}));
                                    if hits.len() >= 50 { break; }
                                }
                            }
                        }
                    }
                }
                if hits.len() >= 50 { break; }
            }
            Ok(json!(hits).to_string())
        }
        "shell" if settings.shell_enabled => {
            let command = required(&call.arguments,"command")?;
            if command.len() > 4096 { return Err("command exceeds 4096 bytes".into()); }
            run_shell(root, command).await
        }
        _ => Err(format!("unknown or disabled tool: {}",call.name)),
    }
}
async fn git_read(root:&Path,args:&[&str])->Result<String,String>{
    let output=tokio::time::timeout(Duration::from_secs(15),Command::new("git")
        .args(args).current_dir(root).kill_on_drop(true).output())
        .await.map_err(|_|"git inspection timed out".to_string())?
        .map_err(|e|format!("git unavailable: {e}"))?;
    if !output.status.success() {
        return Err(format!("git inspection failed: {}",String::from_utf8_lossy(&output.stderr).chars().take(800).collect::<String>()));
    }
    let out=String::from_utf8_lossy(&output.stdout);
    let clipped=out.chars().take(32_000).collect::<String>();
    Ok(if out.chars().count()>32_000 {format!("{clipped}\n[diff truncated; inspect specific files]")}else{clipped})
}
async fn list_relative(root: &Path, starting: &Path,max_files:usize) -> Result<Vec<String>,String> {
    // Stable lexical pagination is essential: truncating during traversal
    // silently hides half of medium-to-large codebases from the agent.
    let mut stack = vec![starting.to_path_buf()];
    let mut found = Vec::new();
    let mut directories=0usize;
    while let Some(path) = stack.pop() {
        if found.len()>=max_files{break;}
        directories+=1;
        if directories>10_000{return Err("repository contains too many directories; narrow path".into());}
        let meta=fs::symlink_metadata(&path).await.map_err(|e|e.to_string())?;
        if meta.file_type().is_symlink(){continue;}
        if meta.is_file(){
            found.push(path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string());
            continue;
        }
        let mut dir = fs::read_dir(&path).await.map_err(|e| e.to_string())?;
        let mut children=Vec::new();
        while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
            let meta = entry.file_type().await.map_err(|e| e.to_string())?;
            if meta.is_symlink(){continue;}
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') || ["node_modules","target","dist","build",".venv","__pycache__"].contains(&name.to_string_lossy().as_ref()) {continue;}
            if meta.is_dir(){children.push(entry.path());}
            else if meta.is_file(){
                found.push(entry.path().strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string());
            }
        }
        children.sort();
        stack.extend(children.into_iter().rev());
    }
    found.sort();
    found.truncate(max_files);
    Ok(found)
}
async fn bounded<R: AsyncRead + Unpin>(mut reader: R) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut limited = (&mut reader).take(16_385);
    limited.read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
    if buf.len() > 16_384 { return Err("shell output exceeded 16 KB".into()); }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
async fn run_shell(root: PathBuf, command: &str) -> Result<String, String> {
    use std::process::Stdio;
    let mut child = crate::sandbox::script(&root,command)?
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e| e.to_string())?;
    let out = child.stdout.take().ok_or("stdout unavailable")?;
    let err = child.stderr.take().ok_or("stderr unavailable")?;
    let result = tokio::time::timeout(Duration::from_secs(40), async {
        let (stdout, stderr, status) = tokio::try_join!(bounded(out), bounded(err), async {child.wait().await.map_err(|e| e.to_string())})?;
        Ok::<_,String>((stdout,stderr,status))
    }).await.map_err(|_|"shell timed out".to_string())??;
    shell_receipt(result.2.success(),&result.2.to_string(),&result.0,&result.1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn denies_parent_traversal_and_absolute_path() {
        let dir = std::env::temp_dir();
        assert!(validated_path(&dir, "../etc/passwd").is_err());
        assert!(validated_path(&dir, "/etc/passwd").is_err());
    }

    #[tokio::test]
    async fn paginates_large_repository_and_searches_beyond_first_200(){
        let mut settings=Settings::from_env().unwrap();
        settings.workspace_dir=std::env::temp_dir().join(format!("dominion-large-tree-{}",uuid::Uuid::new_v4()));
        let id=uuid::Uuid::new_v4();
        let root=settings.workspace_dir.join(id.to_string());
        fs::create_dir_all(&root).await.unwrap();
        for index in 0..255 {
            fs::write(root.join(format!("file-{index:03}.txt")),if index==250{"needle-in-late-file"}else{"ordinary"}).await.unwrap();
        }
        let list=ToolCall{id:"list".into(),name:"list_files".into(),arguments:json!({"path":"","offset":200,"limit":100})};
        let page:Vec<String>=serde_json::from_str(&execute(&settings,id,&list).await.unwrap()).unwrap();
        assert_eq!(page.len(),55);
        assert_eq!(page[50],"file-250.txt");
        let search=ToolCall{id:"search".into(),name:"search_files".into(),arguments:json!({"query":"needle-in-late-file"})};
        let result=execute(&settings,id,&search).await.unwrap();
        assert!(result.contains("file-250.txt"));
        let lines=ToolCall{id:"lines".into(),name:"read_file".into(),arguments:json!({"path":"file-250.txt","start_line":1,"end_line":1})};
        assert!(execute(&settings,id,&lines).await.unwrap().contains("needle-in-late-file"));
        fs::remove_dir_all(settings.workspace_dir).await.unwrap();
    }

    #[tokio::test]
    async fn io_round_trip() {
        let mut settings = Settings::from_env().unwrap();
        settings.workspace_dir = std::env::temp_dir().join(format!("dominion-tools-{}",uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4();
        let write=ToolCall{id:"1".into(),name:"write_file".into(),arguments:json!({"path":"src/file.txt","content":"hi"})};
        execute(&settings,id,&write).await.unwrap();
        let read=ToolCall{id:"2".into(),name:"read_file".into(),arguments:json!({"path":"src/file.txt"})};
        assert_eq!(execute(&settings,id,&read).await.unwrap(),"hi");
        let edit=ToolCall{id:"3".into(),name:"edit_file".into(),arguments:json!({"path":"src/file.txt","old_text":"hi","new_text":"updated"})};
        execute(&settings,id,&edit).await.unwrap();
        assert_eq!(execute(&settings,id,&read).await.unwrap(),"updated");
        assert!(execute(&settings,id,&edit).await.is_err());
        fs::remove_dir_all(settings.workspace_dir).await.unwrap();
    }

    #[test]
    fn shell_nonzero_exit_is_a_tool_error_not_a_success_receipt(){
        assert!(shell_receipt(true,"exit status: 0","ok","").unwrap().contains("stdout:"));
        let error=shell_receipt(false,"exit status: 7","partial","failure").unwrap_err();
        assert!(error.contains("exit status: 7"));
        assert!(error.contains("partial"));
        assert!(error.contains("failure"));
    }

    #[tokio::test]
    async fn atomic_source_updates_preserve_previous_file_and_executable_mode(){
        let root=std::env::temp_dir().join(format!("dominion-atomic-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let existing=root.join("script.sh");
        fs::write(&existing,"#!/bin/sh\\necho old\\n").await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&existing,std::fs::Permissions::from_mode(0o755)).await.unwrap();
        }
        replace_atomically(&existing,b"#!/bin/sh\\necho new\\n").await.unwrap();
        assert_eq!(fs::read_to_string(&existing).await.unwrap(),"#!/bin/sh\\necho new\\n");
        #[cfg(unix)]
        assert_eq!(fs::metadata(&existing).await.unwrap().permissions().mode() & 0o777,0o755);
        let folder=root.join("folder");
        fs::create_dir(&folder).await.unwrap();
        assert!(replace_atomically(&folder,b"should refuse").await.is_err());
        assert!(fs::metadata(&folder).await.unwrap().is_dir());
        replace_atomically(&root.join("new.txt"),b"added").await.unwrap();
        assert_eq!(fs::read_to_string(root.join("new.txt")).await.unwrap(),"added");
        let mut entries=fs::read_dir(&root).await.unwrap();
        while let Some(entry)=entries.next_entry().await.unwrap(){
            assert!(!entry.file_name().to_string_lossy().contains(".dominion-"));
        }
        fs::remove_dir_all(root).await.unwrap();
    }
}
