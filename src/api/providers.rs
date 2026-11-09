use crate::{engine::codex_oauth, state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use super::response::error;

fn home_file(relative: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(relative))
}

fn codex_home_file(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home_file(".codex"))
        .map(|home| home.join(name))
}

/// Codex stores a ChatGPT OAuth session in ~/.codex/auth.json. Only the
/// presence and account are exposed to the UI; tokens never leave the vault.
fn codex_auth() -> Result<Value, String> {
    let path = codex_home_file("auth.json").ok_or("CODEX_HOME/HOME 환경변수를 찾을 수 없습니다.")?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "Codex 로그인 정보를 찾을 수 없습니다. Codex 앱에서 먼저 로그인하세요.")?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| "Codex 로그인 정보 형식을 읽을 수 없습니다.")?;
    let access = value["tokens"]["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Codex OAuth 토큰이 없습니다. Codex 앱에서 ChatGPT로 로그인하세요.")?;
    if codex_oauth::account_id(&value["tokens"]).is_none() {
        return Err("Codex 계정 ID가 없습니다. Codex 앱에서 다시 로그인하세요.".into());
    }
    let _ = access;
    Ok(value)
}

/// OpenCode keeps reusable credentials in a per-provider auth map. Plain API
/// keys can be imported; OAuth access tokens stay with the issuing client.
fn opencode_auth() -> Option<Value> {
    [
        ".local/share/opencode/auth.json",
        ".config/opencode/auth.json",
    ]
    .iter()
    .filter_map(|relative| home_file(relative))
    .find_map(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    })
}

fn opencode_api_entries(auth: &Value) -> Vec<(String, String)> {
    auth.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(provider, credential)| {
            let kind = credential["type"].as_str().unwrap_or("");
            if !matches!(kind, "api" | "wellknown") {
                return None;
            }
            let key = credential["key"].as_str().filter(|value| !value.is_empty())?;
            Some((provider.clone(), key.to_string()))
        })
        .collect()
}

fn opencode_target(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match id {
        "openai" => Some(("OpenAI", "https://api.openai.com/v1", "openai")),
        "anthropic" => Some(("Anthropic", "https://api.anthropic.com/v1", "anthropic")),
        "google" | "gemini" => Some((
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "openai",
        )),
        "openrouter" => Some(("OpenRouter", "https://openrouter.ai/api/v1", "openai")),
        "deepseek" => Some(("DeepSeek", "https://api.deepseek.com/v1", "openai")),
        "groq" => Some(("Groq", "https://api.groq.com/openai/v1", "openai")),
        "xai" => Some(("xAI", "https://api.x.ai/v1", "openai")),
        "mistral" => Some(("Mistral", "https://api.mistral.ai/v1", "openai")),
        "together" => Some(("Together", "https://api.together.xyz/v1", "openai")),
        "cerebras" => Some(("Cerebras", "https://api.cerebras.ai/v1", "openai")),
        "fireworks" => Some((
            "Fireworks",
            "https://api.fireworks.ai/inference/v1",
            "openai",
        )),
        "zai" | "z-ai" => Some(("z.ai", "https://api.z.ai/api/paas/v4", "openai")),
        "moonshotai" => Some(("Moonshot", "https://api.moonshot.cn/v1", "openai")),
        "alibaba" => Some((
            "Alibaba DashScope",
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            "openai",
        )),
        _ => None,
    }
}

fn provider_slug(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn import_opencode(state: &AppState) -> Result<Value, String> {
    let auth = opencode_auth()
        .ok_or("OpenCode 인증 파일을 찾을 수 없습니다. OpenCode에서 먼저 로그인하세요.")?;
    let entries = opencode_api_entries(&auth);
    if entries.is_empty() {
        return Err(
            "OpenCode 인증 파일에 가져올 API 키가 없습니다. OAuth 자격 증명은 다른 앱으로 복사하지 않습니다."
                .into(),
        );
    }
    let mut first: Option<Value> = None;
    let mut imported = 0usize;
    let mut skipped = Vec::new();
    for (provider, key) in entries {
        let Some((name, base_url, kind)) = opencode_target(&provider) else {
            skipped.push(provider);
            continue;
        };
        let id = format!("opencode-{}", provider_slug(&provider));
        let previous = state.store.get("provider", &id).unwrap_or(Value::Null);
        let record = json!({
            "id": id,
            "name": format!("OpenCode · {name}"),
            "baseUrl": base_url,
            "kind": kind,
            "model": previous["model"].as_str().unwrap_or(""),
            "enabled": true,
            "local": false,
            "integration": "opencode",
            "reflection": previous["reflection"].as_bool().unwrap_or(false),
            "learning": previous["reflection"].as_bool().unwrap_or(false),
            "requestsPerMinute": previous["requestsPerMinute"].as_u64().unwrap_or(0),
        });
        state.save_secret(&format!("provider:{id}"), &key);
        if let Some(saved) = state.store.put("provider", &record) {
            state.bus.record("provider", &saved);
            if first.is_none() {
                first = Some(public_provider(state, saved));
            }
            imported += 1;
        }
    }
    let mut response = first.ok_or_else(|| {
        format!(
            "가져올 수 있는 OpenCode 제공자가 없습니다: {}",
            skipped.join(", ")
        )
    })?;
    response["importedCount"] = json!(imported);
    response["skipped"] = json!(skipped);
    state.store.audit(
        "provider.saved",
        json!({"source":"opencode-auth","imported":imported}),
    );
    Ok(response)
}

fn codex_model() -> String {
    if let Some(path) = codex_home_file("config.toml") {
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("model = ") {
                    let model = value.trim().trim_matches('"');
                    if !model.is_empty() {
                        return model.to_string();
                    }
                }
            }
        }
    }
    "gpt-6.1-sol".to_string()
}

/// Public provider catalog backed by models.dev. The cache keeps the Models
/// tab usable offline and avoids hammering the upstream on every visit.
pub async fn provider_catalog(State(state): State<AppState>) -> Json<Value> {
    let cached = state.store.get("provider_catalog", "models.dev");
    if let Some(cached) = &cached {
        let fresh = cached["fetchedAt"]
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .is_some_and(|time| {
                (chrono::Utc::now() - time.with_timezone(&chrono::Utc)).num_seconds() < 86_400
            });
        if fresh {
            return Json(json!({
                "providers":cached["providers"],
                "cached":true
            }));
        }
    }
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(client) => client,
        Err(error) => return Json(json!({"providers":[],"error":error.to_string()})),
    };
    let fetched = async {
        let response = client
            .get("https://models.dev/api.json")
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        response
            .json::<Value>()
            .await
            .map_err(|_| "카탈로그 JSON 형식을 읽을 수 없습니다.".to_string())
    }
    .await;
    let data = match fetched {
        Ok(data) => data,
        Err(error) => {
            if let Some(cached) = cached {
                return Json(json!({
                    "providers":cached["providers"],
                    "cached":true,
                    "error":format!("최신 카탈로그를 가져오지 못했습니다: {error}")
                }));
            }
            return Json(json!({"providers":[],"error":format!("공개 카탈로그를 가져오지 못했습니다: {error}")}));
        }
    };
    let mut providers = Vec::new();
    for (id, item) in data.as_object().into_iter().flatten() {
        let Some(api) = item["api"].as_str().filter(|value| !value.is_empty()) else {
            continue;
        };
        let models = item["models"].as_object().map(|models| models.len()).unwrap_or(0);
        if models == 0 {
            continue;
        }
        providers.push(json!({
            "id":id,
            "name":item["name"].as_str().unwrap_or(id),
            "api":api,
            "models":models,
            "env":item["env"],
            "doc":item["doc"],
        }));
    }
    providers.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    providers.truncate(300);
    let record = json!({
        "id":"models.dev",
        "fetchedAt":chrono::Utc::now().to_rfc3339(),
        "providers":providers
    });
    state.store.put("provider_catalog", &record);
    Json(json!({"providers":record["providers"],"cached":false}))
}

