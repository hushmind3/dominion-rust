//! DOMINION's durable media pipeline. Native cards are rendered by Rust; FFmpeg
//! only encodes them. Optional ComfyUI is an explicit HTTP rendering connector.
use crate::{
    engine::{
        jobs::{self, JobContext},
        oauth, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
mod cinematic;
mod omni;
mod quality;
mod ltx;
mod film;
mod pricing;
use sha2::Digest;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn config(state: &AppState) -> Value {
    let mut value = json!({"id":"default","engine":"native","duration":15,"aspectRatio":"9:16","comfyUrl":"","comfyWorkflow":null,"workflowDir":"","outputDir":"","ffmpegPath":"","providerId":"","model":"","useModel":false,"omniModel":"gemini-omni-1.2-flash","omniResolution":"1080p"});
    if let Some(saved) = state.store.get("video_config", "default") {
        if let Some(map) = saved.as_object() {
            for (k, v) in map {
                if k != "videoApiKey" {
                    value[k] = v.clone();
                }
            }
        }
    }
    if value["engine"]=="veo" {value["engine"]=json!("omni");}
    value["hasVideoKey"] = json!(state.store.has_secret("video:api"));
    value["hasLtxKey"] = json!(state.store.has_secret("video:ltx-api"));
    value
}
/// Owner preview, recomputed by enqueue and again before a billable call.
pub fn paid_quote(state:&AppState,body:&Value)->Result<Value,String>{
    if body["engine"]!="ltx" {return Err("LTX 영상 견적만 지원합니다.".into());}
    pricing::quote(&config(state),body)
}
pub fn quality_available(state:&AppState)->bool {
    quality::available(&config(state))
}
pub fn ffmpeg(state: &AppState) -> Option<PathBuf> {
    ffmpeg_from_config(&config(state))
}

/// Resolve the encoder from the job's frozen config so a later settings change
/// cannot redirect an already queued render.
fn ffmpeg_from_config(cfg: &Value) -> Option<PathBuf> {
    let selected = cfg["ffmpegPath"].as_str().unwrap_or("");
    if !selected.is_empty() {
        let path = PathBuf::from(selected);
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(name))
            .collect();
    paths.extend(
        [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths.into_iter().find(|p| p.is_file())
}
pub fn directory(state: &AppState, id: &str) -> Result<PathBuf, String> {
    // Job IDs are database-owned UUIDs, never arbitrary request paths.
    uuid::Uuid::parse_str(id).map_err(|_| "잘못된 영상 작업 ID")?;
    Ok(state.config.data_dir.join("video").join(id))
}
pub fn output_path(state: &AppState, id: &str) -> Result<PathBuf, String> {
    let record = state
        .store
        .get("job", id)
        .filter(|j| j["kind"] == "video")
        .ok_or("영상 작업이 없습니다.")?;
    if record["status"] != "completed" {
        return Err("완료된 영상이 없습니다.".into());
    }
    let root = directory(state, id)?
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    let path = root
        .join("output.mp4")
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    if !path.starts_with(root) {
        return Err("영상 경로를 확인할 수 없습니다.".into());
    }
    Ok(path)
}
pub fn public_job(job: &Value) -> Value {
    let id = job["id"].as_str().unwrap_or("");
    let ready = job["kind"] == "video" && job["status"] == "completed";
    json!({"id":id,"kind":job["kind"],"title":job["input"]["title"],"brief":job["input"]["brief"],"engine":job["input"]["engine"],"status":job["status"],"phase":job.get("phase").unwrap_or(&job["status"]),"progress":job["progress"],"error":job["error"],"createdAt":job.get("createdAt").or(job.get("created_at")),"videoJobId":job["input"]["videoJobId"],"privacyStatus":job["input"]["privacyStatus"],"outputUrl":ready.then(||format!("/api/video/jobs/{id}/content")),"storyboard":job["videoState"]["scenes"],"result":job["result"],"duration":job["input"]["duration"],"aspectRatio":job["input"]["aspectRatio"],"costQuote":job["input"]["costQuote"],"maxEstimatedKrw":job["input"]["maxEstimatedKrw"],"ltxFilm":job["input"]["ltxFilm"],"film":job["input"]["render"]["film"],"qualityReceipt":job["result"]["qualityReceipt"]})
}
pub fn enqueue(state: &AppState, body: &Value) -> Result<Value, String> {
    let cfg = config(state);
    let brief = body["brief"].as_str().unwrap_or("").trim();
    if brief.is_empty() || brief.len() > 16_000 {
        return Err("영상 주제를 1~16,000바이트로 입력하세요.".into());
    }
    let engine = body["engine"]
        .as_str()
        .or(cfg["engine"].as_str())
        .unwrap_or("native");
    if !matches!(engine, "native" | "comfyui" | "omni" | "ltx") {
        return Err("지원하지 않는 영상 엔진입니다.".into());
    }
    // Reject an unverifiable remote render before its first billable call.
    if !quality::available(&cfg){
        return Err("최종 영상 검수용 FFprobe가 없습니다. FFmpeg와 FFprobe를 설치하거나 실행 파일 경로를 지정하세요.".into());
    }
    if engine == "native" && ffmpeg(state).is_none() {
        return Err("FFmpeg가 필요합니다. 고급 설정에서 실행 파일을 지정하세요.".into());
    }
    if engine == "comfyui"
        && (cfg["comfyUrl"].as_str().unwrap_or("").is_empty() || !cfg["comfyWorkflow"].is_object())
    {
        return Err("ComfyUI 주소와 API 형식 워크플로 JSON을 먼저 저장하세요.".into());
    }
    if engine == "omni" && !cfg["hasVideoKey"].as_bool().unwrap_or(false) {
        return Err("Gemini API 키를 영상 설정에 등록하세요.".into());
    }
    if engine=="ltx" && state.read_secret("video:ltx-api").is_none(){
        return Err("LTX 2.5 API 키를 영상 설정에서 등록하세요.".into());
    }
    if engine=="ltx" && body["confirmPaidGeneration"]!=true {
        return Err("LTX 유료 영상 생성에 명시적으로 동의해야 합니다.".into());
    }
    let ltx_model=body["ltxModel"].as_str().unwrap_or("ltx-2-5-fast");
    let ltx_quality=body["ltxQuality"].as_str().unwrap_or("1080p");
    let ltx_style=body["ltxStyle"].as_str().unwrap_or("cinematic");
    let ltx_film=engine=="ltx" && body["ltxFilm"]==true;
    let cinematic = engine == "comfyui" && body["cinematic"] == true;
    let bad_shot_retries=body["badShotRetries"].as_u64().unwrap_or(0);
    if bad_shot_retries>1 || (bad_shot_retries>0 && !cinematic){
        return Err("불량 샷 재생성은 시네마틱 모드에서 0~1회만 허용합니다.".into());
    }
    if cinematic && ffmpeg(state).is_none() {
        return Err("시네마틱 멀티샷 제작에는 FFmpeg가 필요합니다.".into());
    }
    let duration = body["duration"]
        .as_u64()
        .or(cfg["duration"].as_u64())
        .unwrap_or(15);
    if engine == "omni" && duration != 8 {
        return Err("Omni 영상은 현재 8초를 목표로 생성합니다.".into());
    }
    if engine=="ltx" {
        if !matches!(ltx_model,"ltx-2-5-fast"|"ltx-2-5-pro"){
            return Err("LTX 지원 모델을 확인하세요.".into());
        }
        let _=ltx::resolution(body["aspectRatio"].as_str()
            .or(cfg["aspectRatio"].as_str()).unwrap_or("9:16"),ltx_quality)?;
        ltx::cinematic_prompt(brief,ltx_style)?;
        if !ltx_film && !ltx::valid_duration(ltx_model,ltx_quality,duration) {
            return Err("LTX 2.5 모델·화질별 지원 길이를 확인하세요.".into());
        }
    }
    if !(3..=if cinematic {120} else {180}).contains(&duration) {
        return Err(if cinematic {"시네마틱 영상은 3~120초를 지원합니다."}
            else {"영상 길이는 3~180초입니다."}.into());
    }
    let aspect = body["aspectRatio"]
        .as_str()
        .or(cfg["aspectRatio"].as_str())
        .unwrap_or("9:16");
    if !matches!(aspect, "9:16" | "16:9" | "1:1") {
        return Err("지원 비율은 9:16,16:9,1:1입니다.".into());
    }
    if engine == "omni" && aspect == "1:1" {
        return Err("Omni는 9:16 또는 16:9 비율만 지원합니다.".into());
    }
    let title = body["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(brief)
        .chars()
        .take(90)
        .collect::<String>();
    let fps = body["fps"].as_u64().unwrap_or(16);
    let seed = body["seed"].as_i64().unwrap_or(-1);
    let negative_prompt = body["negativePrompt"].as_str().unwrap_or("");
    if !(1..=60).contains(&fps) || seed < -1 || negative_prompt.len() > 16_000 {
        return Err("FPS, 시드, 네거티브 프롬프트 값을 확인하세요.".into());
    }
    let (width, height) = match aspect {
        "16:9" => (640, 360),
        "1:1" => (480, 480),
        _ => (360, 640),
    };
    let seed = if seed == -1 {
        // Freeze one seed for this job so a safe retry uses identical input.
        i64::from(u32::from_be_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap()))
    } else {
        seed
    };
    let mut input = json!({
        "brief":brief, "title":title, "engine":engine, "duration":duration,
        "cinematic":cinematic,"badShotRetries":bad_shot_retries,
        "ltxFilm":ltx_film,"ltxModel":ltx_model,"ltxQuality":ltx_quality,
        "ltxStyle":ltx_style,"confirmPaidGeneration":body["confirmPaidGeneration"]==true,
        "confirmPaidMultiShot":body["confirmPaidMultiShot"]==true,
        "technicalGate":true,"scenes":body.get("scenes").cloned().unwrap_or(Value::Null),
        "aspectRatio":aspect, "config":cfg, "publish":false,
        "render":{"prompt":brief,"negative":negative_prompt,"negativePrompt":negative_prompt,
                  "duration":duration,"width":width,"height":height,"fps":fps,"frames":duration*fps,"seed":seed}
    });
    if engine=="ltx" {
        if ltx_film {
            input["render"]=film::spec(&input,&input["config"])?;
        }
        let expected_calls=if ltx_film {
            input["render"]["film"]["paidCallCount"].as_u64().unwrap_or(u64::MAX)
        } else {1};
        if expected_calls==0 || expected_calls>6 ||
            body["confirmedPaidCallCount"].as_u64()!=Some(expected_calls) {
            return Err(format!("유료 영상 생성 예상 호출 {expected_calls}회를 정확히 확인해야 합니다."));
        }
        let quote=pricing::quote(&input["config"],&input)?;
        let ceiling=body["maxEstimatedKrw"].as_u64();
        let expected_krw=quote["estimatedKrw"].as_u64().unwrap_or(u64::MAX);
        if body["approvedPaidQuote"]!=true || ceiling.is_none_or(|limit|limit<expected_krw){
            return Err(format!("LTX 예상 원화 생성비 {expected_krw}원을 확인하고 해당 금액 이상의 상한을 승인하세요."));
        }
        input["costQuote"]=quote;
        input["maxEstimatedKrw"]=json!(ceiling.unwrap_or(0));
        input["paidCallCount"] = json!(expected_calls);
    }
    jobs::enqueue(state, "video", input)
        .map(|j| public_job(&j))
        .ok_or("영상 작업을 저장할 수 없습니다.".into())
}
fn progress(ctx: &JobContext, phase: &str, percent: u64) -> Result<(), String> {
    ctx.checkpoint(json!({"phase":phase,"progress":percent}))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut line = String::new();
    let mut used = 0;
    for c in s.chars().take(280) {
        if c == '\n' || used >= width {
            lines.push(std::mem::take(&mut line));
            used = 0;
            if c == '\n' {
                continue;
            }
        }
        line.push(c);
        used += if c.is_ascii() { 1 } else { 2 };
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.truncate(8);
    lines
}
fn make_cards(dir: &Path, title: &str, scenes: &[String], aspect: &str) -> Result<(), String> {
    let (w, h) = match aspect {
        "16:9" => (1280, 720),
        "1:1" => (900, 900),
        _ => (720, 1280),
    };
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let family = options
        .fontdb
        .faces()
        .find(|f| {
            f.families.iter().any(|(name, _)| {
                name.contains("Apple SD Gothic")
                    || name.contains("Malgun")
                    || name.contains("Noto Sans CJK")
            })
        })
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .unwrap_or_else(|| "sans-serif".into());
    for (i, scene) in scenes.iter().enumerate() {
        let font = if aspect == "16:9" { 42 } else { 38 };
        let lines = wrap(scene, if aspect == "16:9" { 48 } else { 29 });
        let mut text = String::new();
        let top = (h as i32 / 2) - (lines.len() as i32 * (font + 18) / 2);
        for (n, line) in lines.iter().enumerate() {
            text.push_str(&format!("<text x='{}' y='{}' text-anchor='middle' fill='#fff9ef' font-size='{font}' font-family='{}'>{}</text>",w/2,top+n as i32*(font+18),escape(&family),escape(line)));
        }
        let heading = wrap(title, if aspect == "16:9" { 70 } else { 38 })
            .first()
            .cloned()
            .unwrap_or_default();
        let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'><defs><linearGradient id='bg' x2='1' y2='1'><stop stop-color='#171f32'/><stop offset='1' stop-color='#314862'/></linearGradient></defs><rect width='{w}' height='{h}' fill='url(#bg)'/><circle cx='{}' cy='{}' r='240' fill='#c96442' opacity='.15'/><rect x='48' y='86' width='80' height='5' rx='2' fill='#edb78e'/><text x='48' y='140' font-family='{}' font-size='22' fill='#edb78e'>{}</text>{text}<text x='48' y='{}' font-family='sans-serif' font-size='20' fill='#d0d9e4'>{:02} / {:02}</text><rect x='48' y='{}' width='{}' height='4' fill='#edb78e'/></svg>",w,h,escape(&family),escape(&heading),h-55,i+1,scenes.len(),h-30,(w-96)*(i+1)as u32/scenes.len()as u32);
        let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
        let mut bitmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("영상 프레임 메모리 부족")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::default(),
            &mut bitmap.as_mut(),
        );
        bitmap
            .save_png(dir.join(format!("card-{i:02}.png")))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
async fn native(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    progress(ctx, "storyboard", 5)?;
    let brief = input["brief"].as_str().unwrap_or("");
    let title = input["title"].as_str().unwrap_or("");
    let cfg = &input["config"];
    let mut scenes: Vec<String> = brief
        .split(['\n', '.', '!', '?', '。'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(12)
        .map(str::to_string)
        .collect();
    if cfg["useModel"] == true {
        let text=crate::engine::llm::complete(&ctx.state,cfg["providerId"].as_str().unwrap_or(""),cfg["model"].as_str().unwrap_or(""),"영상 자막 스토리보드를 만드세요. JSON 문자열 배열만 반환하세요. 3~8개 장면, 장면당 한국어 100자 이내. 사실을 지어내지 마세요.",brief).await?;
        scenes = serde_json::from_str::<Vec<String>>(
            text.trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim(),
        )
        .map_err(|_| "모델 스토리보드가 JSON 문자열 배열이 아닙니다.")?;
        if scenes.is_empty() || scenes.len() > 12 || scenes.iter().any(|s| s.len() > 2000) {
            return Err("스토리보드 장면 수나 길이가 범위를 넘었습니다.".into());
        }
    }
    if scenes.is_empty() {
        scenes.push(brief.into());
    }
    ctx.checkpoint(json!({"videoState":{"scenes":scenes},"phase":"rendering","progress":20}))?;
    make_cards(
        dir,
        title,
        &scenes,
        input["aspectRatio"].as_str().unwrap_or("9:16"),
    )?;
    let duration = input["duration"].as_u64().unwrap_or(15);
    let seconds = duration as f64 / scenes.len() as f64;
    let mut concat = String::new();
    for i in 0..scenes.len() {
        concat.push_str(&format!("file 'card-{i:02}.png'\nduration {seconds:.5}\n"));
    }
    concat.push_str(&format!("file 'card-{:02}.png'\n", scenes.len() - 1));
    std::fs::write(dir.join("frames.txt"), concat).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("storyboard.json"),
        json!({"title":title,"scenes":scenes,"duration":duration,"audio":"silent"}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    let encoder = ffmpeg_from_config(&input["config"]).ok_or("FFmpeg를 찾을 수 없습니다.")?;
    let mut command = tokio::process::Command::new(encoder);
    command.current_dir(dir).args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-f",
        "concat",
        "-safe",
        "1",
        "-i",
        "frames.txt",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo",
        "-t",
        &duration.to_string(),
        "-vf",
        "fps=24,format=yuv420p",
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "22",
        "-c:a",
        "aac",
        "-movflags",
        "+faststart",
        "-shortest",
        "output.part.mp4",
    ]);
    progress(ctx, "encoding", 50)?;
    let process = tools::run_command(&mut command, Duration::from_secs(600));
    tokio::pin!(process);
    let output = loop {
        tokio::select! {result=&mut process=>break result?,_ = tokio::time::sleep(Duration::from_millis(500))=>{ctx.checkpoint(json!({}))?;}}
    };
    if !output.status.success() {
        return Err(format!(
            "FFmpeg 인코딩 실패: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1500)
                .collect::<String>()
        ));
    }
    std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
        .map_err(|e| e.to_string())?;
    Ok(
        json!({"engine":"native","description":"Rust 자막 카드 영상 · 무음","duration":duration,"scenes":scenes.len()}),
    )
}
fn substitute(value: &mut Value, variables: &Value) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}"))
                .and_then(|key| variables.get(key)) {
                *value = replacement.clone();
                return;
            }
            let mut expanded = String::new();
            let mut rest = text.as_str();
            while let Some((prefix, candidate)) = rest.split_once("{{") {
                let Some((key, suffix)) = candidate.split_once("}}") else { break };
                expanded.push_str(prefix);
                match variables.get(key) {
                    Some(Value::String(replacement)) => expanded.push_str(replacement),
                    Some(replacement) => expanded.push_str(&replacement.to_string()),
                    None => { expanded.push_str("{{"); expanded.push_str(key); expanded.push_str("}}"); }
                }
                rest = suffix;
            }
            expanded.push_str(rest);
            *text = expanded;
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, variables)),
        Value::Object(fields) => fields.values_mut().for_each(|item| substitute(item, variables)),
        _ => {}
    }
}
async fn comfy(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let cfg = &input["config"];
    let base = cfg["comfyUrl"].as_str().unwrap_or("").trim_end_matches('/');
    let parsed = reqwest::Url::parse(base).map_err(|_| "ComfyUI 주소 오류")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("ComfyUI HTTP(S) 주소를 확인하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut workflow = cfg["comfyWorkflow"].clone();
    let legacy = json!({"prompt":input["brief"],"duration":input["duration"]});
    substitute(&mut workflow, input.get("render").unwrap_or(&legacy));
    ctx.checkpoint(json!({"phase":"submitting","progress":10,"videoState":{"inFlight":true}}))?;
    let response = client
        .post(format!("{base}/prompt"))
        .json(&json!({"prompt":workflow,"client_id":ctx.job_id}))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 결과를 확인할 수 없습니다.")?;
    if !response.status().is_success() {
        ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
        return Err(format!("ComfyUI 워크플로 거부: HTTP {}", response.status()));
    }
    let answer: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 응답 오류")?;
    let prompt_id = answer["prompt_id"]
        .as_str()
        .ok_or("needs_review: ComfyUI prompt_id가 없습니다.")?;
    ctx.checkpoint(json!({"phase":"rendering","progress":25,"videoState":{"inFlight":true,"promptId":prompt_id}}))?;
    let start = std::time::Instant::now();
    loop {
        ctx.checkpoint(json!({}))?;
        if start.elapsed() > Duration::from_secs(900) {
            return Err(
                "needs_review: ComfyUI 생성 대기시간이 지났습니다. 원격 작업 상태를 확인하세요."
                    .into(),
            );
        }
        let history: Value = client
            .get(format!("{base}/history/{prompt_id}"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let result = &history[prompt_id];
        if result["status"]["status_str"] == "error" {
            ctx.checkpoint(json!({"videoState":{"inFlight":false,"promptId":prompt_id}}))?;
            return Err("ComfyUI 워크플로 실행 실패".into());
        }
        if let Some(outputs) = result["outputs"].as_object() {
            for node in outputs.values() {
                for field in ["videos", "gifs"] {
                    if let Some(files) = node[field].as_array() {
                        for file in files {
                            let filename = file["filename"].as_str().unwrap_or("");
                            if !filename.to_lowercase().ends_with(".mp4") {
                                continue;
                            }
                            let response = client
                                .get(format!("{base}/view"))
                                .query(&[
                                    ("filename", filename),
                                    ("subfolder", file["subfolder"].as_str().unwrap_or("")),
                                    ("type", file["type"].as_str().unwrap_or("output")),
                                ])
                                .timeout(Duration::from_secs(120))
                                .send()
                                .await
                                .map_err(|e| e.to_string())?;
                            if !response.status().is_success() {
                                return Err("ComfyUI 결과 다운로드 실패".into());
                            }
                            use futures_util::StreamExt;
                            use tokio::io::AsyncWriteExt;
                            let mut stream = response.bytes_stream();
                            let mut total = 0usize;
                            let mut output = tokio::fs::File::create(dir.join("output.part.mp4"))
                                .await
                                .map_err(|e| e.to_string())?;
                            while let Some(chunk) = stream.next().await {
                                ctx.checkpoint(json!({}))?;
                                let chunk = chunk.map_err(|e| e.to_string())?;
                                total += chunk.len();
                                if total > 256 * 1024 * 1024 {
                                    return Err("영상 결과가 256MiB 제한을 넘었습니다.".into());
                                }
                                output.write_all(&chunk).await.map_err(|e| e.to_string())?;
                            }
                            drop(output);
                            std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
                                .map_err(|e| e.to_string())?;
                            ctx.checkpoint(
                                json!({"videoState":{"inFlight":false,"promptId":prompt_id}}),
                            )?;
                            return Ok(
                                json!({"engine":"comfyui","promptId":prompt_id,"bytes":total}),
                            );
                        }
                    }
                }
            }
            if result["status"]["completed"] == true {
                return Err(
                    "ComfyUI 완료 결과에 MP4가 없습니다. 영상 출력 노드를 확인하세요.".into(),
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
pub async fn run(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    let dir = directory(&ctx.state, &ctx.job_id)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = &job["input"];
    // Work may sit in a durable queue across pricing table releases. Never
    // submit a paid request when the refreshed estimate exceeds owner consent.
    if input["engine"]=="ltx" {
        let quote=if input["retake"].is_object(){
            let durations=input["retake"]["newPaidDurations"].as_array()
                .ok_or("장면 재생성 원가 기준이 없습니다.")?
                .iter().map(|v|v.as_u64().ok_or("장면 길이 형식 오류"))
                .collect::<Result<Vec<_>,_>>()?;
            pricing::quote_durations(input["ltxModel"].as_str().unwrap_or(""),
                input["ltxQuality"].as_str().unwrap_or(""),&durations)?
        }else{pricing::quote(&input["config"],input)?};
        if quote["priceVersion"]!=input["costQuote"]["priceVersion"] ||
            quote["estimatedKrw"].as_u64().unwrap_or(u64::MAX)>
                input["maxEstimatedKrw"].as_u64().unwrap_or(0) {
            return Err("needs_review: 영상 참조 견적이 변경되었거나 승인액을 초과했습니다. 유료 호출을 하지 않습니다.".into());
        }
    }
    let result = if input["engine"]=="ltx" && input["ltxFilm"]==true {
        film::run(ctx,input,&dir).await
    } else if input["engine"]=="ltx" {
        ltx::render(ctx,input,&dir).await
    } else if input["engine"] == "omni" {
        omni::render(ctx, job, &dir).await
    } else if input["engine"] == "veo" {
        Err("Veo Preview가 종료됐습니다. 새 Omni 작업으로 생성하세요.".into())
    } else if input["engine"] == "comfyui" && input["cinematic"] == true {
        cinematic::render(ctx, input, &dir).await
    } else if input["engine"] == "comfyui" {
        comfy(ctx, input, &dir).await
    } else {
        native(ctx, input, &dir).await
    };
    match result {
        Ok(mut value) => {
            let bytes = std::fs::metadata(dir.join("output.mp4"))
                .map_err(|e| e.to_string())?
                .len();
            if bytes < 100 {
                return Err("생성된 영상이 비어 있습니다.".into());
            }
            // A successful paid API call is not a proof of deliverable quality.
            // This independent local probe makes verification a real job gate.
            progress(ctx,"verifying",96)?;
            let check=quality::inspect(&input["config"],&dir.join("output.mp4"),
                input["duration"].as_u64().unwrap_or(15),
                input["aspectRatio"].as_str().unwrap_or("9:16"),
                if input["engine"]=="ltx" {700} else {quality::min_short_edge(input)},
                input["engine"]=="omni" || input["engine"]=="ltx").await;
            let receipt=match check {
                Ok(receipt)=>receipt,
                Err(reason)=>{
                    ctx.checkpoint(json!({"videoQuality":{"verified":false,
                        "error":reason,"visualQualityAssessed":false}}))?;
                    return Err(format!("영상 최종 기술 검수 실패: {reason}"));
                }
            };
            ctx.checkpoint(json!({"videoQuality":receipt}))?;
            value["qualityReceipt"]=receipt;
            value["bytes"] = json!(bytes);
            value["outputUrl"] = json!(format!("/api/video/jobs/{}/content", ctx.job_id));
            progress(ctx, "completed", 100)?;
            Ok(value)
        }
        Err(e) => {
            let _ = std::fs::remove_file(dir.join("output.part.mp4"));
            if ctx
                .state
                .store
                .get("job", &ctx.job_id)
                .is_some_and(|j| j["videoState"]["inFlight"] == true)
                && !e.starts_with("needs_review:")
            {
                Err(format!("needs_review: {e}"))
            } else {
                Err(e)
            }
        }
    }
}
/// Read-only scene diagnostics including SHA-256 for owner-pinned retakes.
pub fn film_scenes(state:&AppState,id:&str)->Result<Value,String>{
    uuid::Uuid::parse_str(id).map_err(|_|"유효하지 않은 영상 작업 ID")?;
    let job=state.store.get("job",id).ok_or("영상 작업이 없습니다.")?;
    film::scenes(state,&job)
}

/// A retake is a NEW durable video job, never a replay of an uncertain paid
/// submission. Deterministic job IDs make double-clicks/idempotent retries
/// refer to the same billable work rather than creating another order.
pub fn enqueue_retake(state:&AppState,id:&str,body:&Value)->Result<Value,String>{
    uuid::Uuid::parse_str(id).map_err(|_|"유효하지 않은 원본 영상 ID")?;
    let source=state.store.get("job",id).ok_or("원본 영상 작업을 찾지 못했습니다.")?;
    if body["confirmNewPaidCalls"]!=true {
        return Err("장면 재생성으로 발생하는 추가 유료 API 호출에 동의해야 합니다.".into());
    }
    let index=body["index"].as_u64().ok_or("장면 번호가 없습니다.")? as usize;
    let digest=body["expectedSha256"].as_str().ok_or("기존 장면 SHA-256이 없습니다.")?;
    let count=body["confirmedPaidCallCount"].as_u64().ok_or("추가 결제 장면 수에 동의해야 합니다.")?;
    let mut input=film::retake_input(state,&source,index,digest,
        body["prompt"].as_str(),count)?;
    let manifest=film::scenes(state,&source)?;
    let planned=input["render"]["film"]["clips"].as_array().ok_or("장면 계획 오류")?;
    let seen=manifest["clips"].as_array().ok_or("장면 영수증 오류")?;
    let mut pending_durations=Vec::new();
    for (position,clip) in planned.iter().enumerate(){
        if position==index || seen.get(position).is_none_or(|item|item["receiptVerified"]!=true){
            pending_durations.push(clip["duration"].as_u64().ok_or("유료 장면 길이 오류")?);
        }
    }
    if pending_durations.len() as u64!=count {
        return Err("장면 재생성 전 유료 호출 수가 변경됐습니다.".into());
    }
    let quote=pricing::quote_durations(
        input["ltxModel"].as_str().unwrap_or(""),
        input["ltxQuality"].as_str().unwrap_or(""),&pending_durations)?;
    let ceiling=body["maxEstimatedKrw"].as_u64();
    let expected=quote["estimatedKrw"].as_u64().unwrap_or(u64::MAX);
    if body["approvedPaidQuote"]!=true||ceiling.is_none_or(|max|max<expected){
        return Err(format!("장면 재생성 예상 비용 {expected}원을 승인해야 합니다."));
    }
    input["retake"]["newPaidDurations"]=json!(pending_durations);
    input["costQuote"]=quote;
    input["maxEstimatedKrw"]=json!(ceiling.unwrap_or(0));
    let fingerprint=input["retake"]["fingerprint"].as_str().ok_or("재생성 식별자가 없습니다.")?;
    let raw=sha2::Sha256::digest(fingerprint.as_bytes());
    let mut bytes=[0u8;16];
    bytes.copy_from_slice(&raw[..16]);
    bytes[6]=(bytes[6]&0x0f)|0x40;
    bytes[8]=(bytes[8]&0x3f)|0x80;
    let job_id=uuid::Uuid::from_bytes(bytes).to_string();
    let job=jobs::enqueue_once(state,&job_id,"video",input)
        .ok_or("재생성 작업을 저장하지 못했습니다.")?;
    Ok(public_job(&job))
}


pub fn enqueue_upload(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    output_path(state, id)?;
    let video = state.store.get("job", id).ok_or("영상이 없습니다.")?;
    if oauth::access_token(state, "youtube").is_none() {
        return Err("연결 메뉴에서 YouTube 업로드 계정을 먼저 연결하세요.".into());
    }
    let privacy = body["privacyStatus"].as_str().unwrap_or("private");
    if !matches!(privacy, "private" | "unlisted" | "public") {
        return Err("공개 범위 오류".into());
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("dominion-youtube:{id}"));
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    let upload_id = uuid::Uuid::from_bytes(uuid).to_string();
    let made_for_kids = body["madeForKids"].as_bool().unwrap_or(false);
    let title = body["title"].as_str().or_else(|| video["input"]["title"].as_str())
        .unwrap_or_default().trim();
    let description = body["description"].as_str().or_else(|| video["input"]["brief"].as_str())
        .unwrap_or_default();
    let tags = body["tags"].as_str().unwrap_or("").split(',')
        .map(str::trim).filter(|tag| !tag.is_empty()).collect::<Vec<_>>();
    if title.is_empty() || title.chars().count() > 100 || description.len() > 5000
        || tags.join(",").chars().count() > 500 {
        return Err("YouTube 제목, 설명, 태그 길이를 확인하세요.".into());
    }
    let candidate = json!({
        "id":upload_id,"kind":"youtube_upload","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"videoJobId":id,"privacyStatus":privacy,"madeForKids":made_for_kids,
                 "title":title,"description":description,"tags":tags},
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    let job = if let Some(inserted) = state.store.put_if_absent("job", &candidate) {
        state.bus.record("job", &inserted);
        state.store.audit(
            "job.queued",
            json!({"kind":"youtube_upload","jobId":upload_id}),
        );
        inserted
    } else {
        let existing = state
            .store
            .get("job", &upload_id)
            .ok_or("업로드 작업 조회 실패")?;
        if existing["status"] == "failed" && existing["videoState"]["inFlight"] != true {
            state.store.patch_if("job",&upload_id,"status",&json!("failed"),&json!({"status":"queued","error":null,"attempts":0,"queuedAt":chrono::Utc::now().to_rfc3339(),"input":candidate["input"]})).unwrap_or(existing)
        } else {
            existing
        }
    };
    Ok(public_job(&job))
}
pub async fn upload(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    if let Some(result) = job
        .pointer("/videoState/result")
        .filter(|r| r["youtubeId"].as_str().is_some_and(|s| !s.is_empty()))
    {
        return Ok(result.clone());
    }
    if job["videoState"]["inFlight"] == true {
        return Err("needs_review: 이전 업로드 결과를 먼저 확인하세요.".into());
    }
    let id = job["input"]["videoJobId"]
        .as_str()
        .ok_or("영상 작업 ID 필요")?;
    let path = output_path(&ctx.state, id)?;
    let video = ctx.state.store.get("job", id).ok_or("영상이 없습니다.")?;
    let token = oauth::fresh_access_token(&ctx.state, "youtube").await?;
    let size = tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?
        .len();
    if size > 256 * 1024 * 1024 {
        return Err("업로드 크기 제한은 256MiB입니다.".into());
    }
    let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size {
        return Err("업로드 중 파일 크기가 변경되었습니다. 다시 시도하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    progress(ctx, "authorizing", 5)?;
    let title = job["input"]["title"].as_str().or_else(|| video["input"]["title"].as_str());
    let description = job["input"]["description"].as_str().or_else(|| video["input"]["brief"].as_str());
    let tags = job["input"]["tags"].as_array().cloned().unwrap_or_default();
    let response=client.post("https://www.googleapis.com/upload/youtube/v3/videos").query(&[("uploadType","resumable"),("part","snippet,status")]).bearer_auth(&token).header("X-Upload-Content-Length",bytes.len()).header("X-Upload-Content-Type","video/mp4").json(&json!({"snippet":{"title":title,"description":description,"tags":tags},"status":{"privacyStatus":job["input"]["privacyStatus"],"selfDeclaredMadeForKids":job["input"]["madeForKids"].as_bool().unwrap_or(false)}})).timeout(Duration::from_secs(30)).send().await.map_err(|_|"YouTube 업로드 세션 연결 실패")?;
    if !response.status().is_success() {
        return Err(format!(
            "YouTube 업로드 세션 거부: HTTP {}",
            response.status()
        ));
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or("YouTube 업로드 주소가 없습니다.")?;
    let url = reqwest::Url::parse(location).map_err(|_| "YouTube 업로드 주소 오류")?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("www.googleapis.com" | "upload.youtube.com")
        )
    {
        return Err("YouTube가 반환한 업로드 호스트를 확인할 수 없습니다.".into());
    }
    ctx.checkpoint(json!({"phase":"uploading","progress":20,"videoState":{"inFlight":true}}))?;
    // Persist the opaque session URL encrypted. A crash after PUT is uncertain:
    // it is never automatically submitted again and cannot create duplicates.
    ctx.state
        .save_secret(&format!("youtube-upload:{}", ctx.job_id), location);
    let response = client
        .put(url)
        .bearer_auth(token)
        .header("Content-Type", "video/mp4")
        .header("Content-Length", bytes.len())
        .body(bytes)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|_| {
            "needs_review: YouTube 업로드 결과가 불확실합니다. 업로드 기록을 확인하세요."
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "needs_review: YouTube 업로드 응답 HTTP {}. 원격 상태를 확인하세요.",
            response.status()
        ));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: YouTube 업로드 결과 해석 실패")?;
    let youtube_id = value["id"]
        .as_str()
        .filter(|s| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .ok_or("needs_review: YouTube 영상 ID 없음")?;
    let result = json!({"youtubeId":youtube_id,"url":format!("https://www.youtube.com/watch?v={youtube_id}"),"privacyStatus":job["input"]["privacyStatus"]});
    ctx.checkpoint(
        json!({"phase":"completed","progress":100,"videoState":{"inFlight":false,"result":result}}),
    )?;
    ctx.state
        .store
        .remove_secret(&format!("youtube-upload:{}", ctx.job_id));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_ffmpeg_path_wins_over_current_settings() {
        let dir = std::env::temp_dir().join(format!("dominion-ffmpeg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("ffmpeg-custom");
        std::fs::write(&fake, b"binary").unwrap();
        let cfg = json!({"ffmpegPath": fake.to_string_lossy()});
        assert_eq!(ffmpeg_from_config(&cfg).as_deref(), Some(fake.as_path()));
        assert!(ffmpeg_from_config(&json!({"ffmpegPath":"/definitely/missing/ffmpeg"})).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn native_cards_render_unicode_and_escape_markup() {
        let dir =
            std::env::temp_dir().join(format!("dominion-video-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        make_cards(
            &dir,
            "테스트 <제목>",
            &["한글 자막 & 실제 프레임".into()],
            "9:16",
        )
        .unwrap();
        let bytes = std::fs::read(dir.join("card-00.png")).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        assert!(bytes.len() > 10000);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn template_substitution_never_changes_json_structure() {
        let mut value = json!({"1":{"inputs":{"text":"{{prompt}}","seconds":"{{duration}}"}}});
        substitute(&mut value, &json!({"prompt":"\"},evil:{","duration":15}));
        assert_eq!(value["1"]["inputs"]["text"], "\"},evil:{");
        assert_eq!(value["1"]["inputs"]["seconds"], 15);
        assert_eq!(value.as_object().unwrap().len(), 1);
    }
}
