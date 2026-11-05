use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

struct Provider {
    id: &'static str,
    name: &'static str,
    authorize: &'static str,
    token: &'static str,
    user: &'static str,
    scopes: &'static str,
    extra: &'static [(&'static str, &'static str)],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        id: "github",
        name: "GitHub",
        authorize: "https://github.com/login/oauth/authorize",
        token: "https://github.com/login/oauth/access_token",
        user: "https://api.github.com/user",
        scopes: "repo read:user user:email",
        extra: &[],
    },
    Provider {
        id: "google",
        name: "Google 계정",
        authorize: "https://accounts.google.com/o/oauth2/v2/auth",
        token: "https://oauth2.googleapis.com/token",
        user: "https://www.googleapis.com/oauth2/v3/userinfo",
        scopes: "openid email profile",
        extra: &[("access_type", "offline"), ("prompt", "consent")],
    },
    Provider {
        id: "youtube",
        name: "YouTube 업로드",
        authorize: "https://accounts.google.com/o/oauth2/v2/auth",
        token: "https://oauth2.googleapis.com/token",
        user: "https://www.googleapis.com/oauth2/v3/userinfo",
        scopes: "openid email profile https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly",
        extra: &[("access_type", "offline"), ("prompt", "consent")],
    },
];

fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

fn config(state: &AppState, id: &str) -> Option<Value> {
    state.store.get("oauth_config", id)
}

fn callback_url(state: &AppState, id: &str) -> String {
    format!(
        "http://127.0.0.1:{}/api/oauth/{id}/callback",
        state.config.port
    )
}

pub fn status(state: &AppState) -> Vec<Value> {
    PROVIDERS
        .iter()
        .map(|provider| {
            let record = config(state, provider.id).unwrap_or_else(|| json!({}));
            let connected = record
                .get("connected")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                && state
                    .store
                    .has_secret(&format!("oauth:{}:token", provider.id));
            json!({
                "id": provider.id,
                "name": provider.name,
                "configured": client_credentials(state, provider.id).is_ok(),
                "connected": connected,
                "account": record.get("account").cloned().unwrap_or(Value::Null),
                "connectedAt": record.get("connectedAt"),
                "callback": callback_url(state, provider.id),
                "scopes": provider.scopes,
                "scope": provider.scopes,
                "grantedScopes": record.get("scopes"),
            })
        })
        .collect()
}

pub fn save(
    state: &AppState,
    id: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<Value, String> {
    let provider = provider(id).ok_or_else(|| format!("지원하지 않는 OAuth 제공자입니다: {id}"))?;
    if client_id.trim().is_empty() || (id == "github" && client_secret.trim().is_empty()) {
        return Err("Client ID가 필요합니다. GitHub 앱은 Client Secret도 입력하세요.".into());
    }
    let mut record = config(state, id).unwrap_or_else(|| json!({ "id": id }));
    record["id"] = json!(id);
    record["clientId"] = json!(client_id.trim());
    record["configuredAt"] = json!(chrono::Utc::now().to_rfc3339());
    state
        .store
        .put("oauth_config", &record)
        .ok_or("OAuth 앱 설정 저장 실패")?;
    state.save_secret(
        &format!("oauth:{}:secret", provider.id),
        &json!({ "clientId": client_id.trim(), "clientSecret": client_secret.trim() }).to_string(),
    );
    if client_credentials(state, id).is_err() {
        return Err("OAuth 앱 자격 증명 저장 실패".into());
    }
    Ok(json!({
        "id": provider.id,
        "name": provider.name,
        "configured": true,
        "callback": callback_url(state, provider.id),
    }))
}

fn client_credentials(state: &AppState, id: &str) -> Result<(String, String), String> {
    let sealed = state
        .read_secret(&format!("oauth:{id}:secret"))
        .or_else(|| {
            (id == "youtube")
                .then(|| state.read_secret("oauth:google:secret"))
                .flatten()
        })
        .ok_or_else(|| "OAuth 앱이 설정되지 않았습니다.".to_string())?;
    let value: Value = serde_json::from_str(&sealed).map_err(|error| error.to_string())?;
    let client_id = value
        .get("clientId")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let client_secret = value
        .get("clientSecret")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    if client_id.is_empty() || (id == "github" && client_secret.is_empty()) {
        return Err("OAuth 앱 자격 증명이 비어 있습니다.".into());
    }
    Ok((client_id, client_secret))
}

pub fn start(state: &AppState, id: &str) -> Result<Value, String> {
    let provider = provider(id).ok_or_else(|| format!("지원하지 않는 OAuth 제공자입니다: {id}"))?;
    let (client_id, _) = client_credentials(state, id)?;
    use base64::Engine;
    use sha2::Digest;
    let state_token = uuid::Uuid::new_v4().to_string();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    state.save_secret(&format!("oauth-state:{state_token}"), &verifier);
    state.store.put(
        "oauth_state",
        &json!({
            "id": state_token,
            "provider": id,
            "createdMs": chrono::Utc::now().timestamp_millis(),
        }),
    );

    let mut url = reqwest::Url::parse(provider.authorize).map_err(|error| error.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &callback_url(state, id))
            .append_pair("scope", provider.scopes)
            .append_pair("state", &state_token);
        for (key, value) in provider.extra {
            query.append_pair(key, value);
        }
        if id == "google" || id == "youtube" {
            query
                .append_pair("response_type", "code")
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256");
            if let Some(account) =
                config(state, "google").and_then(|r| r["account"].as_str().map(str::to_string))
            {
                query.append_pair("login_hint", &account);
            }
        }
    }
    Ok(json!({ "url": url.to_string(), "state": state_token }))
}

