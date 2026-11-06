use crate::{engine::video, state::AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::time::Duration;
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}
async fn comfy_reachable(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    reqwest::Client::new()
        .get(format!("{}/system_stats", url.trim_end_matches('/')))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}
pub fn outputs(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("job")
        .iter()
        .filter(|j| j["kind"] == "video" && j["status"] == "completed")
        .map(video::public_job)
        .collect()
}
pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let executable = video::ffmpeg(&state);
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let comfy = reachable && cfg["comfyWorkflow"].is_object();
    let workflows = workflows_from_config(&cfg);
    let jobs = state.store.all("job").iter()
        .filter(|job| job["kind"] == "video")
        .map(video::public_job).collect::<Vec<_>>();
    Json(json!({
        "config": cfg,
        "runtime": "rust",
        "available": executable.is_some() || comfy,
        "native": {"available": executable.is_some()},
        "ffmpeg": {"available": executable.is_some(), "path": executable},
        "comfy": {"reachable": reachable, "available": comfy},
        "engines": [
            {"id":"native","name":"자막 카드 영상","available":executable.is_some(),"reason":if executable.is_some(){""}else{"FFmpeg 실행 파일 필요"}},
            {"id":"comfyui","name":"ComfyUI","available":comfy,"reason":if comfy{""}else{"서버와 API 워크플로 설정 필요"}}
        ],
        "jobs": jobs,
        "models": [],
        "modelsReady": false,
        "workflows": workflows,
        "installed": reachable
    }))
}
pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(video::config(&state))
}
pub async fn save_config(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let mut cfg = video::config(&state);
    for key in [
        "engine",
        "duration",
        "aspectRatio",
        "comfyUrl",
        "comfyWorkflow",
        "comfyWorkflowName",
        "workflowDir",
        "outputDir",
        "ffmpegPath",
        "providerId",
        "model",
        "useModel",
    ] {
        if let Some(v) = body.get(key) {
            cfg[key] = v.clone();
        }
    }
    if let Some(url) = cfg["comfyUrl"].as_str().filter(|s| !s.is_empty()) {
        if !reqwest::Url::parse(url).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.username().is_empty()
                && u.password().is_none()
        }) {
            return error(
                StatusCode::BAD_REQUEST,
                "ComfyUI HTTP(S) 주소를 확인하세요.",
            );
        }
    }
    if !cfg["comfyWorkflow"].is_null()
        && !cfg["comfyWorkflow"].as_object().is_some_and(|nodes| {
            !nodes.is_empty() && nodes.values().all(|node| {
                node["class_type"].is_string() && node["inputs"].is_object()
            })
        }) {
        return error(
            StatusCode::BAD_REQUEST,
            "API 형식 워크플로 JSON 객체가 필요합니다.",
        );
    }
    if cfg["comfyWorkflow"].to_string().len() > 1024 * 1024 {
        return error(
            StatusCode::BAD_REQUEST,
            "워크플로는 1MiB 이하로 저장하세요.",
        );
    }
    if state.store.put("video_config", &cfg).is_none() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "영상 설정을 저장할 수 없습니다.");
    }
    state.bus.record("video_config", &json!({"id":"default"}));
    Json(video::config(&state)).into_response()
}
pub async fn list_jobs(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state
        .store
        .all("job")
        .iter()
        .filter(|j| matches!(j["kind"].as_str(), Some("video" | "youtube_upload")))
        .map(video::public_job)
        .collect::<Vec<_>>()))
}
pub async fn create_job(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match video::enqueue(&state, &body) {
        Ok(j) => (StatusCode::ACCEPTED, Json(j)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state
        .store
        .get("job", &id)
        .filter(|j| matches!(j["kind"].as_str(), Some("video" | "youtube_upload")))
    {
        Some(j) => Json(video::public_job(&j)).into_response(),
        None => error(StatusCode::NOT_FOUND, "영상 작업을 찾을 수 없습니다."),
    }
}
pub async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match video::enqueue_upload(&state, &id, &body) {
        Ok(j) => (StatusCode::ACCEPTED, Json(j)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    let path = match video::output_path(&state, &id) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::NOT_FOUND, e),
    };
    match tower_http::services::ServeFile::new(path)
        .try_call(request)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            response
                .headers_mut()
                .insert("x-content-type-options", "nosniff".parse().unwrap());
            response
        }
        Err(_) => error(StatusCode::NOT_FOUND, "영상 파일을 열 수 없습니다."),
    }
}
pub async fn outputs_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!(outputs(&state)))
}
pub async fn locations(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"outputs":[state.config.data_dir.join("video")],"comfy":[],"workflows":[]}))
}
fn workflows_from_config(cfg: &Value) -> Vec<Value> {
    if !cfg["comfyWorkflow"].is_object() {
        return Vec::new();
    }
    vec![json!({
        "id": "default",
        "name": cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),
        "bytes": cfg["comfyWorkflow"].to_string().len(),
    })]
}