/// Refreshes every enabled provider's model catalogue and stores the
/// free/paid/unknown classification with a timestamp. Runs on demand and from
/// the background scheduler; the Models page reads the cached result.
pub async fn refresh_free_catalog(state: &AppState) -> Value {
    let lock = "free-catalog-refresh";
    if !state.begin_session(lock) {
        return state
            .store
            .get("free_catalog", "catalog")
            .unwrap_or_else(|| json!({"id":"catalog","status":"running","providers":[]}));
    }
    let _guard = state.session_guard(lock);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(error) => return json!({"id":"catalog","status":"error","error":error.to_string()}),
    };
    let port = state.config.port;
    let mut providers = Vec::new();
    let (mut free, mut paid, mut unknown) = (0u64, 0u64, 0u64);
    for provider in state.store.all("provider") {
        if provider["enabled"].as_bool() == Some(false) {
            continue;
        }
        let Some(id) = provider["id"].as_str() else {
            continue;
        };
        let name = provider["name"].as_str().unwrap_or(id);
        let url = format!("http://127.0.0.1:{port}/api/providers/{id}/models");
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let models = response.json::<Vec<Value>>().await.unwrap_or_default();
                let mut rows = Vec::new();
                for model in models {
                    match model["freeStatus"].as_str() {
                        Some("free") => free += 1,
                        Some("paid") => paid += 1,
                        _ => unknown += 1,
                    }
                    rows.push(json!({
                        "id":model["id"],
                        "free":model["free"],
                        "freeStatus":model["freeStatus"],
                        "freeSource":model["freeSource"],
                    }));
                }
                providers.push(json!({
                    "providerId":id,"providerName":name,"reachable":true,
                    "models":rows
                }));
            }
            Ok(response) => {
                providers.push(json!({
                    "providerId":id,"providerName":name,"reachable":false,
                    "error":format!("HTTP {}",response.status()),"models":[]
                }));
            }
            Err(error) => {
                providers.push(json!({
                    "providerId":id,"providerName":name,"reachable":false,
                    "error":error.to_string(),"models":[]
                }));
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let record = json!({
        "id":"catalog",
        "status":"ready",
        "updatedAt":chrono::Utc::now().to_rfc3339(),
        "providers":providers,
        "counts":{"free":free,"paid":paid,"unknown":unknown},
    });
    state.store.put("free_catalog", &record);
    record
}

pub async fn free_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(
        state
            .store
            .get("free_catalog", "catalog")
            .unwrap_or_else(|| {
                json!({
                    "id":"catalog","status":"empty","providers":[],
                    "counts":{"free":0,"paid":0,"unknown":0}
                })
            }),
    )
}

pub async fn free_catalog_refresh(State(state): State<AppState>) -> Json<Value> {
    Json(refresh_free_catalog(&state).await)
}

pub async fn cli_credentials(State(state): State<AppState>) -> Json<Value> {
    let codex = codex_auth().ok();
    let connected = state.store.get("provider", "codex-oauth");
    let claude = home_file(".claude/.credentials.json").is_some_and(|path| path.exists());
    let gemini = home_file(".gemini/oauth_creds.json").is_some_and(|path| path.exists());
    let opencode_auth = opencode_auth();
    let opencode = opencode_auth.is_some();
    let opencode_importable = opencode_auth
        .as_ref()
        .map(|auth| {
            opencode_api_entries(auth)
                .into_iter()
                .filter(|(provider, _)| opencode_target(provider).is_some())
                .count()
        })
        .unwrap_or(0);
    Json(json!([
        {
            "id":"codex",
            "name":"Codex (ChatGPT OAuth)",
            "available":codex.is_some() || connected.is_some(),
            "importAvailable":codex.is_some(),
            "connected":connected.is_some(),
            "supported":true,
            "accountId":connected.as_ref().and_then(|value| value["accountId"].as_str()).or_else(||codex.as_ref().and_then(|value| value["tokens"]["account_id"].as_str())),
            "model":connected.as_ref().and_then(|value| value["model"].as_str()).unwrap_or_else(||""),
            "hint":if connected.is_some() { "DOMINION에 ChatGPT 계정이 연결되어 있습니다. 사용 가능한 모델은 실제 계정 목록에서 조회합니다." } else if codex.is_some() { "Codex 앱 로그인이 감지되었습니다. 기존 인증 가져오기를 누르면 재로그인 없이 연결됩니다." } else { "브라우저에서 ChatGPT로 로그인하거나 Codex 앱의 기존 인증을 가져오세요." }
        },
        {
            "id":"claude",
            "name":"Claude Code (OAuth)",
            "available":claude,
            "supported":false,
            "hint":if claude { "Claude Code의 OAuth 자격 증명은 다른 앱에서 재사용할 수 없습니다. 모델 관리에서 Anthropic API 키를 등록하세요." } else { "Claude Code에서 로그인하면 감지됩니다. 모델 관리에서 Anthropic API 키를 직접 등록할 수도 있습니다." }
        },
        {
            "id":"gemini",
            "name":"Gemini CLI (OAuth)",
            "available":gemini,
            "supported":false,
            "hint":if gemini { "Gemini CLI의 OAuth 자격 증명은 다른 앱에서 재사용할 수 없습니다. 모델 관리에서 Google Gemini API 키를 등록하세요." } else { "Gemini CLI에서 로그인하면 감지됩니다. 모델 관리에서 Google Gemini API 키를 직접 등록할 수도 있습니다." }
        },
        {
            "id":"opencode",
            "name":"OpenCode (OAuth/CLI)",
            "available":opencode,
            "importAvailable":opencode_importable > 0,
            "supported":opencode_importable > 0,
            "hint":if opencode_importable > 0 { format!("OpenCode 인증에서 API 키 {opencode_importable}개를 가져올 수 있습니다. OAuth 자격 증명은 복사하지 않습니다.") } else if opencode { "OpenCode 인증에 가져올 API 키가 없습니다. 모델 관리에서 직접 API 키를 등록하세요.".to_string() } else { "OpenCode에서 로그인하면 감지됩니다.".to_string() }
        }
    ]))
}

