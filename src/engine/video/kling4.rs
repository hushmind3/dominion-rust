//! Kling 4.0 Runway Dev contract (released 2026-10-23).
//! 30-second native audio clips use promptText, NOT Kling 3 multiPrompt.
//! Image-to-video only accepts auto_* resolution tokens; the first frame
//! establishes aspect and reference consistency for a later clip.
use serde_json::{json, Value};

pub(super) const MAX_CLIP_SECONDS: u64 = 30;

pub(super) fn clip_durations(seconds: u64) -> Result<Vec<u64>, String> {
    if !(3..=180).contains(&seconds) {
        return Err("Kling 4.0 영상은 3~180초로 설정하세요.".into());
    }
    let count = seconds.div_ceil(MAX_CLIP_SECONDS);
    let base = seconds / count;
    let rem = seconds % count;
    let clips = (0..count)
        .map(|i| base + u64::from(i < rem))
        .collect::<Vec<_>>();
    if clips.iter().any(|s| !(3..=MAX_CLIP_SECONDS).contains(s)) {
        return Err("Kling 4.0 원본 클립 길이를 나눌 수 없습니다.".into());
    }
    Ok(clips)
}

fn text_ratio(aspect: &str) -> &'static str {
    match aspect {
        "9:16" => "1080:1920",
        "1:1" => "1440:1440",
        _ => "1920:1080",
    }
}

pub(super) fn direction(brief: &str, scene: &str, index: usize, count: usize) -> String {
    let anchor = brief.trim().chars().take(1200).collect::<String>();
    let action = scene.trim().chars().take(1900).collect::<String>();
    format!(
        "Premium cinematic film with synchronized natural dialogue, foley and ambience. \
         Precise realistic motion, anatomically coherent subjects, deliberate camera blocking, \
         high dynamic range, stable color science. One internally continuous sequence. \
         Maintain the same cast, clothing, props, setting and lighting across clips. \
         Film-wide character and art direction: {anchor}. \
         Clip {}/{} narrative beat: {action}. \
         Begin from the preceding clip's visual state when a first frame is provided. \
         Do not add text, logos, captions or transitions unless requested.",
        index + 1, count,
    )
    .chars()
    .take(4600)
    .collect()
}

/// Text-to-video: 1080p ratio is a pixel size. This endpoint has no
/// configurable audio=false mode on Kling 4.0; do not send unknown fields.
pub(super) fn request(aspect: &str, duration: u64, prompt: &str) -> Value {
    json!({
        "model": "kling_4",
        "ratio": text_ratio(aspect),
        "duration": duration,
        "promptText": prompt
    })
}

/// Image-to-video follows the conditioning image. Pixel ratios are rejected.
pub(super) fn attach_prior_frame(payload: &mut Value, jpeg_data_uri: String) {
    payload["ratio"] = json!("auto_1080p");
    payload["promptImage"] = json!(jpeg_data_uri);
}

#[cfg(test)]
#[path = "../../../tests/unit/video_kling4_contract_tests.rs"]
mod tests;
