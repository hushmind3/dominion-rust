use crate::db::Store;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Everything a tool needs in order to run. Tools are pure Rust: no Node,
/// no sidecar process.
pub struct ToolContext<'a> {
    pub workspace: &'a Path,
    pub store: &'a Store,
    pub state: &'a crate::state::AppState,
    pub session_id: &'a str,
    pub agent_id: Option<&'a str>,
}

/// OpenAI-style tool definitions handed to the model.
pub fn definitions() -> Vec<Value> {
    let object = |properties: Value, required: Vec<&str>| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    };
    let text = |description: &str| json!({ "type": "string", "description": description });

    vec![
        tool("workspace_list", "List files and folders in the workspace", object(json!({"path": text("Relative folder path")}), vec![])),
        tool("workspace_read", "Read a UTF-8 file from the workspace", object(json!({"path": text("Relative file path")}), vec!["path"])),
        tool("workspace_write", "Write a UTF-8 file inside the workspace (approval required)", object(json!({"path": text("Relative file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("file_list", "List any folder on this computer using an absolute path", object(json!({"path": text("Absolute folder path")}), vec!["path"])),
        tool("file_read", "Read any UTF-8 text file using an absolute path", object(json!({"path": text("Absolute file path")}), vec!["path"])),
        tool("file_write", "Create or overwrite any file using an absolute path (approval required)", object(json!({"path": text("Absolute file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("shell_exec", "Run a shell command on this computer and return stdout/stderr (approval required)", object(json!({"command": text("Shell command line"), "cwd": text("Optional absolute working directory")}), vec!["command"])),
        tool("terminal_exec", "Run an executable with an argument array without a shell (approval required)", object(json!({"command": text("Executable"), "args": json!({"type": "array", "items": {"type": "string"}})}), vec!["command"])),
        tool("web_search", "Search the public web for current information", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("http_fetch", "Fetch a URL and return its readable text content", object(json!({"url": text("HTTP or HTTPS URL")}), vec!["url"])),
        tool("memory_store", "Save a grounded memory for this agent. Quote the user's exact words; invented facts are rejected. Shared memories are managed by the user.", object(json!({"text": text("Fact to remember, maximum 1600 characters"), "evidenceQuote": text("Exact quote from the current user message supporting the fact"), "sourceMessageId": text("Optional earlier source message ID"), "type": json!({"type":"string","enum":["fact","preference","decision","procedure","lesson"]}), "key": text("Optional stable fact key; a new value replaces the previous value for this key"), "scope": json!({"type":"string","enum":["agent"]})}), vec!["text", "evidenceQuote"])),
        tool("memory_search", "Find saved knowledge relevant to a topic", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("skill_propose", "Draft a reusable skill for user review", object(json!({"name": text("Skill name"), "instructions": text("Reusable instructions")}), vec!["name", "instructions"])),
        tool("account_status", "List OAuth accounts connected to DOMINION", object(json!({}), vec![])),
        tool("github_api", "Call the GitHub REST API as the connected account. Write methods require approval (approval required)", object(json!({"method": json!({"type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"]}), "path": text("GitHub API path beginning with /"), "body": json!({"type": "object"})}), vec!["method", "path"])),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "workspace_write" | "file_write" | "shell_exec" | "terminal_exec" | "github_api"
    )
}

pub async fn execute(ctx: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "workspace_list" => {
            let path = resolve_workspace(ctx.workspace, string_arg(args, "path").unwrap_or("."))?;
            list_dir(&path).await
        }
        "workspace_read" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            read_text(&path).await
        }
        "workspace_write" => {
            let path = resolve_workspace(ctx.workspace, &required_arg(args, "path")?)?;
            write_text(&path, &required_arg(args, "content")?).await
        }
        "file_list" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            list_dir(&path).await
        }
        "file_read" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            read_text(&path).await
        }
        "file_write" => {
            let path = PathBuf::from(required_arg(args, "path")?);
            write_text(&path, &required_arg(args, "content")?).await
        }
        "shell_exec" => {
            let command = required_arg(args, "command")?;
            let cwd = match string_arg(args, "cwd") {
                Some(dir) => PathBuf::from(dir),
                None => ctx.workspace.to_path_buf(),
            };
            shell(&command, &cwd).await
        }
        "terminal_exec" => {
            let command = required_arg(args, "command")?;
            let argv: Vec<String> = args
                .get("args")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            executable(&command, &argv, ctx.workspace).await
        }
        "web_search" => {
            let query = required_arg(args, "query")?;
            web_search(&query).await
        }
        "http_fetch" => {
            let url = required_arg(args, "url")?;
            http_fetch(&url).await
        }
        "memory_store" => {
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            crate::engine::memory::store_tool(ctx.store, &scope, args)
        }
        "memory_search" => {
            let query = required_arg(args, "query")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            Ok(json!({
                "results": crate::engine::memory::retrieve(ctx.store, &scope, &query, 10)
            }))
        }
        "skill_propose" => {
            let name = required_arg(args, "name")?;
            let instructions = required_arg(args, "instructions")?;
            let scope = crate::engine::memory::MemoryScope::for_session(
                ctx.store,
                ctx.session_id,
                ctx.agent_id,
            );
            let record = ctx.store.put(
                "skill",
                &json!({
                    "name": name,
                    "instructions": instructions,
                    "description": instructions.chars().take(160).collect::<String>(),
                    "enabled": false,
                    "status": "draft",
                    "source": "agent",
                    "scope": "agent",
                    "sessionId": ctx.session_id,
                    "agentId": ctx.agent_id,
                    "partitionKey": scope.partition_key,
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            Ok(json!({ "proposed": true, "skill": record }))
        }
        "account_status" => Ok(json!({
            "providers": crate::engine::oauth::status(ctx.state),
        })),
        "github_api" => github_api(ctx, args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}

async fn github_api(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let token = crate::engine::oauth::access_token(ctx.state, "github")
        .ok_or("GitHub 계정이 연결되지 않았습니다. 설정 > 계정 연동에서 연결하세요.")?;
    let method = string_arg(args, "method").unwrap_or("GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("지원하지 않는 HTTP 메서드입니다.".into());
    }
    let path = required_arg(args, "path")?;
    if !path.starts_with('/') || path.contains("..") {
        return Err("GitHub API 경로는 / 로 시작해야 합니다.".into());
    }
    let method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| error.to_string())?;
    let mut request = reqwest::Client::new()
        .request(method, format!("https://api.github.com{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(30));
    if let Some(body) = args.get("body").filter(|value| !value.is_null()) {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
    Ok(json!({
        "status": status,
        "body": parsed.to_string().chars().take(20_000).collect::<String>(),
    }))
}

fn required_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("Missing argument: {key}"))
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|value| value.as_str())
}

/// Reject `..` traversal and symlink escapes for workspace-scoped tools.
fn resolve_workspace(workspace: &Path, relative: &str) -> Result<PathBuf, String> {
    let base = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let candidate = Path::new(relative);
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("Path escapes the workspace".into());
    }
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };
    let resolved = joined.canonicalize().unwrap_or(joined);
    if !resolved.starts_with(&base) {
        return Err("Path escapes the workspace".into());
    }
    Ok(resolved)
}

async fn list_dir(path: &Path) -> Result<Value, String> {
    let mut entries = Vec::new();
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    while let Ok(Some(entry)) = reader.next_entry().await {
        let metadata = entry.metadata().await.ok();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "type": if metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) { "dir" } else { "file" },
            "size": metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        }));
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Ok(json!({ "path": path.to_string_lossy(), "entries": entries }))
}

async fn read_text(path: &Path) -> Result<Value, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > 1_500_000 {
        return Err("File is larger than 1.5 MB".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy(), "content": text }))
}

async fn write_text(path: &Path, content: &str) -> Result<Value, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes": content.len(),
        "written": true
    }))
}