pub async fn import_cli_credential(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let source = body["source"].as_str().unwrap_or("");
    let result = match source {
        "codex" => (|| {
            let auth = codex_auth()?;
            let saved = save_codex_provider(&state, &auth["tokens"], true)?;
            Ok(public_provider(&state, saved))
        })(),
        "opencode" => import_opencode(&state),
        _ => Err("지원하지 않는 OAuth/CLI 소스입니다.".to_string()),
    };
    match result {
        Ok(saved) => Json(saved).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

fn save_codex_provider(state: &AppState, tokens: &Value, shared: bool) -> Result<Value, String> {
    let id = "codex-oauth";
    let account = codex_oauth::account_id(tokens)
        .ok_or("Codex 계정 ID를 찾을 수 없습니다. 다시 로그인하세요.")?;
    codex_oauth::save_tokens(state, id, tokens, shared)?;
    let previous = state.store.get("provider", id).unwrap_or(Value::Null);
    let same_account = previous["accountId"].as_str() == Some(account.as_str());
    let selected_model = if same_account {
        previous["model"].as_str().filter(|model|!model.is_empty())
            .map(str::to_string).unwrap_or_else(codex_model)
    } else {
        codex_model()
    };
    let record = json!({
        "id":id,
        "name":"Codex (ChatGPT OAuth)",
        "baseUrl":"https://chatgpt.com/backend-api/codex",
        "kind":"codex",
        "model":selected_model,
        "accountId":account,
        "enabled":true,
        "local":false,
        "toolMode":"compat",
        "reasoningProtocol":"openai",
        "reflection":previous["reflection"].as_bool().unwrap_or(false),
        "learning":previous["learning"].as_bool().unwrap_or(false),
        "requestsPerMinute":0
    });
    let saved = state
        .store
        .put("provider", &record)
        .ok_or("Codex 연결을 저장할 수 없습니다.")?;
    state
        .store
        .audit("provider.saved", json!({"providerId":id,"source":"codex-oauth"}));
    state.bus.record("provider", &saved);
    Ok(saved)
}

fn login_page(title: &str, message: &str, ok: bool) -> String {
    let color = if ok { "#4F6F52" } else { "#9A5B48" };
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title>         <body style=\"font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#FAF7F1;color:#3A342D;display:grid;place-items:center;height:100vh;margin:0\">         <div style=\"background:#FFFDF9;border:1px solid #E7E0D5;border-radius:18px;padding:32px 40px;text-align:center;max-width:520px\">         <h1 style=\"font-size:18px;color:{color};margin:0 0 10px\">{title}</h1>         <p style=\"font-size:13px;line-height:1.7;color:#7C7469;margin:0\">{message}</p>         <p style=\"font-size:12px;color:#A79C8E;margin-top:18px\">이 창을 닫고 DOMINION으로 돌아가세요.</p></div></body>"
    )
}

async fn codex_callback_task(state: AppState, listener: TcpListener) -> Result<String, String> {
    let accepted = tokio::time::timeout(Duration::from_secs(300), listener.accept())
        .await
        .map_err(|_| "Codex 로그인 시간이 초과되었습니다.")?
        .map_err(|error| format!("Codex 콜백 연결 실패: {error}"))?;
    let (mut socket, _) = accepted;
    let mut buffer = [0u8; 16384];
    let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut buffer))
        .await
        .map_err(|_| "Codex 콜백 요청 시간이 초과되었습니다.")?
        .map_err(|error| format!("Codex 콜백 읽기 실패: {error}"))?;
    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let url = reqwest::Url::parse(&format!("http://localhost:1455{path}"))
        .map_err(|_| "Codex 콜백 주소를 읽을 수 없습니다.")?;
    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let pending = state
        .store
        .get("codex_login", "codex")
        .unwrap_or_else(|| json!({"id":"codex"}));
    let code = params.get("code").cloned().unwrap_or_default();
    let returned_state = params.get("state").cloned().unwrap_or_default();
    let result = if code.is_empty() {
        Err("Codex 로그인 콜백에 code가 없습니다.".to_string())
    } else if returned_state != pending["state"].as_str().unwrap_or("") {
        Err("Codex 로그인 state가 일치하지 않습니다. 다시 시도하세요.".to_string())
    } else {
        let client = reqwest::Client::new();
        codex_oauth::exchange_code(
            &client,
            &code,
            pending["verifier"].as_str().unwrap_or(""),
        )
        .await
        .and_then(|tokens| {
            let account = codex_oauth::account_id(&tokens)
                .ok_or("Codex 계정 ID를 찾을 수 없습니다.")?;
            save_codex_provider(&state, &tokens, false)?;
            Ok(account)
        })
    };
    match &result {
        Ok(_) => {
            let body = login_page(
                "Codex 로그인 완료",
                "ChatGPT 계정이 DOMINION에 연결되었습니다.",
                true,
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
        Err(message) => {
            let body = login_page("Codex 로그인 실패", message, false);
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    }
    let _ = socket.shutdown().await;
    result
}

pub async fn codex_login_start(State(state): State<AppState>) -> Response {
    let listener = match TcpListener::bind(("127.0.0.1", 1455)).await {
        Ok(listener) => listener,
        Err(issue) => {
            return error(
                StatusCode::CONFLICT,
                &format!("Codex 로그인 콜백 포트(1455)를 열 수 없습니다. Codex 로그인 창을 닫고 다시 시도하세요. ({issue})"),
            )
        }
    };
    let (verifier, challenge) = codex_oauth::pkce();
    let state_token = codex_oauth::new_state();
    let url = codex_oauth::authorize_url(&challenge, &state_token);
    state.store.put(
        "codex_login",
        &json!({
            "id":"codex","status":"pending","state":state_token,"verifier":verifier,
            "createdAt":chrono::Utc::now().to_rfc3339()
        }),
    );
    let task_state = state.clone();
    tokio::spawn(async move {
        let result = codex_callback_task(task_state.clone(), listener).await;
        let mut record = task_state
            .store
            .get("codex_login", "codex")
            .unwrap_or_else(|| json!({"id":"codex"}));
        match result {
            Ok(account) => {
                record["status"] = json!("success");
                record["accountId"] = json!(account);
                record["message"] = json!("Codex 로그인 완료");
            }
            Err(message) => {
                record["status"] = json!("error");
                record["message"] = json!(message);
            }
        }
        record["verifier"] = Value::Null;
        record["state"] = Value::Null;
        task_state.store.put("codex_login", &record);
    });
    Json(json!({"status":"pending","url":url})).into_response()
}

fn set_codex_login_status(state: &AppState, status: &str, message: &str, account: Option<&str>) {
    let mut record = state
        .store
        .get("codex_login", "codex")
        .unwrap_or_else(|| json!({"id":"codex"}));
    record["status"] = json!(status);
    record["message"] = json!(message);
    if let Some(account) = account {
        record["accountId"] = json!(account);
    }
    state.store.put("codex_login", &record);
}

async fn codex_device_poll(
    state: AppState,
    client: reqwest::Client,
    device_auth_id: String,
    user_code: String,
    interval: u64,
) {
    let expected = device_auth_id.clone();
    let is_current = |state: &AppState| {
        state
            .store
            .get("codex_login", "codex")
            .and_then(|record| record["deviceAuthId"].as_str().map(str::to_string))
            .as_deref()
            == Some(expected.as_str())
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(900);
    loop {
        if std::time::Instant::now() >= deadline {
            if !is_current(&state) {
                return;
            }
            set_codex_login_status(
                &state,
                "error",
                "기기 코드 로그인 시간이 초과되었습니다. 다시 시도하세요.",
                None,
            );
            return;
        }
        tokio::time::sleep(Duration::from_secs(interval.clamp(1, 30))).await;
        let response = client
            .post("https://auth.openai.com/api/accounts/deviceauth/token")
            .json(&json!({
                "device_auth_id":device_auth_id,
                "user_code":user_code,
            }))
            .timeout(Duration::from_secs(20))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(issue) => {
                set_codex_login_status(
                    &state,
                    "error",
                    &format!("기기 코드 확인 실패: {issue}"),
                    None,
                );
                return;
            }
        };
        let status = response.status();
        if status.is_success() {
            let Ok(data) = response.json::<Value>().await else {
                if !is_current(&state) {
                    return;
                }
                set_codex_login_status(
                    &state,
                    "error",
                    "기기 코드 응답 형식을 읽을 수 없습니다.",
                    None,
                );
                return;
            };
            let code = data["authorization_code"].as_str().unwrap_or("");
            let verifier = data["code_verifier"].as_str().unwrap_or("");
            if code.is_empty() || verifier.is_empty() {
                if !is_current(&state) {
                    return;
                }
                set_codex_login_status(
                    &state,
                    "error",
                    "기기 코드 응답에 교환 정보가 없습니다.",
                    None,
                );
                return;
            }
            if !is_current(&state) {
                return;
            }
            match codex_oauth::exchange_device_code(&client, code, verifier).await {
                Ok(tokens) => match save_codex_provider(&state, &tokens, false) {
                    Ok(saved) => {
                        set_codex_login_status(
                            &state,
                            "success",
                            "Codex 기기 코드 로그인 완료",
                            saved["accountId"].as_str(),
                        );
                    }
                    Err(message) => {
                        set_codex_login_status(&state, "error", &message, None);
                    }
                },
                Err(message) => {
                    set_codex_login_status(&state, "error", &message, None);
                }
            }
            return;
        }
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        if !is_current(&state) {
            return;
        }
        set_codex_login_status(
            &state,
            "error",
            &format!("기기 코드 인증 실패 (HTTP {status})."),
            None,
        );
        return;
    }
}

pub async fn codex_device_start(State(state): State<AppState>) -> Response {
    let client = reqwest::Client::new();
    let response = client
        .post("https://auth.openai.com/api/accounts/deviceauth/usercode")
        .json(&json!({"client_id":codex_oauth::CLIENT_ID}))
        .timeout(Duration::from_secs(20))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(issue) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("Codex 기기 코드 요청 실패: {issue}"),
            )
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return error(
                StatusCode::BAD_REQUEST,
                "ChatGPT 보안 설정에서 기기 코드 로그인이 꺼져 있습니다. ChatGPT 설정 → 보안 → 기기 코드 로그인을 켠 뒤 다시 시도하세요.",
            );
        }
        let text = response.text().await.unwrap_or_default();
        return error(
            StatusCode::BAD_GATEWAY,
            &format!(
                "Codex 기기 코드 요청 실패 (HTTP {status}): {}",
                text.chars().take(300).collect::<String>()
            ),
        );
    }
    let Ok(data) = response.json::<Value>().await else {
        return error(
            StatusCode::BAD_GATEWAY,
            "Codex 기기 코드 응답 형식을 읽을 수 없습니다.",
        );
    };
    let device_auth_id = data["device_auth_id"].as_str().unwrap_or("").to_string();
    let user_code = data["user_code"].as_str().unwrap_or("").to_string();
    if device_auth_id.is_empty() || user_code.is_empty() {
        return error(StatusCode::BAD_GATEWAY, "Codex 기기 코드가 비어 있습니다.");
    }
    let interval = data["interval"]
        .as_u64()
        .or_else(|| {
            data["interval"]
                .as_str()
                .and_then(|value| value.trim().parse::<u64>().ok())
        })
        .unwrap_or(5)
        .clamp(1, 30);
    let verification_url = "https://auth.openai.com/codex/device";
    state.store.put(
        "codex_login",
        &json!({
            "id":"codex","status":"device_pending","userCode":user_code,
            "verificationUrl":verification_url,"interval":interval,
            "deviceAuthId":device_auth_id,
            "createdAt":chrono::Utc::now().to_rfc3339()
        }),
    );
    let task_state = state.clone();
    let task_device = device_auth_id;
    let task_code = user_code.clone();
    tokio::spawn(async move {
        codex_device_poll(task_state, client, task_device, task_code, interval).await;
    });
    Json(json!({
        "status":"device_pending",
        "user_code":user_code,
        "verification_url":verification_url,
        "interval":interval,
    }))
    .into_response()
}

