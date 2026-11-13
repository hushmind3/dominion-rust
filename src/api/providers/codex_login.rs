//! Codex login callbacks, device flow and session status.
use super::{
    codex_oauth,
    AppState,
    State,
    StatusCode,
    Response,
    Json,
    json,
    Value,
    Duration,
    TcpListener,
    error,
    codex_model,
    IntoResponse,
    AsyncReadExt,
    AsyncWriteExt,
};

pub(super) fn save_codex_provider(state: &AppState, tokens: &Value, shared: bool) -> Result<Value, String> {
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

pub(super) fn login_page(title: &str, message: &str, ok: bool) -> String {
    let color = if ok { "#4F6F52" } else { "#9A5B48" };
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title>         <body style=\"font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#FAF7F1;color:#3A342D;display:grid;place-items:center;height:100vh;margin:0\">         <div style=\"background:#FFFDF9;border:1px solid #E7E0D5;border-radius:18px;padding:32px 40px;text-align:center;max-width:520px\">         <h1 style=\"font-size:18px;color:{color};margin:0 0 10px\">{title}</h1>         <p style=\"font-size:13px;line-height:1.7;color:#7C7469;margin:0\">{message}</p>         <p style=\"font-size:12px;color:#A79C8E;margin-top:18px\">이 창을 닫고 DOMINION으로 돌아가세요.</p></div></body>"
    )
}

pub(super) async fn codex_callback_task(state: AppState, listener: TcpListener) -> Result<String, String> {
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

pub(super) fn set_codex_login_status(state: &AppState, status: &str, message: &str, account: Option<&str>) {
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

pub(super) async fn codex_device_poll(
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

