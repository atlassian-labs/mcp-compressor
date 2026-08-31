mod common;

use std::time::Duration;

use mcp_compressor_core::app::entrypoint::run_from;

/// A caller stack far below what argument parsing plus the CLI's top-level
/// future needs. Measured on Windows debug builds: without the entrypoint's own
/// sized thread, the run overflows even at 512 KiB, while with it 128 KiB is
/// still enough, so this sits clear of both bounds.
const SMALL_CALLER_STACK_SIZE: usize = 256 * 1024;

/// `run_from` must run on a stack it sizes itself, not on the caller's.
///
/// The process main thread reserves 1 MiB on Windows and is not ours to resize,
/// and embedders may call the entrypoint from any thread. Both `clap` parsing
/// and the aggregated startup/discovery/transform future are large and grow as
/// command paths are added, and exhausting the stack aborts the whole process
/// with only "has overflowed its stack" and no backtrace.
///
/// Driving the entrypoint from a deliberately small thread pins that guarantee:
/// without the re-spawn the test binary dies with STATUS_STACK_OVERFLOW rather
/// than failing an assertion.
#[test]
fn cli_entrypoint_runs_on_its_own_stack_not_the_callers() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    // Serialized rather than interpolated: the fixture path is absolute, and on
    // Windows its separators would otherwise land in the JSON as escapes.
    let config = serde_json::json!({
        "mcpServers": {
            "alpha": {
                "command": common::python_command(),
                "args": [common::fixture_path("alpha_server.py")],
            }
        }
    });
    std::fs::write(&config_path, config.to_string()).unwrap();

    std::env::set_var("MCP_COMPRESSOR_EXIT_AFTER_READY", "1");

    let worker = std::thread::Builder::new()
        .stack_size(SMALL_CALLER_STACK_SIZE)
        .spawn(move || {
            run_from([
                "mcp-compressor".to_string(),
                "--just-bash".to_string(),
                "--config".to_string(),
                config_path.to_string_lossy().into_owned(),
            ])
        })
        .expect("spawn small-stack caller");

    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while !worker.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "the entrypoint did not return"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let result = worker.join().expect("the entrypoint thread panicked");
    std::env::remove_var("MCP_COMPRESSOR_EXIT_AFTER_READY");
    result.expect("the entrypoint must complete from a small caller stack");
}
