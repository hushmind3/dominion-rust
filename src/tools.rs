use crate::{config::Settings, domain::ToolCall};
use serde_json::{json, Value};
use std::{path::{Component, Path, PathBuf}, time::Duration};
use tokio::{fs, io::{AsyncRead, AsyncReadExt}, process::Command};

pub fn definitions(shell_enabled: bool) -> Vec<Value> {
    let mut tools = vec![
        tool("list_files", "List files relative to the workspace", json!({"type":"object","properties":{"path":{"type":"string","description":"Relative directory; empty for root"}},"required":["path"]})),
        tool("read_file", "Read a UTF-8 file, up to 128 KB", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        tool("write_file", "Write a UTF-8 file inside workspace; replaces existing content", json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})),
        tool("search_files", "Find a literal string in workspace text files", json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]})),
        tool("edit_file", "Edit one exact occurrence in a file; fails when the expected text differs or appears more than once", json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["path","old_text","new_text"]})),
        tool("git_status", "Inspect changed files in an attached Git worktree, without modifying anything", json!({"type":"object","properties":{}})),
        tool("git_diff", "Inspect uncommitted changes in an attached Git worktree, without modifying anything", json!({"type":"object","properties":{}})),
        tool("spawn_subagent", "Delegate an independent task to an isolated Rust worker with reserved budget; use check_subagent to read results", json!({"type":"object","properties":{"instruction":{"type":"string"},"budget_krw":{"type":"number"},"max_steps":{"type":"integer"},"read_only":{"type":"boolean","description":"True for read-only investigators: no writes, shell, external MCP mutations or patch adoption."}},"required":["instruction"]})),
        tool("check_subagent", "Read a child agent status, final answer and measured cost", json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("read_subagent_file", "Inspect a file created or changed by one of your isolated subagents", json!({"type":"object","properties":{"id":{"type":"string"},"path":{"type":"string"}},"required":["id","path"]})),
        tool("merge_subagent", "Propose adopting a completed child\u0027s Git patch into the parent worktree. This requires explicit human approval and checks conflicts.", json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
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
pub async fn execute(settings: &Settings, run_id: uuid::Uuid, call: &ToolCall) -> Result<String, String> {
    let root = settings.workspace_dir.join(run_id.to_string());
    fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
    match call.name.as_str() {
        "read_file" => {
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            let meta = fs::metadata(&path).await.map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.len() > 131_072 { return Err("file is not regular or exceeds 128 KB".into()); }
            let bytes = fs::read(path).await.map_err(|e| e.to_string())?;
            String::from_utf8(bytes).map_err(|e| e.to_string())
        }
        "write_file" => {
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            let content = required(&call.arguments,"content")?;
            if content.len() > 262_144 { return Err("write exceeds 256 KB".into()); }
            if path == root.canonicalize().map_err(|e| e.to_string())? { return Err("path must name a file".into()); }
            if let Some(parent) = path.parent() { fs::create_dir_all(parent).await.map_err(|e| e.to_string())?; }
            // Recheck all components after directory creation.
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            fs::write(&path, content).await.map_err(|e| e.to_string())?;
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
            if !metadata.is_file()||metadata.len()>262_144 {return Err("file exceeds 256 KB or is not regular".into());}
            let content=fs::read_to_string(&path).await.map_err(|e|e.to_string())?;
            let matches=content.match_indices(old).count();
            if matches!=1 {return Err(format!("edit_file requires exactly one exact match, found {matches}")); }
            let updated=content.replacen(old,new,1);
            if updated.len()>524_288 {return Err("edited file exceeds 512 KB".into());}
            let checked=validated_path(&root,raw_path)?;
            let tmp=checked.with_extension(format!("dominion-{}.tmp",uuid::Uuid::new_v4()));
            fs::write(&tmp,updated).await.map_err(|e|e.to_string())?;
            if let Err(err)=fs::rename(&tmp,&checked).await {
                let _=fs::remove_file(&tmp).await;
                return Err(format!("atomic edit failed: {err}"));
            }
            Ok(format!("edited {}: one exact match replaced",raw_path))
        }
        "git_status" => git_read(&root,&["status","--short"]).await,
        "git_diff" => git_read(&root,&["diff","--no-ext-diff","--"]).await,
        "list_files" => {
            let path = validated_path(&root, required(&call.arguments,"path")?)?;
            let paths = list_relative(&root, &path).await?;
            Ok(serde_json::to_string(&paths).map_err(|e| e.to_string())?)
        }
        "search_files" => {
            let query = required(&call.arguments,"query")?;
            if query.is_empty() || query.len() > 200 { return Err("query must be 1..200 bytes".into()); }
            let files = list_relative(&root, &root).await?;
            let mut hits = vec![];
            for file in files.iter().take(200) {
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
async fn list_relative(root: &Path, starting: &Path) -> Result<Vec<String>,String> {
    let mut stack = vec![starting.to_path_buf()];
    let mut found = Vec::new();
    while let Some(path) = stack.pop() {
        if found.len() >= 200 { break; }
        if fs::metadata(&path).await.map_err(|e| e.to_string())?.is_file() {
            found.push(path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string());
            continue;
        }
        let mut dir = fs::read_dir(&path).await.map_err(|e| e.to_string())?;
        while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
            let meta = entry.file_type().await.map_err(|e| e.to_string())?;
            if meta.is_symlink() { continue; }
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') || ["node_modules","target","dist","build",".venv","__pycache__"].contains(&name.to_string_lossy().as_ref()) { continue; }
            if meta.is_dir() { stack.push(entry.path()); }
            else if meta.is_file() {
                found.push(entry.path().strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string());
                if found.len() >= 200 { break; }
            }
        }
    }
    found.sort();
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
    let mut child = Command::new("sh")
        .arg("-lc").arg(command).current_dir(&root)
        .env_clear().env("PATH","/usr/local/bin:/usr/bin:/bin").env("HOME",&root)
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e| e.to_string())?;
    let out = child.stdout.take().ok_or("stdout unavailable")?;
    let err = child.stderr.take().ok_or("stderr unavailable")?;
    let result = tokio::time::timeout(Duration::from_secs(40), async {
        let (stdout, stderr, status) = tokio::try_join!(bounded(out), bounded(err), async {child.wait().await.map_err(|e| e.to_string())})?;
        Ok::<_,String>((stdout,stderr,status))
    }).await.map_err(|_|"shell timed out".to_string())??;
    Ok(format!("exit={}\nstdout:\n{}\nstderr:\n{}",result.2,result.0,result.1))
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
}
