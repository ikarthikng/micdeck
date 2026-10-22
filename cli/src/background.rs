//! `micdeck meeting on|off|status`: run meeting mode as a background process.
//!
//! "On" starts `micdeck meeting-run` detached from the terminal and remembers its process id in a small
//! file; "off" reads that file and stops the process. This is only a thin shim around the engine in
//! `micdeck_core::stream`, which the future menu-bar app will use directly instead.
// ponytail: no restart after a crash or reboot. A launchd agent can add that, but it needs an app bundle
// to get microphone permission, so it waits for the menu-bar app.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{fs, thread};

// Two C functions from the system library, declared by hand so we need no extra dependency.
extern "C" {
    /// Start a new session, so closing the terminal does not also stop the background process.
    fn setsid() -> i32;
    /// Send a signal to a process. Signal 15 (SIGTERM) politely asks it to stop.
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Where the process id is remembered. The temp folder is private to your macOS user and is cleared on
/// reboot, which matches the fact that meeting mode does not survive a reboot.
fn pid_file() -> PathBuf {
    std::env::temp_dir().join("micdeck-meeting.pid")
}

/// Where the background process writes its error messages (it has no terminal to print to).
fn log_file() -> PathBuf {
    std::env::temp_dir().join("micdeck-meeting.log")
}

/// The process id of the running meeting mode, if there is one.
fn running() -> Option<u32> {
    let pid: u32 = fs::read_to_string(pid_file()).ok()?.trim().parse().ok()?;
    // After a crash or reboot the number may belong to some unrelated program now, so check that the
    // process with this id really is ours: `ps` prints its command line, or nothing if it does not exist.
    let out = Command::new("ps").args(["-p", &pid.to_string(), "-o", "command="]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).contains("meeting-run").then_some(pid)
}

/// Start meeting mode in the background: DuoCast in, cleaned audio into the `output` device.
pub fn on(strength: u8, output: &str) -> Result<(), String> {
    if let Some(pid) = running() {
        return Err(format!("meeting mode is already on (pid {pid}); turn it off first with: micdeck meeting off"));
    }
    let log = fs::File::create(log_file()).map_err(|e| format!("cannot write the log file: {e}"))?;
    // Run ourselves again with the `meeting-run` command, with no input, no output, and errors sent to the log.
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command.args(["meeting-run", &strength.to_string(), output]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
    // SAFETY: `pre_exec` runs in the new process just before it starts; `setsid` is safe to call there.
    unsafe {
        command.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|e| format!("cannot start: {e}"))?;

    // If it is going to fail (DuoCast or the output device missing, say), it fails within a moment. Wait
    // briefly so we can show the reason now instead of letting you find out in a meeting.
    thread::sleep(Duration::from_millis(1500));
    if let Ok(Some(_)) = child.try_wait() {
        let why = fs::read_to_string(log_file()).unwrap_or_default();
        return Err(format!("meeting mode did not start: {}", why.trim()));
    }
    fs::write(pid_file(), child.id().to_string()).map_err(|e| format!("cannot remember the process id: {e}"))?;
    println!("meeting mode on (pid {}): DuoCast -> {output}, strength {strength}. Stop it with: micdeck meeting off", child.id());
    Ok(())
}

/// Stop meeting mode.
pub fn off() -> Result<(), String> {
    let pid = running().ok_or("meeting mode is not running")?;
    if unsafe { kill(pid as i32, 15) } != 0 {
        return Err(format!("could not stop process {pid}"));
    }
    let _ = fs::remove_file(pid_file()); // fine if it is already gone
    println!("meeting mode off");
    Ok(())
}

/// Say whether meeting mode is running.
pub fn status() -> Result<(), String> {
    match running() {
        Some(pid) => println!("meeting mode is on (pid {pid})"),
        None => println!("meeting mode is off"),
    }
    Ok(())
}
