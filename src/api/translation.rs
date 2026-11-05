use axum::{http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use super::response::error;

pub async fn translate(Json(body): Json<Value>) -> Response {
    let text = body
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let target = body
        .get("target")
        .and_then(|value| value.as_str())
        .unwrap_or("ko");
    if text.is_empty() {
        return Json(json!({ "text": "" })).into_response();
    }
    if !text.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return Json(json!({ "text": text })).into_response();
    }
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={target}&dt=t&q={}",
        urlencode(&text)
    );
    match reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
    {
        Ok(response) => match response.json::<Value>().await {
            Ok(value) => {
                let translated = value
                    .get(0)
                    .and_then(|rows| rows.as_array())
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|row| row.get(0).and_then(|cell| cell.as_str()))
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                if translated.is_empty() {
                    Json(json!({ "text": text })).into_response()
                } else {
                    Json(json!({ "text": translated })).into_response()
                }
            }
            Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
        },
        Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
    }
}

pub async fn translate_config() -> Json<Value> {
    Json(json!({ "target": "ko", "provider": "google" }))
}

fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

