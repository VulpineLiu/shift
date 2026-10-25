//! Process helpers that never open a console window.
//!
//! A GUI build owns no console, so on Windows every child process would
//! otherwise allocate a fresh one: users see a black window blink for each tool
//! probe, each conversion and each registry write. `CREATE_NO_WINDOW` suppresses
//! that, and centralising it here means a new `Command` cannot forget it the way
//! the scattered per-call constants did.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Windows console flag: run the child without giving it a window.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(target_os = "windows")]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Keep probe output bounded while still draining the pipe. Stopping the read
/// would let a verbose child fill the pipe and block until the deadline.
const MAX_CAPTURED_OUTPUT: usize = 4 * 1024 * 1024;

fn drain_output(mut reader: impl Read) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = MAX_CAPTURED_OUTPUT.saturating_sub(captured.len());
                if remaining > 0 {
                    captured.extend_from_slice(&buffer[..count.min(remaining)]);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    captured
}

/// Suppress the child's console window. Every spawn in the app goes through this
/// so a background tool call never flashes on screen.
pub fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // A separate process group also keeps a console attached to an inherited
        // launcher (WinGet, a debug terminal) from appearing above Shift.
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
    command
}

/// Keep a long conversion below the UI's scheduling priority. A renderer that
/// saturates every core can otherwise make the whole desktop appear frozen even
/// though Shift itself is still responsive.
pub fn background(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | BELOW_NORMAL_PRIORITY_CLASS,
        );
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
    command
}

/// A new `Command` for `program` with the console and standard streams already
/// detached, for the chained `Command::new(x).args(..).status()` shape.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hidden(&mut command);
    command
}

/// `Command::output` with the console suppressed.
pub fn output(program: impl AsRef<OsStr>, args: &[&str]) -> io::Result<Output> {
    let mut command = command(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.output()
}

/// `Command::output` with a deadline. Version probes must never keep the
/// component panel in "Inspecting…" because a tool is waiting for input.
pub fn output_with_timeout(
    program: impl AsRef<OsStr>,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let mut command = command(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let deadline = Instant::now() + timeout;
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start the command: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .map(|pipe| std::thread::spawn(move || drain_output(pipe)));
    let stderr = child
        .stderr
        .take()
        .map(|pipe| std::thread::spawn(move || drain_output(pipe)));
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "The command did not finish within {} seconds.",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("Could not monitor the command: {error}"));
            }
        }
    };
    let stdout = stdout
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    result.map(|status| Output {
        status,
        stdout,
        stderr,
    })
}

/// Run a child with a hard deadline. Rendered PDF pages are the slowest step
/// in PDF-to-Word, and a malformed page must not leave the UI processing
/// forever. Returning an error also makes the caller remove partial output.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
pub fn status_with_timeout(
    program: impl AsRef<OsStr>,
    args: &[&str],
    timeout: Duration,
) -> Result<ExitStatus, String> {
    status_with_args_and_timeout(program, args, timeout)?
        .map(|(status, _)| status)
        .ok_or_else(|| "The converter exited without a status.".to_string())
}

fn status_with_args_and_timeout(
    program: impl AsRef<OsStr>,
    args: &[&str],
    timeout: Duration,
) -> Result<Option<(ExitStatus, Duration)>, String> {
    let mut command = crate::process::command(program);
    background(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let started = Instant::now();
    let deadline = started + timeout;
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start the converter: {error}"))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some((status, started.elapsed()))),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "The conversion did not finish within {} seconds.",
                    started.elapsed().as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not monitor the converter: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_command_still_runs() {
        let output = if cfg!(windows) {
            output("cmd", &["/C", "echo shift"]).unwrap()
        } else {
            output("/bin/echo", &["shift"]).unwrap()
        };
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("shift"));
    }

    #[test]
    fn hiding_the_console_twice_changes_nothing() {
        let mut command = Command::new(if cfg!(windows) { "cmd" } else { "/bin/echo" });
        command.args(if cfg!(windows) {
            &["/C", "echo", "shift"][..]
        } else {
            &["shift"][..]
        });
        hidden(&mut command);
        assert!(hidden(&mut command).status().unwrap().success());
    }

    #[test]
    fn default_commands_do_not_inherit_standard_streams() {
        let result = command(if cfg!(windows) { "cmd" } else { "/bin/echo" })
            .args(if cfg!(windows) {
                &["/C", "echo", "shift"][..]
            } else {
                &["shift"][..]
            })
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_timed_out_child_is_killed() {
        let result = status_with_timeout("/bin/sleep", &["5"], Duration::from_millis(50));
        assert!(result.is_err(), "the timeout should terminate the child");
    }

    #[cfg(unix)]
    #[test]
    fn captured_output_has_a_deadline() {
        let result = output_with_timeout("/bin/echo", &["shift"], Duration::from_secs(2)).unwrap();
        assert!(String::from_utf8_lossy(&result.stdout).contains("shift"));
        assert!(output_with_timeout("/bin/sleep", &["5"], Duration::from_millis(50)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn verbose_output_is_drained_without_blocking_the_child() {
        let result = output_with_timeout(
            "/bin/sh",
            &["-c", "yes shift | head -c 8388608"],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(result.status.success());
        assert_eq!(result.stdout.len(), MAX_CAPTURED_OUTPUT);
    }
}
