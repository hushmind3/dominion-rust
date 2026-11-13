//! Cancellable process execution and captured outputs.
//! Active Rust execution paths imported by tools.rs.
use super::*;

/// A tool command gets a private Unix process group. Killing only the shell
/// leaves its children running after Stop; the guard covers cancellation,
/// timeout and I/O errors and is disarmed only after a completed command.
#[cfg(unix)]
struct ToolProcessGroup {
    id: Option<i32>,
}
#[cfg(unix)]
impl ToolProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self {
            id: pid
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 1),
        }
    }
    fn disarm(&mut self) {
        self.id = None;
    }
}
#[cfg(unix)]
impl Drop for ToolProcessGroup {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // This is the exact child PID created with process_group(0).
            // A negative PID targets that private group, never our own group;
            // zero and special global process IDs are excluded by new().
            // SIGKILL is 9 on the supported macOS/Linux Unix hosts.
            // SAFETY: the target is the private process group created above;
            // the group id is positive and our own group is never targeted.
            unsafe {
                kill(-id, 9);
            }
        }
    }
}

pub(crate) async fn read_command_output(
    mut stream: impl tokio::io::AsyncRead + Unpin,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    // Keep enough UTF-8 bytes for 20,001 characters so output_payload can
    // preserve its existing 20,000-character truncation indicator. Continue
    // draining after this cap so a chatty child cannot fill a pipe or memory.
    const MAX_RETAINED_BYTES: usize = 4 * 20_001;
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(kept);
        }
        let retain = count.min(MAX_RETAINED_BYTES.saturating_sub(kept.len()));
        kept.extend_from_slice(&buffer[..retain]);
    }
}

pub(crate) async fn run_command(
    command: &mut tokio::process::Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let mut group = ToolProcessGroup::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or("Command stdout is not available")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Command stderr is not available")?;
    let outcome = tokio::time::timeout(limit, async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_command_output(stdout),
            read_command_output(stderr)
        )?;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match outcome {
        Ok(Ok(output)) => {
            #[cfg(unix)]
            group.disarm();
            Ok(output)
        }
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err(if limit.subsec_nanos() == 0 {
            format!("Command timed out after {}s", limit.as_secs())
        } else {
            format!("Command timed out after {}ms", limit.as_millis())
        }),
    }
}

pub(crate) async fn shell(command: &str, cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    #[cfg(windows)]
    let mut child = {
        let mut child = Command::new("cmd");
        child.args(["/C", command]);
        child
    };
    #[cfg(not(windows))]
    let mut child = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut child = Command::new(shell);
        child.args(["-lc", command]);
        child
    };
    child.current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

pub(crate) async fn executable(command: &str, args: &[String], cwd: &Path) -> Result<Value, String> {
    use tokio::process::Command;
    if !command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:".contains(c))
    {
        return Err("Invalid executable".into());
    }
    let mut child = Command::new(command);
    child.args(args).current_dir(cwd);
    let output = run_command(&mut child, Duration::from_secs(120)).await?;
    Ok(output_payload(&output))
}

pub(crate) fn output_payload(output: &std::process::Output) -> Value {
    let clip = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes).to_string();
        if text.chars().count() > 20_000 {
            text.chars().take(20_000).collect::<String>() + "\n… (truncated)"
        } else {
            text
        }
    };
    json!({
        "exitCode": output.status.code(),
        "stdout": clip(&output.stdout),
        "stderr": clip(&output.stderr),
    })
}