pub async fn callback(
    state: &AppState,
    id: &str,
    code: &str,
    state_token: &str,
) -> Result<Value, String> {
    let provider = provider(id).ok_or_else(|| format!("지원하지 않는 OAuth 제공자입니다: {id}"))?;
    if code.trim().is_empty() || state_token.trim().is_empty() {
        return Err("code 와 state 가 필요합니다.".into());
    }

    let record = state
        .store
        .get("oauth_state", state_token)
        .ok_or("알 수 없는 OAuth state 입니다. 다시 연결하세요.")?;
    let created_ms = record
        .get("createdMs")
        .and_then(|value| value.as_i64())
        .ok_or("OAuth state 가 손상되었습니다.")?;
    if chrono::Utc::now().timestamp_millis() - created_ms > 10 * 60 * 1000 {
        state.store.remove("oauth_state", state_token);
        state
            .store
            .remove_secret(&format!("oauth-state:{state_token}"));
        return Err("OAuth state 가 만료되었습니다. 다시 연결하세요.".into());
    }
    if record.get("provider").and_then(|value| value.as_str()) != Some(provider.id) {
        return Err("OAuth state 제공자가 일치하지 않습니다.".into());
    }
    state
        .store
        .patch_if(
            "oauth_state",
            state_token,
            "used",
            &Value::Null,
            &json!({"used":true}),
        )
        .ok_or("이미 처리한 OAuth state입니다.")?;
    state.store.remove("oauth_state", state_token);
    let verifier = state.read_secret(&format!("oauth-state:{state_token}"));
    state
        .store
        .remove_secret(&format!("oauth-state:{state_token}"));

    let (client_id, client_secret) = client_credentials(state, id)?;
    let client = reqwest::Client::new();
    let mut form = vec![
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code.to_string()),
        ("redirect_uri", callback_url(state, id)),
        ("grant_type", "authorization_code".to_string()),
    ];
    if provider.id == "github" {
        form.pop();
    } else {
        form.push((
            "code_verifier",
            verifier.ok_or("PKCE verifier가 없습니다. 다시 연결하세요.")?,
        ));
        form.retain(|(key, value)| *key != "client_secret" || !value.is_empty());
    }

    let response = client
        .post(provider.token)
        .header("Accept", "application/json")
        .form(&form)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| format!("토큰 교환 실패: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("OAuth 토큰 교환 거부: HTTP {status}"));
    }
    let text = response.text().await.unwrap_or_default();
    let token_value: Value = serde_json::from_str(&text)
        .map_err(|_| format!("토큰 응답을 해석할 수 없습니다 (HTTP {status})"))?;
    let access_token = token_value
        .get("access_token")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if access_token.is_empty() {
        let error = token_value
            .get("error_description")
            .or_else(|| token_value.get("error"))
            .and_then(|value| value.as_str())
            .unwrap_or("access_token 없음");
        return Err(format!("OAuth 토큰 교환 거부됨: {error}"));
    }

    let mut account = Value::Null;
    if let Ok(info) = client
        .get(provider.user)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(20))
        .send()
        .await
    {
        if let Ok(value) = info.json::<Value>().await {
            account = value
                .get("login")
                .or_else(|| value.get("email"))
                .or_else(|| value.get("name"))
                .cloned()
                .unwrap_or(Value::Null);
        }
    }

    state.save_secret(
        &format!("oauth:{id}:token"),
        &json!({
            "accessToken": access_token,
            "refreshToken": token_value.get("refresh_token"),
            "scopes": token_value.get("scope"),
            "obtainedAt": chrono::Utc::now().to_rfc3339(),
            "expiresAt": token_value["expires_in"].as_i64().map(|seconds| chrono::Utc::now().timestamp() + seconds),
        })
        .to_string(),
    );

    let mut record = config(state, id).unwrap_or_else(|| json!({ "id": id }));
    record["id"] = json!(id);
    record["connected"] = json!(true);
    record["scopes"] = token_value
        .get("scope")
        .cloned()
        .unwrap_or(json!(provider.scopes));
    record["account"] = account.clone();
    record["connectedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if self::access_token(state, id).as_deref() != Some(access_token.as_str()) {
        return Err("OAuth 토큰 저장 실패".into());
    }
    state
        .store
        .put("oauth_config", &record)
        .ok_or("OAuth 연결 상태 저장 실패")?;
    state.bus.record("oauth_config", &record);
    state.store.audit(
        "oauth.connected",
        json!({ "provider": id, "account": account }),
    );

    Ok(json!({ "provider": id, "connected": true, "account": account }))
}

