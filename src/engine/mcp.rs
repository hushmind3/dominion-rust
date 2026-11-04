use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const CACHE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct McpTool {
    pub server_name: String,
    pub slug: String,
    pub tool: String,
    pub description: String,
    pub schema: Value,
}

impl McpTool {
    /// Tool name exposed to the model. Prefixed so two servers cannot collide.
    pub fn qualified(&self) -> String {
        format!("mcp__{}__{}", self.slug, self.tool)
    }
}

type Cache = Mutex<HashMap<String, (Instant, Vec<McpTool>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn invalidate() {
    cache().lock().unwrap().clear();
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn enabled_servers(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("mcp")
        .into_iter()
        .filter(|record| record.get("enabled").and_then(|value| value.as_bool()) != Some(false))
        .collect()
}

/// Fetch (and cache) the tool list of every enabled MCP server.
pub async fn tools(state: &AppState) -> Vec<McpTool> {
    let mut collected = Vec::new();
    for server in enabled_servers(state) {
        let id = server
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        if let Some((at, tools)) = cache().lock().unwrap().get(&id) {
            if at.elapsed() < CACHE_TTL {
                collected.extend(tools.clone());
                continue;
            }
        }
        match list_server(state, &server).await {
            Ok(tools) => {
                cache()
                    .lock()
                    .unwrap()
                    .insert(id.clone(), (Instant::now(), tools.clone()));
                collected.extend(tools);
            }
            Err(error) => {
                state
                    .store
                    .audit("mcp.list_failed", json!({ "server": id, "error": error }));
            }
        }
    }
    collected
}

async fn list_server(state: &AppState, server: &Value) -> Result<Vec<McpTool>, String> {
    let server_name = server
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("mcp")
        .to_string();
    let server_slug = slug(&server_name);
    let transport = server
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("stdio");

    let result = if transport == "http" {
        let mut session = HttpSession::open(server, state)?;
        session.request("tools/list", json!({})).await?
    } else {
        let mut session = StdioSession::open(server, &state.config.workspace).await?;
        let result = session.request("tools/list", json!({})).await;
        drop(session);
        result?
    };

    let tools = result
        .get("tools")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(tools
        .into_iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(|value| value.as_str())?.to_string();
            Some(McpTool {
                server_name: server_name.clone(),
                slug: server_slug.clone(),
                tool: name,
                description: tool
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            })
        })
        .collect())
}

pub async fn definitions(state: &AppState) -> Vec<Value> {
    tools(state)
        .await
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.qualified(),
                    "description": format!("[MCP · {}] {}", tool.server_name, tool.description),
                    "parameters": tool.schema,
                }
            })
        })
        .collect()
}

pub fn requires_approval(state: &AppState, qualified: &str) -> bool {
    let policy = state
        .store
        .get("policy", "default")
        .unwrap_or_else(|| json!({}));
    if policy.get("autoApproveMcp").and_then(|value| value.as_bool()) == Some(true) {
        return false;
    }
    let qualified = qualified.to_string();
    let server_slug = qualified
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
        .map(|(slug, _)| slug.to_string())
        .unwrap_or_default();
    let servers = enabled_servers(state);
    servers
        .iter()
        .find(|server| {
            slug(
                server
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("mcp"),
            ) == server_slug
        })
        .and_then(|server| server.get("approval").and_then(|value| value.as_bool()))
        .unwrap_or(true)
}

/// Executes `mcp__{server}__{tool}`.
pub async fn call(state: &AppState, qualified: &str, args: &Value) -> Result<Value, String> {
    let rest = qualified
        .strip_prefix("mcp__")
        .ok_or_else(|| format!("MCP 도구 이름이 아닙니다: {qualified}"))?;
    let (server_slug, tool_name) = rest
        .split_once("__")
        .ok_or_else(|| format!("MCP 도구 이름을 해석할 수 없습니다: {qualified}"))?;

    let server = enabled_servers(state)
        .into_iter()
        .find(|server| {
            slug(
                server
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("mcp"),
            ) == server_slug
        })
        .ok_or_else(|| format!("MCP 서버를 찾을 수 없습니다: {server_slug}"))?;

    let transport = server
        .get("transport")
        .and_then(|value| value.as_str())
        .unwrap_or("stdio");
    let params = json!({ "name": tool_name, "arguments": args });

    if transport == "http" {
        let mut session = HttpSession::open(&server, state)?;
        session.request("tools/call", params).await
    } else {
        let mut session = StdioSession::open(&server, &state.config.workspace).await?;
        let result = session.request("tools/call", params).await;
        drop(session);
        result
    }
}

// ------------------------------------------------------------------ stdio

