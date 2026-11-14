//! Durable LTX cinematic film with scene-level paid receipts.
//! Explicit consent for multiple paid clips. Unknown paid outcomes are never
//! resubmitted automatically. Existing verified shots are reused, not rebought.
use super::{ffmpeg_from_config,ltx,quality};
use crate::engine::jobs::JobContext;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{io::{Read,Write},path::Path,time::Duration};

const MAX_CLIPS:usize=6;
fn split_duration(total:u64,maximum:u64)->Result<Vec<u64>,String>{
    if total<12||total%2!=0||maximum<6||maximum%2!=0 {
        return Err("장편 영상은 12초 이상의 짝수 길이를 요청하세요.".into());
    }
    let count=total.div_ceil(maximum) as usize;
    if count<2||count>MAX_CLIPS||total<count as u64*6 {
        return Err("장면 수 2~6개 범위에서 생성할 수 없는 영상 길이입니다.".into());
    }
    let mut parts=vec![6u64;count];
    let mut remaining=total-count as u64*6;
    for part in &mut parts {
        let extra=(maximum-6).min(remaining);
        *part+=extra;
        remaining-=extra;
    }
    if remaining!=0||parts.iter().any(|part|*part>maximum){
        return Err("지원하지 않는 장면 길이 분해입니다.".into());
    }
    Ok(parts)
}
fn max_clip(model:&str,quality:&str,fps:u64)->Result<u64,String>{
    if !matches!(fps,24|48){return Err("LTX FPS는 24 또는 48만 허용됩니다.".into());}
    match model {
        "ltx-2-5-pro"=>Ok(10),
        "ltx-2-5-fast" if matches!(quality,"720p"|"1080p")&&fps==24=>Ok(20),
        "ltx-2-5-fast"=>Ok(10),
        _=>Err("지원하지 않는 LTX 모델입니다.".into())
    }
}
fn shot_prompt(brief:&str,index:usize,total:usize,custom:Option<&str>)->String{
    if let Some(prompt)=custom.filter(|p|!p.trim().is_empty()){
        return prompt.trim().chars().take(4750).collect();
    }
    // Without an extra planning model or API call, carry one immutable
    // character/environment anchor across scenes. These are instructions to
    // the video model and are not a claim that visual continuity is proven.
    format!("Continuous film, shot {}/{}. Maintain the same characters, wardrobe, lighting, location, color palette and voice across all cuts. No title cards. Original story: {}. Compose shot {} as the next coherent event.",index+1,total,
        brief.chars().take(4200).collect::<String>(),index+1)
}
pub(super) fn spec(input:&Value,cfg:&Value)->Result<Value,String>{
    let total=input["duration"].as_u64().ok_or("총 영상 길이가 필요합니다.")?;
    let model=input["videoModel"].as_str().or(cfg["videoModel"].as_str())
        .unwrap_or("ltx-2-5-pro");
    let quality=input["videoQuality"].as_str().or(cfg["videoQuality"].as_str())
        .unwrap_or("1080p");
    let fps=input["fps"].as_u64().unwrap_or(24);
    let parts=split_duration(total,max_clip(model,quality,fps)?)?;
    if input["confirmPaidMultiShot"]!=true {
        return Err(format!("LTX 영상이 유료 장면 {}개를 생성합니다. 다중 유료 생성에 명시적으로 동의하세요.",parts.len()));
    }
    if ffmpeg_from_config(cfg).is_none(){
        return Err("멀티샷 영상 연결에는 FFmpeg가 필요합니다. API 영상 생성 전에 설치하세요.".into());
    }
    let brief=input["brief"].as_str().unwrap_or("");
    let supplied=input["scenes"].as_array().cloned().unwrap_or_default();
    if !supplied.is_empty() && supplied.len()!=parts.len() {
        return Err(format!("장면 프롬프트를 지정한다면 생성 장면 {}개와 정확히 일치해야 합니다.",parts.len()));
    }
    let mut clips=Vec::new();
    for (index,duration) in parts.iter().enumerate(){
        let custom=supplied.get(index).and_then(|scene|
            scene.as_str().or_else(||scene["prompt"].as_str()));
        if supplied.get(index).is_some()&&custom.is_none(){
            return Err("장면 프롬프트는 문자열 또는 prompt 속성이 있는 객체여야 합니다.".into());
        }
        let prompt=shot_prompt(brief,index,parts.len(),custom);
        let mut prepared=input.clone();
        prepared["duration"]=json!(duration);
        prepared["brief"]=json!(prompt);
        let spec=ltx::spec(&prepared,cfg)?;
        clips.push(json!({"index":index,"duration":duration,"render":spec}));
    }
    Ok(json!({"film":{"version":1,"totalDuration":total,"clips":clips,
        "paidCallCount":clips.len(),"verifiedContinuity":false},
        "model":model,"quality":quality,"fps":fps}))
}