pub async fn codex_login_status(State(state): State<AppState>) -> Json<Value> {
    let record = state
        .store
        .get("codex_login", "codex")
        .unwrap_or_else(|| json!({"status":"idle"}));
    Json(json!({
        "status":record["status"],
        "message":record["message"],
        "accountId":record["accountId"],
        "userCode":record["userCode"],
        "verificationUrl":record["verificationUrl"],
        "providerId":if record["status"] == "success" { json!("codex-oauth") } else { Value::Null },
    }))
}

fn needs_account_discovery(provider: &Value) -> bool {
    provider["accountId"].as_str().unwrap_or("").trim().is_empty()
        && provider["baseUrl"].as_str().is_some_and(|url| {
            url.contains("ACCOUNT_ID") || url.contains("{accountId}")
        })
}

/// Cloudflare lets a token list every account it can see, so the owner never
/// has to hunt for the account ID by hand. Any other provider with a
/// placeholder URL is left untouched.
async fn discover_cloudflare_account(api_key: &str) -> Option<String> {
    if api_key.is_empty() {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .ok()?;
    let response = client
        .get("https://api.cloudflare.com/client/v4/accounts")
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.json().await.ok()?;
    value["result"].as_array()?.iter()
        .find_map(|account| account["id"].as_str().map(str::to_string))
}

async fn fill_missing_account(state: &AppState, provider: &Value) -> Option<String> {
    if !needs_account_discovery(provider) {
        return None;
    }
    let id = provider["id"].as_str()?;
    let key = state.read_secret(&format!("provider:{id}"))?;
    discover_cloudflare_account(&key).await
}

pub async fn list_providers(State(state): State<AppState>) -> Json<Value> {
    let mut items = Vec::new();
    for mut provider in state.store.all("provider") {
        // Lazily fill `ACCOUNT_ID` from the key the owner already saved, then
        // persist it so chat/model routes use the real URL.
        if let Some(account) = fill_missing_account(&state, &provider).await {
            provider["accountId"] = json!(account);
            if let Some(saved) = state.store.put("provider", &provider) {
                state.bus.record("provider", &saved);
            }
        }
        items.push(public_provider(&state, provider));
    }
    Json(json!(items))
}

fn public_provider(state: &AppState, mut provider: Value) -> Value {
    let id = provider
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    if let Value::Object(ref mut map) = provider {
        let reflection = map
            .get("reflection")
            .and_then(Value::as_bool)
            .or_else(|| map.get("learning").and_then(Value::as_bool))
            .unwrap_or(false);
        map.insert("reflection".into(), json!(reflection));
        map.remove("apiKey");
        map.insert(
            "hasKey".into(),
            json!(state.store.has_secret(&format!("provider:{id}"))),
        );
    }
    provider
}

pub async fn save_provider(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if let Err(message) = crate::engine::reasoning::validate(&body) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let name = body
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let base_url = body
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_string();
    if base_url.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "서버 주소를 입력하세요.",
        );
    }
    let parsed = match reqwest::Url::parse(&base_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return error(StatusCode::BAD_REQUEST, "HTTP(S) 주소만 허용됩니다."),
    };

    let name = if name.is_empty() { parsed.host_str().unwrap_or("API").to_owned() } else { name };

    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let kind = match body.get("kind").and_then(|value| value.as_str()) {
        Some("ollama") => "ollama",
        Some("anthropic") => "anthropic",
        Some("codex") => "codex",
        _ => "openai",
    };
    let existing = state.store.get("provider", &id);
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| existing.as_ref().and_then(|provider| provider["model"].as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    if model.len() > 256 {
        return error(StatusCode::BAD_REQUEST, "모델 ID가 너무 깁니다.");
    }
    // `reflection` is the public role. Older records and clients used
    // `learning`, so accept it as a compatibility alias.
    let reflection = body
        .get("reflection")
        .or_else(|| body.get("learning"))
        .and_then(|value| value.as_bool())
        .or_else(|| existing.as_ref().and_then(|provider| {
            provider["reflection"].as_bool().or_else(|| provider["learning"].as_bool())
        }))
        .unwrap_or(false);
    let review_model = body["reflectionModel"].as_str()
        .or_else(|| existing.as_ref().and_then(|provider| provider["reflectionModel"].as_str()))
        .unwrap_or(&model).trim();
    if reflection && (review_model.is_empty() || review_model.len() > 256) {
        return error(
            StatusCode::BAD_REQUEST,
            "실행 검토에 사용할 모델 ID를 입력하세요.",
        );
    }

    if body.get("accountId").and_then(Value::as_str).is_some_and(|value| value.len() > 128) {
        return error(StatusCode::BAD_REQUEST, "Account ID가 너무 깁니다.");
    }
    let mut record = body.clone();
    record["id"] = json!(id);
    record["name"] = json!(name);
    record["baseUrl"] = json!(parsed.to_string().trim_end_matches('/'));
    record["kind"] = json!(kind);
    record["reflection"] = json!(reflection);
    // Keep the legacy key in sync so existing readers keep working.
    record["learning"] = json!(reflection);
    if reflection {
        record["reflectionModel"] = json!(review_model);
    } else if let Some(fields) = record.as_object_mut() {
        fields.remove("reflectionModel");
    }
    if model.is_empty() {
        if let Value::Object(ref mut map) = record {
            map.remove("model");
        }
    } else {
        record["model"] = json!(model);
    }
    record["enabled"] = json!(body
        .get("enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(true));
    record["toolMode"] = json!(
        match body.get("toolMode").and_then(|value| value.as_str()) {
            Some("native") => "native",
            Some("compat") => "compat",
            Some("auto") => "auto",
            _ =>
                if kind == "ollama" || kind == "codex" {
                    "compat"
                } else if kind == "anthropic" {
                    "native"
                } else {
                    "auto"
                },
        }
    );
    if let Value::Object(ref mut map) = record {
        map.remove("apiKey");
        map.remove("hasKey");
    }

    let mut fresh_key: Option<String> = None;
    if let Some(api_key) = body.get("apiKey").and_then(|value| value.as_str()) {
        if api_key.starts_with("••") {
            // Masked placeholder sent back by the UI: keep the stored secret.
        } else if api_key.is_empty() {
            state.store.remove_secret(&format!("provider:{id}"));
        } else {
            state.save_secret(&format!("provider:{id}"), api_key);
            fresh_key = Some(api_key.to_string());
        }
    }
    // Save the account ID immediately when the key can reveal it, so the next
    // model list or chat does not need a second trip to the provider.
    if needs_account_discovery(&record) {
        let key = fresh_key
            .clone()
            .or_else(|| state.read_secret(&format!("provider:{id}")))
            .unwrap_or_default();
        if let Some(account) = discover_cloudflare_account(&key).await {
            record["accountId"] = json!(account);
        }
    }

    let Some(saved) = state.store.put("provider", &record) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "API 연결을 저장할 수 없습니다.");
    };
    // Only one provider can own the reflection route. Selecting a new one
    // clears both the public and legacy flags on every other provider.
    if reflection {
        for mut other in state.store.all("provider") {
            if other["id"].as_str() != Some(id.as_str())
                && (other["reflection"].as_bool() == Some(true)
                    || other["learning"].as_bool() == Some(true))
            {
                other["reflection"] = json!(false);
                other["learning"] = json!(false);
                if let Some(saved) = state.store.put("provider", &other) {
                    state.bus.record("provider", &saved);
                }
            }
        }
    }
    state
        .store
        .audit("provider.saved", json!({ "providerId": saved.get("id") }));
    state.bus.record("provider", &saved);
    Json(public_provider(&state, saved)).into_response()
}

