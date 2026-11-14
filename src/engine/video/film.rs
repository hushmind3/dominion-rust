//! Durable LTX cinematic film with scene-level paid receipts.
//! Explicit consent for multiple paid clips. Unknown paid outcomes are never
//! resubmitted automatically. Existing verified shots are reused, not rebought.
use super::{ffmpeg_from_config,ltx,quality};
use crate::engine::jobs::JobContext;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{io::{Read,Write},path::Path,time::Duration};

const MAX_CLIPS:usize=6;
pub(super) fn split_duration(total:u64,maximum:u64)->Result<Vec<u64>,String>{
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
pub(super) fn max_clip(model:&str,quality:&str,fps:u64)->Result<u64,String>{
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
/// Owner-visible clip receipts; no raw video bytes or credential-bearing
/// cloud links. The SHA is an optimistic concurrency token for exact retakes.
pub(super) fn scenes(state:&crate::state::AppState,job:&Value)->Result<Value,String>{
    if job["kind"]!="video"||job["input"]["engine"]!="ltx"
        || job["input"]["ltxFilm"]!=true {
        return Err("LTX 멀티샷 영상 작업이 아닙니다.".into());
    }
    let id=job["id"].as_str().ok_or("영상 작업 ID 오류")?;
    let root=super::directory(state,id)?;
    let planned=job["input"]["render"]["film"]["clips"].as_array()
        .ok_or("영상 장면 계획이 없습니다.")?;
    let mut clips=Vec::new();
    // A paid LTX submission can be accepted remotely before its local MP4
    // exists. Source-level inFlight means another paid side effect is still
    // unresolved; never infer a cheap "missing shot" from absence of bytes.
    let pending_remote=job["videoState"]["inFlight"]==true;
    let mut unknown=pending_remote;
    let mut missing=0u64;
    for (index,item) in planned.iter().enumerate(){
        let clip_dir=root.join(format!("clip-{index:02}"));
        let file=clip_dir.join("output.mp4");
        let exists=std::fs::symlink_metadata(&file)
            .is_ok_and(|meta|meta.is_file()&&meta.len()>=1024
                && meta.len()<=256*1024*1024);
        let sha=if exists {file_sha(&file).ok()} else {None};
        let receipt=if exists {saved_receipt(&clip_dir).ok().flatten()} else {None};
        let valid=receipt.is_some();
        if !valid {
            if exists||clip_dir.join("receipt.json").exists(){unknown=true;}
            else{missing+=1;}
        }
        clips.push(json!({"index":index,"duration":item["duration"],
            "outputSha256":sha,"isAvailable":exists,
            "receiptVerified":valid,"qualityStatus":receipt.as_ref().map(|r|
                r["quality"]["status"].clone()).unwrap_or(Value::Null)}));
    }
    let terminal=matches!(job["status"].as_str(),
        Some("completed"|"needs_review"|"failed"|"cancelled"));
    let unknown_indices=clips.iter().enumerate().filter_map(|(index,clip)|
        (clip["isAvailable"]==true && clip["receiptVerified"]!=true)
            .then_some(index)).collect::<Vec<_>>();
    for (index,item) in clips.iter_mut().enumerate() {
        let target_exists=item["isAvailable"]==true;
        let others_unknown=unknown_indices.iter().any(|other|*other!=index);
        item["canRetake"]=json!(terminal&&target_exists&&!others_unknown&&!pending_remote);
        item["expectedPaidCalls"]=json!(1+missing);
    }
    Ok(json!({"jobId":id,"status":job["status"],"film":true,
        "unknownEffect":unknown,"clips":clips,
        "recordedPaidCalls":planned.len(),"terminal":terminal}))
}

/// Recompute only the clips actually charged for a reviewed partial retake.
/// A verified sibling is never included in the new billable-second estimate.
pub(super) fn retake_quote(job:&Value,manifest:&Value,index:usize)->Result<Value,String>{
    let clips=manifest["clips"].as_array().ok_or("재생성 장면 목록이 없습니다.")?;
    if clips.get(index).is_none_or(|clip|clip["canRetake"]!=true){
        return Err("불명확한 유료 결과는 자동으로 재생성할 수 없습니다.".into())
    }
    let billed=clips.iter().enumerate().filter_map(|(pos,clip)|{
        (pos==index || (clip["receiptVerified"]!=true && clip["isAvailable"]!=true))
            .then(||clip["duration"].as_u64()).flatten()
    }).collect::<Vec<_>>();
    if billed.len() as u64!=clips[index]["expectedPaidCalls"].as_u64().unwrap_or(u64::MAX){
        return Err("추가 유료 장면의 초수 또는 호출 수가 불일치합니다.".into())
    }
    super::pricing::quote(
        job["input"]["videoModel"].as_str().ok_or("고정된 LTX 모델 정보가 없습니다.")?,
        job["input"]["videoQuality"].as_str().ok_or("고정된 영상 해상도가 없습니다.")?,
        &billed,job["input"]["generateAudio"]!=false,
    )
}

/// Require the owner to pin a visible source clip AND the exact number of
/// new billable calls (target + still-uncreated tail shots). Every other
/// completed clip remains reused by SHA, never submitted again.
pub(super) fn retake_input(state:&crate::state::AppState,job:&Value,
    index:usize,expected:&str,new_prompt:Option<&str>,confirmed_calls:u64)
    ->Result<Value,String>{
    let manifest=scenes(state,job)?;
    let list=manifest["clips"].as_array().ok_or("장면 목록 오류")?;
    let target=list.get(index).ok_or("장면 번호가 잘못됐습니다.")?;
    if target["canRetake"]!=true{
        return Err("격리된 유료 결과가 불확실하거나 작업이 진행 중이어서 장면 재생성을 시작할 수 없습니다.".into());
    }
    if expected.len()!=64||!expected.bytes().all(|v|v.is_ascii_hexdigit())
        ||target["outputSha256"].as_str()!=Some(expected){
        return Err("장면이 변경됐습니다. 영상을 다시 확인한 뒤 재생성하세요.".into());
    }
    let needed=target["expectedPaidCalls"].as_u64().unwrap_or(u64::MAX);
    if confirmed_calls!=needed{
        return Err(format!("새로 결제될 장면 {needed}개에 동의해야 합니다."));
    }
    let mut input=job["input"].clone();
    if let Some(prompt)=new_prompt{
        let prompt=prompt.trim();
        if prompt.is_empty()||prompt.chars().count()>4750{
            return Err("새 장면 프롬프트는 1~4750자입니다.".into());
        }
        input["render"]["film"]["clips"][index]["render"]["prompt"]=json!(prompt);
    }
    let source=job["id"].as_str().ok_or("기존 영상 ID가 없습니다.")?;
    let fingerprint=format!("{:x}",Sha256::digest(
        format!("{source}|{index}|{expected}|{}",
            input["render"]["film"]["clips"][index]["render"]["prompt"]).as_bytes()));
    input["retake"]=json!({"sourceJobId":source,"index":index,
        "expectedSourceSha256":expected,"newPaidCalls":needed,
        "fingerprint":fingerprint,"confirmed":true});
    Ok(input)
}
fn reuse_other_shots(ctx:&JobContext,input:&Value,dir:&Path)->Result<(),String>{
    if !input["retake"].is_object(){return Ok(());}
    let source_id=input["retake"]["sourceJobId"].as_str()
        .ok_or("이전 영상 ID가 없습니다.")?;
    let source=ctx.state.store.get("job",source_id)
        .filter(|row|row["kind"]=="video"&&row["input"]["ltxFilm"]==true)
        .ok_or("원본 유료 영상이 없습니다.")?;
    if !matches!(source["status"].as_str(),
        Some("completed"|"failed"|"needs_review"|"cancelled")){
        return Err("원본 영상 작업이 아직 진행 중입니다.".into());
    }
    let index=input["retake"]["index"].as_u64().ok_or("재생성 장면 번호 오류")? as usize;
    let expected=input["retake"]["expectedSourceSha256"].as_str()
        .ok_or("장면 버전 해시가 없습니다.")?;
    let source_dir=super::directory(&ctx.state,source_id)?;
    let original=source_dir.join(format!("clip-{index:02}/output.mp4"));
    if file_sha(&original)?!=expected {
        return Err("needs_review: 원본 유료 장면 파일이 요청 뒤 변경됐습니다. 다시 결제하지 않습니다.".into());
    }
    let frozen=input["render"]["film"]["clips"].as_array()
        .ok_or("동결 장면 구성을 찾지 못했습니다.")?;
    for other in 0..frozen.len(){
        if other==index{continue;}
        let from=source_dir.join(format!("clip-{other:02}"));
        match saved_receipt(&from)?{
            Some(receipt)=>{
                let target=dir.join(format!("clip-{other:02}"));
                std::fs::create_dir_all(&target).map_err(|e|e.to_string())?;
                std::fs::copy(from.join("output.mp4"),target.join("output.mp4"))
                    .map_err(|e|e.to_string())?;
                let mut copied=receipt;
                copied["reused"]=json!(true);
                copied["reusedFromJobId"]=json!(source_id);
                persist_receipt(&target,&copied)?;
            }
            None=>{}, // no prior output: requires an explicitly consented new call
        }
    }
    Ok(())
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
    reuse_other_shots(ctx,input,dir)?;
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
#[path = "film/tests.rs"]
mod tests;