struct StdioSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl StdioSession {
    async fn open(record: &Value, workspace: &Path) -> Result<Self, String> {
        let command = record
            .get("command")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or("MCP stdio 서버에 command 가 없습니다.")?;
        let args = record
            .get("args")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut child = Command::new(command);
        child
            .args(&args)
            .current_dir(workspace)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(env) = record.get("env").and_then(|value| value.as_object()) {
            for (key, value) in env {
                if let Some(value) = value.as_str() {
                    child.env(key, value);
                }
            }
        }

        let mut child = child
            .spawn()
            .map_err(|error| format!("MCP 서버 실행 실패 ({command}): {error}"))?;
        let stdin = child.stdin.take().ok_or("MCP stdin 을 열 수 없습니다.")?;
        let stdout = child.stdout.take().ok_or("MCP stdout 을 열 수 없습니다.")?;
        drop(child.stderr.take());

        let mut session = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };
        session
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "dominion-rust", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        session.notify("notifications/initialized", json!({})).await?;
        Ok(session)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let payload = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.send(&payload).await
    }

    async fn send(&mut self, payload: &Value) -> Result<(), String> {
        self.stdin
            .write_all(format!("{payload}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        self.stdin.flush().await.map_err(|error| error.to_string())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&payload).await?;

        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(Duration::from_secs(40), self.stdout.read_line(&mut line))
                .await
                .map_err(|_| format!("MCP 응답 시간 초과: {method}"))?
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Err("MCP 서버가 응답 없이 종료되었습니다.".into());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            if value.get("id").and_then(|item| item.as_u64()) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(format!("MCP 오류: {error}"));
            }
            return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
        }
    }
}

// ------------------------------------------------------------------- http

struct HttpSession {
    client: reqwest::Client,
    url: String,
    session: Option<String>,
    token: Option<String>,
    next_id: u64,
}

impl HttpSession {
    fn open(record: &Value, state: &AppState) -> Result<Self, String> {
        let url = record
            .get("url")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or("MCP HTTP 서버에 url 이 없습니다.")?
            .to_string();
        let parsed = reqwest::Url::parse(&url).map_err(|error| error.to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("MCP URL 은 HTTP(S)여야 합니다.".into());
        }
        let id = record
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let token = state.read_secret(&format!("mcp:{id}"));
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|error| error.to_string())?,
            url,
            session: None,
            token,
            next_id: 1,
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut value = self.post(&payload).await?;

        if method == "initialize" {
            let _ = self
                .post(&json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                }))
                .await;
            return Ok(value);
        }

        if value.get("result").is_none() && value.get("error").is_none() {
            // Some servers answer with an SSE stream that ends before our id.
            value = self.post(&payload).await?;
        }
        if let Some(error) = value.get("error") {
            return Err(format!("MCP 오류: {error}"));
        }
        Ok(value.get("result").cloned().unwrap_or_else(|| json!({})))
    }

    async fn post(&mut self, payload: &Value) -> Result<Value, String> {
        let mut request = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(payload);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        if let Some(token) = &self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }

        let response = request.send().await.map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!(
                "MCP HTTP {status}: {}",
                text.chars().take(400).collect::<String>()
            ));
        }
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session = Some(session.to_string());
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = response.text().await.map_err(|error| error.to_string())?;
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        if content_type.contains("text/event-stream") {
            for block in text.split("\n\n") {
                let payload = block
                    .lines()
                    .filter(|line| line.starts_with("data:"))
                    .map(|line| line[5..].trim_start())
                    .collect::<Vec<_>>()
                    .join("\n");
                if payload.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                    if value.get("result").is_some() || value.get("error").is_some() {
                        return Ok(value);
                    }
                }
            }
            return Ok(json!({}));
        }
        serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn qualified_names_are_namespaced() {
        let tool = McpTool {
            server_name: "Echo Lab".into(),
            slug: slug("Echo Lab"),
            tool: "echo".into(),
            description: String::new(),
            schema: json!({}),
        };
        assert_eq!(slug("Echo Lab"), "echo_lab");
        assert_eq!(tool.qualified(), "mcp__echo_lab__echo");
    }

    #[test]
    fn mcp_approval_follows_policy() {
        let state = temp_state();
        state.store.put(
            "mcp",
            &json!({ "name": "Echo Lab", "transport": "stdio", "command": "node" }),
        );
        assert!(requires_approval(&state, "mcp__echo_lab__echo"));

        state.store.put("policy", &json!({ "id": "default", "autoApproveMcp": true }));
        assert!(!requires_approval(&state, "mcp__echo_lab__echo"));
    }
}