pub async fn delete_provider(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if id == "dominion-native" {
        return error(StatusCode::CONFLICT, "실행 중인 로컬 모델은 모델 관리에서 중지하세요.");
    }
    if state.store.get("provider", &id).is_none() {
        return error(StatusCode::NOT_FOUND, "API 연결을 찾을 수 없습니다.");
    }
    if !state.store.remove("provider", &id) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "API 연결을 해제할 수 없습니다.");
    }
    state.store.remove_secret(&format!("provider:{id}"));
    state.bus.emit(json!({ "type": "record", "kind": "provider", "removed": id }));
    Json(json!({ "ok": true })).into_response()
}

/// Returns (free?, source). `None` means the provider did not publish enough
/// information to decide; the UI must show "확인 필요", never a fake FREE.
fn model_free_status(
    _provider: &Value,
    kind: &str,
    base: &str,
    item: &Value,
    id: &str,
) -> (Option<bool>, &'static str) {
    // Local runtimes on their standard ports are free. A localhost shim that
    // proxies paid providers (for example port 4399) is not automatically free.
    if kind == "ollama" || base.contains(":11434") || base.contains(":1234") {
        return (Some(true), "local-runtime");
    }
    if base.contains(":4399") {
        return (Some(false), "subscription-proxy");
    }
    if base.contains("z.ai") {
        return (
            Some(matches!(id, "glm-4.7-flash" | "glm-4.5-flash" | "glm-4.6v-flash")),
            "z.ai-free-list",
        );
    }
    if base.contains("oai.endpoints.kepler.ai.cloud.ovh.net") {
        return (Some(true), "ovh-free-endpoint");
    }
    // Groq's Free Plan has per-model rate limits. /models lists the models
    // accessible to the key, not a "free-only" catalog.
    // Source: https://console.groq.com/docs/rate-limits
    if base.contains("api.groq.com") {
        return (
            if matches!(
                id,
                "openai/gpt-oss-120b"
                    | "openai/gpt-oss-20b"
                    | "openai/gpt-oss-safeguard-20b"
                    | "qwen/qwen3.8-27b"
            ) {
                Some(true)
            } else {
                None
            },
            "groq-free-list",
        );
    }
    // Cloudflare Workers Free includes 10,000 Neurons per day shared across
    // supported models. The catalogue does not publish per-model free status,
    // so only models confirmed by the official pricing page are FREE; the
    // rest are "unknown", never a guessed FREE.
    if base.contains("api.cloudflare.com") && base.contains("/ai/") {
        if !id.starts_with("@cf/") {
            return (Some(false), "not-workers-ai");
        }
        if matches!(
            id,
            "@cf/moonshotai/kimi-k2.6"
                | "@cf/moonshotai/kimi-k2.7-code"
                | "@cf/zai-org/glm-5.2"
                | "@cf/zai-org/glm-5.3"
                | "@cf/zai-org/glm-5.3-flash"
                | "@cf/deepseek-ai/deepseek-v4-flash-0731"
                | "@cf/deepseek-ai/deepseek-v4-pro-0813"
        ) {
            return (Some(false), "cloudflare-paid-only");
        }
        if matches!(
            id,
            "@cf/zai-org/glm-4.7-flash"
                | "@cf/google/gemma-4-26b-a4b-it"
                | "@cf/nvidia/nemotron-3-120b-a12b"
                | "@cf/deepseek-ai/deepseek-v4.1-flash-1017"
        ) {
            return (Some(true), "cloudflare-confirmed-free");
        }
        return (None, "cloudflare-no-free-metadata");
    }
    if base.contains("127.0.0.1:31415") || base.contains("127.0.0.1:3001") {
        return (Some(true), "freellmapi-hub");
    }
    if item["free"].as_bool() == Some(true) || item["is_free"].as_bool() == Some(true) {
        return (Some(true), "provider-metadata");
    }
    if id.ends_with(":free") || id.contains(":free") {
        return (Some(true), "openrouter-free-suffix");
    }
    if let Some(pricing) = item.get("pricing") {
        let zero = |key: &str| {
            pricing
                .get(key)
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|text| text.parse::<f64>().ok())
                        .or_else(|| value.as_f64())
                })
                .is_some_and(|value| value == 0.0)
        };
        return (Some(zero("prompt") && zero("completion")), "provider-pricing");
    }
    (None, "no-pricing-metadata")
}

fn model_is_free(provider: &Value, kind: &str, base: &str, item: &Value, id: &str) -> bool {
    model_free_status(provider, kind, base, item, id).0 == Some(true)
}

