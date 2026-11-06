use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use super::response::error;

pub async fn list_providers(State(state): State<AppState>) -> Json<Value> {
    let items = state
        .store
        .all("provider")
        .into_iter()
        .map(|provider| public_provider(&state, provider))
        .collect::<Vec<_>>();
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

    if let Some(api_key) = body.get("apiKey").and_then(|value| value.as_str()) {
        if api_key.starts_with("••") {
            // Masked placeholder sent back by the UI: keep the stored secret.
        } else if api_key.is_empty() {
            state.store.remove_secret(&format!("provider:{id}"));
        } else {
            state.save_secret(&format!("provider:{id}"), api_key);
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

pub async fn provider_models(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(provider) = state.store.get("provider", &id) else {
        return error(StatusCode::NOT_FOUND, "제공자를 찾을 수 없습니다.");
    };
    let base = provider
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let kind = provider
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("openai");
    let url = if kind == "ollama" {
        let native = base.trim_end_matches("/v1").trim_end_matches('/');
        format!("{native}/api/tags")
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
    let key = state
        .read_secret(&format!("provider:{id}"))
        .unwrap_or_default();

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
            let Some(items) = value[if kind == "ollama" { "models" } else { "data" }].as_array() else {
                return error(StatusCode::BAD_GATEWAY, "모델 목록 형식을 확인할 수 없습니다. 서버 주소와 API 방식을 확인하세요.");
            };
            let models: Vec<Value> = items.iter().filter_map(|item| {
                let id = item[if kind == "ollama" { "name" } else { "id" }].as_str()?;
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
