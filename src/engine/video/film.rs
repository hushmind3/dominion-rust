//! Durable LTX multi-clip film generation; no local model or GPU.
//! Each paid submission is checkpointed. Uncertain requests are never retried.
use super::{director, ffmpeg_from_config, ltx, quality};
use crate::engine::jobs::JobContext;
use serde_json::{json, Value};
use sha2::{Digest,Sha256};
use std::{io::{Read,Write},path::Path,time::Duration};

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
    let quality=body["videoQuality"].as_str().or(cfg["videoQuality"].as_str())
        .unwrap_or("1080p");
    let fps=body["fps"].as_u64().unwrap_or(24);
    let max_duration=ltx::maximum_duration(model,quality,fps)?;
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


const MAX_MP4:u64=256*1024*1024;
fn file_sha(path:&Path)->Result<String,String>{
    let mut file=std::fs::File::open(path).map_err(|e|e.to_string())?;
    let mut hash=Sha256::new();
    let mut buf=[0u8;32768];
    loop{
        let n=file.read(&mut buf).map_err(|e|e.to_string())?;
        if n==0{break;}
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}",hash.finalize()))
}
fn prompt_sha(clip:&Value)->Result<String,String>{
    let prompt=clip["prompt"].as_str().ok_or("동결 장면 프롬프트 누락")?;
    Ok(format!("{:x}",Sha256::digest(prompt.as_bytes())))
}
/// Never consider a paid output reusable merely because output.mp4 exists.
fn saved_receipt(dir:&Path,clip:&Value,index:usize)->Result<Option<Value>,String>{
    let output=dir.join("output.mp4");
    let receipt=dir.join("receipt.json");
    if !output.exists()&&!receipt.exists(){return Ok(None);}
    let meta=std::fs::symlink_metadata(&output)
        .map_err(|_|"needs_review: 유료 영상 파일이 누락되었습니다.")?;
    if !meta.file_type().is_file()||meta.len()<1024||meta.len()>MAX_MP4{
        return Err("needs_review: 유료 영상 파일 크기 또는 유형 오류".into());
    }
    let bytes=std::fs::read(&receipt)
        .map_err(|_|"needs_review: 이미 생성한 유료 클립의 영수증이 없습니다. 재구매하지 않습니다.")?;
    if bytes.len()>32768{return Err("needs_review: 클립 영수증 크기 오류".into());}
    let parsed:Value=serde_json::from_slice(&bytes)
        .map_err(|_|"needs_review: 클립 영수증 JSON 오류")?;
    let actual=file_sha(&output)?;
    let prompt=prompt_sha(clip)?;
    if parsed["version"]!=1||parsed["index"].as_u64()!=Some(index as u64)
        ||parsed["duration"]!=clip["duration"]
        ||parsed["promptSha256"].as_str()!=Some(prompt.as_str())
        ||parsed["outputSha256"].as_str()!=Some(actual.as_str())
        ||parsed["technicalQuality"]["verified"]!=true{
        return Err("needs_review: 영상·SHA 영수증·동결 장면 입력이 다릅니다.".into());
    }
    Ok(Some(parsed))
}
fn persist_receipt(dir:&Path,receipt:&Value)->Result<(),String>{
    let temp=dir.join("receipt.part.json");
    let mut f=std::fs::File::create(&temp).map_err(|e|e.to_string())?;
    f.write_all(receipt.to_string().as_bytes()).map_err(|e|e.to_string())?;
    f.sync_all().map_err(|e|e.to_string())?;
    drop(f);
    std::fs::rename(temp,dir.join("receipt.json")).map_err(|e|e.to_string())
}
pub(super) fn scenes(state:&crate::state::AppState,job:&Value)->Result<Value,String>{
    let clips=job["input"]["render"]["film"]["clips"].as_array()
        .filter(|v|v.len()>=2&&v.len()<=MAX_CLIPS)
        .ok_or("LTX 다중 장면 작업이 아닙니다.")?;
    if job["kind"]!="video"||job["input"]["engine"]!="ltx" {
        return Err("LTX 다중 장면 작업이 아닙니다.".into());
    }
    let id=job["id"].as_str().ok_or("영상 작업 ID가 없습니다.")?;
    let root=super::directory(state,id)?;
    let terminal=matches!(job["status"].as_str(),
        Some("completed"|"needs_review"|"failed"|"cancelled"));
    let mut verified_all=true;
    let mut rows=Vec::new();
    for (i,clip) in clips.iter().enumerate(){
        let path=root.join(format!("clip-{i:02}"));
        let output=path.join("output.mp4");
        let available=std::fs::symlink_metadata(&output).is_ok_and(|m|
            m.file_type().is_file()&&m.len()>=1024&&m.len()<=MAX_MP4);
        let digest=if available{file_sha(&output).ok()}else{None};
        let verified=available&&saved_receipt(&path,clip,i).ok().flatten().is_some();
        verified_all &=verified;
        rows.push(json!({"index":i,"duration":clip["duration"],
            "isAvailable":available,"outputSha256":digest,"receiptVerified":verified}));
    }
    let safe=terminal&&verified_all&&job["videoState"]["inFlight"]!=true;
    for row in &mut rows{
        row["canRetake"]=json!(safe);
        row["expectedPaidCalls"]=json!(1);
    }
    Ok(json!({"jobId":id,"status":job["status"],"clips":rows,
        "allReceiptsVerified":verified_all,"terminal":terminal,"unknownEffect":!safe}))
}
pub(super) fn retake_input(state:&crate::state::AppState,job:&Value,index:usize,
    expected:&str,prompt:Option<&str>,calls:u64)->Result<Value,String>{
    let manifest=scenes(state,job)?;
    let clip=manifest["clips"].as_array().and_then(|v|v.get(index))
        .ok_or("장면 번호가 잘못됐습니다.")?;
    if clip["canRetake"]!=true{
        return Err("모든 클립의 SHA 영수증 확인 후 재생성할 수 있습니다.".into());
    }
    if expected.len()!=64||!expected.bytes().all(|c|c.is_ascii_hexdigit())
        ||clip["outputSha256"].as_str()!=Some(expected){
        return Err("검토한 영상 SHA와 실제 파일이 다릅니다.".into());
    }
    if calls!=1{return Err("신규 유료 클립 1회에 동의해야 합니다.".into());}
    let mut input=job["input"].clone();
    let target=&mut input["render"]["film"]["clips"][index];
    if let Some(text)=prompt{
        let value=text.trim();
        if value.is_empty()||value.chars().count()>5000 {
            return Err("새 영상 프롬프트는 1~5000자입니다.".into());
        }
        target["prompt"]=json!(value);
    }
    let source=job["id"].as_str().ok_or("원본 영상 ID 누락")?;
    let key=format!("{source}|{index}|{expected}|{}|{}",
        target["prompt"],target["duration"]);
    input["retake"]=json!({"sourceJobId":source,"index":index,
        "expectedSourceSha256":expected,"newPaidCalls":1,
        "fingerprint":format!("{:x}",Sha256::digest(key.as_bytes()))});
    Ok(input)
}
fn reuse_other_shots(ctx:&JobContext,input:&Value,dir:&Path)->Result<(),String>{
    if !input["retake"].is_object(){return Ok(());}
    let source_id=input["retake"]["sourceJobId"].as_str().ok_or("원본 ID 누락")?;
    let source=ctx.state.store.get("job",source_id).ok_or("원본 영상 누락")?;
    if scenes(&ctx.state,&source)?["allReceiptsVerified"]!=true{
        return Err("needs_review: 원본 클립 영수증을 재검증하지 못했습니다.".into());
    }
    let original=super::directory(&ctx.state,source_id)?;
    let target=input["retake"]["index"].as_u64().ok_or("장면 번호 누락")? as usize;
    let clips=input["render"]["film"]["clips"].as_array().ok_or("장면 목록 누락")?;
    if target>=clips.len(){return Err("장면 번호 범위 오류".into());}
    if file_sha(&original.join(format!("clip-{target:02}/output.mp4")))?
        !=input["retake"]["expectedSourceSha256"]{
        return Err("needs_review: 원본 클립 SHA가 변경됐습니다. 과금하지 않습니다.".into());
    }
    for (index,clip) in clips.iter().enumerate(){
        if index==target{continue;}
        let from=original.join(format!("clip-{index:02}"));
        let receipt=saved_receipt(&from,clip,index)?
            .ok_or("needs_review: 재사용 장면 영수증 누락")?;
        let to=dir.join(format!("clip-{index:02}"));
        std::fs::create_dir_all(&to).map_err(|e|e.to_string())?;
        std::fs::copy(from.join("output.mp4"),to.join("output.mp4"))
            .map_err(|e|e.to_string())?;
        let mut reused=receipt;
        reused["outputSha256"]=json!(file_sha(&to.join("output.mp4"))?);
        reused["reused"]=json!(true);
        reused["reusedFromJobId"]=json!(source_id);
        persist_receipt(&to,&reused)?;
    }
    Ok(())
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
    let mut clip_quality = Vec::new();
    let gate=input["technicalGate"]==true;
    let aspect=input["aspectRatio"].as_str().unwrap_or("9:16");
    let audio=input["render"]["generate_audio"]==true;
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
        // Check every charged scene BEFORE stitching or touching the next
        // paid request. An invalid scene remains on disk for review, so
        // restarted work cannot silently buy it a second time.
        if gate {
            let report=quality::inspect(&input["config"],&clip_dir.join("output.mp4"),
                duration,aspect,audio).await
                .map_err(|e|format!("needs_review: Scene {} paid output is preserved, but ffprobe verification failed: {e}. Do not resubmit without owner action.",index+1))?;
            clip_quality.push(report);
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
        "quality":input["render"]["quality"],"clipTechnicalQuality":clip_quality}))
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