/// Verifies a provider without saving anything. The UI can test the key the
/// owner just typed, so an invalid token is reported immediately instead of
/// silently failing later during a room conversation.
pub async fn test_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    // `new` (or any unsaved id) tests the draft in the dialog. Stored
    // providers are tested against their saved record plus the typed fields.
    let mut provider = state
        .store
        .get("provider", &id)
        .unwrap_or_else(|| json!({"id":id,"enabled":true}));
    for field in ["baseUrl", "accountId", "kind", "integration"] {
        if let Some(value) = body.get(field).and_then(Value::as_str) {
            provider[field] = json!(value);
        }
    }
    let key = body
        .get("apiKey")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.starts_with("••"))
        .map(str::to_string)
        .or_else(|| state.read_secret(&format!("provider:{id}")))
        .unwrap_or_default();
    if key.is_empty() {
        return Json(json!({"ok":false,"message":"API 키가 저장되어 있지 않습니다."})).into_response();
    }
    let key = key.trim();
    let base = crate::engine::gateway::resolved_base_url(&provider);
    if base.contains("ACCOUNT_ID") || base.contains("{accountId}") {
        if let Some(account) = discover_cloudflare_account(key).await {
            provider["accountId"] = json!(account);
        }
    }
    let base = crate::engine::gateway::resolved_base_url(&provider);
    let kind = provider["kind"].as_str().unwrap_or("openai");
    let key = if kind == "codex" {
        let client = reqwest::Client::new();
        match codex_oauth::fresh_access(&state, &client, &id, key).await {
            Ok(token) => token,
            Err(message) => return Json(json!({"ok":false,"message":message})).into_response(),
        }
    } else {
        key.to_string()
    };
    let model = body["model"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| provider["model"].as_str())
        .unwrap_or("")
        .to_string();
    let cloudflare = base.contains("/client/v4/accounts/") && base.contains("/ai/v1");
    let tested_model = if cloudflare && model.is_empty() {
        "@cf/zai-org/glm-4.7-flash".to_string()
    } else if kind == "codex" && model.is_empty() {
        codex_model()
    } else {
        model.clone()
    };
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(client) => client,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    if kind == "codex" {
        // Verify account credentials and model visibility without spending a
        // generation or calling unsupported non-streaming Codex responses.
        let account = provider["accountId"].as_str().unwrap_or("");
        match codex_models_live(&state, &client, &id, account, &key).await {
            Ok(models) => {
                let listed = models.iter().any(|entry|entry["id"].as_str()==Some(tested_model.as_str()));
                return Json(json!({
                    "ok":listed,
                    "status":200,
                    "model":tested_model,
                    "accountId":account,
                    "modelCount":models.len(),
                    "message":if listed {
                        format!("Codex OAuth 인증 성공. 계정에서 {}개 모델을 조회했고 선택된 모델도 목록에 있습니다.",models.len())
                    } else {
                        format!("Codex OAuth 인증 성공. 계정에서 {}개 모델을 조회했지만 설정된 모델 '{}'은 목록에 없습니다. 계정 모델을 다시 선택하세요.",models.len(),tested_model)
                    }
                })).into_response();
            }
            Err(message) => return Json(json!({
                "ok":false,
                "message":format!("Codex OAuth 모델 조회 실패: {message}"),
                "model":tested_model
            })).into_response(),
        }
    }
    let request = if kind == "codex" {
        let account = provider["accountId"].as_str().unwrap_or("");
        if account.is_empty() {
            return Json(json!({"ok":false,"message":"Codex 계정 ID가 없습니다. Codex를 다시 가져오세요."})).into_response();
        }
        client
            .post(format!("{base}/responses"))
            .header("Authorization", format!("Bearer {key}"))
            .header("chatgpt-account-id", account)
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "codex_cli_rs")
            .header("User-Agent", "codex_cli_rs/0.0.0")
            .json(&json!({
                "model":tested_model,
                "input":[{"role":"user","content":[{"type":"input_text","text":"ping"}]}],
                "stream":false,
                "store":false,
                "max_output_tokens":16
            }))
    } else if cloudflare {
        client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&json!({
                "model":tested_model,
                "messages":[{"role":"user","content":"ping"}],
                "max_tokens":4,
                "stream":false
            }))
    } else if kind == "ollama" {
        client
            .get(format!("{}/models", base.trim_end_matches("/v1").trim_end_matches('/')))
            .header("Authorization", format!("Bearer {key}"))
    } else if kind == "openai" && !model.is_empty() {
        client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&json!({
                "model":model,
                "messages":[{"role":"user","content":"ping"}],
                "max_tokens":4,
                "stream":false
            }))
    } else if kind == "anthropic" {
        client
            .get(format!("{base}/models"))
            .header("x-api-key", key.as_str())
            .header("anthropic-version", "2023-06-01")
    } else {
        client
            .get(format!("{base}/models"))
            .header("Authorization", format!("Bearer {key}"))
    };
    match request.send().await {
        Ok(response) if response.status().is_success() => {
            Json(json!({
                "ok":true,
                "status":response.status().as_u16(),
                "accountId":provider["accountId"],
                "model":tested_model
            })).into_response()
        }
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let detail = text.chars().take(300).collect::<String>();
            let overloaded = detail.contains("1305")
                || detail.to_ascii_lowercase().contains("temporarily overloaded");
            let cloudflare_key_shape = cloudflare && key.len() != 40 && !key.starts_with("cfk_");
            let message = if cloudflare_key_shape {
                "Cloudflare API 토큰 형식이 아닙니다. 40자 토큰 또는 cfk_ 토큰을 붙여넣으세요."
            } else if overloaded {
                "제공자가 혼잡합니다(1305). 다른 무료 모델을 선택하거나 잠시 후 다시 시도하세요."
            } else {
                match status.as_u16() {
                    401 | 403 => "키가 거부되었습니다. API 토큰 권한과 계정을 확인하세요.",
                    404 | 405 => "주소는 응답했지만 이 경로를 지원하지 않습니다. 서버 주소를 확인하세요.",
                    429 => "호출 한도에 걸렸습니다. 잠시 후 다시 시도하세요.",
                    _ => "연결 테스트에 실패했습니다.",
                }
            };
            Json(json!({
                "ok":false,
                "status":status.as_u16(),
                "keyLength":key.chars().count(),
                "model":tested_model,
                "message":message,
                "detail":detail
            })).into_response()
        }
        Err(err) => Json(json!({"ok":false,"message":format!("연결할 수 없습니다: {err}")})).into_response(),
    }
}

/// Decode the account-specific Codex catalog, never manufacture model IDs.
fn parse_codex_models(value: &Value) -> Result<Vec<Value>, String> {
    let rows = value["models"].as_array()
        .or_else(||value["data"].as_array())
        .ok_or("Codex 응답에 models 배열이 없습니다.")?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        if row["hidden"] == true
            || row["show_in_picker"] == false
            || matches!(row["visibility"].as_str(), Some("hide"|"hidden"|"none"))
        { continue; }
        let Some(id) = row["slug"].as_str()
            .or_else(||row["model"].as_str())
            .or_else(||row["id"].as_str())
            .filter(|id|!id.is_empty()) else { continue };
        if !seen.insert(id.to_string()) { continue; }
        let efforts = row["supported_reasoning_levels"].as_array()
            .or_else(||row["supported_reasoning_efforts"].as_array())
            .or_else(||row["supportedReasoningEfforts"].as_array())
            .cloned().unwrap_or_default();
        models.push(json!({
            "id":id,
            "name":row["display_name"].as_str()
                .or_else(||row["displayName"].as_str()).unwrap_or(id),
            "free":null,
            "freeStatus":"unknown",
            "freeSource":"chatgpt-subscription",
            "reasoningEfforts":efforts.iter().filter_map(|value|
                value.as_str().or_else(||value["effort"].as_str()).or_else(||value["reasoningEffort"].as_str())
            ).collect::<Vec<_>>(),
            "defaultReasoningEffort":row["default_reasoning_level"].as_str()
                .or_else(||row["default_reasoning_effort"].as_str())
                .or_else(||row["defaultReasoningEffort"].as_str()),
            "isDefault":row["is_default"].as_bool()
                .or_else(||row["isDefault"].as_bool()).unwrap_or(false),
        }));
    }
    if models.is_empty() {
        return Err("Codex 계정에서 선택 가능한 모델이 반환되지 않았습니다.".into());
    }
    Ok(models)
}