/// A tool command gets a private Unix process group. Killing only the shell
/// leaves its children running after Stop; the guard covers cancellation,
/// timeout and I/O errors and is disarmed only after a completed command.
#[cfg(unix)]
struct ToolProcessGroup {
    id: Option<i32>,
}
#[cfg(unix)]
impl ToolProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self {
            id: pid
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1),
        }
    }
    fn disarm(&mut self) {
        self.id = None;
    }
}
#[cfg(unix)]
impl Drop for ToolProcessGroup {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // This is the exact child PID created with process_group(0).
            // A negative PID targets that private group, never our own group;
            // zero and special global process IDs are excluded by new().
            // SIGKILL is 9 on the supported macOS/Linux Unix hosts.
            unsafe {
                kill(-id, 9);
            }
        }
    }
}

async fn read_command_output(
    mut stream: impl tokio::io::AsyncRead + Unpin,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    // Keep enough UTF-8 bytes for 20,001 characters so output_payload can
    // preserve its existing 20,000-character truncation indicator. Continue
    // draining after this cap so a chatty child cannot fill a pipe or memory.
    const MAX_RETAINED_BYTES: usize = 4 * 20_001;
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(kept);
        }
        let retain = count.min(MAX_RETAINED_BYTES.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..retain]);
    }
}

pub(crate) async fn run_command(
    command: &mut tokio::process::Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let mut group = ToolProcessGroup::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or("Command stdout is not available")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Command stderr is not available")?;
    let outcome = tokio::time::timeout(limit, async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_command_output(stdout),
            read_command_output(stderr)
        )?;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match outcome {
        Ok(Ok(output)) => {
            #[cfg(unix)]
            group.disarm();
            Ok(output)
        }
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(if limit.subsec_nanos() == 0 {
            format!("Command timed out after {}s", limit.as_secs())
        } else {
            format!("Command timed out after {}ms", limit.as_millis())
        }),
    }
}

