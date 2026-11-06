use super::response::error;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::time::Duration;

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
    if text.chars().count() > 5000 {
        return error(
            StatusCode::BAD_REQUEST,
            "번역할 텍스트를 5,000자 이하로 선택하세요.",
        );
    }
    if target.is_empty()
        || target.len() > 20
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
    {
        return error(
            StatusCode::BAD_REQUEST,
            "번역 대상 언어가 올바르지 않습니다.",
        );
    }
    if text.is_empty() {
        return Json(json!({ "text": "" })).into_response();
    }
    if !text.chars().any(|ch| ch.is_alphabetic()) {
        return Json(json!({ "text": text })).into_response();
    }
    match reqwest::Client::new()
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", target),
            ("dt", "t"),
            ("q", &text),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => match response.json::<Value>().await {
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
                    error(StatusCode::BAD_GATEWAY, "번역 결과가 비어 있습니다.")
                } else {
                    Json(json!({ "text": translated })).into_response()
                }
            }
            Err(_) => error(StatusCode::BAD_GATEWAY, "번역 응답을 읽지 못했습니다."),
        },
        Ok(_) | Err(_) => error(StatusCode::BAD_GATEWAY, "번역 서버에 연결할 수 없습니다."),
    }
}

pub async fn translate_config() -> Json<Value> {
    Json(json!({ "target": "ko", "provider": "google" }))
}