pub fn disconnect(state: &AppState, id: &str) {
    state.store.remove_secret(&format!("oauth:{id}:token"));
    if let Some(mut record) = config(state, id) {
        record["connected"] = json!(false);
        record["account"] = Value::Null;
        state.store.put("oauth_config", &record);
        state.bus.record("oauth_config", &record);
    }
}

/// Bearer token for tools (GitHub / YouTube). `None` until the user connects.
pub fn access_token(state: &AppState, id: &str) -> Option<String> {
    let sealed = state.read_secret(&format!("oauth:{id}:token"))?;
    let value: Value = serde_json::from_str(&sealed).ok()?;
    value
        .get("accessToken")
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

/// Refresh long-lived service grants in the vault; Google identity and YouTube
/// intentionally have independent grants, expiry and disconnect operations.
pub async fn fresh_access_token(state: &AppState, id: &str) -> Result<String, String> {
    let raw = state
        .read_secret(&format!("oauth:{id}:token"))
        .ok_or("계정을 먼저 연결하세요.")?;
    let mut token: Value =
        serde_json::from_str(&raw).map_err(|_| "저장된 OAuth 토큰이 손상되었습니다.")?;
    if token["expiresAt"]
        .as_i64()
        .is_none_or(|expiry| expiry > chrono::Utc::now().timestamp() + 60)
    {
        return token["accessToken"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .ok_or("액세스 토큰이 없습니다.".into());
    }
    let refresh = token["refreshToken"]
        .as_str()
        .ok_or("인증이 만료되었습니다. 계정을 다시 연결하세요.")?;
    let provider = provider(id).ok_or("지원하지 않는 OAuth 제공자입니다.")?;
    let (client_id, client_secret) = client_credentials(state, id)?;
    let response = reqwest::Client::new()
        .post(provider.token)
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "OAuth 갱신 연결 실패")?;
    if !response.status().is_success() {
        return Err("OAuth 갱신이 거부되었습니다. 계정을 다시 연결하세요.".into());
    }
    let updated: Value = response.json().await.map_err(|_| "OAuth 갱신 응답 오류")?;
    let access = updated["access_token"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("갱신된 토큰이 없습니다.")?
        .to_string();
    token["accessToken"] = json!(access);
    token["expiresAt"] =
        json!(chrono::Utc::now().timestamp() + updated["expires_in"].as_i64().unwrap_or(3600));
    state.save_secret(&format!("oauth:{id}:token"), &token.to_string());
    Ok(access)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-oauth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 4999,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn start_requires_configured_credentials() {
        let (state, dir) = temp_state();
        assert!(start(&state, "github").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn authorize_url_contains_state_and_redirect() {
        let (state, dir) = temp_state();
        save(&state, "github", "client-123", "secret-456").expect("save");
        let started = start(&state, "github").expect("start");
        let url = started["url"].as_str().unwrap();
        assert!(url.starts_with("https://github.com/login/oauth/authorize"));
        assert!(url.contains("client_id=client-123"));
        assert!(url.contains("state="));
        assert!(
            url.contains("127.0.0.1%3A4999%2Fapi%2Foauth%2Fgithub%2Fcallback")
                || url.contains("127.0.0.1:4999")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn secrets_are_not_stored_in_records() {
        let (state, dir) = temp_state();
        save(&state, "github", "client-123", "super-secret").expect("save");
        let record = state
            .store
            .get("oauth_config", "github")
            .unwrap()
            .to_string();
        // The client id is public; the client secret must never appear.
        assert!(!record.contains("super-secret"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn callback_rejects_unknown_state() {
        let (state, dir) = temp_state();
        save(&state, "github", "client-123", "secret").unwrap();
        let error = callback(&state, "github", "code", "bogus-state")
            .await
            .unwrap_err();
        assert!(error.contains("state"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
