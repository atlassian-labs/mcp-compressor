mod common;

use std::io::{BufRead, BufReader};
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;

fn core_cmd() -> Command {
    Command::cargo_bin("mcp-compressor-core").unwrap()
}

#[test]
fn rust_cli_help_describes_supported_modes() {
    let mut cmd = core_cmd();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--compression <LEVEL>"))
        .stdout(predicate::str::contains("--config <PATH>"))
        .stdout(predicate::str::contains("--transform-mode <MODE>"))
        .stdout(predicate::str::contains("--cli-mode"))
        .stdout(predicate::str::contains("--just-bash"));
}

#[test]
fn rust_cli_invalid_compression_level_exits_nonzero() {
    let mut cmd = core_cmd();
    cmd.args(["--compression", "verbose"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("unknown compression level: verbose"));
}

#[test]
fn rust_cli_contract_single_server_direct_command_all_compression_levels() {
    for level in ["low", "medium", "high", "max"] {
        let mut cmd = core_cmd();
        cmd.args([
            "--compression",
            level,
            "--server-name",
            "alpha",
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha_get_tool_schema"))
        .stdout(predicate::str::contains("alpha_invoke_tool"));
    }
}

#[test]
fn rust_cli_contract_single_server_json_config() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    std::fs::write(
        &config_path,
        common::mcp_config_json(&[("alpha", "alpha_server.py")]),
    )
    .unwrap();

    let mut cmd = core_cmd();
    cmd.args([
        "--compression",
        "max",
        "--config",
        config_path.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("get_tool_schema"))
    .stdout(predicate::str::contains("invoke_tool"))
    .stdout(predicate::str::contains("list_tools"));
}

#[test]
fn rust_cli_contract_multi_server_json_config() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    std::fs::write(
        &config_path,
        common::mcp_config_json(&[
            ("alpha", "alpha_server.py"),
            ("beta", "beta_server.py"),
        ]),
    )
    .unwrap();

    let mut cmd = core_cmd();
    cmd.args([
        "--compression",
        "max",
        "--server-name",
        "suite",
        "--config",
        config_path.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("suite_alpha_invoke_tool"))
    .stdout(predicate::str::contains("suite_beta_invoke_tool"));
}

#[test]
fn rust_cli_contract_cli_transform_mode() {
    let mut cmd = core_cmd();
    cmd.env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1");
    cmd.args([
        "--transform-mode",
        "cli",
        "--server-name",
        "alpha",
        "--",
        &common::python_command(),
        common::fixture_path("alpha_server.py").to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("CLI ready"))
        .stdout(predicate::str::contains("Generated CLI:"));
}

#[test]
fn rust_cli_mode_installs_generated_script_in_path_candidate_by_default() {
    let tempdir = tempfile::tempdir().unwrap();
    let home = tempdir.path().join("home");
    let bin = home.join(".local").join("bin");
    std::fs::create_dir_all(&bin).unwrap();

    let path = std::env::join_paths(
        std::iter::once(bin.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .unwrap();

    let expected_script = bin.canonicalize().unwrap().join("alpha");

    let mut cmd = core_cmd();
    cmd.env("HOME", &home)
        .env("PATH", path)
        .env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")
        .args([
            "--cli-mode",
            "--server-name",
            "alpha",
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Generated CLI: {}",
            expected_script.display()
        )))
        .stdout(predicate::str::contains("Invoke with: alpha <subcommand> [args...]"));

    assert!(bin.join("alpha").exists());
}

#[test]
fn rust_cli_mode_manual_flow_generates_script_that_invokes_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("generated");
    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("mcp-compressor-core"))
        .env("MCP_COMPRESSOR_CLI_OUTPUT_DIR", &output_dir)
        .args([
            "--cli-mode",
            "--server-name",
            "alpha",
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let script_path = wait_for_generated_cli_path(&mut reader);

    let help = StdCommand::new(&script_path).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("alpha - the alpha toolset"));
    assert!(help.contains("SUBCOMMANDS:"));
    assert!(help.contains("echo"));
    assert!(help.contains("Echo a message from alpha"));

    let subcommand_help = StdCommand::new(&script_path)
        .args(["echo", "--help"])
        .output()
        .unwrap();
    assert!(subcommand_help.status.success());
    let subcommand_help = String::from_utf8_lossy(&subcommand_help.stdout);
    assert!(subcommand_help.contains("alpha echo"));
    assert!(subcommand_help.contains("--message <value>"));

    let output = StdCommand::new(&script_path)
        .args(["echo", "--message", "hello"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "alpha:hello");

    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_generated_cli_path(reader: &mut impl BufRead) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        let bytes = reader.read_line(&mut line).unwrap();
        if bytes == 0 {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        if let Some(path) = line.trim().strip_prefix("Generated CLI: ") {
            return path.to_string();
        }
    }
    panic!("timed out waiting for Generated CLI line");
}

#[test]
#[ignore = "Just Bash runtime is implemented after CLI mode/proxy runtime"]
fn rust_cli_contract_just_bash_transform_mode_multi_server() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    std::fs::write(
        &config_path,
        common::mcp_config_json(&[
            ("alpha", "alpha_server.py"),
            ("beta", "beta_server.py"),
        ]),
    )
    .unwrap();

    let mut cmd = core_cmd();
    cmd.args([
        "--just-bash",
        "--config",
        config_path.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("bash_tool"))
    .stdout(predicate::str::contains("alpha_help"))
    .stdout(predicate::str::contains("beta_help"));
}