async fn shell(command: &str, cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    #[cfg(windows)]
    let mut child = {
        let mut child = Command::new("cmd");
        child.args(["/C", command]);
        child
    };
    #[cfg(not(windows))]
    let mut child = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut child = Command::new(shell);
        child.args(["-lc", command]);
        child
    };
    child.current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

async fn executable(command: &str, args: &[String], cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    if !command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:".contains(c))
    {
        return Err("Invalid executable".into());
    }
    let mut child = Command::new(command);
    child.args(args).current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

fn output_payload(output: &std::process::Output) -> Value {
    let clip = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes).to_string();
        if text.chars().count() > 20_000 {
            text.chars().take(20_000).collect::<String>() + "\n… (truncated)"
        } else {
            text
        }
    };
    json!({
        "exitCode": output.status.code(),
        "stdout": clip(&output.stdout),
        "stderr": clip(&output.stderr),
    })
}

async fn http_fetch(url: &str) -> Result<Value, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP(S) URLs are allowed".into());
    }
    let response = reqwest::Client::new()
        .get(parsed)
        .timeout(Duration::from_secs(30))
        .header("User-Agent", "DOMINION/0.2 (+rust)")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.map_err(|error| error.to_string())?;
    let text = if content_type.contains("html") {
        strip_html(&body)
    } else {
        body
    };
    Ok(json!({
        "status": status,
        "contentType": content_type,
        "text": text.chars().take(60_000).collect::<String>(),
    }))
}

async fn web_search(query: &str) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .timeout(Duration::from_secs(25))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let html = response.text().await.map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for chunk in html.split("result__a").skip(1).take(10) {
        let Some(href_start) = chunk.find("href=\"") else {
            continue;
        };
        let rest = &chunk[href_start + 6..];
        let Some(href_end) = rest.find('"') else {
            continue;
        };
        let href = decode_ddg(&rest[..href_end]);
        let title = rest[href_end..]
            .find('>')
            .map(|start| {
                let tail = &rest[href_end + start + 1..];
                let end = tail.find("</a>").unwrap_or(tail.len().min(200));
                strip_html(&tail[..end]).trim().to_string()
            })
            .unwrap_or_default();
        if !href.is_empty() && !title.is_empty() {
            results.push(json!({ "title": title, "url": href }));
        }
    }
    Ok(json!({ "query": query, "results": results }))
}

fn decode_ddg(raw: &str) -> String {
    let href = raw
        .strip_prefix("//")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or_else(|| raw.to_string());
    if let Some(index) = href.find("uddg=") {
        let value = &href[index + 5..];
        let value = value.split('&').next().unwrap_or(value);
        return percent_decode(value);
    }
    href
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("dominion-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(resolve_workspace(&dir, "sub").is_ok());
        assert!(resolve_workspace(&dir, "../etc/passwd").is_err());
        assert!(resolve_workspace(&dir, "/etc/passwd").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tool_definitions_cover_the_engine() {
        let names = definitions()
            .into_iter()
            .filter_map(|definition| definition["function"]["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        for expected in [
            "workspace_read",
            "workspace_write",
            "shell_exec",
            "memory_store",
            "memory_search",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} missing"
            );
        }
        assert!(requires_approval("shell_exec"));
        assert!(!requires_approval("workspace_read"));
    }
    #[cfg(unix)]
    async fn wait_for_marker(path: &Path) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "test command did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(unix)]
    fn delayed_child_command(dir: &Path) -> tokio::process::Command {
        // The parent waits while an ordinary child will mutate the temp
        // workspace later. No daemon, user process or external service is used.
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args([
                "-c",
                "(sleep 0.6; printf leaked > escaped.txt) & printf ready > ready.txt; wait",
            ])
            .current_dir(dir);
        command
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.clone();
        let task = tokio::spawn(async move {
            let mut command = delayed_child_command(&cwd);
            run_command(&mut command, Duration::from_secs(10)).await
        });
        wait_for_marker(&dir.join("ready.txt")).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after Stop"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut command = delayed_child_command(&dir);
        let result = run_command(&mut command, Duration::from_millis(150)).await;
        assert!(result.unwrap_err().contains("timed out"));
        assert!(dir.join("ready.txt").exists());
        tokio::time::sleep(Duration::from_millis(650)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after timeout"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn tool_command_preserves_output_exit_status_and_bounded_capture() {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "printf normal; printf warning >&2; exit 7"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        let payload = output_payload(&output);
        assert_eq!(payload["exitCode"], 7);
        assert_eq!(payload["stdout"], "normal");
        assert_eq!(payload["stderr"], "warning");
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "head -c 250000 /dev/zero"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.len() <= 4 * 20_001);
        assert!(output_payload(&output)["stdout"]
            .as_str()
            .unwrap()
            .contains("truncated"));
    }
}
