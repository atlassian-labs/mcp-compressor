use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{
    BackendConfigSource, BackendServerConfig, CompressedServerConfig, ProxyTransformMode,
};

/// A spawned child process that is killed, with its descendants, when dropped.
///
/// Tests that spawn `mcp-compressor` also spawn its backend processes. Killing
/// the child at the end of the test body is not enough: a failed assertion
/// unwinds past that call, leaving the whole tree running. On Windows the
/// stranded binary then holds `target\debug\mcp-compressor.exe` open, so every
/// later `cargo test` fails with "failed to remove file ... (os error 5)" and a
/// single test failure blocks the entire suite.
///
/// Cleaning up in `Drop` runs on both the success and the unwind path.
#[allow(dead_code)]
pub struct ChildGuard {
    child: Option<Child>,
}

#[allow(dead_code)]
impl ChildGuard {
    /// Spawn `command` under a guard.
    ///
    /// On Unix the child gets its own process group so the group can be
    /// signalled as a unit; Windows has no equivalent at spawn time, so the
    /// tree is walked at kill time instead.
    pub fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        Ok(Self {
            child: Some(command.spawn()?),
        })
    }

    pub fn id(&self) -> u32 {
        self.child
            .as_ref()
            .expect("child is only taken in Drop")
            .id()
    }

    pub fn take_stdout(&mut self) -> std::process::ChildStdout {
        self.child
            .as_mut()
            .expect("child is only taken in Drop")
            .stdout
            .take()
            .expect("stdout was piped")
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child
            .as_mut()
            .expect("child is only taken in Drop")
            .try_wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        kill_process_tree(&mut child);
        let _ = child.wait();
    }
}

#[cfg(windows)]
fn kill_process_tree(child: &mut Child) {
    // taskkill /T terminates the process and every descendant. Without it the
    // backend the compressor spawned survives its parent.
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(unix)]
fn kill_process_tree(child: &mut Child) {
    // The child leads its own process group, so signalling the negated pid
    // reaches the backends it spawned as well.
    unsafe {
        libc::killpg(child.id() as i32, libc::SIGKILL);
    }
    let _ = child.kill();
}

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn python_command() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
}

pub fn backend(name: &str, fixture: &str) -> BackendServerConfig {
    BackendServerConfig::new(
        name,
        python_command(),
        [fixture_path(fixture).to_string_lossy().into_owned()],
    )
}

pub fn config(
    level: CompressionLevel,
    server_name: impl Into<Option<&'static str>>,
    transform_mode: ProxyTransformMode,
    config_source: BackendConfigSource,
) -> CompressedServerConfig {
    CompressedServerConfig {
        level,
        server_name: server_name.into().map(str::to_string),
        include_tools: Vec::new(),
        exclude_tools: Vec::new(),
        toonify: false,
        transform_mode,
        config_source,
    }
}

pub fn max_config(server_name: impl Into<Option<&'static str>>) -> CompressedServerConfig {
    config(
        CompressionLevel::Max,
        server_name,
        ProxyTransformMode::CompressedTools,
        BackendConfigSource::Command,
    )
}

pub fn mcp_config_json(backends: &[(&str, &str)]) -> String {
    let servers = backends
        .iter()
        .map(|(name, fixture)| {
            let path = fixture_path(fixture).to_string_lossy().into_owned();
            format!(
                r#""{name}":{{"command":"{}","args":["{}"]}}"#,
                python_command(),
                path
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"mcpServers":{{{servers}}}}}"#)
}
