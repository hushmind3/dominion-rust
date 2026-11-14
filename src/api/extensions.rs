use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,Query,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::collections::HashMap;
use std::time::Duration;
use super::response::error;

pub async fn list_tools(State(state): State<AppState>) -> Json<Value> {
    let mut items = engine::tools::definitions()
        .into_iter()
        .filter_map(|definition| {
            let function = definition.get("function")?;
            let name = function.get("name")?.as_str()?.to_string();
            Some(json!({
                "name": name,
                "description": function.get("description"),
                "parameters": function.get("parameters"),
                "approval": engine::tools::requires_approval(&name),
                "source": "builtin",
            }))
        })
        .collect::<Vec<_>>();

    for tool in engine::mcp::tools(&state).await {
        let qualified = tool.qualified();
        items.push(json!({
            "name": qualified,
            "description": format!("[MCP · {}] {}", tool.server_name, tool.description),
            "parameters": tool.schema,
            "approval": engine::mcp::requires_approval(&state, &qualified),
            "source": "mcp",
            "server": tool.server_name,
        }));
    }
    Json(json!(items))
}

pub async fn mcp_presets() -> Json<Value> {
    // Filesystem, git, memory and shell are native Rust tools. Do not push
    // third-party npm MCP servers for capabilities the engine already owns.
    Json(json!([]))
}

pub async fn mcp_discover(State(state): State<AppState>) -> Json<Value> {
    let items = engine::mcp::tools(&state)
        .await
        .into_iter()
        .map(|tool| {
            let qualified = tool.qualified();
            json!({
                "server": tool.server_name,
                "name": qualified,
                "tool": tool.tool,
                "description": tool.description,
                "parameters": tool.schema,
                "approval": engine::mcp::requires_approval(&state, &qualified),
            })
        })
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn export_skill(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("skill", &id) {
        Some(skill) => Json(skill).into_response(),
        None => error(StatusCode::NOT_FOUND, "스킬을 찾을 수 없습니다."),
    }
}

/// Installed, enabled/disabled skills usable by agents; catalogue entries
/// are only install candidates and must not be promised as executable skills.
pub async fn installed_skills(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("skill").into_iter().map(|item|json!({
        "id":item["id"],"name":item["name"],"description":item["description"],
        "enabled":item["enabled"]!=false,"status":item["status"],
    })).collect::<Vec<_>>()))
}

pub async fn skill_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(engine::skills::catalog(&state))
}

pub async fn skill_catalog_meta(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "roots": engine::skills::roots(&state).into_iter().map(|(source, path)| json!({
            "source": source,
            "path": path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    }))
}

pub async fn import_skill(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match engine::skills::import(&state, id) {
        Ok(record) => Json(record).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn import_skill_repo(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let repo = body
        .get("repo")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match engine::skills::import_repo(&state, repo) {
        Ok(result) => Json(result).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn mcp_catalog(Query(query): Query<HashMap<String, String>>) -> Response {
    let mut endpoint =
        match reqwest::Url::parse("https://registry.modelcontextprotocol.io/v0.1/servers") {
            Ok(url) => url,
            Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, &message.to_string()),
        };
    let search = query.get("q").cloned().unwrap_or_default();
    if search.len() >= 3 {
        endpoint.query_pairs_mut().append_pair("search", &search);
    }
    endpoint
        .query_pairs_mut()
        .append_pair("version", "latest")
        .append_pair("limit", "60");

    let response = reqwest::Client::new()
        .get(endpoint)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("MCP registry HTTP {}", response.status()),
            )
        }
        Err(message) => return error(StatusCode::BAD_GATEWAY, &message.to_string()),
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_GATEWAY, &message.to_string()),
    };
    let mut seen = std::collections::HashSet::new();
    let items = value
        .get("servers")
        .and_then(|value| value.as_array())
        .map(|servers| {
            servers
                .iter()
                .filter_map(|entry| {
                    let server = entry.get("server").unwrap_or(entry);
                    let name = server.get("name").and_then(|value| value.as_str())?;
                    if !seen.insert(name.to_string()) {
                        return None;
                    }
                    let packages = server
                        .get("packages")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items.iter().find(|package| {
                                package
                                    .get("registryType")
                                    .or_else(|| package.get("registry_type"))
                                    .and_then(|value| value.as_str())
                                    == Some("npm")
                            })
                        })
                        .flatten();
                    let remote = server
                        .get("remotes")
                        .and_then(|value| value.as_array())
                        .and_then(|items| {
                            items.iter().find(|remote| {
                                remote
                                    .get("url")
                                    .and_then(|value| value.as_str())
                                    .map(|url| url.starts_with("https://"))
                                    .unwrap_or(false)
                            })
                        });
                    Some(json!({
                        "name": name,
                        "description": server.get("description"),
                        "packageName": packages.and_then(|package| package.get("identifier")),
                        "url": remote.and_then(|remote| remote.get("url")),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Json(json!(items)).into_response()
}
