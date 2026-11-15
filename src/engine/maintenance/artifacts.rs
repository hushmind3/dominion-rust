//! Reclaim only named, regenerable DOMINION-owned artifacts.
//! NEVER walk a user workspace, delete completed outputs, delete evidence or
//! prune an active/paused/needs_review job. Paths are confined to data_dir.
use crate::state::AppState;
use serde_json::{json,Value};
use std::{
    path::{Component,Path,PathBuf},
    time::{Duration,SystemTime},
};

const GRACE:Duration=Duration::from_secs(24*60*60);
const BUILD_GRACE:Duration=Duration::from_secs(48*60*60);

#[derive(Debug,Default)]
pub(super) struct SweepReport {
    pub files:u64,
    pub directories:u64,
    pub file_bytes:u64,
    pub errors:u64,
}

impl SweepReport {
    fn remove_file(&mut self,path:&Path){
        let Ok(meta)=std::fs::symlink_metadata(path) else{return};
        if !meta.file_type().is_file(){return;}
        match std::fs::remove_file(path) {
            Ok(())=>{self.files+=1;self.file_bytes+=meta.len();}
            Err(_)=>self.errors+=1,
        }
    }
    fn remove_generated_dir(&mut self,path:&Path){
        let Ok(meta)=std::fs::symlink_metadata(path) else{return};
        if !meta.file_type().is_dir(){return;}
        match std::fs::remove_dir_all(path){
            Ok(())=>self.directories+=1,
            Err(_)=>self.errors+=1,
        }
    }
}

fn is_dir(path:&Path)->bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta|meta.file_type().is_dir())
}
fn is_regular(path:&Path)->bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta|meta.file_type().is_file())
}
fn older_than_file(path:&Path,now:SystemTime,grace:Duration)->bool {
    std::fs::symlink_metadata(path).ok()
        .and_then(|meta|meta.modified().ok())
        .and_then(|modified|now.duration_since(modified).ok())
        .is_some_and(|elapsed|elapsed>=grace)
}
fn older_than_record(record:&Value,now:SystemTime,grace:Duration)->bool {
    record["finishedAt"].as_str()
        .and_then(|text|chrono::DateTime::parse_from_rfc3339(text).ok())
        .and_then(|timestamp|now.duration_since(SystemTime::from(timestamp)).ok())
        .is_some_and(|elapsed|elapsed>=grace)
}
fn closed(record:&Value)->bool {
    matches!(record["status"].as_str(),Some("completed"|"failed"|"cancelled"))
}
fn owned_relative(path:&str)->bool {
    !path.is_empty() && !path.contains('\\') && !path.contains(':')
        && Path::new(path).components().all(|part|matches!(part,Component::Normal(_)))
}
fn entries(directory:&Path)->Vec<PathBuf> {
    if !is_dir(directory){return Vec::new();}
    std::fs::read_dir(directory).ok().into_iter().flatten()
        .filter_map(|entry|entry.ok().map(|entry|entry.path())).collect()
}

/// Remove only abandoned plaintext VACUUM snapshots. Encrypted snapshots
/// already have their own rolling retention policy (default: five).
fn abandoned_backup_temps(state:&AppState,now:SystemTime,report:&mut SweepReport){
    for path in entries(&state.config.data_dir.join("backups")){
        let Some(name)=path.file_name().and_then(|name|name.to_str()) else{continue};
        if name.starts_with(".backup-") && name.ends_with(".tmp")
            && older_than_file(&path,now,GRACE) {
            report.remove_file(&path);
        }
    }
}

