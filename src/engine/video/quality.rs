//! Deterministic technical video inspection. A successful HTTP response or an
//! MP4 filename is never proof of a playable, correctly sized final film.
//! No additional model/API calls are made by this quality gate.
use super::ffmpeg_from_config;
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},time::Duration};

fn ffprobe(cfg:&Value)->Option<PathBuf>{
    if let Some(ffmpeg)=ffmpeg_from_config(cfg){
        let name=if cfg!(windows){"ffprobe.exe"}else{"ffprobe"};
        let neighbor=ffmpeg.with_file_name(name);
        if neighbor.is_file(){return Some(neighbor);}
    }
    let name=if cfg!(windows){"ffprobe.exe"}else{"ffprobe"};
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|entry|entry.join(name)).find(|path|path.is_file())
}
fn duration(value:&Value)->Option<f64>{
    value.as_f64().or_else(||value.as_str().and_then(|s|s.parse().ok()))
        .filter(|v:f64|v.is_finite()&&*v>0.0)
}
pub(super) fn min_short_edge(input:&Value)->u64{
    match input["engine"].as_str().unwrap_or("native"){
        "omni"=>match input["config"]["omniResolution"].as_str().unwrap_or("1080p"){
            "4k"=>1450,"1080p"=>620,"720p"=>420,_=>200
        },
        "comfyui" if input["cinematic"]==true=>900,
        "comfyui"=>input["render"]["width"].as_u64().unwrap_or(360)
            .min(input["render"]["height"].as_u64().unwrap_or(640))
            .saturating_mul(3)/4,
        _=>200
    }
}
pub(super) fn evaluate(probe:&Value,expected:u64,aspect:&str,
    min_short:u64,require_audio:bool)->Result<Value,String>{
    let streams=probe["streams"].as_array().ok_or("ffprobe 스트림 분석 결과가 없습니다.")?;
    let video=streams.iter().find(|item|item["codec_type"]=="video")
        .ok_or("출력에 실제 비디오 스트림이 없습니다.")?;
    let w=video["width"].as_u64().unwrap_or(0);
    let h=video["height"].as_u64().unwrap_or(0);
    if w==0||h==0||w.min(h)<min_short{
        return Err(format!("영상 해상도 검수 실패: {w}x{h} (짧은 변 {min_short}px 이상 필요)"));
    }
    let desired=match aspect{"1:1"=>1.0,"9:16"=>9.0/16.0,_=>16.0/9.0};
    let actual=w as f64/h as f64;
    if (actual/desired-1.0).abs()>0.10{
        return Err(format!("영상 화면비 검수 실패: {w}x{h}, 요청 {aspect}"));
    }
    let seconds=duration(&probe["format"]["duration"])
        .or_else(||duration(&video["duration"]))
        .ok_or("영상 길이 검수에 필요한 duration이 없습니다.")?;
    let allowance=(expected as f64*0.2).max(2.5);
    if (seconds-expected as f64).abs()>allowance{
        return Err(format!("영상 길이 불일치: {seconds:.1}초 / 요청 {expected}초"));
    }
    let has_audio=streams.iter().any(|item|item["codec_type"]=="audio");
    if require_audio&&!has_audio{
        return Err("요청한 동기화 오디오 트랙이 출력에 없습니다.".into());
    }
    Ok(json!({"verified":true,"rule":"technical-ffprobe-v1",
        "width":w,"height":h,"durationSeconds":seconds,"aspectRatio":aspect,
        "hasAudio":has_audio,"requiredShortEdge":min_short,"visualQualityAssessed":false}))
}
pub(super) async fn inspect(cfg:&Value,file:&Path,seconds:u64,aspect:&str,
    min_short:u64,require_audio:bool)->Result<Value,String>{
    let bin=ffprobe(cfg).ok_or("영상 검수에 FFprobe가 필요합니다. FFmpeg 실행 파일과 같은 폴더에 설치하세요.")?;
    let mut command=tokio::process::Command::new(bin);
    command.kill_on_drop(true).args(["-v","error","-show_entries",
        "format=duration:stream=codec_type,width,height,duration",
        "-of","json"]).arg(file);
    let output=tokio::time::timeout(Duration::from_secs(45),command.output())
        .await.map_err(|_|"FFprobe 영상 분석 시간이 초과됐습니다.")?
        .map_err(|e|format!("FFprobe 실행 오류: {e}"))?;
    if !output.status.success(){
        return Err(format!("FFprobe 재생 검수 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(400).collect::<String>()));
    }
    if output.stdout.len()>32*1024{return Err("FFprobe 결과가 비정상적으로 큽니다.".into());}
    let value:Value=serde_json::from_slice(&output.stdout)
        .map_err(|e|format!("FFprobe 응답 파싱 오류: {e}"))?;
    evaluate(&value,seconds,aspect,min_short,require_audio)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn invalid_audio_ratio_and_duration_are_rejected(){
        let mut probe=json!({"streams":[{"codec_type":"video","width":1920,"height":1080}],
            "format":{"duration":"8.0"}});
        assert!(evaluate(&probe,8,"16:9",620,false).is_ok());
        assert!(evaluate(&probe,8,"16:9",620,true).is_err());
        assert!(evaluate(&probe,8,"9:16",620,false).is_err());
        assert!(evaluate(&probe,16,"16:9",620,false).is_err());
        probe["streams"][0]["width"]=json!(20);
        assert!(evaluate(&probe,8,"16:9",620,false).is_err());
    }
    #[test]
    fn video_engine_has_no_unsupported_quality_claim(){
        assert_eq!(min_short_edge(&json!({"engine":"native"})),200);
        assert_eq!(min_short_edge(&json!({"engine":"omni",
            "config":{"omniResolution":"1080p"}})),620);
    }
}
