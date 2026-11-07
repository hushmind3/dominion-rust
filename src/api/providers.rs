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

/// Codex stores a ChatGPT OAuth session in ~/.codex/auth.json. Only the
/// presence and account are exposed to the UI; tokens never leave the vault.
fn codex_auth() -> Result<Value, String> {
    let path = home_file(".codex/auth.json").ok_or("HOME 환경변수를 찾을 수 없습니다.")?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "Codex 로그인 정보를 찾을 수 없습니다. Codex 앱에서 먼저 로그인하세요.")?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| "Codex 로그인 정보 형식을 읽을 수 없습니다.")?;
    let access = value["tokens"]["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Codex OAuth 토큰이 없습니다. Codex 앱에서 ChatGPT로 로그인하세요.")?;
    if value["tokens"]["account_id"].as_str().is_none() {
        return Err("Codex 계정 ID가 없습니다. Codex 앱에서 다시 로그인하세요.".into());
    }
    let _ = access;
    Ok(value)
}

fn codex_model() -> String {
    if let Some(path) = home_file(".codex/config.toml") {
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

pub async fn cli_credentials() -> Json<Value> {
    let codex = codex_auth().ok();
    let claude = home_file(".claude/.credentials.json").is_some_and(|path| path.exists());
    let gemini = home_file(".gemini/oauth_creds.json").is_some_and(|path| path.exists());
    let opencode = [
        ".local/share/opencode/auth.json",
        ".config/opencode/auth.json",
    ]
    .iter()
    .any(|path| home_file(path).is_some_and(|path| path.exists()));
    Json(json!([
        {
            "id":"codex",
            "name":"Codex (ChatGPT OAuth)",
            "available":codex.is_some(),
            "supported":true,
            "accountId":codex.as_ref().and_then(|value| value["tokens"]["account_id"].as_str()),
            "model":codex_model(),
            "hint":if codex.is_some() { "Codex 앱의 ChatGPT 로그인을 그대로 사용합니다." } else { "Codex 앱에서 ChatGPT로 로그인한 뒤 다시 시도하세요." }
        },
        {
            "id":"claude",
            "name":"Claude Code (OAuth)",
            "available":claude,
            "supported":false,
            "hint":if claude { "로그인 파일은 찾았지만 아직 가져오기를 지원하지 않습니다." } else { "Claude Code에서 로그인하면 감지됩니다." }
        },
        {
            "id":"gemini",
            "name":"Gemini CLI (OAuth)",
            "available":gemini,
            "supported":false,
            "hint":if gemini { "로그인 파일은 찾았지만 아직 가져오기를 지원하지 않습니다." } else { "Gemini CLI에서 로그인하면 감지됩니다." }
        },
        {
            "id":"opencode",
            "name":"OpenCode (OAuth/CLI)",
            "available":opencode,
            "supported":false,
            "hint":if opencode { "OpenCode 인증 파일은 찾았지만 아직 가져오기를 지원하지 않습니다." } else { "OpenCode에서 로그인하면 감지됩니다." }
        }
    ]))
}

pub async fn import_cli_credential(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let source = body["source"].as_str().unwrap_or("");
    if source != "codex" {
        return error(StatusCode::BAD_REQUEST, "지원하지 않는 OAuth/CLI 소스입니다.");
    }
    let auth = match codex_auth() {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    match save_codex_provider(&state, &auth["tokens"], true) {
        Ok(saved) => Json(public_provider(&state, saved)).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

fn save_codex_provider(state: &AppState, tokens: &Value, shared: bool) -> Result<Value, String> {
    let id = "codex-oauth";
    let account = codex_oauth::account_id(tokens)
        .ok_or("Codex 계정 ID를 찾을 수 없습니다. 다시 로그인하세요.")?;
    codex_oauth::save_tokens(state, id, tokens, shared)?;
    let previous = state.store.get("provider", id).unwrap_or(Value::Null);
    let record = json!({
        "id":id,
        "name":"Codex (ChatGPT OAuth)",
        "baseUrl":"https://chatgpt.com/backend-api/codex",
        "kind":"codex",
        "model":codex_model(),
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

fn model_is_free(_provider: &Value, kind: &str, base: &str, item: &Value, id: &str) -> bool {
    // Local runtimes on their standard ports are free. A localhost shim that
    // proxies paid providers (for example port 4399) is not automatically free.
    if kind == "ollama" || base.contains(":11434") || base.contains(":1234") {
        return true;
    }
    // Z.ai publishes three models with free input and output.
    if base.contains("z.ai") {
        return matches!(id, "glm-4.7-flash" | "glm-4.5-flash" | "glm-4.6v-flash");
    }
    // Workers AI draws from the free daily Neuron allowance, and Groq's
    // on-demand tier is free within its rate limits.
    if base.contains("api.cloudflare.com") || base.contains("api.groq.com") {
        return true;
    }
    if id.ends_with(":free") || id.contains(":free") {
        return true;
    }
    if item["free"].as_bool() == Some(true) || item["is_free"].as_bool() == Some(true) {
        return true;
    }
    let Some(pricing) = item.get("pricing") else {
        return false;
    };
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
    zero("prompt") && zero("completion")
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
        codex_oauth::fresh_access(&state, &client, &id, key)
            .await
            .unwrap_or_else(|_| key.to_string())
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
        let current = codex_model();
        let mut seen = std::collections::HashSet::new();
        let models: Vec<Value> = [
            current.clone(),
            "gpt-5.1-codex".to_string(),
            "gpt-5.1-codex-mini".to_string(),
            "gpt-5.1".to_string(),
        ]
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .map(|id| json!({"id":id,"name":id,"free":false}))
        .collect();
        return Json(json!(models)).into_response();
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
                Some(json!({
                    "id": id,
                    "name": item["display_name"].as_str().unwrap_or(id),
                    "free": model_is_free(&provider, kind, &base, item, id),
                }))
            }).collect();
            // Z.ai's model endpoint omits the free flash models even though
            // they are callable. Add them so the picker can show free options.
            if base.contains("z.ai") {
                for id in ["glm-4.7-flash", "glm-4.5-flash", "glm-4.6v-flash"] {
                    if let Some(row) = models.iter_mut().find(|row| row["id"] == id) {
                        row["free"] = json!(true);
                    } else {
                        models.push(json!({"id":id,"name":id,"free":true}));
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
}