/// The same authenticated /models endpoint used by Codex's model manager.
/// Client version comes from the installed CLI or its cache, not a fake list.
async fn codex_models_live(
    state: &AppState, client: &reqwest::Client,
    provider_id: &str, account: &str, saved_token: &str,
) -> Result<Vec<Value>, String> {
    if account.is_empty() { return Err("Codex 계정 ID가 없습니다.".into()); }
    let access = codex_oauth::fresh_access(state, client, provider_id, saved_token).await?;
    if access.is_empty() { return Err("Codex OAuth 연결 토큰이 없습니다.".into()); }
    // Codex requires client_version. An absent query gives HTTP 400; using
    // only a stale installed CLI's version hides newer account models.
    // Query the full account catalog first (Hermes uses the same 99.0.0
    // compatibility value), then retry with the installed CLI's version.
    let mut versions = vec!["99.0.0".to_string()];
    let cli_version = tokio::time::timeout(Duration::from_secs(3),
        tokio::process::Command::new("codex").arg("--version").output()
    ).await.ok().and_then(Result::ok)
        .and_then(|output|String::from_utf8(output.stdout).ok())
        .and_then(|stdout|stdout.split_whitespace()
            .find(|part|part.chars().next().is_some_and(|c|c.is_ascii_digit()))
            .map(str::to_string));
    let cache_version = codex_home_file("models_cache.json")
        .and_then(|path|std::fs::read_to_string(path).ok())
        .and_then(|text|serde_json::from_str::<Value>(&text).ok())
        .and_then(|value|value["client_version"].as_str().map(str::to_string));
    if let Some(version) = cli_version.or(cache_version) {
        if !versions.contains(&version) { versions.push(version); }
    }
    versions.push("0.0.0".to_string());
    let mut failure = String::from("Codex 계정 모델을 조회하지 못했습니다.");
    for version in versions {
        let mut url = reqwest::Url::parse("https://chatgpt.com/backend-api/codex/models")
            .map_err(|e|e.to_string())?;
        url.query_pairs_mut().append_pair("client_version", &version);
        let response = match client.get(url)
            .bearer_auth(&access)
            .header("chatgpt-account-id",account)
            .header("originator","codex_cli_rs")
            .header("Accept","application/json")
            .send().await {
                Ok(response) => response,
                Err(e) => {failure = format!("Codex 모델 조회 연결 실패: {e}");break;}
            };
        if matches!(response.status().as_u16(),401|403) {
            return Err(format!("Codex 계정 인증이 거부되었습니다 (HTTP {}). 재로그인하세요.",response.status()));
        }
        if !response.status().is_success() {
            failure = format!("Codex 모델 조회 HTTP {} (client_version={version})",response.status());
            continue;
        }
        let data:Value = response.json().await
            .map_err(|_|"Codex 모델 응답 JSON 오류")?;
        match parse_codex_models(&data) {
            Ok(models) => return Ok(models),
            Err(message) => failure = message,
        }
    }
    Err(failure)
}

