//! OpenAI Codex ChatGPT OAuth (PKCE). Tokens are kept in the same vault as
//! every other provider secret; the OAuth callback listens only on loopback
//! port 1455 for the duration of a login.
use crate::state::AppState;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const AUTHORIZE: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN: &str = "https://auth.openai.com/oauth/token";

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

/// Returns (code_verifier, code_challenge).
pub fn pkce() -> (String, String) {
    let verifier = random_token(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

pub fn new_state() -> String {
    random_token(24)
}

pub fn authorize_url(challenge: &str, state: &str) -> String {
    let mut url = reqwest::Url::parse(AUTHORIZE).expect("static authorize URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true");
    url.to_string()
}

pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
) -> Result<Value, String> {
    let response = client
        .post(TOKEN)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("Codex 토큰 교환 실패: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Codex 토큰 교환 실패 (HTTP {status}): {}",
            text.chars().take(300).collect::<String>()
        ));
    }
    response
        .json::<Value>()
        .await
        .map_err(|_| "Codex 토큰 응답 형식을 읽을 수 없습니다.".into())
}

pub async fn refresh(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<Value, String> {
    let response = client
        .post(TOKEN)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("Codex 토큰 갱신 실패: {error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Codex 토큰 갱신 실패 (HTTP {status}): {}",
            text.chars().take(300).collect::<String>()
        ));
    }
    response
        .json::<Value>()
        .await
        .map_err(|_| "Codex 갱신 응답 형식을 읽을 수 없습니다.".into())
}

pub fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn account_id(tokens: &Value) -> Option<String> {
    let id_token = tokens["id_token"].as_str().unwrap_or("");
    let access_token = tokens["access_token"].as_str().unwrap_or("");
    [id_token, access_token].into_iter().find_map(|token| {
        let claims = jwt_claims(token)?;
        claims["https://api.openai.com/auth"]["chatgpt_account_id"]
            .as_str()
            .or_else(|| claims["chatgpt_account_id"].as_str())
            .map(str::to_string)
    })
}

pub fn expires_at(tokens: &Value) -> i64 {
    if let Some(expires_in) = tokens["expires_in"].as_i64() {
        return chrono::Utc::now().timestamp() + expires_in;
    }
    tokens["access_token"]
        .as_str()
        .and_then(jwt_claims)
        .and_then(|claims| claims["exp"].as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600)
}

pub fn save_tokens(
    state: &AppState,
    provider_id: &str,
    tokens: &Value,
    shared: bool,
) -> Result<(), String> {
    let access = tokens["access_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("Codex 응답에 access_token이 없습니다.")?;
    state.save_secret(&format!("provider:{provider_id}"), access);
    let previous = state
        .read_secret(&format!("provider:{provider_id}:oauth"))
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    let refresh = tokens["refresh_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| previous["refreshToken"].as_str())
        .unwrap_or("");
    state.save_secret(
        &format!("provider:{provider_id}:oauth"),
        &json!({
            "refreshToken":refresh,
            "clientId":CLIENT_ID,
            "expiresAt":expires_at(tokens),
            "shared":shared,
        })
        .to_string(),
    );
    Ok(())
}

/// Returns a usable access token, refreshing only tokens that belong to
/// DOMINION. Imported Codex CLI tokens are marked shared and never rotated
/// here, so importing cannot invalidate the CLI's own login.
pub async fn fresh_access(
    state: &AppState,
    client: &reqwest::Client,
    provider_id: &str,
    current: &str,
) -> Result<String, String> {
    let Some(meta) = state
        .read_secret(&format!("provider:{provider_id}:oauth"))
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return Ok(current.to_string());
    };
    if meta["shared"].as_bool() == Some(true) {
        return Ok(current.to_string());
    }
    let expires = meta["expiresAt"].as_i64().unwrap_or(0);
    let now = chrono::Utc::now().timestamp();
    if !current.is_empty() && expires > now + 300 {
        return Ok(current.to_string());
    }
    let refresh_token = meta["refreshToken"].as_str().unwrap_or("");
    if refresh_token.is_empty() {
        return Ok(current.to_string());
    }
    let tokens = refresh(client, refresh_token).await?;
    save_tokens(state, provider_id, &tokens, false)?;
    tokens["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or("Codex 갱신 응답에 access_token이 없습니다.".into())
}
