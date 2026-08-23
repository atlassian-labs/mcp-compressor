mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::ChildGuard;

/// A test that spawns `mcp-compressor` must not strand it when an assertion
/// fails. The stranded binary keeps `target/debug/mcp-compressor.exe` open on
/// Windows, so one failing test turns every later `cargo test` into a build
/// error, and the backend it spawned keeps running too.
#[test]
fn child_guard_kills_the_process_tree_when_a_test_unwinds() {
    let tempdir = tempfile::tempdir().unwrap();
    let parent_beat = tempdir.path().join("beat");
    let child_beat = parent_beat.with_extension("child");

    let result = std::panic::catch_unwind({
        let parent_beat = parent_beat.clone();
        let child_beat = child_beat.clone();
        move || {
            let mut command = Command::new(common::python_command());
            command
                .arg(common::fixture_path("process_tree_server.py"))
                .arg("parent")
                .arg(&parent_beat)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _guard = ChildGuard::spawn(&mut command).unwrap();

            wait_for_growth(&parent_beat);
            wait_for_growth(&child_beat);

            panic!("simulated assertion failure");
        }
    });

    assert!(result.is_err(), "the closure must unwind");

    // Give the tree a moment to die, then prove neither process is still
    // writing: a survivor appends a heartbeat every 50ms.
    std::thread::sleep(Duration::from_millis(500));
    let parent_after = fs::metadata(&parent_beat).unwrap().len();
    let child_after = fs::metadata(&child_beat).unwrap().len();
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(
        fs::metadata(&parent_beat).unwrap().len(),
        parent_after,
        "the spawned process outlived the guard"
    );
    assert_eq!(
        fs::metadata(&child_beat).unwrap().len(),
        child_after,
        "a descendant of the spawned process outlived the guard"
    );
}

fn wait_for_growth(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = 0;
    while Instant::now() < deadline {
        if let Ok(metadata) = fs::metadata(path) {
            if metadata.len() > seen {
                seen = metadata.len();
                if seen > 0 {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {} to be written", path.display());
}
