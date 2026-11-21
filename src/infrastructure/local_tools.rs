use crate::{config::Settings, domain::ToolCall};
use serde_json::{json, Value};
use std::{collections::VecDeque,path::{Component, Path, PathBuf}, time::Duration};
use tokio::{fs, io::{AsyncRead, AsyncReadExt, AsyncWriteExt}, process::Command};

pub fn definitions(shell_enabled: bool) -> Vec<Value> {
    // Never advertise shell inside the program DSL when disabled; scoped
    // owner-approved tests remain executable without global shell access.
    let mut program_step_tools=vec![
        "list_files","read_file","search_files","git_status","git_diff",
        "read_program","write_file","edit_file","run_tests"
    ];
    if shell_enabled{program_step_tools.push("shell");}
    let mut tools = vec![
        tool("list_files", "List workspace files (sorted), with offset/limit pagination for repositories larger than 200 files", json!({"type":"object","properties":{"path":{"type":"string","description":"Relative directory; empty for root"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":400}},"required":["path"]})),
        tool("read_file", "Read a UTF-8 source file, optionally selecting 1-based start_line and end_line; response is capped at 128 KB", json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},"required":["path"]})),
        tool("read_tool_receipt", "Read an exact slice from an earlier tool result in THIS Run without repeating the external operation. Supply its tool_call_id; response is paginated at 6000 characters.", json!({"type":"object","properties":{"call_id":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":6000}},"required":["call_id"]})),
        tool("search_tool_receipts", "Search earlier exact tool results within THIS Run by a 2..128-character phrase or terms. Returns bounded snippets and call_id plus character offsets; use read_tool_receipt for exact source evidence. No external tool is rerun.", json!({"type":"object","properties":{"query":{"type":"string","minLength":2,"maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":8}},"required":["query"],"additionalProperties":false})),
        tool("write_file", "Write a UTF-8 file inside workspace; replaces existing content", json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})),
        tool("search_files", "Search text across up to 10000 repository files, returning bounded matching lines; path scopes to a subtree", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string","description":"Relative search subtree, default workspace root"}},"required":["query"]})),
        tool("edit_file", "Edit one exact occurrence in a file; fails when the expected text differs or appears more than once", json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["path","old_text","new_text"]})),
        tool("git_status", "Inspect changed files in an attached Git worktree, without modifying anything", json!({"type":"object","properties":{}})),
        tool("git_diff", "Inspect uncommitted changes in an attached Git worktree, without modifying anything", json!({"type":"object","properties":{}})),
        tool("run_tests", "Run a bounded test command in the isolated Git worktree through the existing OS sandbox, with explicit owner approval every time. Use this even if general shell execution is disabled. Example: cargo test --locked, python3 -m unittest discover. No shell pipelines or arbitrary scripts.", json!({"type":"object","properties":{"command":{"type":"string","maxLength":512,"description":"One recognized test invocation, no pipes, redirects or chained commands"}},"required":["command"],"additionalProperties":false})),
        tool("read_program", "Execute one model-authored read-only evidence DAG entirely in Rust: independent reads run in parallel, dependencies/typed errors drive branches, list_files -> read_listed_files and search_files -> read_matched_files discover paths at runtime, objective witnesses stop unnecessary work. Receipts contain source hashes. No nested model, shell, writes or external actions.", json!({"type":"object","properties":{"steps":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"object","properties":{"id":{"type":"string"},"tool":{"type":"string","enum":["list_files","read_file","search_files","git_status","git_diff","read_listed_files","read_matched_files"]},"arguments":{"type":"object","description":"Tool arguments; any scalar argument may bind to a prior step via {from:stepId,path:/value/0/file}. Bindings resolve from actual typed receipts and pass normal tool schema admission."},"after":{"type":"array","maxItems":12,"items":{"type":"string"}},"when":{"type":"object","properties":{"step":{"type":"string"},"status":{"type":"string","enum":["ok","error","skipped"]},"contains":{"type":"string"},"not":{"type":"boolean"},"predicate":{"type":"object","description":"Main Program IR three-valued predicate. Types: equals, exists, compare, not, all, any. Use from=prior step, path=/status or /value/...; missing evidence is Unknown.","properties":{"type":{"type":"string","enum":["equals","exists","compare","not","all","any"]},"from":{"type":"string"},"path":{"type":"string"},"value":{},"op":{"type":"string","enum":["gt","gte","lt","lte"]},"predicate":{"type":"object"},"predicates":{"type":"array","items":{"type":"object"}}},"required":["type"]}},"required":["step"]},"from":{"type":"string"},"suffix":{"type":"string"},"contains":{"type":"string"},"max_files":{"type":"integer","minimum":1,"maximum":8},"lines_containing":{"type":"string"}},"required":["id","tool"]}},"objective":{"type":"object","properties":{"mode":{"type":"string","enum":["all","any"]},"assertions":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","properties":{"step":{"type":"string"},"status":{"type":"string","enum":["ok","error"]},"contains":{"type":"string"}},"required":["step"]}}},"required":["mode","assertions"]}},"required":["steps"]})),
        tool("run_program","Schedule a short dependent tool program: model chooses native read/edit/write/test steps once, Rust resolves typed observations and runs each step through the ORIGINAL durable Run, schema, approvals and crash recovery. No intermediate model calls; an error returns to the model for repair. Call this tool alone. External accounts, MCP, subagents and nested run_program are forbidden.", json!({"type":"object","properties":{"steps":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"object","properties":{"id":{"type":"string"},"tool":{"type":"string","enum":program_step_tools},"arguments":{"type":"object","description":"Native tool arguments, or a typed reference {from:earlierStepId,path:/value/0/file} from an earlier actual tool receipt."},"when":{"type":"object","description":"Typed observation predicate from Main Program IR. Missing evidence is Unknown, never false.","properties":{"type":{"type":"string","enum":["exists","equals","compare","not","all","any"]},"from":{"type":"string"},"path":{"type":"string"},"value":{},"op":{"type":"string","enum":["gt","gte","lt","lte"]},"predicate":{"type":"object"},"predicates":{"type":"array","items":{"type":"object"}}},"required":["type"]},"on_error":{"type":"string","enum":["stop","continue"]}},"required":["id","tool","arguments"]}},"stop_when":{"type":"object","description":"Optional typed literal evidence predicate; stop remaining program steps when proven by actual tool receipts (not a final task pass).","properties":{"type":{"type":"string","enum":["exists","equals","compare","not","all","any"]},"from":{"type":"string"},"path":{"type":"string"},"value":{},"op":{"type":"string","enum":["gt","gte","lt","lte"]},"predicate":{"type":"object"},"predicates":{"type":"array","items":{"type":"object"}}},"required":["type"]}},"required":["steps"]})),
        tool("spawn_subagent", "Delegate an independent task to an isolated Rust worker with reserved budget; use check_subagent to read results", json!({"type":"object","properties":{"instruction":{"type":"string"},"budget_krw":{"type":"number"},"max_steps":{"type":"integer"},"read_only":{"type":"boolean","description":"True for read-only investigators: no writes, shell, external MCP mutations or patch adoption."},"profile":{"type":"string","enum":["general","inspection","implementation","verification"],"description":"Optional scoped tools: inspection=local and connected-account reads, implementation=workspace edits, verification=reads plus approval-gated shell. Defaults to general. Child retains the original Run, model and budget authority."}},"required":["instruction"]})),
        tool("check_subagent", "Read a child agent status, final answer and measured cost", json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("read_subagent_file", "Inspect a file created or changed by one of your isolated subagents", json!({"type":"object","properties":{"id":{"type":"string"},"path":{"type":"string"}},"required":["id","path"]})),
        tool("merge_subagent", "Propose adopting a completed child's Git patch into the parent worktree. This requires explicit human approval and checks conflicts.", json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("memory_search", "Retrieve project-scoped persistent notes and reusable lessons without any model API call",json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]})),
        tool("memory_store", "Store or update one reusable non-weight project note, keyed by title; visible to future runs in the same repository",json!({"type":"object","properties":{"title":{"type":"string"},"content":{"type":"string"}},"required":["title","content"]})),
        tool("tool_search","Search currently enabled MCP and connected-account tools by the action you need. When many tools are connected their large schemas are deferred; matching tools become directly callable on the NEXT model turn through the ordinary Rust approval and receipt gateway.",json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":256},"limit":{"type":"integer","minimum":1,"maximum":8}},"required":["query"],"additionalProperties":false})),
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
        "run_tests" => {
            let command=required(&call.arguments,"command")?;
            if command.len()>512||!crate::acceptance::test_command(command){
                return Err("run_tests accepts a single approved test command only".into());
            }
            run_shell(root,command,Duration::from_secs(120)).await
        }
        "shell" if settings.shell_enabled => {
            let command = required(&call.arguments,"command")?;
            if command.len() > 4096 { return Err("command exceeds 4096 bytes".into()); }
            run_shell(root,command,Duration::from_secs(40)).await
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
    // macOS can expose the same temp root as /var and /private/var.
    // Resolve both sides once; never fall back to returning absolute paths
    // when strip_prefix sees different spellings of the same directory.
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let starting=starting.canonicalize().map_err(|e|e.to_string())?;
    if !starting.starts_with(&root){return Err("list root escapes workspace".into());}
    let mut stack = vec![starting];
    let mut found = Vec::new();
    let mut directories=0usize;
    while let Some(path) = stack.pop() {
        if found.len()>=max_files{break;}
        directories+=1;
        if directories>10_000{return Err("repository contains too many directories; narrow path".into());}
        let meta=fs::symlink_metadata(&path).await.map_err(|e|e.to_string())?;
        if meta.file_type().is_symlink(){continue;}
        if meta.is_file(){
            found.push(path.strip_prefix(&root).map_err(|_|"list path escapes workspace")?
                .to_string_lossy().into_owned());
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
                found.push(entry.path().strip_prefix(&root).map_err(|_|"list entry escapes workspace")?
                    .to_string_lossy().into_owned());
            }
        }
        children.sort();
        stack.extend(children.into_iter().rev());
    }
    found.sort();
    found.truncate(max_files);
    Ok(found)
}
/// Drain both pipes to EOF while retaining bounded beginning and end.
/// A verbose test's success/failure is determined by its real exit status,
/// not by crossing an arbitrary stdout/stderr threshold. Keep the test
/// summary near the end; mark omitted bytes so excerpts are not mistaken
/// for an exact, complete log.
async fn bounded<R: AsyncRead + Unpin>(mut reader: R) -> Result<String, String> {
    const FULL:usize=16_384;
    const HEAD:usize=4_096;
    const TAIL:usize=8_192;
    let mut initial=Vec::with_capacity(FULL);
    let mut trailing=VecDeque::with_capacity(TAIL);
    let mut total=0usize;
    let mut chunk=[0u8;4_096];
    loop {
        let count=reader.read(&mut chunk).await.map_err(|e|e.to_string())?;
        if count==0{break;}
        total=total.saturating_add(count);
        let keep=FULL.saturating_sub(initial.len()).min(count);
        initial.extend_from_slice(&chunk[..keep]);
        for &byte in &chunk[..count]{
            if trailing.len()==TAIL{trailing.pop_front();}
            trailing.push_back(byte);
        }
    }
    if total<=FULL{return Ok(String::from_utf8_lossy(&initial).into_owned());}
    let omitted=total.saturating_sub(HEAD+TAIL);
    let tail=trailing.into_iter().collect::<Vec<_>>();
    Ok(format!(
        "{}\n[... output truncated: {omitted} bytes omitted; first {HEAD} and last {TAIL} bytes retained ...]\n{}",
        String::from_utf8_lossy(&initial[..HEAD]),String::from_utf8_lossy(&tail)
    ))
}
async fn run_shell(root: PathBuf, command: &str, timeout:Duration) -> Result<String, String> {
    use std::process::Stdio;
    let mut child = crate::sandbox::script(&root,command)?
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e| e.to_string())?;
    let out = child.stdout.take().ok_or("stdout unavailable")?;
    let err = child.stderr.take().ok_or("stderr unavailable")?;
    let result = tokio::time::timeout(timeout, async {
        let (stdout, stderr, status) = tokio::try_join!(bounded(out), bounded(err), async {child.wait().await.map_err(|e| e.to_string())})?;
        Ok::<_,String>((stdout,stderr,status))
    }).await.map_err(|_|format!("shell timed out after {} seconds",timeout.as_secs()))??;
    shell_receipt(result.2.success(),&result.2.to_string(),&result.0,&result.1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn denies_parent_traversal_and_absolute_path() {
        let dir = std::env::temp_dir();
        assert!(validated_path(&dir, "../etc/passwd").is_err());
        assert!(validated_path(&dir, "/etc/passwd").is_err());
    }

    #[tokio::test]
    async fn verbose_command_output_preserves_exit_evidence_and_failure_tail(){
        let (mut sender,receiver)=tokio::io::duplex(512);
        let mut bytes=b"FIRST_LINE\n".to_vec();
        bytes.extend(vec![b'x';50_000]);
        bytes.extend(b"\nFINAL_FAILURE_MARKER\n");
        let produce=tokio::spawn(async move{
            sender.write_all(&bytes).await.unwrap();
        });
        let result=bounded(receiver).await.unwrap();
        produce.await.unwrap();
        assert!(result.starts_with("FIRST_LINE"));
        assert!(result.contains("output truncated:"));
        assert!(result.contains("bytes omitted"));
        assert!(result.ends_with("FINAL_FAILURE_MARKER\n"));
        assert!(result.len()<13_000);
        assert!(shell_receipt(false,"exit status: 1",&result,"")
            .unwrap_err().contains("FINAL_FAILURE_MARKER"));
        assert!(shell_receipt(true,"exit status: 0",&result,"").is_ok(),
            "a successful command remains successful even when output was long");
        let (mut sender,receiver)=tokio::io::duplex(64);
        let small=tokio::spawn(async move{
            sender.write_all(b"short test result\n").await.unwrap();
        });
        assert_eq!(bounded(receiver).await.unwrap(),"short test result\n");
        small.await.unwrap();
    }

    #[test]
    fn program_schema_exposes_scoped_tests_but_hides_disabled_shell(){
        let disabled=definitions(false);
        let program=disabled.iter().find(|tool|
            tool.pointer("/function/name").and_then(Value::as_str)==Some("run_program"))
            .unwrap();
        let args=program.pointer("/function/parameters/properties/steps/items/properties/tool/enum")
            .and_then(Value::as_array).unwrap();
        assert!(args.iter().any(|value|value=="run_tests"));
        assert!(!args.iter().any(|value|value=="shell"));
        let enabled=definitions(true);
        let program=enabled.iter().find(|tool|
            tool.pointer("/function/name").and_then(Value::as_str)==Some("run_program"))
            .unwrap();
        let args=program.pointer("/function/parameters/properties/steps/items/properties/tool/enum")
            .and_then(Value::as_array).unwrap();
        assert!(args.iter().any(|value|value=="run_tests"));
        assert!(args.iter().any(|value|value=="shell"));
    }

    #[test]
    fn safe_test_runner_is_listed_without_unrestricted_shell(){
        let tools=definitions(false);
        let names=tools.iter().filter_map(|tool|
            tool.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"run_tests"));
        assert!(!names.contains(&"shell"));
        assert!(crate::acceptance::test_command("cargo test --locked"));
        assert!(crate::acceptance::test_command("python3 -m unittest discover"));
        for unsafe_command in [
            "echo cargo test",
            "cargo test && rm -rf src",
            "cargo test --help",
            "cargo test -- $(whoami)",
            "cargo test > output.txt",
            "cargo test; echo done",
        ]{
            assert!(!crate::acceptance::test_command(unsafe_command),
                "non-test command must not pass: {unsafe_command}");
        }
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
        assert!(page.iter().all(|item|!Path::new(item).is_absolute()));
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
        fs::write(&existing,"#!/bin/sh\necho old\n").await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&existing,std::fs::Permissions::from_mode(0o755)).await.unwrap();
        }
        replace_atomically(&existing,b"#!/bin/sh\necho new\n").await.unwrap();
        assert_eq!(fs::read_to_string(&existing).await.unwrap(),"#!/bin/sh\necho new\n");
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
