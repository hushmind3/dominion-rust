//! Evidence-backed video quality gate. This proves media container/streams,
//! duration, geometry and audio presence, NOT cinematic or semantic quality.
//! Optional ffprobe keeps API-only users operational without a local GPU.
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},time::Duration};

fn executable(cfg:&Value)->Option<PathBuf>{
    let name=if cfg!(windows){"ffprobe.exe"}else{"ffprobe"};
    let mut paths=Vec::new();
    if let Some(ffmpeg)=super::ffmpeg_from_config(cfg) {
        paths.push(ffmpeg.with_file_name(name));
    }
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|root|root.join(name)));
    paths.extend(["/opt/homebrew/bin","/usr/local/bin","/usr/bin"]
        .into_iter().map(|dir|PathBuf::from(dir).join(name)));
    paths.into_iter().find(|path|path.is_file())
}
pub(super) fn available(cfg:&Value)->bool{executable(cfg).is_some()}
fn duration(value:&Value)->Option<f64>{
    value.as_str().and_then(|v|v.parse::<f64>().ok()).or_else(||value.as_f64())
        .filter(|v|v.is_finite()&&*v>0.0)
}
fn ratio(aspect:&str)->Result<f64,String>{
    match aspect {
        "16:9"=>Ok(16.0/9.0),"9:16"=>Ok(9.0/16.0),"1:1"=>Ok(1.0),
        _=>Err("지원되지 않는 영상 화면비입니다.".into())
    }
}
fn evaluate(probe:&Value,seconds:u64,aspect:&str,audio:bool)->Result<Value,String>{
    let streams=probe["streams"].as_array().ok_or("ffprobe 스트림 정보가 없습니다.")?;
    let video=streams.iter().find(|s|s["codec_type"]=="video")
        .ok_or("실제 비디오 스트림이 없습니다.")?;
    let width=video["width"].as_u64().unwrap_or(0);
    let height=video["height"].as_u64().unwrap_or(0);
    if width<320||height<320 {
        return Err(format!("비디오 해상도가 부족합니다: {width}x{height}"));
    }
    let actual_ratio=width as f64/height as f64;
    let expected=ratio(aspect)?;
    if (actual_ratio/expected-1.0).abs()>0.10 {
        return Err(format!("영상 화면비가 목표 {aspect}와 다릅니다: {width}x{height}"));
    }
    let found=duration(&probe["format"]["duration"])
        .or_else(||duration(&video["duration"]))
        .ok_or("영상의 재생 시간을 확인할 수 없습니다.")?;
    let tolerance=(seconds as f64*0.18).max(2.0);
    if (found-seconds as f64).abs()>tolerance {
        return Err(format!("영상 길이가 요청과 다릅니다: 실제 {found:.1}초, 요청 {seconds}초"));
    }
    let audio_stream=streams.iter().any(|stream|stream["codec_type"]=="audio");
    if audio&&!audio_stream {
        return Err("오디오 요청 영상인데 오디오 스트림이 없습니다.".into());
    }
    let codec=video["codec_name"].as_str().unwrap_or("unknown");
    Ok(json!({"status":"technical_pass","verified":true,
        "verification":"ffprobe-stream-duration-aspect-audio-v1",
        "durationSeconds":found,"width":width,"height":height,
        "codec":codec,"ratio":aspect,"audioStream":audio_stream,
        "semanticQualityVerified":false}))
}
pub(super) async fn inspect(cfg:&Value,path:&Path,seconds:u64,aspect:&str,
    required_audio:bool,strict:bool)->Result<Value,String>{
    let Some(probe)=executable(cfg) else {
        if strict {return Err("기술 검수가 선택됐지만 ffprobe가 설치되지 않았습니다.".into());}
        return Ok(json!({"status":"not_verified","verified":false,
            "reason":"ffprobe_missing","semanticQualityVerified":false}));
    };
    let meta=std::fs::metadata(path).map_err(|e|e.to_string())?;
    if !meta.is_file() || meta.len()<100 {
        return Err("영상이 재생 가능한 일반 파일이 아닙니다.".into());
    }
    let mut cmd=tokio::process::Command::new(probe);
    cmd.kill_on_drop(true).args(["-v","error","-show_entries",
        "format=duration:stream=codec_type,codec_name,width,height,duration",
        "-of","json"]).arg(path);
    let output=tokio::time::timeout(Duration::from_secs(45),cmd.output())
        .await.map_err(|_|"ffprobe 검수 제한시간을 초과했습니다.".to_string())?
        .map_err(|e|format!("ffprobe 실행 오류: {e}"))?;
    if !output.status.success(){
        return Err(format!("ffprobe가 영상을 읽지 못했습니다: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(400).collect::<String>()));
    }
    if output.stdout.len()>64*1024 {
        return Err("영상 기술 검수 데이터가 비정상적으로 큽니다.".into());
    }
    let probe:Value=serde_json::from_slice(&output.stdout)
        .map_err(|e|format!("ffprobe JSON 오류: {e}"))?;
    evaluate(&probe,seconds,aspect,required_audio)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn checks_real_dimensions_duration_and_audio_not_semantics(){
        let good=json!({"streams":[{"codec_type":"video","codec_name":"h264",
            "width":1080,"height":1920},{"codec_type":"audio","codec_name":"aac"}],
            "format":{"duration":"8.05"}});
        let report=evaluate(&good,8,"9:16",true).unwrap();
        assert_eq!(report["status"],"technical_pass");
        assert_eq!(report["semanticQualityVerified"],false);
        assert!(evaluate(&good,8,"16:9",true).is_err());
        assert!(evaluate(&good,16,"9:16",true).is_err());
        assert!(evaluate(&good,8,"9:16",false).is_ok());
        let missing=json!({"streams":[{"codec_type":"video","width":1280,
            "height":720}],"format":{"duration":"8"}});
        assert!(evaluate(&missing,8,"16:9",true).is_err());
        assert!(evaluate(&json!({"streams":[]}),8,"16:9",false).is_err());
    }
}