pub async fn provider_models(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(mut provider) = state.store.get("provider", &id) else {
        return error(StatusCode::NOT_FOUND, "제공자를 찾을 수 없습니다.");
    };
    let key = state.read_secret(&format!("provider:{id}")).unwrap_or_default();
    if needs_account_discovery(&provider) {
        if let Some(account) = discover_cloudflare_account(&key).await {
            provider["accountId"] = json!(account);
            if let Some(saved) = state.store.put("provider", &provider) {
                state.bus.record("provider", &saved);
            }
        }
    }
    let base = crate::engine::gateway::resolved_base_url(&provider);
    let kind = provider.get("kind").and_then(|value| value.as_str()).unwrap_or("openai");
    if kind == "codex" {
        // The owner's actual Codex catalog replaces the historical four-name stub.
        // Subscription models are not metered as ordinary API-key purchases.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let account = provider["accountId"].as_str().unwrap_or("");
        let cached = state.store.get("codex_model_catalog", &id)
            .filter(|row| row["accountId"].as_str() == Some(account));
        match codex_models_live(&state, &client, &id, account, &key).await {
            Ok(models) => {
                state.store.put("codex_model_catalog", &json!({
                    "id":id,"accountId":account,"models":models,
                    "fetchedAt":chrono::Utc::now().to_rfc3339()
                }));
                return Json(json!(models)).into_response();
            }
            Err(reason) => {
                if let Some(models) = cached.as_ref().and_then(|row| row["models"].as_array()) {
                    return Json(json!(models.iter().map(|model| {
                        let mut item = model.clone();
                        item["catalogStale"] = json!(true);
                        item
                    }).collect::<Vec<_>>())).into_response();
                }
                // A matching Codex CLI account may already have a local catalog.
                // Never expose the models of another logged-in account.
                if codex_auth().ok().and_then(|auth|
                    auth["tokens"]["account_id"].as_str().map(str::to_string)
                ).as_deref() == Some(account) {
                    if let Some(path) = codex_home_file("models_cache.json") {
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            if let Ok(value) = serde_json::from_str::<Value>(&contents) {
                                if let Ok(mut rows) = parse_codex_models(&value) {
                                    for row in &mut rows {
                                        row["catalogStale"] = json!(true);
                                    }
                                    return Json(json!(rows)).into_response();
                                }
                            }
                        }
                    }
                }
                return error(StatusCode::BAD_GATEWAY,
                    &format!("Codex 계정 모델 목록을 조회하지 못했습니다: {reason}. 계정 재연결 또는 Codex CLI 모델 동기화를 확인하세요."));
            }
        }
    }
    // Cloudflare's OpenAI-compatible surface has no /models route. Its native
    // model search returns the catalogue instead.
    let cloudflare = base.contains("/client/v4/accounts/") && base.contains("/ai/v1");
    let url = if kind == "ollama" {
        let native = base.trim_end_matches("/v1").trim_end_matches('/');
        format!("{native}/api/tags")
    } else if cloudflare {
        format!(
            "{}/ai/models/search?per_page=100",
            base.trim_end_matches("/ai/v1").trim_end_matches('/')
        )
    } else if kind == "anthropic" {
        format!("{base}/models")
    } else {
        format!(
            "{base}/models{}",
            if provider["integration"] == "freellmapi" {
                "?available=true"
            } else {
                ""
            }
        )
    };

    let client = reqwest::Client::new();
    let mut request = client.get(&url).timeout(Duration::from_secs(12));
    if kind == "anthropic" {
        request = request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
    } else if !key.is_empty() {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => {
            let Ok(value) = response.json::<Value>().await else {
                return error(StatusCode::BAD_GATEWAY, "서버가 모델 목록 JSON을 반환하지 않았습니다. 서버 주소를 확인하세요.");
            };
            let key_name = if kind == "ollama" {
                "models"
            } else if cloudflare {
                "result"
            } else {
                "data"
            };
            let Some(items) = value[key_name].as_array() else {
                return error(StatusCode::BAD_GATEWAY, "모델 목록 형식을 확인할 수 없습니다. 서버 주소와 API 방식을 확인하세요.");
            };
            let mut models: Vec<Value> = items.iter().filter_map(|item| {
                let id = if kind == "ollama" || cloudflare {
                    item["name"].as_str().or_else(|| item["id"].as_str())?
                } else {
                    item["id"].as_str()?
                };
                let (free, source) = model_free_status(&provider, kind, &base, item, id);
                Some(json!({
                    "id": id,
                    "name": item["display_name"].as_str().unwrap_or(id),
                    "free": free,
                    "freeStatus": match free {
                        Some(true) => "free",
                        Some(false) => "paid",
                        None => "unknown",
                    },
                    "freeSource": source,
                }))
            }).collect();
            // Z.ai's model endpoint omits the free flash models even though
            // they are callable. Add them so the picker can show free options.
            if base.contains("z.ai") {
                for id in ["glm-4.7-flash", "glm-4.5-flash", "glm-4.6v-flash"] {
                    if let Some(row) = models.iter_mut().find(|row| row["id"] == id) {
                        row["free"] = json!(true);
                        row["freeStatus"] = json!("free");
                        row["freeSource"] = json!("z.ai-free-list");
                    } else {
                        models.push(json!({
                            "id":id,"name":id,"free":true,
                            "freeStatus":"free","freeSource":"z.ai-free-list"
                        }));
                    }
                }
            }
            Json(json!(models)).into_response()
        }
        Ok(response) => {
            let status = response.status();
            let hint = match status.as_u16() {
                401 | 403 => "API 키와 모델 조회 권한을 확인하세요.",
                404 | 405 => "서버 주소를 확인하세요. 모델 목록을 제공하지 않는 API는 모델 ID를 직접 입력할 수 있습니다.",
                _ => "잠시 후 다시 조회하거나 API 서비스 상태를 확인하세요.",
            };
            error(StatusCode::BAD_GATEWAY, &format!("모델 목록 조회 실패 (HTTP {status}). {hint}"))
        }
        Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_catalog_uses_real_slugs_and_no_obsolete_stub() {
        let models = parse_codex_models(&json!({"models":[
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol",
             "visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]},
            {"slug":"gpt-6.1-terra","visibility":"list"},
            {"slug":"gpt-6-sol","visibility":"list"},
            {"slug":"gpt-6-luna","visibility":"list"},
            {"slug":"hidden-model","visibility":"hide"},
            {"slug":"gpt-6.1-sol","visibility":"list"}
        ]})).unwrap();
        assert_eq!(models.len(),4);
        assert_eq!(models[0]["id"],"gpt-6.1-sol");
        assert_eq!(models[0]["reasoningEfforts"],json!(["low","high"]));
        assert_eq!(models[0]["freeStatus"],"unknown");
        assert!(!models.iter().any(|model|model["id"]=="hidden-model"));
    }

    #[test]
    fn codex_catalog_rejects_empty_or_invented_model_list() {
        assert!(parse_codex_models(&json!({"models":[]})).is_err());
        assert!(parse_codex_models(&json!({"data":[{"model":"real-model"}]})).is_ok());
        assert!(parse_codex_models(&json!({"wrong":[]})).is_err());
    }

    #[test]
    fn codex_catalog_has_no_four_model_cap_and_preserves_all_advertised_efforts() {
        let rows = (0..9).map(|index| json!({
            "id":format!("account-model-{index}"), "displayName":format!("Account model {index}"),
            "supportedReasoningEfforts":["low",{"reasoningEffort":"xhigh"},{"effort":"max"},"ultra","future_tier"],
            "defaultReasoningEffort":"xhigh"
        })).collect::<Vec<_>>();
        let models = parse_codex_models(&json!({"data":rows})).unwrap();
        assert_eq!(models.len(), 9);
        assert_eq!(models[8]["reasoningEfforts"], json!(["low","xhigh","max","ultra","future_tier"]));
        assert_eq!(models[8]["defaultReasoningEffort"], "xhigh");
    }

    #[test]
    fn account_placeholder_is_detected_only_when_unfilled() {
        assert!(needs_account_discovery(&json!({
            "baseUrl":"https://api.cloudflare.com/client/v4/accounts/ACCOUNT_ID/ai/v1"
        })));
        assert!(!needs_account_discovery(&json!({
            "baseUrl":"https://api.cloudflare.com/client/v4/accounts/ACCOUNT_ID/ai/v1",
            "accountId":"abc123"
        })));
        assert!(!needs_account_discovery(&json!({"baseUrl":"https://api.example.com/v1"})));
    }

    #[test]
    fn groq_free_plan_does_not_mark_all_catalog_models_free() {
        let base = "https://api.groq.com/openai/v1";
        for id in [
            "openai/gpt-oss-120b",
            "openai/gpt-oss-20b",
            "openai/gpt-oss-safeguard-20b",
            "qwen/qwen3.8-27b",
        ] {
            assert!(model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
        for id in ["llama-3.1-8b-instant", "unknown-model", "whisper-large-v3"] {
            assert!(!model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
    }

    #[test]
    fn cloudflare_free_plan_excludes_models_requiring_paid_billing() {
        let base = "https://api.cloudflare.com/client/v4/accounts/abc/ai/v1";
        for id in [
            "@cf/zai-org/glm-4.7-flash",
            "@cf/google/gemma-4-26b-a4b-it",
            "@cf/nvidia/nemotron-3-120b-a12b",
            "@cf/deepseek-ai/deepseek-v4.1-flash-1017",
        ] {
            assert!(model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
        for id in [
            "@cf/moonshotai/kimi-k2.6",
            "@cf/moonshotai/kimi-k2.7-code",
            "@cf/zai-org/glm-5.2",
            "@cf/zai-org/glm-5.3",
            "@cf/zai-org/glm-5.3-flash",
            "@cf/deepseek-ai/deepseek-v4-flash-0731",
            "@cf/deepseek-ai/deepseek-v4-pro-0813",
            "unrecognized-model",
        ] {
            assert!(!model_is_free(&Value::Null, "openai", base, &Value::Null, id), "{id}");
        }
    }

    #[test]
    fn free_models_require_local_runtime_or_pricing_evidence() {
        let local = json!({"id":"p","name":"local","baseUrl":"http://127.0.0.1:11434/v1"});
        assert!(model_is_free(&local, "openai", "http://127.0.0.1:11434/v1", &Value::Null, "x"));
        let shim = json!({"id":"p","name":"shim","baseUrl":"http://127.0.0.1:4399/v1"});
        assert!(!model_is_free(&shim, "openai", "http://127.0.0.1:4399/v1", &Value::Null, "paid/model"));
        let free_api = json!({"id":"p","name":"free","integration":"freellmapi"});
        assert!(!model_is_free(&free_api, "openai", "https://api.example/v1", &Value::Null, "x"));
        let free_suffix = json!({"id":"p","name":"openrouter"});
        assert!(model_is_free(&free_suffix, "openai", "https://openrouter.ai/api/v1", &Value::Null, "meta/llama:free"));
        let zero = json!({"pricing":{"prompt":"0","completion":"0"}});
        assert!(model_is_free(&free_suffix, "openai", "https://openrouter.ai/api/v1", &zero, "meta/llama"));
        let paid = json!({"pricing":{"prompt":"0.000001","completion":"0.000002"}});
        assert!(!model_is_free(&free_suffix, "openai", "https://openrouter.ai/api/v1", &paid, "meta/llama"));
    }

    #[test]
    fn opencode_import_reads_only_plain_api_keys() {
        let auth = json!({
            "openrouter":{"type":"api","key":"sk-or-real"},
            "anthropic":{"type":"oauth","access":"oauth-token","refresh":"refresh"},
            "google":{"type":"wellknown","key":"google-key"},
            "unknown-provider":{"type":"api","key":"unknown-key"}
        });
        let mut entries = opencode_api_entries(&auth);
        entries.sort();
        assert_eq!(entries.len(), 3);
        assert!(entries.contains(&("openrouter".into(), "sk-or-real".into())));
        assert!(entries.contains(&("google".into(), "google-key".into())));
        assert!(!entries.iter().any(|(provider, _)| provider == "anthropic"));
        assert_eq!(opencode_target("openrouter").unwrap().1, "https://openrouter.ai/api/v1");
        assert_eq!(opencode_target("anthropic").unwrap().2, "anthropic");
        assert!(opencode_target("unknown-provider").is_none());
        assert_eq!(provider_slug("z-ai"), "z-ai");
    }
}
