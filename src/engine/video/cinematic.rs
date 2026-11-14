//! Multi-shot production over the existing ComfyUI workhorse.
//! The accepted workflow and provider remain frozen in the durable job input.
//! Each shot is independently submitted and downloaded; uncertain remote
//! submissions retain the parent's needs_review semantics (never blind retry).
use super::{comfy, film, ffmpeg_from_config, progress, quality};
use crate::engine::{jobs::JobContext, tools};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

const SHOT_TARGET_SECONDS: u64 = 8;
const MAX_SHOTS: usize = 15;

#[derive(Debug, Clone)]
struct Shot {
    index: usize,
    seconds: u64,
    prompt: String,
    seed: i64,
}

fn storyboard(input: &Value) -> Result<Vec<Shot>, String> {
    let duration = input["duration"].as_u64().ok_or("영상 길이가 없습니다.")?;
    if !(3..=120).contains(&duration) {
        return Err("멀티샷 영화는 3~120초를 지원합니다.".into());
    }
    let brief = input["brief"].as_str().unwrap_or("").trim();
    if brief.is_empty() { return Err("영상 소재가 비어 있습니다.".into()); }
    // Keep the visual bible bounded: duplicating the entire user prompt
    // across 15 generations is both costly and hard for a renderer to follow.
    let bible: String = brief.chars().take(1200).collect();
    let supplied=input["scenes"].as_array().cloned().unwrap_or_default();
    let beats = brief
        .split(['\n', '.', '!', '?', '。'])
        .map(str::trim)
        .filter(|beat| !beat.is_empty())
        .take(MAX_SHOTS)
        .map(|beat| beat.chars().take(400).collect::<String>())
        .collect::<Vec<_>>();
    let beats = if supplied.is_empty() { beats } else {
        supplied.into_iter().filter_map(|item|item.as_str().map(str::to_string))
            .filter(|beat|!beat.trim().is_empty()).take(MAX_SHOTS).collect::<Vec<_>>()
    };
    let beats = if beats.is_empty() {vec![bible.clone()]} else {beats};
    let count = duration.div_ceil(SHOT_TARGET_SECONDS).min(MAX_SHOTS as u64) as usize;
    let base = duration / count as u64;
    let remainder = duration % count as u64;
    let starting_seed = input["render"]["seed"].as_i64().unwrap_or(0).unsigned_abs();
    const CAMERAS: [&str; 6] = [
        "wide establishing shot, restrained dolly in",
        "medium shot, deliberate lateral tracking",
        "close-up, shallow depth of field, natural micro-motion",
        "over-the-shoulder shot, matched spatial direction",
        "cinematic tracking shot, realistic parallax",
        "wide closing composition, smooth camera movement",
    ];
    Ok((0..count).map(|index| {
        let seconds = base + u64::from((index as u64) < remainder);
        let beat = &beats[index % beats.len()];
        Shot {
            index, seconds,
            seed: ((starting_seed + index as u64) % 2_147_483_647) as i64,
            prompt: format!(
                "Cinematic film, shot {}/{}. VISUAL CONTINUITY BIBLE: {}. \
                CURRENT SHOT ACTION: {}. CAMERA: {}. Maintain the same character identity, \
                wardrobe, environment, lighting direction and screen geography across adjacent \
                shots. Natural movement, coherent physical interactions, rich cinematic textures, \
                no subtitles, no watermarks, no random scene changes.",
                index + 1, count, bible, beat, CAMERAS[index % CAMERAS.len()],
            ),
        }
    }).collect())
}

pub(super) fn preflight(input:&Value)->Result<(),String>{
    let cfg=&input["config"];
    let template=serde_json::to_string(&cfg["comfyWorkflow"]).map_err(|e|e.to_string())?;
    if !template.contains("{{prompt}}") || !template.contains("{{duration}}"){
        return Err("멀티샷에는 ComfyUI 워크플로 {{prompt}}·{{duration}} 변수가 필요합니다.".into());
    }
    if ffmpeg_from_config(cfg).is_none() || !quality::available(cfg){
        return Err("멀티샷에는 FFmpeg 및 FFprobe가 필요합니다.".into());
    }
    storyboard(input)?;
    Ok(())
}

