//! FFmpeg pixel-level defect checks for short AI-generated shots.
//! A conservative deterministic gate, not a semantic/aesthetic evaluator.
//! This is deliberately separate from ffprobe metadata inspection.
use super::{ffmpeg_from_config, tools};
use serde_json::{json, Value};
use std::{path::Path, time::{Duration, Instant}};

fn field(line: &str, name: &str) -> Option<f64> {
    let tail = line.split_once(name)?.1.trim_start();
    tail.split_whitespace().next()?.trim_end_matches(']').parse::<f64>()
        .ok().filter(|n| n.is_finite() && *n >= 0.0)
}

/// FFmpeg blackdetect / freezedetect event ledger. A motionless composition
/// is sometimes intentional, so only near-entire-clip freezes are rejected.
/// All diagnostics remain visible in the actual shot quality receipt.
fn summarize(log: &str, seconds: f64) -> Value {
    let mut longest_black = 0.0_f64;
    let mut longest_freeze = 0.0_f64;
    let mut open_freeze: Option<f64> = None;
    let mut open_black: Option<f64> = None;
    for line in log.lines() {
        if let Some(start) = field(line, "black_start:") {
            open_black = Some(start);
        }
        if let Some(duration) = field(line, "black_duration:") {
            longest_black = longest_black.max(duration);
            open_black = None;
        }
        if let Some(start) = field(line, "freeze_start:") {
            open_freeze = Some(start);
        }
        if let Some(duration) = field(line, "freeze_duration:") {
            longest_freeze = longest_freeze.max(duration);
            open_freeze = None;
        } else if let Some(end) = field(line, "freeze_end:") {
            if let Some(start) = open_freeze.take() {
                longest_freeze = longest_freeze.max((end - start).max(0.0));
            }
        }
    }
    if let Some(start) = open_black {
        longest_black = longest_black.max((seconds - start).max(0.0));
    }
    if let Some(start) = open_freeze {
        longest_freeze = longest_freeze.max((seconds - start).max(0.0));
    }
    let black_fraction = (longest_black / seconds.max(0.1)).clamp(0.0, 1.0);
    let freeze_fraction = (longest_freeze / seconds.max(0.1)).clamp(0.0, 1.0);
    let black_defect = longest_black >= 3.0 && black_fraction >= 0.75;
    let freeze_defect = longest_freeze >= 5.0 && freeze_fraction >= 0.92;
    json!({
        "checked": true,
        "method": "ffmpeg-full-decode-blackdetect-freezedetect-v1",
        "longestBlackSeconds": longest_black,
        "longestFrozenSeconds": longest_freeze,
        "blackFraction": black_fraction,
        "frozenFraction": freeze_fraction,
        "blackDefect": black_defect,
        "frozenDefect": freeze_defect,
        "passed": !(black_defect || freeze_defect),
        "semanticContinuityAssessed": false
    })
}

/// Decode every video frame. Exit failure is itself a defect even if ffprobe
/// claims the MP4 has the expected size and duration. Never submits a video
/// generation request: this is a local-only inspection.
pub(super) async fn inspect(cfg: &Value, path: &Path, seconds: f64) -> Result<Value, String> {
    let ffmpeg = ffmpeg_from_config(cfg).ok_or("영상 프레임 검수에 FFmpeg가 필요합니다.")?;
    let mut command = tokio::process::Command::new(ffmpeg);
    command.kill_on_drop(true).args([
        "-hide_banner", "-nostdin", "-nostats", "-loglevel", "info",
        "-xerror", "-i"
    ]).arg(path).args([
        "-map", "0:v:0", "-an",
        "-vf", "blackdetect=d=1:pix_th=0.05:pic_th=0.98,freezedetect=n=-60dB:d=2",
        "-f", "null", "-"
    ]);
    let started = Instant::now();
    let output = tools::run_command(&mut command, Duration::from_secs(180)).await
        .map_err(|e| format!("영상 프레임 검수 실행 오류: {e}"))?;
    if !output.status.success() {
        return Err(format!("영상 프레임 디코딩 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>()));
    }
    if output.stderr.len() > 256 * 1024 {
        return Err("영상 프레임 검수 출력이 비정상적으로 큽니다.".into());
    }
    let log = String::from_utf8_lossy(&output.stderr);
    let mut receipt = summarize(&log, seconds);
    receipt["elapsedMs"] = json!(started.elapsed().as_millis() as u64);
    if receipt["blackDefect"] == true {
        return Err(format!("거의 검은 화면이 {:.1}초 지속돼 재검수가 필요합니다.",
            receipt["longestBlackSeconds"].as_f64().unwrap_or(0.0)));
    }
    if receipt["frozenDefect"] == true {
        return Err(format!("멈춘 프레임이 {:.1}초 지속돼 재검수가 필요합니다.",
            receipt["longestFrozenSeconds"].as_f64().unwrap_or(0.0)));
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_scene_does_not_trigger_defect() {
        let report=summarize("[blackdetect] black_start:0 black_end:0.6 black_duration:0.6\n[freezedetect] lavfi.freezedetect.freeze_start: 4\n[freezedetect] lavfi.freezedetect.freeze_end: 5 freeze_duration: 1",8.0);
        assert_eq!(report["passed"],true);
        assert_eq!(report["longestBlackSeconds"],0.6);
    }
    #[test]
    fn nearly_all_black_is_a_real_defect() {
        let report=summarize("[blackdetect] black_start:0 black_end:7.6 black_duration:7.6",8.0);
        assert_eq!(report["blackDefect"],true);
        assert_eq!(report["passed"],false);
    }
    #[test]
    fn unclosed_freeze_to_end_is_counted() {
        let report=summarize("[freezedetect] lavfi.freezedetect.freeze_start: 0",8.0);
        assert_eq!(report["frozenDefect"],true);
    }
    #[test]
    fn short_fade_and_deliberate_hold_are_not_over_rejected() {
        let report=summarize("[blackdetect] black_start:0 black_end:1.5 black_duration:1.5\n[freezedetect] lavfi.freezedetect.freeze_start: 2\n[freezedetect] lavfi.freezedetect.freeze_end: 6 freeze_duration: 4",8.0);
        assert_eq!(report["passed"],true);
    }
    #[test]
    fn duplicate_reports_do_not_add_lengths() {
        let report=summarize("black_duration:2\nblack_duration:2\nblack_duration:2",8.0);
        assert_eq!(report["longestBlackSeconds"],2.0);
    }
}