fn file_sha(path:&Path)->Result<String,String>{
    let mut f=std::fs::File::open(path).map_err(|e|e.to_string())?;
    let mut hash=Sha256::new();
    let mut buffer=[0u8;32*1024];
    loop{
        let count=f.read(&mut buffer).map_err(|e|e.to_string())?;
        if count==0{break;}
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}",hash.finalize()))
}
fn saved_receipt(dir:&Path)->Result<Option<Value>,String>{
    let path=dir.join("output.mp4");
    let receipt=dir.join("receipt.json");
    if !path.exists() && !receipt.exists(){return Ok(None);}
    if !path.is_file()||!receipt.is_file(){
        return Err("needs_review: 이전 유료 장면의 영상 또는 영수증이 누락됐습니다. 재구매하지 않습니다.".into());
    }
    let data=std::fs::read(&receipt).map_err(|e|e.to_string())?;
    if data.len()>32*1024{return Err("needs_review: 장면 영수증이 과도하게 큽니다.".into());}
    let parsed:Value=serde_json::from_slice(&data)
        .map_err(|_|"needs_review: 이전 장면 영수증 형식 오류")?;
    let current=file_sha(&path)?;
    if parsed["outputSha256"].as_str()!=Some(current.as_str())
        || parsed["quality"]["status"]=="technical_fail" {
        return Err("needs_review: 기존 유료 영상과 영수증이 다릅니다. 자동 재제출하지 않습니다.".into());
    }
    Ok(Some(parsed))
}
fn persist_receipt(dir:&Path,receipt:&Value)->Result<(),String>{
    let tmp=dir.join("receipt.part.json");
    let mut file=std::fs::File::create(&tmp).map_err(|e|e.to_string())?;
    file.write_all(receipt.to_string().as_bytes()).map_err(|e|e.to_string())?;
    file.sync_all().map_err(|e|e.to_string())?;
    drop(file);
    std::fs::rename(tmp,dir.join("receipt.json")).map_err(|e|e.to_string())
}
pub(super) async fn run(ctx:&JobContext,input:&Value,dir:&Path)->Result<Value,String>{
    let clips=input["render"]["film"]["clips"].as_array()
        .ok_or("동결된 멀티샷 계획이 없습니다.")?;
    if clips.len()<2||clips.len()>MAX_CLIPS {
        return Err("동결된 멀티샷 장면 수가 유효하지 않습니다.".into());
    }
    let strict=input["technicalGate"]==true;
    let cfg=&input["config"];
    let aspect=input["aspectRatio"].as_str().unwrap_or("9:16");
    let ffmpeg=ffmpeg_from_config(cfg).ok_or("장면 연결에 FFmpeg가 필요합니다.")?;
    if strict&&!quality::available(cfg){
        return Err("ffprobe 검수를 선택했지만 실행 파일이 없어 유료 호출을 시작하지 않습니다.".into());
    }
    let mut receipts=Vec::new();
    let mut charged=0u64;
    for (index,clip) in clips.iter().enumerate(){
        ctx.checkpoint(json!({}))?;
        let dir_clip=dir.join(format!("clip-{index:02}"));
        std::fs::create_dir_all(&dir_clip).map_err(|e|e.to_string())?;
        if let Some(previous)=saved_receipt(&dir_clip)?{
            receipts.push(previous);
            continue;
        }
        let mut frozen=input.clone();
        frozen["render"]=clip["render"].clone();
        let duration=clip["duration"].as_u64().ok_or("장면 길이가 없습니다.")?;
        ctx.checkpoint(json!({"phase":"rendering","progress":5+75*index/clips.len(),
            "videoState":{"inFlight":false,"filmClip":index+1,"filmTotal":clips.len()}}))?;
        let remote=ltx::run(ctx,&frozen,&dir_clip).await?;
        // At this boundary the paid output already exists. A failed probe
        // must preserve it for inspection, not submit an extra paid request.
        let report=quality::inspect(cfg,&dir_clip.join("output.mp4"),
            duration,aspect,clip["render"]["generate_audio"]==true,strict)
            .await.map_err(|e|format!("needs_review: 유료 장면 {}의 기술 검수 실패: {e}. 파일을 보존했으며 자동 재생성하지 않습니다.",index+1))?;
        let hash=file_sha(&dir_clip.join("output.mp4"))?;
        let receipt=json!({"index":index,"duration":duration,"remote":remote,
            "quality":report,"outputSha256":hash,"reused":false});
        persist_receipt(&dir_clip,&receipt)?;
        receipts.push(receipt);
        charged+=1;
        ctx.checkpoint(json!({"phase":"rendering","progress":5+75*(index+1)/clips.len(),
            "videoState":{"inFlight":false,"filmCompleted":index+1,
                "filmTotal":clips.len()}}))?;
    }
    let mut playlist=String::new();
    for index in 0..clips.len(){
        playlist.push_str(&format!("file 'clip-{index:02}/output.mp4'\n"));
    }
    std::fs::write(dir.join("clips.txt"),playlist).map_err(|e|e.to_string())?;
    ctx.checkpoint(json!({"phase":"encoding","progress":90,
        "videoState":{"inFlight":false,"filmTotal":clips.len()}}))?;
    let mut command=tokio::process::Command::new(ffmpeg);
    command.current_dir(dir).args(["-hide_banner","-loglevel","error","-nostdin","-y",
        "-f","concat","-safe","0","-i","clips.txt","-c","copy",
        "-movflags","+faststart","output.part.mp4"]);
    let output=crate::engine::tools::run_command(&mut command,Duration::from_secs(600)).await?;
    if !output.status.success(){
        return Err(format!("장면 결합에 실패했습니다. 이미 구매한 장면은 보존됩니다: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>()));
    }
    let target=dir.join("output.part.mp4");
    if !std::fs::metadata(&target).is_ok_and(|m|m.len()>1024){
        return Err("최종 연결 영상이 비어 있습니다. 구매한 장면은 유지합니다.".into());
    }
    std::fs::rename(target,dir.join("output.mp4")).map_err(|e|e.to_string())?;
    let total=input["render"]["film"]["totalDuration"].as_u64()
        .ok_or("원본 동결된 영상 길이가 없습니다.")?;
    let report=quality::inspect(cfg,&dir.join("output.mp4"),total,aspect,
        input["generateAudio"]==true,strict).await
        .map_err(|e|format!("needs_review: 최종 영상 결합 기술 검수 실패: {e}. 개별 유료 장면은 보존됩니다."))?;
    Ok(json!({"engine":"ltx","mode":"film","shots":clips.len(),"model":input["videoModel"],
        "duration":total,"clipReceipts":receipts,"newPaidCallsThisRun":charged,
        "technicalQuality":report,"semanticQualityVerified":false,
        "quality":"multi-shot-static-consistency-prompt-not-measured"}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn segmentation_requires_real_multi_clip_consent_and_valid_profiles(){
        assert_eq!(split_duration(22,10).unwrap(),vec![10,6,6]);
        assert_eq!(split_duration(60,10).unwrap(),vec![10,10,10,10,10,10]);
        assert_eq!(split_duration(38,20).unwrap(),vec![20,18]);
        assert!(split_duration(62,10).is_err());
        assert!(split_duration(8,10).is_err());
        let cfg=json!({"videoModel":"ltx-2-5-pro","videoQuality":"1080p"});
        let body=json!({"duration":22,"brief":"Ocean lighthouse",
            "videoModel":"ltx-2-5-pro","videoQuality":"1080p","fps":24,
            "aspectRatio":"16:9"});
        assert!(spec(&body,&cfg).is_err());
        let mut yes=body;
        yes["confirmPaidMultiShot"]=json!(true);
        // The caller's FFmpeg configuration is a prerequisite, so only
        // exercise pure segmentation independent of local installations.
        assert_eq!(max_clip("ltx-2-5-pro","1080p",24).unwrap(),10);
    }
}
