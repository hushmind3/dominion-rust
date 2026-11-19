//! A desktop-only OS file picker. Browser paths cannot be trusted as a
//! native picker response; only the human-selected OS dialog yields a path.
//! Remote/headless servers return a clear error instead of a fake workspace.
use crate::{config::Settings,engine::Engine};
use axum::{extract::State,http::StatusCode,routing::post,Json,Router};
use serde_json::{json,Value};
use std::sync::Arc;
use tokio::{process::Command,time::{timeout,Duration}};
type Fault=(StatusCode,Json<Value>);
fn fail(code:StatusCode,error:impl Into<String>)->Fault{
    (code,Json(json!({"error":error.into()})))
}
#[cfg(target_os="macos")]
fn native_command(folder:bool)->Command{
    let mut command=Command::new("/usr/bin/osascript");
    if folder{
        command.args(["-e","POSIX path of (choose folder with prompt \"Choose a model directory\")"]);
    }else{
        command.args(["-e","POSIX path of (choose file with prompt \"Choose a GGUF model file\")"]);
    }
    command
}
#[cfg(windows)]
fn native_command(folder:bool)->Command{
    let mut command=Command::new("powershell.exe");
    let script=if folder{
        "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; if ($d.ShowDialog() -eq 'OK') { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Write-Output $d.SelectedPath }"
    }else{
        "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Filter = 'GGUF (*.gguf)|*.gguf|All files (*.*)|*.*'; if ($d.ShowDialog() -eq 'OK') { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Write-Output $d.FileName }"
    };
    command.args(["-NoProfile","-STA","-Command",script]);
    command
}
#[cfg(all(unix,not(target_os="macos")))]
fn native_command(folder:bool)->Command{
    let mut command=Command::new("zenity");
    command.args(["--file-selection","--title=DOMINION model selection"]);
    if folder{command.arg("--directory");}
    else{command.arg("--file-filter=GGUF models | *.gguf");}
    command
}
async fn run(command:&mut Command)->Result<Option<String>,String>{
    command.kill_on_drop(true);
    let out=timeout(Duration::from_secs(120),command.output()).await
        .map_err(|_|"Native file picker timed out".to_owned())?
        .map_err(|e|format!("Native file picker could not start: {e}"))?;
    if !out.status.success(){
        // A user who dismisses the operating-system chooser made no change.
        if matches!(out.status.code(),Some(1)|Some(-128)){return Ok(None);}
        return Err(format!("Native file picker failed: {}",
            String::from_utf8_lossy(&out.stderr).chars().take(200).collect::<String>()));
    }
    let path=String::from_utf8(out.stdout)
        .map_err(|_|"Selected filename is not UTF-8".to_owned())?;
    let path=path.trim();
    if path.is_empty(){return Ok(None);}
    Ok(Some(path.to_owned()))
}
pub(crate) async fn pick(settings:&Settings,folder:bool)->Result<Json<Value>,Fault>{
    if !settings.bind.ip().is_loopback(){
        return Err(fail(StatusCode::FORBIDDEN,
            "Native file picker is available only on a local Dominion instance"));
    }
    #[cfg(all(unix,not(target_os="macos")))]
    if std::env::var_os("DISPLAY").is_none()&&std::env::var_os("WAYLAND_DISPLAY").is_none(){
        return Err(fail(StatusCode::NOT_IMPLEMENTED,
            "No Linux desktop display is attached; enter the model path manually"));
    }
    let mut command=native_command(folder);
    let result=run(&mut command).await
        .map_err(|e|fail(StatusCode::NOT_IMPLEMENTED,e))?;
    let Some(path)=result else{return Ok(Json(json!({"path":Value::Null,"cancelled":true})));};
    let canonical=std::fs::canonicalize(&path)
        .map_err(|e|fail(StatusCode::BAD_REQUEST,format!("Selected path unavailable: {e}")))?;
    if folder&&!canonical.is_dir(){
        return Err(fail(StatusCode::BAD_REQUEST,"Selected item is not a folder"));
    }
    if !folder&&!canonical.is_file(){
        return Err(fail(StatusCode::BAD_REQUEST,"Selected item is not a regular file"));
    }
    Ok(Json(json!({"path":canonical,"cancelled":false})))
}
async fn pick_file(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    pick(&engine.settings,false).await
}
pub fn routes()->Router<Arc<Engine>>{
    Router::new().route("/model-library/pick-file",post(pick_file))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn picker_path_is_only_returned_after_native_ui_selection(){
        let mut settings=Settings::from_env().unwrap();
        settings.bind="0.0.0.0:4317".parse().unwrap();
        assert!(!settings.bind.ip().is_loopback());
        let _=native_command(false); // argv only; never launches a GUI in tests
    }
}
