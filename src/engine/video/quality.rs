//! Decode-proof media quality gate after paid remote generation.
//! This is a deterministic technical inspection, NOT a semantic film critic.
use super::*;
use serde_json::{json,Value};
use std::path::{Path,PathBuf};
use std::time::Duration;

pub(super) fn executable(cfg:&Value)->Option<PathBuf>{
    let ffmpeg=super::ffmpeg_from_config(cfg)?;
    let name=if cfg!(windows){"ffprobe.exe"}else{"ffprobe"};
    let path=ffmpeg.with_file_name(name);
    path.is_file().then_some(path)
}
fn expected_ratio(aspect:&str)->f64 {
    match aspect {"9:16"=>9.0/16.0,"1:1"=>1.0,_=>16.0/9.0}
}
fn duration(value:&Value)->Option<f64>{
    value.as_str().and_then(|s|s.parse::<f64>().ok())
        .or_else(||value.as_f64()).filter(|x|x.is_finite()&&*x>0.0)
}
pub(super) fn evaluate(probe:&Value,expected_seconds:u64,aspect:&str,require_audio:bool)->Result<Value,String>{
    evaluate_with_minimum(probe,expected_seconds,aspect,640,require_audio)
}
fn evaluate_with_minimum(probe:&Value,expected_seconds:u64,aspect:&str,min_short_edge:u64,require_audio:bool)->Result<Value,String>{
    let streams=probe["streams"].as_array().ok_or("ffprobe 영상 스트림 정보가 없습니다.")?;
    let video=streams.iter().find(|stream|stream["codec_type"]=="video")
        .ok_or("MP4에 재생 가능한 비디오 스트림이 없습니다.")?;
    let width=video["width"].as_u64().unwrap_or(0);
    let height=video["height"].as_u64().unwrap_or(0);
    if width<min_short_edge || height<min_short_edge {
        return Err(format!("해상도가 너무 낮습니다: {width}x{height}"));
    }
    let actual_ratio=width as f64/height as f64;
    let desired_ratio=expected_ratio(aspect);
    if (actual_ratio/desired_ratio-1.0).abs()>0.12 {
        return Err(format!("영상 화면비가 요청과 맞지 않습니다: {width}x{height} / {aspect}"));
    }
    let seconds=duration(&probe["format"]["duration"])
        .or_else(||duration(&video["duration"]))
        .ok_or("영상 재생 길이를 확인할 수 없습니다.")?;
    let allowance=(expected_seconds as f64*0.18).max(2.5);
    if (seconds-expected_seconds as f64).abs()>allowance {
        return Err(format!("영상 재생 시간이 요청 길이를 벗어났습니다: {seconds:.1}s / {expected_seconds}s"));
    }
    let audio=streams.iter().any(|s|s["codec_type"]=="audio");
    if require_audio && !audio {
        return Err("오디오를 요청했지만 생성 파일에 오디오 스트림이 없습니다.".into());
    }
    Ok(json!({"verified":true,"durationSeconds":seconds,"width":width,"height":height,
        "ratio":aspect,"audioStream":audio,"visualQualityAssessed":false,
        "requiredShortEdge":min_short_edge,"verification":"ffprobe-stream-duration-aspect-v1"}))
}

pub(super) fn min_short_edge(input:&Value)->u64{
    input["render"]["width"].as_u64().unwrap_or(360)
        .min(input["render"]["height"].as_u64().unwrap_or(640))
        .saturating_mul(3)/4
}
pub(super) async fn inspect(cfg:&Value,path:&Path,seconds:u64,aspect:&str,audio:bool)->Result<Value,String>{
    inspect_clip(cfg,path,seconds,aspect,640,audio).await
}
pub(super) async fn inspect_clip(cfg:&Value,path:&Path,seconds:u64,aspect:&str,min_short_edge:u64,audio:bool)->Result<Value,String>{
    let probe=executable(cfg).ok_or("영상 검수에는 FFmpeg와 같은 폴더의 ffprobe가 필요합니다.")?;
    let mut command=tokio::process::Command::new(probe);
    command.kill_on_drop(true).args(["-v","error","-show_entries",
        "format=duration:stream=codec_type,width,height,duration","-of","json"])
        .arg(path);
    let output=tokio::time::timeout(Duration::from_secs(30),command.output())
        .await.map_err(|_|"ffprobe 영상 검수 시간이 초과됐습니다.")?
        .map_err(|e|format!("ffprobe 검수 실행 오류: {e}"))?;
    if !output.status.success() {
        return Err(format!("ffprobe 검수 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(400).collect::<String>()));
    }
    if output.stdout.len()>32768 {return Err("영상 검수 출력이 비정상적으로 큽니다.".into());}
    let report:Value=serde_json::from_slice(&output.stdout)
        .map_err(|e|format!("ffprobe JSON 해석 오류: {e}"))?;
    evaluate_with_minimum(&report,seconds,aspect,min_short_edge,audio)
}

#[cfg(test)]
#[path = "../../../tests/unit/video_quality_gate_tests.rs"]
mod tests;