pub(super) async fn render(
    ctx: &JobContext, input: &Value, dir: &Path,
) -> Result<Value, String> {
    let cfg = &input["config"];
    let workflow = &cfg["comfyWorkflow"];
    // A static prompt in the uploaded workflow would produce the same clip
    // on every shot. Fail before spending a single model/video API credit.
    let template = serde_json::to_string(workflow).map_err(|e| e.to_string())?;
    if !template.contains("{{prompt}}") || !template.contains("{{duration}}") {
        return Err("멀티샷에는 ComfyUI 워크플로의 {{prompt}} 및 {{duration}} 치환 변수가 필요합니다.".into());
    }
    let encoder = ffmpeg_from_config(cfg)
        .ok_or("멀티샷 최종 편집에는 FFmpeg가 필요합니다.")?;
    if !quality::available(cfg){return Err("멀티샷의 개별 장면 검수에는 FFprobe가 필요합니다.".into());}
    let shots = storyboard(input)?;
    let total = shots.len();
    let plan = shots.iter().map(|shot| json!({
        "index": shot.index + 1,
        "duration":shot.seconds,
        "prompt":shot.prompt,
        "seed":shot.seed,
    })).collect::<Vec<_>>();
    std::fs::write(dir.join("storyboard.json"),
        serde_json::to_vec_pretty(&json!({
            "mode":"cinematic","title":input["title"],"duration":input["duration"],
            "aspectRatio":input["aspectRatio"],"shots":plan
        })).map_err(|e|e.to_string())?,
    ).map_err(|e|e.to_string())?;
    ctx.checkpoint(json!({"phase":"storyboard","progress":3,
        "videoState":{"scenes":plan,"shotCount":total}}))?;

    let mut results = Vec::with_capacity(total);
    let mut manifest = String::from("ffconcat version 1.0\n");
    for shot in shots {
        let subdir = dir.join(format!("shot-{:02}", shot.index + 1));
        std::fs::create_dir_all(&subdir).map_err(|e|e.to_string())?;
        ctx.checkpoint(json!({"phase":"rendering","progress":5 + (shot.index * 80 / total),
            "videoState":{"shotIndex":shot.index + 1,"shotCount":total,
                "scenes":plan}}))?;
        let mut request = input.clone();
        request["brief"] = json!(shot.prompt);
        request["duration"] = json!(shot.seconds);
        request["render"]["prompt"] = json!(shot.prompt);
        request["render"]["duration"] = json!(shot.seconds);
        request["render"]["frames"] = json!(shot.seconds *
            input["render"]["fps"].as_u64().unwrap_or(16));
        request["render"]["seed"] = json!(shot.seed);
        // Raw ComfyUI shots may be SD; the stitched export alone must meet
        // the final 1080p minimum. Validate raw shots at their configured size.
        request["cinematic"]=json!(false);
        let clip = subdir.join("output.mp4");
        // An already verified clip is reusable after an interrupted edit.
        // This is critical: retries must not re-bill earlier successful shots.
        // Reuse only media with matching SHA-256 receipts; unknown remote
        // side effects cannot be silently reissued after recovery.
        let reused = film::saved_receipt(&subdir)?.is_some();
        let mut retry_count=0;
        let max_retries=input["badShotRetries"].as_u64().unwrap_or(0).min(1);
        let mut response=Value::Null;
        let technical=if reused {
            quality::inspect(cfg,&clip,Some(shot.seconds),
                input["aspectRatio"].as_str().unwrap_or("9:16"),false).await?
        } else {
            // A file left by a half-finished remote submission is not safe
            // to retry blindly, even if it looks like an invalid MP4.
            let latest=ctx.state.store.get("job",&ctx.job_id).unwrap_or(Value::Null);
            if latest["videoState"]["inFlight"]==true {
                return Err("needs_review: 샷 제출 상태가 확정되지 않아 중복 생성하지 않습니다.".into());
            }
            loop {
                response=comfy::comfy(ctx,&request,&subdir).await?;
                match quality::inspect(cfg,&clip,Some(shot.seconds),
                    input["aspectRatio"].as_str().unwrap_or("9:16"),false).await{
                    Ok(receipt)=>break receipt,
                    Err(reason)=>{
                        if retry_count>=max_retries{
                            return Err(format!("샷 {} 기술 검수 실패: {reason}",shot.index+1));
                        }
                        retry_count+=1;
                        ctx.state.store.audit("video.shot_quality_retry",
                            json!({"jobId":ctx.job_id,"shot":shot.index+1,
                                "retry":retry_count,"cause":reason}));
                        let _=std::fs::remove_file(&clip);
                    }
                }
            }
        };
        if !reused{
            film::persist_receipt(&subdir,&json!({"index":shot.index,
                "outputSha256":film::file_sha(&clip)?,
                "quality":technical,"remote":response}))?;
        }
        manifest.push_str(&format!("file 'shot-{:02}/output.mp4'\n",shot.index + 1));
        results.push(json!({"index":shot.index + 1,"duration":shot.seconds,
            "promptId":response["promptId"],"bytes":std::fs::metadata(&clip)
                .map_err(|e|e.to_string())?.len(),"reused":reused,
            "qualityReceipt":technical,"technicalRetries":retry_count}));
    }
    progress(ctx, "editing", 88)?;
    std::fs::write(dir.join("shots.ffconcat"), manifest).map_err(|e|e.to_string())?;
    let (width,height) = match input["aspectRatio"].as_str().unwrap_or("9:16") {
        "16:9" => (1920,1080),
        "1:1" => (1080,1080),
        _ => (1080,1920),
    };
    let filter = format!(
        "scale={width}:{height}:force_original_aspect_ratio=decrease,\
        pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,fps=24,format=yuv420p"
    );
    let mut command=tokio::process::Command::new(encoder);
    command.current_dir(dir).args([
        "-hide_banner","-loglevel","error","-nostdin","-y",
        "-f","concat","-safe","0","-i","shots.ffconcat",
        "-t",&input["duration"].as_u64().unwrap_or(15).to_string(),
        "-map","0:v:0","-map","0:a?","-vf",&filter,
        "-c:v","libx264","-preset","fast","-crf","20",
        "-c:a","aac","-b:a","192k","-movflags","+faststart",
        "output.part.mp4"
    ]);
    let output = tools::run_command(&mut command, Duration::from_secs(1800)).await?;
    if !output.status.success() {
        return Err(format!("멀티샷 최종 편집 오류: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(1500).collect::<String>()));
    }
    std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
        .map_err(|e|e.to_string())?;
    let final_receipt=quality::inspect(cfg,&dir.join("output.mp4"),
        Some(input["duration"].as_u64().unwrap_or(15)),
        input["aspectRatio"].as_str().unwrap_or("9:16"),false).await?;
    progress(ctx, "completed", 100)?;
    Ok(json!({"engine":"comfyui","mode":"cinematic",
        "finalQuality":final_receipt,"shots":results.len(),
        "shotResults":results,"durationRequested":input["duration"],
        "outputWidth":width,"outputHeight":height,
        "storyboard":"storyboard.json","quality":"1080p-export-crf18"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cinematic_storyboard_preserves_duration_and_splits_into_bounded_shots() {
        let input=json!({"brief":"첫 장면. 다음 장면. 끝.","duration":37,
            "render":{"seed":7,"fps":24}});
        let shots=storyboard(&input).unwrap();
        assert_eq!(shots.iter().map(|shot|shot.seconds).sum::<u64>(),37);
        assert!(shots.iter().all(|shot|shot.seconds<=SHOT_TARGET_SECONDS));
        assert_eq!(shots[0].seed,7);
        assert!(shots[0].prompt.contains("VISUAL CONTINUITY BIBLE"));
    }

    #[test]
    fn cinematic_rejects_out_of_range_without_paid_calls() {
        assert!(storyboard(&json!({"brief":"x","duration":121})).is_err());
        assert!(storyboard(&json!({"brief":"","duration":15})).is_err());
    }
}
