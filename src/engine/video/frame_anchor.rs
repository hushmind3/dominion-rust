//! Turns the last independently downloaded video frame into a verified
//! Runway image_to_video first-frame anchor. No remote storage or model weights.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};

const MAX_JPEG_BYTES: u64 = 3 * 1024 * 1024;

/// Called only AFTER clip N is durably downloaded, BEFORE submitting N+1.
/// Extraction failure must not charge a different remote generation.
pub(super) async fn first_frame_for_next(
    ctx: &JobContext, cfg: &Value, dir: &Path, previous_index: usize,
) -> Result<String, String> {
    let encoder = super::ffmpeg_from_config(cfg)
        .ok_or("연속 장면 생성에는 FFmpeg가 필요합니다.")?;
    let previous = dir.join(format!("clip-{previous_index:02}.mp4"));
    let jpg = dir.join(format!("continuity-{previous_index:02}.jpg"));
    if !previous.is_file() {
        return Err("이전 영상 클립이 없어 연속 장면을 생성할 수 없습니다.".into());
    }
    let mut command = tokio::process::Command::new(encoder);
    command.current_dir(dir).args([
        "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
        "-sseof", "-0.25", "-i",
    ]).arg(&previous).args([
        "-frames:v", "1", "-update", "1",
        "-vf", "scale=1280:-2:force_original_aspect_ratio=decrease",
        "-q:v", "3",
    ]).arg(&jpg);
    let running = tools::run_command(&mut command, Duration::from_secs(70));
    tokio::pin!(running);
    let result = loop {
        tokio::select! {
            answer = &mut running => break answer?,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                ctx.checkpoint(json!({}))?;
            }
        }
    };
    if !result.status.success() {
        return Err(format!("연속 장면 프레임 추출 실패: {}",
            String::from_utf8_lossy(&result.stderr).chars().take(350).collect::<String>()));
    }
    let size = std::fs::metadata(&jpg).map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_JPEG_BYTES {
        return Err("연속 장면 참조 이미지가 API 크기 제한을 벗어났습니다.".into());
    }
    let bytes = std::fs::read(&jpg).map_err(|e| e.to_string())?;
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Err("연속 장면의 JPEG 참조 프레임이 올바르지 않습니다.".into());
    }
    // 3 MiB of JPEG encodes below Runway Dev's 5 MiB data-URI limit.
    Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
}