pub async fn workflows(State(state): State<AppState>) -> Json<Value> {
    Json(json!(workflows_from_config(&video::config(&state))))
}

pub async fn reveal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let path = match video::output_path(&state, &id) {
        Ok(path) => path,
        Err(message) => return error(StatusCode::NOT_FOUND, message),
    };
    #[cfg(target_os = "macos")]
    let launched = tokio::process::Command::new("/usr/bin/open").arg("-R").arg(&path).status().await;
    #[cfg(target_os = "windows")]
    let launched = tokio::process::Command::new("explorer.exe")
        .arg(path.parent().expect("output directory")).status().await;
    #[cfg(all(unix, not(target_os = "macos")))]
    let launched = tokio::process::Command::new("xdg-open")
        .arg(path.parent().expect("output directory")).status().await;
    #[cfg(not(any(unix, windows)))]
    let launched: std::io::Result<std::process::ExitStatus> =
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported));

    match launched {
        Ok(status) if status.success() => Json(json!({"opened":true})).into_response(),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, "영상 폴더를 열 수 없습니다."),
    }
}

pub async fn empty_list() -> Json<Value> {
    Json(json!([]))
}
pub async fn not_available() -> Response {
    error(
        StatusCode::NOT_IMPLEMENTED,
        "이 작업은 지원하지 않습니다. 영상 작업 만들기를 사용하세요.",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-video-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir,
        };
        AppState::new(
            cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()),
        )
    }
    #[test]
    fn enqueue_is_durable_and_never_publishes_implicitly() {
        let s = state();
        let result = video::enqueue(
            &s,
            &json!({"brief":"테스트 영상","engine":"native","duration":3,"publish":true}),
        );
        if video::ffmpeg(&s).is_some() {
            let j = result.unwrap();
            let raw = s.store.get("job", j["id"].as_str().unwrap()).unwrap();
            assert_eq!(raw["input"]["publish"], false);
            assert_eq!(raw["status"], "queued");
            assert!(outputs(&s).is_empty());
        }
        assert!(video::enqueue(&s, &json!({"brief":"x","duration":900})).is_err());
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }
    #[tokio::test]
    async fn native_pipeline_produces_a_real_playable_container() {
        let s = state();
        if video::ffmpeg(&s).is_none() {
            return;
        }
        let j = video::enqueue(
            &s,
            &json!({"brief":"실제 영상 테스트. 한글 프레임 확인","duration":3}),
        )
        .unwrap();
        let id = j["id"].as_str().unwrap().to_string();
        let job = s.store.get("job", &id).unwrap();
        let ctx = crate::engine::jobs::JobContext::new(s.clone(), id.clone());
        let result = video::run(&ctx, &job).await.unwrap();
        assert!(result["bytes"].as_u64().unwrap() > 1000);
        let data = std::fs::read(video::directory(&s, &id).unwrap().join("output.mp4")).unwrap();
        assert_eq!(&data[4..8], b"ftyp");
        assert!(
            video::output_path(&s, &id).is_err(),
            "unfinished job must not be served"
        );
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }
}
