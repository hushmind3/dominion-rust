//! Pricing/free status evidence and provider connection probing.
use super::*;

/// Returns (free?, source). `None` means the provider did not publish enough
/// information to decide; the UI must show "확인 필요", never a fake FREE.
pub(super) fn model_free_status(
    _provider: &Value,
    kind: &str,
    base: &str,
    item: &Value,
    id: &str,
) -> (Option<bool>, &'static str) {
    // Local runtimes on their standard ports are free. Any other localhost
    // proxy may front a paid provider, so it is not automatically free.
    if kind == "ollama" || base.contains(":11434") || base.contains(":1234") {
        return (Some(true), "local-runtime");
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

pub(super) fn model_is_free(provider: &Value, kind: &str, base: &str, item: &Value, id: &str) -> bool {
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
    if provider["kind"]=="cli"{
        let Some(source)=provider["baseUrl"].as_str().and_then(|s|s.strip_prefix("cli://")) else{
            return Json(json!({"ok":false,"message":"CLI 주소 오류"})).into_response();
        };
        let valid=super::super::cli_oauth::credential_verified(source).await;
        return Json(json!({"ok":valid,"message":if valid{"CLI 계정 인증을 확인했습니다."}else{"CLI 로그인이 필요합니다."}})).into_response();
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
    let opencode_go = provider["integration"] == "opencode-go";
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
    } else if opencode_go {
        // No model generation is charged for this credential check.
        client.get("https://opencode.ai/inference/go/v1/models")
            .header("Authorization", format!("Bearer {key}"))
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

