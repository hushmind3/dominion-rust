//! Provider-scoped live model catalogs and Codex response mapping.
use super::*;

/// Decode the account-specific Codex catalog, never manufacture model IDs.
pub(super) fn parse_codex_models(value: &Value) -> Result<Vec<Value>, String> {
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
pub(super) async fn codex_models_live(
    state: &AppState, client: &reqwest::Client,
    provider_id: &str, account: &str, saved_token: &str,
) -> Result<Vec<Value>, String> {
    if account.is_empty() { return Err("Codex 계정 ID가 없습니다.".into()); }
    let access = codex_oauth::fresh_access(state, client, provider_id, saved_token).await?;
    if access.is_empty() { return Err("Codex OAuth 연결 토큰이 없습니다.".into()); }
    // Codex requires client_version. An absent query gives HTTP 400; using
    // only a stale installed CLI's version hides newer account models.
    // Query the full account catalog first with a generic compatibility
    // version, then retry with the installed CLI's version.
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
    if provider["kind"]=="cli"{
        let Some(source)=provider["baseUrl"].as_str().and_then(|s|s.strip_prefix("cli://")) else{
            return error(StatusCode::BAD_REQUEST,"CLI 제공자 주소 오류");
        };
        return match crate::engine::cli_model::models(source).await{
            Ok(models)=>Json(json!(models)).into_response(),
            Err(message)=>error(StatusCode::BAD_GATEWAY,&message)
        };
    }
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
    let url = if provider["integration"] == "opencode-go" {
        "https://opencode.ai/inference/go/v1/models".to_string()
    } else if kind == "ollama" {
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
            let Some(items) = value[key_name].as_array()
                .or_else(|| if provider["integration"] == "opencode-go" { value["models"].as_array() } else { None }) else {
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

