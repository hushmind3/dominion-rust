//! Optional, bounded ACTUAL video decode diagnostics. ffprobe checks codec
//! metadata but cannot see a black/frozen video body. Decode 15s at 320px,
//! record anomalous intervals; never call this semantic or cinematic quality.
//! FFmpeg filter semantics: https://ffmpeg.org/ffmpeg-filters.html
use serde_json::{json,Value};
use std::{path::Path,time::{Duration,Instant}};

fn first_number(line:&str,field:&str)->Option<f64>{
    let value=line.split(field).nth(1)?.split_whitespace().next()?;
    value.trim_matches(|c:char|c==','||c==';')
        .parse::<f64>().ok().filter(|x|x.is_finite()&&*x>=0.0)
}
fn diagnostics(log:&str,sampled_secs:u64)->Value{
    let mut black=Vec::new();
    let mut freeze=Vec::new();
    for line in log.lines(){
        if let (Some(start),Some(end))=(first_number(line,"black_start:"),
            first_number(line,"black_end:")){
            if end>=start && black.len()<32 {
                black.push(json!({"start":start,"end":end,
                    "seconds":(end-start).max(0.0)}));
            }
        }
        if let Some(start)=first_number(line,"freeze_start:") {
            if freeze.len()<32{freeze.push(json!({"start":start}));}
        }
    }
    json!({"status":"sampled","verification":"ffmpeg-blackdetect-freezedetect-v1",
        "sampledSeconds":sampled_secs,"sampleWidth":320,
        "blackIntervals":black,"frozenIntervals":freeze,
        "blackIntervalCount":black.len(),"freezeIntervalCount":freeze.len(),
        "semanticQualityVerified":false,
        "characterContinuityVerified":false,
        "note":"Technical near-black/frozen-frame warnings only. Intentional still shots or fades are not defects by themselves."})
}
/// Optional diagnostics: a missing FFmpeg, filter or timeout NEVER causes a
/// new paid submission, and never marks a video as aesthetically verified.
pub(super) async fn inspect(cfg:&Value,path:&Path,seconds:u64)->Value{
    let Some(ffmpeg)=super::ffmpeg_from_config(cfg) else{
        return json!({"status":"not_measured","reason":"ffmpeg_missing",
            "semanticQualityVerified":false});
    };
    let sample=seconds.clamp(1,15);
    let started=Instant::now();
    let mut cmd=tokio::process::Command::new(ffmpeg);
    cmd.kill_on_drop(true).args([
        "-hide_banner","-nostdin","-loglevel","info",
        "-i"
    ]).arg(path).args([
        "-t",&sample.to_string(),"-vf",
        "scale=320:-2,blackdetect=d=0.6:pix_th=0.08,freezedetect=n=-50dB:d=1.5",
        "-an","-f","null","-"
    ]);
    let output=match tokio::time::timeout(Duration::from_secs(90),cmd.output()).await {
        Ok(Ok(out))=>out,
        Ok(Err(_))=>return json!({"status":"not_measured",
            "reason":"ffmpeg_launch_failed","semanticQualityVerified":false}),
        Err(_)=>return json!({"status":"not_measured",
            "reason":"decode_timeout","semanticQualityVerified":false}),
    };
    if !output.status.success()||output.stderr.len()>256*1024{
        return json!({"status":"not_measured",
            "reason":if output.status.success(){"log_too_large"}else{"decode_or_filter_failed"},
            "semanticQualityVerified":false});
    }
    let mut result=diagnostics(&String::from_utf8_lossy(&output.stderr),sample);
    result["analysisElapsedMs"]=json!(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
    result
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn detect_anomalies_without_asserting_semantic_quality(){
        let sample="[blackdetect] black_start:1.0 black_end:2.5 black_duration:1.5\n[freezedetect] freeze_start:3.0\n";
        let d=diagnostics(sample,15);
        assert_eq!(d["blackIntervalCount"],1);
        assert_eq!(d["freezeIntervalCount"],1);
        assert_eq!(d["blackIntervals"][0]["seconds"],1.5);
        assert_eq!(d["semanticQualityVerified"],false);
        assert_eq!(d["characterContinuityVerified"],false);
    }
    #[tokio::test]
    async fn real_decoded_video_returns_sampled_provenance_if_ffmpeg_available(){
        let cfg=json!({});
        if super::super::ffmpeg_from_config(&cfg).is_none(){return;}
        let dir=std::env::temp_dir().join(format!("visual-probe-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file=dir.join("video.mp4");
        let mut cmd=tokio::process::Command::new(super::super::ffmpeg_from_config(&cfg).unwrap());
        cmd.kill_on_drop(true).args([
            "-nostdin","-y","-loglevel","error",
            "-f","lavfi","-i","testsrc2=size=320x320:rate=12",
            "-t","2","-c:v","mpeg4"
        ]).arg(&file);
        let ok=tokio::time::timeout(Duration::from_secs(20),cmd.output()).await;
        if !matches!(ok,Ok(Ok(ref output)) if output.status.success()){
            let _=std::fs::remove_dir_all(dir);return;
        }
        let signal=inspect(&cfg,&file,2).await;
        assert_eq!(signal["status"],"sampled","{signal}");
        assert_eq!(signal["semanticQualityVerified"],false);
        assert_eq!(signal["sampledSeconds"],2);
        let _=std::fs::remove_dir_all(dir);
    }
}
