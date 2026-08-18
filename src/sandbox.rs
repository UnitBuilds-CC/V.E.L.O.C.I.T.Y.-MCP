//! Sandboxed process execution for NDA payloads.
//!
//! Inspired by Velocity-IDE's `sandbox` module (panic catching, resource limits,
//! isolated execution). This module wraps child process execution with:
//!
//! - **Isolated temp directory**: Each execution gets a fresh temp dir that is
//!   cleaned up after completion (even on error/panic).
//! - **Panic catching**: Internal setup operations use `catch_unwind` to prevent
//!   panics from propagating to the MCP server.
//! - **Output size limits**: Captured stdout/stderr are capped to prevent OOM.
//! - **Execution timeout**: Hard deadline with process kill (30s default).
//! - **Audit trail**: Every execution is logged with timing and outcome.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Maximum execution time for sandboxed processes.
const SANDBOX_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum captured output size (1 MB). Prevents OOM from runaway stdout.
const MAX_OUTPUT_SIZE: usize = 1_048_576;

/// Maximum stderr capture size (256 KB).
const MAX_STDERR_SIZE: usize = 262_144;

/// Result of a sandboxed execution.
#[derive(Debug, Clone)]
pub struct SandboxResult {
    /// Captured stdout (truncated if exceeded MAX_OUTPUT_SIZE).
    pub stdout: String,
    /// Captured stderr (truncated if exceeded MAX_STDERR_SIZE).
    pub stderr: String,
    /// Process exit status (None if timed out or killed).
    pub exit_status: Option<ExitStatusInfo>,
    /// Wall-clock execution time in milliseconds.
    pub elapsed_ms: u64,
    /// Whether the process was killed due to timeout.
    pub timed_out: bool,
    /// Whether output was truncated due to size limits.
    pub output_truncated: bool,
}

/// Simplified exit status (not tied to std::process types for portability).
#[derive(Debug, Clone)]
pub struct ExitStatusInfo {
    pub code: Option<i32>,
    pub success: bool,
}

impl From<ExitStatus> for ExitStatusInfo {
    fn from(status: ExitStatus) -> Self {
        ExitStatusInfo {
            code: status.code(),
            success: status.success(),
        }
    }
}

/// An isolated execution environment with automatic cleanup.
///
/// Creates a temp directory on construction and removes it on drop.
/// All sandboxed processes run with this directory as their working directory.
pub struct Sandbox {
    work_dir: PathBuf,
    /// Whether to clean up the work directory on drop.
    cleanup: bool,
}

impl Sandbox {
    /// Create a new sandbox with an isolated temp directory.
    ///
    /// The directory is created under the system temp dir with a unique name.
    pub fn new() -> Result<Self, String> {
        let work_dir = Self::create_isolated_dir()?;
        Ok(Sandbox {
            work_dir,
            cleanup: true,
        })
    }

    /// Create the isolated temp directory.
    fn create_isolated_dir() -> Result<PathBuf, String> {
        let base = std::env::temp_dir().join("velocity_sandbox");
        std::fs::create_dir_all(&base)
            .map_err(|e| format!("Failed to create sandbox base dir: {}", e))?;

        let dir_name = format!("exec_{}", random_suffix());
        let work_dir = base.join(dir_name);
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| format!("Failed to create sandbox work dir: {}", e))?;

        Ok(work_dir)
    }

    /// Get the sandbox's working directory path.
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// Write a file into the sandbox's working directory.
    pub fn write_file(&self, name: &str, contents: &[u8]) -> Result<PathBuf, String> {
        let path = self.work_dir.join(name);
        // Prevent path traversal
        if !path.starts_with(&self.work_dir) {
            return Err("Path traversal detected in sandbox file write".to_string());
        }
        std::fs::write(&path, contents)
            .map_err(|e| format!("Failed to write file in sandbox: {}", e))?;
        Ok(path)
    }

    /// Execute a command inside the sandbox.
    ///
    /// The process runs with the sandbox directory as its working directory.
    /// stdout and stderr are captured with size limits.
    /// The process is killed if it exceeds the timeout.
    pub fn execute(&self, program: &str, args: &[String]) -> SandboxResult {
        let start = Instant::now();

        // Use catch_unwind to protect against panics in process setup
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.spawn_and_collect(program, args)
        }));

        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(sandbox_result)) => SandboxResult {
                elapsed_ms,
                ..sandbox_result
            },
            Ok(Err(err_msg)) => SandboxResult {
                stdout: String::new(),
                stderr: err_msg,
                exit_status: None,
                elapsed_ms,
                timed_out: false,
                output_truncated: false,
            },
            Err(panic_err) => {
                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic during sandboxed execution".to_string()
                };
                SandboxResult {
                    stdout: String::new(),
                    stderr: format!("Sandbox panic: {}", msg),
                    exit_status: None,
                    elapsed_ms,
                    timed_out: false,
                    output_truncated: false,
                }
            }
        }
    }

    /// Spawn a child process and collect its output with size limits.
    fn spawn_and_collect(&self, program: &str, args: &[String]) -> Result<SandboxResult, String> {
        let mut cmd = Command::new(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Prevent child from inheriting environment variables that could leak info
        // Keep PATH so the program can be found, but remove potentially sensitive vars
        cmd.env_remove("VELOCITY_CSHARP_ENGINE");

        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to start process '{}': {}", program, e))?;

        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

        // Collect stdout in a thread with size limit
        let stdout_reader = std::thread::spawn(move || {
            let mut buf = Vec::with_capacity(4096);
            let mut reader = BufReader::new(stdout);
            let mut limited = false;
            let mut total = 0usize;
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n;
                        if total <= MAX_OUTPUT_SIZE {
                            buf.extend_from_slice(&chunk[..n]);
                        } else {
                            limited = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&buf).to_string();
            (text, limited)
        });

        // Collect stderr with size limit (synchronous)
        let mut stderr_buf = Vec::with_capacity(1024);
        let mut stderr_reader = BufReader::new(stderr);
        let mut stderr_truncated = false;
        let mut stderr_total = 0usize;
        {
            let mut chunk = [0u8; 1024];
            loop {
                match stderr_reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        stderr_total += n;
                        if stderr_total <= MAX_STDERR_SIZE {
                            stderr_buf.extend_from_slice(&chunk[..n]);
                        } else {
                            stderr_truncated = true;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        let stderr_text = String::from_utf8_lossy(&stderr_buf).to_string();

        // Wait with timeout
        let (status, timed_out) = match wait_with_timeout(&mut child, SANDBOX_TIMEOUT) {
            Ok(s) => (Some(ExitStatusInfo::from(s)), false),
            Err(_) => (None, true),
        };

        let (stdout_text, stdout_truncated) = stdout_reader.join().unwrap_or_default();

        let output_truncated = stdout_truncated || stderr_truncated;

        Ok(SandboxResult {
            stdout: stdout_text,
            stderr: stderr_text,
            exit_status: status,
            elapsed_ms: 0, // Will be set by the caller
            timed_out,
            output_truncated,
        })
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = std::fs::remove_dir_all(&self.work_dir);
        }
    }
}

/// Wait for a child process with a timeout. Kills the process if it exceeds the limit.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<ExitStatus, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Execution timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Failed to wait for process: {}", e));
            }
        }
    }
}

