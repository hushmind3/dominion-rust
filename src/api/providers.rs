use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use super::response::error;

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
                if kind == "ollama" {
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
    let model = body["model"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| provider["model"].as_str())
        .unwrap_or("")
        .to_string();
    let cloudflare = base.contains("/client/v4/accounts/") && base.contains("/ai/v1");
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(client) => client,
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let request = if cloudflare {
        let cloudflare_model = if model.is_empty() {
            "@cf/meta/llama-3.1-8b-instruct".to_string()
        } else {
            model.clone()
        };
        client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&json!({
                "model":cloudflare_model,
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
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
    } else {
        client
            .get(format!("{base}/models"))
            .header("Authorization", format!("Bearer {key}"))
    };
    match request.send().await {
        Ok(response) if response.status().is_success() => {
            Json(json!({"ok":true,"status":response.status().as_u16(),"accountId":provider["accountId"]})).into_response()
        }
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let detail = text.chars().take(300).collect::<String>();
            let message = match status.as_u16() {
                401 | 403 => "키가 거부되었습니다. API 토큰 권한과 계정을 확인하세요.",
                404 | 405 => "주소는 응답했지만 이 경로를 지원하지 않습니다. 서버 주소를 확인하세요.",
                429 => "호출 한도에 걸렸습니다. 잠시 후 다시 시도하세요.",
                _ => "연결 테스트에 실패했습니다.",
            };
            Json(json!({"ok":false,"status":status.as_u16(),"message":message,"detail":detail})).into_response()
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
            let models: Vec<Value> = items.iter().filter_map(|item| {
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
