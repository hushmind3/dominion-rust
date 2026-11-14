//! Best-effort, local, GPU-free visual diagnostics for generated clips.
//!
//! This runs AFTER the existing ffprobe structural gate. Black/frozen images
//! can be intentional creative choices: report warnings, never assert semantic
//! failure or automatically buy a replacement clip.
use serde_json::{json,Value};
use std::{path::Path,time::Duration};

fn metric(line:&str,name:&str)->Option<f64>{
    let (_,tail)=line.split_once(name)?;
    tail.split_whitespace().next()?.parse::<f64>().ok()
        .filter(|number|number.is_finite()&&*number>=0.0)
}

/// Parse only FFmpeg's measured durations. Neither raw process output nor
/// user-controlled filenames are returned in the API.
fn summarize(log:&str,window:u64)->Value{
    let mut black=0.0f64;
    let mut frozen=0.0f64;
    let mut black_intervals=0usize;
    let mut frozen_intervals=0usize;
    for line in log.lines(){
        if let Some(value)=metric(line,"black_duration:"){
            black+=value;
            black_intervals+=1;
        }
        if let Some(value)=metric(line,"freeze_duration:"){
            frozen+=value;
            frozen_intervals+=1;
        }
    }
    let length=window.max(1) as f64;
    let black=black.min(length);
    let frozen=frozen.min(length);
    let possible=black>=2.0 && black/length>=0.30
        ||frozen>=3.0 && frozen/length>=0.55;
    json!({
        "status":"measured",
        "source":"local_ffmpeg_blackdetect_freezedetect_v1",
        "sampledSeconds":window,
        "blackSeconds":(black*100.0).round()/100.0,
        "frozenSeconds":(frozen*100.0).round()/100.0,
        "blackIntervals":black_intervals,
        "frozenIntervals":frozen_intervals,
        "potentialVisualDefect":possible,
        "semanticQualityVerified":false,
        "interpretation":"advisory_only_owner_may_intend_dark_or_still_frames"
    })
}

/// Scan at most twenty seconds per video. An unavailable filter does NOT
/// invalidate an already-paid artifact or cause a second provider request.
pub(super) async fn inspect(cfg:&Value,path:&Path,seconds:u64)->Value{
    let Some(ffmpeg)=super::ffmpeg_from_config(cfg) else {
        return json!({"status":"unavailable","reason":"ffmpeg_missing"});
    };
    let window=seconds.clamp(1,20);
    let mut command=tokio::process::Command::new(ffmpeg);
    command.kill_on_drop(true).args([
        "-hide_banner","-loglevel","info","-nostats","-y",
        "-t",&window.to_string(),"-i"
    ]).arg(path).args([
        "-map","0:v:0","-vf",
        "blackdetect=d=0.75:pix_th=0.10,freezedetect=n=-60dB:d=1.5",
        "-an","-sn","-f","null","-"
    ]);
    match tokio::time::timeout(Duration::from_secs(35),command.output()).await {
        Ok(Ok(output)) if output.status.success()&&output.stderr.len()<=512*1024 =>
            summarize(&String::from_utf8_lossy(&output.stderr),window),
        Ok(Ok(_)) => json!({"status":"unavailable","reason":"filter_or_output_unavailable"}),
        Ok(Err(_)) => json!({"status":"unavailable","reason":"process_failed"}),
        Err(_) => json!({"status":"unavailable","reason":"time_budget"}),
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn flags_long_black_or_frozen_runs_without_claiming_semantic_quality(){
        let dark=summarize("[blackdetect] black_start:1 black_end:8 black_duration:7",10);
        assert_eq!(dark["potentialVisualDefect"],true);
        assert_eq!(dark["semanticQualityVerified"],false);
        let freeze=summarize("[freezedetect] lavfi.freezedetect.freeze_duration: 7.5",10);
        assert_eq!(freeze["potentialVisualDefect"],true);
    }
    #[test]
    fn short_stills_and_normal_clips_are_not_failed(){
        let brief=summarize("[blackdetect] black_duration:1.0\n[freezedetect] freeze_duration:1.9",10);
        assert_eq!(brief["potentialVisualDefect"],false);
        let clean=summarize("frame=200 fps=20\nvideo:1024kB",8);
        assert_eq!(clean["potentialVisualDefect"],false);
        assert_eq!(clean["blackSeconds"],0.0);
    }
    #[test]
    fn malformed_metrics_and_infinite_values_do_not_produce_unbounded_results(){
        let corrupt=summarize("black_duration:NaN\nfreeze_duration:inf\nblack_duration:40000",8);
        assert_eq!(corrupt["blackSeconds"],8.0);
        assert_eq!(corrupt["frozenSeconds"],0.0);
    }
}