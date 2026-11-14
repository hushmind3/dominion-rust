//! Technical media probe for generated MP4s. This is decode metadata
//! validation, not semantic film quality or an automatic paid retry policy.
use super::*;
use std::{path::{Path,PathBuf},process::Stdio};

fn probe_executable(cfg:&Value)->Option<PathBuf>{
    let name=if cfg!(windows){"ffprobe.exe"}else{"ffprobe"};
    let mut choices=Vec::new();
    if let Some(ffmpeg)=ffmpeg_from_config(cfg){
        choices.push(ffmpeg.with_file_name(name));
    }
    // Cloud generation does not require FFmpeg; permit an independently
    // installed ffprobe and never classify its absence as technical success.
    choices.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|folder|folder.join(name)));
    choices.extend(["/opt/homebrew/bin","/usr/local/bin","/usr/bin"]
        .into_iter().map(|folder|PathBuf::from(folder).join(name)));
    choices.into_iter().find(|path|path.is_file())
}
pub(super) fn available(cfg:&Value)->bool{probe_executable(cfg).is_some()}
fn duration_seconds(value:&Value)->Option<f64>{
    value.as_str().and_then(|s|s.parse::<f64>().ok())
        .or_else(||value.as_f64())
        .filter(|v|v.is_finite()&&*v>0.0)
}
fn expected_ratio(aspect:&str)->Option<f64>{
    match aspect{
        "16:9"=>Some(16.0/9.0),
        "9:16"=>Some(9.0/16.0),
        "1:1"=>Some(1.0),
        _=>None
    }
}
pub(super) fn assess(probe:&Value,expected_seconds:Option<u64>,
    aspect:&str,require_audio:bool)->Result<Value,String>{
    let streams=probe["streams"].as_array().ok_or("MP4 비디오 스트림 정보를 읽지 못했습니다.")?;
    let video=streams.iter().find(|s|s["codec_type"]=="video")
        .ok_or("MP4 비디오 스트림이 없습니다.")?;
    let width=video["width"].as_u64().unwrap_or(0);
    let height=video["height"].as_u64().unwrap_or(0);
    if width<320||height<320{
        return Err(format!("영상 해상도가 너무 낮습니다: {width}x{height}"));
    }
    let ratio=width as f64/height as f64;
    if let Some(want)=expected_ratio(aspect){
        if (ratio/want-1.0).abs()>0.10{
            return Err(format!("영상 화면비가 요청과 다릅니다: {width}x{height}, 요청 {aspect}"));
        }
    }
    let actual=duration_seconds(&probe["format"]["duration"])
        .ok_or("MP4 재생 시간을 읽지 못했습니다.")?;
    if let Some(expected)=expected_seconds.filter(|s|*s>0){
        let tolerance=(expected as f64*0.15).max(1.5);
        if (actual-expected as f64).abs()>tolerance{
            return Err(format!("영상 재생 시간이 요청과 다릅니다: {actual:.1}초, 요청 {expected}초"));
        }
    }
    let audio=streams.iter().any(|s|s["codec_type"]=="audio");
    if require_audio&&!audio{
        return Err("오디오 포함 모델의 생성물에 오디오 스트림이 없습니다.".into());
    }
    Ok(json!({"status":"technical_pass","source":"ffprobe_stream_metadata",
        "width":width,"height":height,"durationSeconds":actual,
        "hasAudio":audio,"requestedAspect":aspect,
        "visualQualityMeasured":false,"sceneContinuityMeasured":false,
        "automaticPaidRetry":false}))
}
pub(super) async fn inspect(cfg:&Value,path:&Path,seconds:Option<u64>,
    aspect:&str,audio:bool)->Result<Value,String>{
    let Some(executable)=probe_executable(cfg) else{
        return Ok(json!({"status":"not_verified","reason":"ffprobe_not_installed",
            "visualQualityMeasured":false,"automaticPaidRetry":false}));
    };
    let output=tokio::time::timeout(Duration::from_secs(15),
        tokio::process::Command::new(executable)
            .args(["-v","error","-show_entries",
                "stream=codec_type,width,height:format=duration",
                "-of","json"])
            .arg(path).stdin(Stdio::null()).kill_on_drop(true).output())
        .await.map_err(|_|"영상 기술 검수 제한 시간 초과".to_string())?
        .map_err(|e|format!("ffprobe 영상 검사 실패: {e}"))?;
    if !output.status.success(){
        return Err(format!("ffprobe가 재생 가능한 영상으로 인식하지 못했습니다: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(240).collect::<String>()));
    }
    if output.stdout.len()>64*1024{
        return Err("ffprobe 출력이 허용 크기를 초과했습니다.".into());
    }
    let probe:Value=serde_json::from_slice(&output.stdout)
        .map_err(|e|format!("ffprobe 정보 해석 실패: {e}"))?;
    assess(&probe,seconds,aspect,audio)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn rejects_missing_audio_and_wrong_geometry(){
        let correct=json!({"streams":[
            {"codec_type":"video","width":1920,"height":1080},
            {"codec_type":"audio"}],"format":{"duration":"10.00"}});
        assert_eq!(assess(&correct,Some(10),"16:9",true).unwrap()["status"],
            "technical_pass");
        assert!(assess(&correct,Some(10),"9:16",true).is_err());
        assert!(assess(&correct,Some(20),"16:9",true).is_err());
        let silent=json!({"streams":[{"codec_type":"video",
            "width":1920,"height":1080}],"format":{"duration":"10.0"}});
        assert!(assess(&silent,Some(10),"16:9",true).is_err());
        assert!(assess(&silent,Some(10),"16:9",false).is_ok());
    }
}
