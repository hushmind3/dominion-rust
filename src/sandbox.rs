//! Explicit OS sandbox for arbitrary approved shell and RSI score/test steps.
//! Required by default. No unsupported-platform or missing-backend fallback.
use std::{env,path::Path};
use tokio::process::Command;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SandboxMode { Required,Off }
impl SandboxMode{
    fn from_env()->Result<Self,String>{
        match env::var("DOMINION_SHELL_SANDBOX").unwrap_or_else(|_|"required".into()).as_str(){
            "required"=>Ok(Self::Required),
            "off"=>Ok(Self::Off),
            _=>Err("DOMINION_SHELL_SANDBOX must be required or off".into())
        }
    }
}
/// Create a command for an explicitly approved model-generated shell script.
/// "off" is an explicit operator choice and never happens as silent fallback.
pub fn script(workspace:&Path,script:&str)->Result<Command,String>{
    if script.is_empty()||script.len()>4096{return Err("invalid shell script length".into());}
    let workspace=workspace.canonicalize().map_err(|e|e.to_string())?;
    match SandboxMode::from_env()?{
        SandboxMode::Off=>{
            let mut command=Command::new("sh");
            command.arg("-lc").arg(script);
            command.current_dir(&workspace).env_clear()
                .env("PATH","/usr/local/bin:/usr/bin:/bin").env("HOME",&workspace);
            Ok(command)
        },
        SandboxMode::Required=>sandboxed(&workspace,script)
    }
}
#[cfg(target_os="linux")]
fn sandboxed(workspace:&Path,script:&str)->Result<Command,String>{
    let executable=["/usr/bin/bwrap","/bin/bwrap"].iter()
        .find(|path|Path::new(path).is_file())
        .ok_or("Linux bubblewrap required for shell execution; install bwrap or explicitly set DOMINION_SHELL_SANDBOX=off after assessing risk")?;
    let mut command=Command::new(executable);
    command.arg("--die-with-parent").arg("--new-session")
        .arg("--unshare-pid").arg("--unshare-net").arg("--unshare-ipc")
        .arg("--proc").arg("/proc").arg("--dev").arg("/dev")
        .arg("--tmpfs").arg("/tmp");
    // Expose only system executables/libraries read-only. Do not bind the
    // operator's home, credential stores, or the model-provider key environment.
    for dir in ["/usr","/bin","/lib","/lib64","/usr/local","/etc/ssl","/etc/alternatives"]{
        if Path::new(dir).exists(){command.arg("--ro-bind").arg(dir).arg(dir);}
    }
    command.arg("--bind").arg(workspace).arg(workspace)
        .arg("--chdir").arg(workspace)
        .arg("--setenv").arg("HOME").arg(workspace)
        .arg("--setenv").arg("PATH").arg("/usr/local/bin:/usr/bin:/bin")
        .arg("--").arg("/bin/sh").arg("-lc").arg(script)
        .current_dir(workspace).env_clear()
        .env("PATH","/usr/local/bin:/usr/bin:/bin");
    Ok(command)
}
#[cfg(target_os="macos")]
fn sandboxed(workspace:&Path,script:&str)->Result<Command,String>{
    if !Path::new("/usr/bin/sandbox-exec").is_file(){
        return Err("macOS Seatbelt sandbox-exec required; no unrestricted fallback".into());
    }
    let escaped=workspace.to_string_lossy().replace('\\',"\\\\").replace('"',"\\\"");
    let profile=format!(r#"(version 1)
(deny default)
(allow process*)
(allow file-read* (subpath "/usr") (subpath "/bin") (subpath "/System") (subpath "/Library") (subpath "{escaped}"))
(allow file-write* (subpath "{escaped}") (subpath "/private/tmp"))
(deny network*)"#
    );
    let mut command=Command::new("/usr/bin/sandbox-exec");
    command.arg("-p").arg(profile).arg("/bin/sh").arg("-lc").arg(script)
        .current_dir(workspace).env_clear().env("PATH","/usr/bin:/bin").env("HOME",workspace);
    Ok(command)
}
#[cfg(not(any(target_os="linux",target_os="macos")))]
fn sandboxed(_workspace:&Path,_script:&str)->Result<Command,String>{
    Err("no enforced shell sandbox backend for this operating system; use an isolated VM or explicitly opt out".into())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn shell_rejects_empty_and_overlong_scripts(){
        let dir=std::env::temp_dir();
        assert!(script(&dir,"").is_err());
        assert!(script(&dir,&"x".repeat(4097)).is_err());
    }
}