/// Successful cinematic edits have a canonical final output.mp4. Intermediate
/// per-shot renders can take gigabytes, and are not needed after the job seals.
/// Leave interrupted/uncertain shots intact: they are precious retry evidence.
fn completed_video_intermediates(state:&AppState,now:SystemTime,report:&mut SweepReport){
    for directory in entries(&state.config.data_dir.join("video")){
        let Some(id)=directory.file_name().and_then(|name|name.to_str()) else{continue};
        if uuid::Uuid::parse_str(id).is_err() || !is_dir(&directory){continue;}
        let Some(job)=state.store.get("job",id) else{continue};
        if job["kind"]!="video"||!closed(&job)||!older_than_record(&job,now,GRACE){continue;}
        let partial=directory.join("output.part.mp4");
        if older_than_file(&partial,now,GRACE){report.remove_file(&partial);}
        if job["status"]!="completed" {continue;}
        let final_video=directory.join("output.mp4");
        if !is_regular(&final_video)
            || std::fs::metadata(&final_video).is_err_or(|meta|meta.len()<100) {continue;}
        for index in 1..=12 {
            let shot=directory.join(format!("shot-{index:02}"));
            if !is_dir(&shot){continue;}
            let rendered=shot.join("output.mp4");
            if older_than_file(&rendered,now,GRACE){report.remove_file(&rendered);}
            let incomplete=shot.join("output.part.mp4");
            if older_than_file(&incomplete,now,GRACE){report.remove_file(&incomplete);}
            // Only remove an empty shot directory; never delete unknown
            // user/debug files via an unbounded recursive folder deletion.
            if std::fs::read_dir(&shot).is_ok_and(|mut items|items.next().is_none()){
                let _=std::fs::remove_dir(&shot);
            }
        }
        let concat=directory.join("shots.ffconcat");
        if older_than_file(&concat,now,GRACE){report.remove_file(&concat);}
    }
}

/// Cargo target-dir is a pure compiler cache. The evidence, source patches,
/// lineage and A/B results live elsewhere; those records are never touched.
fn finished_rsi_build_caches(state:&AppState,now:SystemTime,report:&mut SweepReport){
    let bases=[
        (state.config.data_dir.join("rsi-build").join("generator"),"rsi_evolve"),
        (state.config.data_dir.join("rsi-paired-build"),"rsi_compare"),
    ];
    for (base,kind) in bases {
        for directory in entries(&base){
            let Some(id)=directory.file_name().and_then(|name|name.to_str()) else{continue};
            if uuid::Uuid::parse_str(id).is_err()||!is_dir(&directory){continue;}
            let Some(job)=state.store.get("job",id) else{continue};
            if job["kind"]==kind && closed(&job)
                && older_than_record(&job,now,BUILD_GRACE) {
                report.remove_generated_dir(&directory);
            }
        }
    }
}

/// Cancelled downloads are no longer active by the 24h grace window.
/// Paused/failed jobs retain byte-range resume data. Never delete a real
/// installed GGUF/MLX file or a partial owned by another active download.
fn cancelled_model_parts(state:&AppState,now:SystemTime,report:&mut SweepReport){
    let root=state.config.data_dir.join("models");
    let Ok(root)=root.canonicalize() else{return};
    if !is_dir(&root){return;}
    let jobs=state.store.all("model_job");
    for job in jobs.iter().filter(|job|job["status"]=="cancelled"
        &&older_than_record(job,now,GRACE)) {
        let Some(dir)=job["targetDir"].as_str() else{continue};
        let Ok(dir)=Path::new(dir).canonicalize() else{continue};
        if dir==root||!dir.starts_with(&root)||!is_dir(&dir){continue;}
        if jobs.iter().any(|other|other["id"]!=job["id"]
            && matches!(other["status"].as_str(),Some("downloading"|"paused"))
            && other["targetDir"].as_str().is_some_and(|path|
                Path::new(path).canonicalize().is_ok_and(|other_dir|other_dir==dir))) {
            continue;
        }
        let revision=job["revision"].as_str().unwrap_or("main");
        let suffix=if revision.len()==40&&revision.bytes().all(|c|c.is_ascii_hexdigit()){
            format!(".part.{revision}")
        }else if revision=="main" {
            ".part".to_string()
        }else{
            continue;
        };
        for row in job["files"].as_array().into_iter().flatten(){
            let Some(path)=row["path"].as_str().filter(|path|owned_relative(path)) else{continue};
            let target=dir.join(path);
            let Some(parent)=target.parent() else{continue};
            if !parent.canonicalize().is_ok_and(|parent|parent.starts_with(&root)){continue;}
            let candidate=PathBuf::from(format!("{}{}",target.display(),suffix));
            if older_than_file(&candidate,now,GRACE){report.remove_file(&candidate);}
        }
    }
}

pub(super) fn sweep(state:&AppState)->SweepReport {
    sweep_at(state,SystemTime::now())
}
fn sweep_at(state:&AppState,now:SystemTime)->SweepReport {
    let mut report=SweepReport::default();
    abandoned_backup_temps(state,now,&mut report);
    completed_video_intermediates(state,now,&mut report);
    finished_rsi_build_caches(state,now,&mut report);
    cancelled_model_parts(state,now,&mut report);
    report
}

#[cfg(test)]
#[path = "../../../tests/unit/maintenance_artifacts_tests.rs"]
mod tests;
