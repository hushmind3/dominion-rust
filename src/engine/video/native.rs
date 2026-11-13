//! Rust storyboard and FFmpeg card render, separate from cloud video generation.
use super::*;

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
pub(super) fn make_cards(dir: &Path, title: &str, scenes: &[String], aspect: &str) -> Result<(), String> {
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
pub(super) async fn native(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
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
