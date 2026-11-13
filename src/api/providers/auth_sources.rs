//! Provider credential discovery and local CLI imports.
use super::*;

pub(super) fn home_file(relative: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(relative))
}

pub(super) fn codex_home_file(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home_file(".codex"))
        .map(|home| home.join(name))
}

/// Codex stores a ChatGPT OAuth session in ~/.codex/auth.json. Only the
/// presence and account are exposed to the UI; tokens never leave the vault.
pub(super) fn codex_auth() -> Result<Value, String> {
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
pub(super) fn opencode_auth() -> Option<Value> {
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


pub(super) fn opencode_go_key_in_auth(value: &Value) -> Option<&str> {
    ["opencode-go", "opencode_go"].iter().find_map(|name| {
        let entry = value.get(*name)?;
        if !matches!(entry["type"].as_str(), Some("api" | "wellknown")) { return None; }
        entry["key"].as_str().map(str::trim).filter(|key| !key.is_empty())
    })
}

/// One-time explicit import; subsequent model requests use the DOMINION vault.
pub(super) fn local_opencode_go_key() -> Option<(String, &'static str)> {
    if let Ok(value) = std::env::var("OPENCODE_API_KEY") {
        let value = value.trim();
        if !value.is_empty() { return Some((value.to_string(), "environment")); }
    }
    if let Some(auth) = opencode_auth() {
        if let Some(key) = opencode_go_key_in_auth(&auth) {
            return Some((key.to_string(), "opencode-cli"));
        }
    }
    None
}

pub(super) fn official_opencode_go_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("https://opencode.ai/zen/go")
        || trimmed == "https://opencode.ai/inference/go"
        || trimmed == "https://opencode.ai/inference/go/v1"
    {
        "https://opencode.ai/inference/go/openai/v1".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn direct_opencode_go(state: &AppState) -> Option<Value> {
    state.store.all("provider").into_iter().find(|provider| {
        provider["integration"] == "opencode-go"
            || provider["baseUrl"].as_str().is_some_and(|url|
                url.starts_with("https://opencode.ai/inference/go/openai/v1")
                || url.starts_with("https://opencode.ai/zen/go/v1"))
    })
}

pub(super) fn import_opencode_go(state: &AppState) -> Result<Value, String> {
    let (key, source) = local_opencode_go_key()
        .ok_or("OpenCode Go 키가 로컬에 없습니다. API 키 직접 등록을 이용하세요.")?;
    let previous = direct_opencode_go(state).unwrap_or(Value::Null);
    let id = previous["id"].as_str().unwrap_or("opencode-go").to_string();
    let record = json!({
        "id": id,
        "name": previous["name"].as_str().unwrap_or("OpenCode Go"),
        "baseUrl": previous["baseUrl"].as_str().unwrap_or("https://opencode.ai/inference/go/openai/v1"),
        "kind": "openai",
        "model": previous["model"].as_str().unwrap_or(""),
        "enabled": true,
        "local": false,
        "integration": "opencode-go",
        "toolMode": previous["toolMode"].as_str().unwrap_or("auto"),
        "reflection": previous["reflection"].as_bool().unwrap_or(false),
        "requestsPerMinute": previous["requestsPerMinute"].as_u64().unwrap_or(0)
    });
    state.save_secret(&format!("provider:{id}"), &key);
    let saved = state.store.put("provider", &record)
        .ok_or("OpenCode Go 연결 정보를 저장하지 못했습니다.")?;
    state.store.audit("provider.credential_imported",
        json!({"providerId":id,"source":source,"credentialCopied":true}));
    state.bus.record("provider", &saved);
    let mut result = public_provider(state, saved);
    result["importedCount"] = json!(1);
    result["credentialSource"] = json!(source);
    result["verification"] = json!("not_tested");
    Ok(result)
}

pub(super) fn opencode_api_entries(auth: &Value) -> Vec<(String, String)> {
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

pub(super) fn opencode_target(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
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

pub(super) fn provider_slug(value: &str) -> String {
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

pub(super) fn import_opencode(state: &AppState) -> Result<Value, String> {
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

pub(super) fn codex_model() -> String {
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