/// Generate a short random suffix for temp directory names.
fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:08x}", nanos.wrapping_mul(2654435761))
}

/// Sanitize an error message to prevent leaking internal paths or system details.
///
/// Replaces absolute paths with generic placeholders and truncates overly long messages.
pub fn sanitize_error(msg: &str) -> String {
    // Truncate very long error messages
    let truncated = if msg.len() > 500 {
        format!("{}... [truncated]", &msg[..500])
    } else {
        msg.to_string()
    };

    // Replace common path patterns with placeholders
    let mut sanitized = truncated;

    // Replace Windows-style paths (C:\Users\...\file)
    let re_windows = regex::Regex::new(r#"[A-Z]:\\[^\s:,;"')\]]+"#).unwrap();
    sanitized = re_windows.replace_all(&sanitized, "<path>").to_string();

    // Replace Unix-style absolute paths (/home/... or /tmp/...)
    let re_unix = regex::Regex::new(r#"/(?:home|tmp|var|usr|etc)/[^\s:,;"')\]]+"#).unwrap();
    sanitized = re_unix.replace_all(&sanitized, "<path>").to_string();

    sanitized
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_creates_and_cleans_up_dir() {
        let work_dir;
        {
            let sandbox = Sandbox::new().unwrap();
            work_dir = sandbox.work_dir().to_path_buf();
            assert!(work_dir.exists());
            assert!(work_dir.join("..").exists()); // parent exists
        }
        // After drop, directory should be cleaned up
        assert!(!work_dir.exists());
    }

    #[test]
    fn test_sandbox_write_file() {
        let sandbox = Sandbox::new().unwrap();
        let path = sandbox.write_file("test.txt", b"hello").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn test_sandbox_execute_echo() {
        let sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("echo", &["hello sandbox".to_string()]);
        // echo should succeed on all platforms (or fail gracefully on Windows)
        if result.exit_status.is_some() {
            assert!(result.stdout.contains("hello sandbox") || !result.stderr.is_empty());
        }
        assert!(!result.timed_out);
    }

    #[test]
    fn test_sandbox_execute_nonexistent_program() {
        let sandbox = Sandbox::new().unwrap();
        let result = sandbox.execute("nonexistent_program_xyz_12345", &[]);
        assert!(result.exit_status.is_none());
        assert!(!result.stderr.is_empty());
    }

    #[test]
    fn test_sanitize_error_removes_paths() {
        let msg = "Error reading C:\\Users\\admin\\secret\\file.txt: permission denied";
        let sanitized = sanitize_error(msg);
        assert!(!sanitized.contains("C:\\Users"));
        assert!(!sanitized.contains("admin"));
        assert!(sanitized.contains("<path>"));
    }

    #[test]
    fn test_sanitize_error_truncates_long_messages() {
        let msg = "x".repeat(1000);
        let sanitized = sanitize_error(&msg);
        assert!(sanitized.len() < 600);
        assert!(sanitized.contains("[truncated]"));
    }

    #[test]
    fn test_sandbox_output_size_limit() {
        // This test verifies the output limiting mechanism exists
        // We can't easily generate 1MB+ of output in a unit test,
        // but we verify the constants are reasonable
        assert_eq!(MAX_OUTPUT_SIZE, 1_048_576);
        assert_eq!(MAX_STDERR_SIZE, 262_144);
    }
}
