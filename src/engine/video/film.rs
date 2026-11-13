//! Durable LTX multi-clip film generation; no local model or GPU.
//! Each paid submission is checkpointed. Uncertain requests are never retried.
use super::{director, ffmpeg_from_config, ltx};
use crate::engine::jobs::JobContext;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

const MAX_CLIPS: usize = 6;

fn split_duration(total: u64, maximum: u64) -> Result<Vec<u64>, String> {
    if total < 6 || total % 2 != 0 || maximum < 6 {
        return Err("LTX 영상 길이는 6초 이상의 짝수여야 합니다.".into());
    }
    let count = total.div_ceil(maximum) as usize;
    if count == 0 || count > MAX_CLIPS || total < count as u64 * 6 {
        return Err(format!("최대 {MAX_CLIPS}개 클립까지 생성할 수 있습니다."));
    }
    let mut pieces = vec![6u64; count];
    let mut remaining = total - count as u64 * 6;
    for piece in &mut pieces {
        let extra = (maximum - 6).min(remaining);
        *piece += extra;
        remaining -= extra;
    }
    if remaining != 0 || pieces.iter().any(|duration| *duration > maximum) {
        return Err("영상 길이를 구성할 수 없습니다.".into());
    }
    Ok(pieces)
}

/// Freeze all paid scene inputs at enqueue, independent of future settings.
pub(super) fn spec(body: &Value, cfg: &Value, total: u64) -> Result<Value, String> {
    let model = body["videoModel"].as_str()
        .or(cfg["videoModel"].as_str()).unwrap_or("ltx-2-5-pro");
    let max_duration = match model {
        "ltx-2-5-pro" => 10,
        "ltx-2-5-fast" => 20,
        _ => return Err("지원하지 않는 LTX 모델입니다.".into()),
    };
    let parts = split_duration(total, max_duration)?;
    if parts.len() > 1 && body["confirmPaidMultiShot"] != true {
        return Err(format!("LTX 유료 영상 생성 {}회에 동의해야 합니다.",parts.len()));
    }
    let brief = body["brief"].as_str().unwrap_or("");
    let notes = body["shotNotes"].as_str().unwrap_or("");
    let cinematic = body["cinematic"].as_bool().unwrap_or(true);
    let mut clips = Vec::with_capacity(parts.len());
    for (index, duration) in parts.iter().enumerate() {
        let prompt = director::shot_prompt(brief, notes, index, parts.len(), cinematic)?;
        let mut request = body.clone();
        request["duration"] = json!(*duration);
        request["brief"] = json!(prompt);
        let validated = ltx::spec(&request, cfg)?;
        clips.push(json!({"index":index,"duration":duration,"prompt":validated["prompt"]}));
    }
    let mut first = body.clone();
    first["duration"] = json!(parts[0]);
    first["brief"] = clips[0]["prompt"].clone();
    let mut spec = ltx::spec(&first, cfg)?;
    spec["film"] = json!({"version":1,"duration":total,"clips":clips,"paidCalls":parts.len()});
    Ok(spec)
}

pub(super) async fn run(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let clips = input["render"]["film"]["clips"].as_array()
        .ok_or("동결된 장면 계획이 없습니다.")?;
    if clips.len() < 2 || clips.len() > MAX_CLIPS {
        return Err("장면 개수가 허용 범위를 벗어났습니다.".into());
    }
    let ffmpeg = ffmpeg_from_config(&input["config"])
        .ok_or("장편 영상 합성에 FFmpeg가 필요합니다.")?;
    let mut receipts = Vec::new();
    for (index, clip) in clips.iter().enumerate() {
        ctx.checkpoint(json!({}))?;
        let duration = clip["duration"].as_u64().ok_or("장면 길이가 없습니다.")?;
        let prompt = clip["prompt"].as_str().ok_or("장면 프롬프트가 없습니다.")?;
        let clip_dir = dir.join(format!("clip-{index:02}"));
        std::fs::create_dir_all(&clip_dir).map_err(|e|e.to_string())?;
        // LTX's downloader verifies MP4 before atomic output.mp4 rename.
        let completed = std::fs::metadata(clip_dir.join("output.mp4"))
            .is_ok_and(|metadata| metadata.len() > 1024);
        if !completed {
            let mut frozen = input.clone();
            frozen["render"]["film"] = Value::Null;
            frozen["render"]["duration"] = json!(duration);
            frozen["render"]["prompt"] = json!(prompt);
            ctx.checkpoint(json!({"phase":"rendering",
                "progress":5+80*index/clips.len(),
                "videoState":{"filmClip":index+1,"filmTotal":clips.len(),"inFlight":false}}))?;
            receipts.push(ltx::run(ctx,&frozen,&clip_dir).await?);
        }
        ctx.checkpoint(json!({"phase":"rendering",
            "progress":5+80*(index+1)/clips.len(),
            "videoState":{"filmCompleted":index+1,"filmTotal":clips.len(),"inFlight":false}}))?;
    }
    let mut playlist = String::new();
    for index in 0..clips.len() {
        playlist.push_str(&format!("file 'clip-{index:02}/output.mp4'\n"));
    }
    std::fs::write(dir.join("clips.txt"),playlist).map_err(|e|e.to_string())?;
    ctx.checkpoint(json!({"phase":"finalizing","progress":90,"videoState":{"inFlight":false}}))?;
    // Same model/resolution/FPS/audio: stream-copy avoids losing 4K detail.
    let child = tokio::process::Command::new(ffmpeg)
        .current_dir(dir)
        .args(["-hide_banner","-loglevel","error","-y","-f","concat","-safe","0",
               "-i","clips.txt","-c","copy","-movflags","+faststart","output.part.mp4"])
        .kill_on_drop(true).output();
    let result = tokio::time::timeout(Duration::from_secs(900),child).await
        .map_err(|_|"FFmpeg 합성 제한시간 초과")?
        .map_err(|e|format!("FFmpeg 실행 오류: {e}"))?;
    if !result.status.success() {
        return Err(format!("FFmpeg 합성 오류: {}",String::from_utf8_lossy(&result.stderr)
            .chars().take(400).collect::<String>()));
    }
    let staged = dir.join("output.part.mp4");
    if !std::fs::metadata(&staged).is_ok_and(|metadata| metadata.len()>1024) {
        return Err("영상 합성 파일이 비어 있습니다.".into());
    }
    std::fs::rename(staged,dir.join("output.mp4")).map_err(|e|e.to_string())?;
    Ok(json!({"engine":"ltx","mode":"film","model":input["render"]["model"],
        "duration":input["render"]["film"]["duration"],"shots":clips.len(),
        "providerReceipts":receipts,"audio":input["render"]["generate_audio"],
        "quality":input["render"]["quality"]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pro_segments_are_supported_even_durations() {
        assert_eq!(split_duration(22,10).unwrap(),vec![10,6,6]);
        assert_eq!(split_duration(60,10).unwrap(),vec![10,10,10,10,10,10]);
        assert_eq!(split_duration(38,20).unwrap(),vec![20,18]);
        assert!(split_duration(62,10).is_err());
    }
}
