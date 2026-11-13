//! One deterministic, auditable Kling 3.0 shot plan per paid clip.
//! Up to three camera beats share a continuity anchor. The API's native
//! multiPrompt keeps these beats in one charged generation, instead of
//! pretending several unrelated clips form a coherent camera take.
use serde_json::{json, Value};

const DIRECTIONS: [&str; 3] = [
    "Establish the geography and subject with a deliberate cinematic wide or medium shot.",
    "Follow the SAME subject and wardrobe through the next physical action; purposeful moving camera.",
    "Resolve the beat with a motivated close-up or reveal; maintain the SAME location, light and subject.",
];

fn clip(text: &str, max_chars: usize) -> String {
    text.trim().chars().take(max_chars).collect()
}

/// Every shot is at least one second. The native Kling multi-shot limit is
/// six shots, 3..15 seconds total; we intentionally use <=3 for longer beats.
pub(super) fn shots(brief: &str, scene: &str, duration: u64, index: usize, total: usize) -> Vec<Value> {
    let count: usize = if duration >= 10 { 3 } else if duration >= 6 { 2 } else { 1 };
    let anchor = clip(brief, 150);
    let action = clip(scene, 205);
    let base = duration / count as u64;
    let remainder = duration % count as u64;
    (0..count)
        .map(|i| {
            let duration = base + u64::from((i as u64) < remainder);
            let shot = format!(
                "Film {}/{}, shot {}/{}. One continuous cinematic world. Character, costume, setting and lighting continuity: {}. Current story action: {}. {} Natural physical movement and scene-matched ambience. No overlays or captions unless requested.",
                index + 1, total, i + 1, count, anchor, action, DIRECTIONS[i]
            );
            // Kling rejects individual multiPrompt entries over 512 chars.
            json!({"prompt": clip(&shot, 510), "duration": duration})
        })
        .collect()
}

/// Use documented Runway Dev providerSettings.multiPrompt when there is more
/// than one camera beat. With this field, promptText must be omitted.
/// This API call still generates ONE clip, preserves one remote task ID, and
/// cannot silently increase paid seconds.
pub(super) fn request(
    model: &str, ratio: &str, duration: u64, audio: bool, shots: &[Value]
) -> Value {
    let mut payload = json!({
        "model": model, "ratio": ratio, "duration": duration, "audio": audio
    });
    if shots.len() > 1 {
        payload["providerSettings"] = json!({"multiPrompt": shots});
    } else {
        payload["promptText"] = shots.first()
            .and_then(|shot| shot["prompt"].as_str())
            .unwrap_or("")
            .into();
    }
    payload
}

#[cfg(test)]
#[path = "../../../tests/unit/video_shot_plan_tests.rs"]
mod tests;
