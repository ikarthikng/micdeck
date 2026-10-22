//! Background processes for `micdeck meeting on|off|status` and `micdeck light on|off|status`.
//!
//! "Start" runs the matching foreground command (`meeting-run`, `effect`) again, detached from the terminal,
//! and remembers its process id in a small file; "stop" reads that file and stops the process. This is only
//! a thin shim: the work is done by `micdeck_core::stream` and the DuoCast driver, which the future menu-bar
//! app will use directly instead.
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

/// One kind of background process. Each has its own process-id file and log, so meeting mode and the lights
/// can run at the same time.
pub struct Job {
    /// What to call it in messages.
    name: &'static str,
    /// The command group typed by the user (`micdeck <command> off`); also used in the file names.
    command: &'static str,
    /// A piece of text that appears in the command line of this job's process. After a crash or reboot a
    /// remembered process id may belong to an unrelated program, so we check for this text before trusting it.
    marker: &'static str,
}

/// Meeting mode: the DuoCast's audio, cleaned, into a virtual mic.
pub const MEETING: Job = Job { name: "meeting mode", command: "meeting", marker: "meeting-run" };
/// The mic's lights: a color or an effect that keeps streaming.
pub const LIGHT: Job = Job { name: "lighting", command: "light", marker: " effect " };

impl Job {
    /// Where the process id is remembered. The temp folder is private to your macOS user and is cleared on
    /// reboot, which matches the fact that these jobs do not survive a reboot.
    fn pid_file(&self) -> PathBuf {
        std::env::temp_dir().join(format!("micdeck-{}.pid", self.command))
    }

    /// Where the background process writes its messages (it has no terminal to print to).
    fn log_file(&self) -> PathBuf {
        std::env::temp_dir().join(format!("micdeck-{}.log", self.command))
    }

    /// The process id of the running job, if there is one.
    fn running(&self) -> Option<u32> {
        let pid: u32 = fs::read_to_string(self.pid_file()).ok()?.trim().parse().ok()?;
        // `ps` prints the process's command line, or nothing if no such process exists.
        let out = Command::new("ps").args(["-p", &pid.to_string(), "-o", "command="]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).contains(self.marker).then_some(pid)
    }

    /// Run `micdeck <args>` in the background. `summary` says what it does, for the confirmation message.
    pub fn start(&self, args: &[&str], summary: &str) -> Result<(), String> {
        if let Some(pid) = self.running() {
            return Err(format!("{} is already running (pid {pid}); stop it first with: micdeck {} off", self.name, self.command));
        }
        let log = fs::File::create(self.log_file()).map_err(|e| format!("cannot write the log file: {e}"))?;
        // Run ourselves again with no input, no output, and errors sent to the log.
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
        // SAFETY: `pre_exec` runs in the new process just before it starts; `setsid` is safe to call there.
        unsafe {
            command.pre_exec(|| {
                setsid();
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|e| format!("cannot start: {e}"))?;

        // If it is going to fail (the DuoCast or the output device missing, say), it fails within a moment.
        // Wait briefly so we can show the reason now instead of letting you find out in a meeting.
        thread::sleep(Duration::from_millis(1500));
        if let Ok(Some(_)) = child.try_wait() {
            let why = fs::read_to_string(self.log_file()).unwrap_or_default();
            return Err(format!("{} did not start: {}", self.name, why.trim()));
        }
        fs::write(self.pid_file(), child.id().to_string()).map_err(|e| format!("cannot remember the process id: {e}"))?;
        println!("{} started (pid {}): {summary}. Stop it with: micdeck {} off", self.name, child.id(), self.command);
        Ok(())
    }

    /// Stop the job.
    pub fn stop(&self) -> Result<(), String> {
        let pid = self.running().ok_or_else(|| format!("{} is not running", self.name))?;
        if unsafe { kill(pid as i32, 15) } != 0 {
            return Err(format!("could not stop process {pid}"));
        }
        let _ = fs::remove_file(self.pid_file()); // fine if it is already gone
        println!("{} stopped", self.name);
        Ok(())
    }

    /// Say whether the job is running.
    pub fn status(&self) -> Result<(), String> {
        match self.running() {
            Some(pid) => {
                println!("{} is running (pid {pid})", self.name);
                // The background process reports what it is doing in its log, e.g. "a device was unplugged".
                let log = fs::read_to_string(self.log_file()).unwrap_or_default();
                if let Some(last) = log.lines().rev().find(|line| !line.trim().is_empty()) {
                    println!("latest: {last}");
                }
            }
            None => println!("{} is not running", self.name),
        }
        Ok(())
    }
}
