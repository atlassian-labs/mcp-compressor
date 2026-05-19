mod common;

use std::io::{BufRead, BufReader};
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;

fn core_cmd() -> Command {
    Command::cargo_bin("mcp-compressor").unwrap()
}

#[test]
fn rust_cli_help_describes_supported_modes() {
    let mut cmd = core_cmd();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("<URL_OR_COMMAND>"))
        .stdout(predicate::str::contains("Backend URL or command plus backend arguments"))
        .stdout(predicate::str::contains("--compression <COMPRESSION>"))
        .stdout(predicate::str::contains("--config <CONFIG_PATH>"))
        .stdout(predicate::str::contains(
            "--transform-mode <TRANSFORM_MODE>",
        ))
        .stdout(predicate::str::contains("--cli-mode"))
        .stdout(predicate::str::contains("--just-bash"));
}

#[test]
fn rust_cli_clear_oauth_without_state_reports_no_credentials() {
    let tempdir = tempfile::tempdir().unwrap();
    let mut cmd = core_cmd();
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("XDG_CONFIG_HOME", tempdir.path())
        .env("HOME", tempdir.path().join("home"))
        .arg("clear-oauth")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No stored OAuth credentials found.",
        ));
}

#[test]
fn rust_cli_invalid_compression_level_exits_nonzero() {
    let mut cmd = core_cmd();
    cmd.args(["--compression", "verbose"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'verbose'"));
}

#[test]
#[ignore = "normal mode is now a long-running stdio MCP server; covered by Python e2e"]
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
#[ignore = "normal mode is now a long-running stdio MCP server; covered by Python e2e"]
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
#[ignore = "normal mode is now a long-running stdio MCP server; covered by Python e2e"]
fn rust_cli_contract_multi_server_json_config() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    std::fs::write(
        &config_path,
        common::mcp_config_json(&[("alpha", "alpha_server.py"), ("beta", "beta_server.py")]),
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
    .stdout(predicate::str::contains("alpha_invoke_tool"))
    .stdout(predicate::str::contains("beta_invoke_tool"));
}

#[test]
fn rust_cli_rejects_server_name_with_mcp_config() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    std::fs::write(&config_path, common::mcp_config_json(&[("alpha", "alpha_server.py")])).unwrap();

    let mut cmd = core_cmd();
    cmd.args([
        "--server-name",
        "custom",
        "--config",
        config_path.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .code(2)
    .stderr(predicate::str::contains("--server-name cannot be used with --config"));
}

#[test]
fn rust_cli_code_mode_python_generates_python_client() {
    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("generated-py");

    let mut cmd = core_cmd();
    cmd.env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")
        .args([
            "--code-mode",
            "python",
            "--server-name",
            "alpha",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Python code client ready"))
        .stdout(predicate::str::contains("Generated files:"))
        .stdout(predicate::str::contains("Import the generated client"))
        .stdout(predicate::str::contains("Invoke with:").not());

    assert!(output_dir.join("alpha.py").exists());
}

#[test]
fn rust_cli_code_mode_defaults_to_dist_directory() {
    let tempdir = tempfile::tempdir().unwrap();
    let dist = tempdir.path().join("dist");

    let mut cmd = core_cmd();
    cmd.current_dir(tempdir.path())
        .env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")
        .args([
            "--code-mode",
            "python",
            "--server-name",
            "alpha",
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Python code client ready"))
        .stdout(predicate::str::contains("dist/alpha.py"));

    assert!(dist.join("alpha.py").exists());
}

#[test]
fn rust_cli_code_mode_typescript_generates_typescript_client() {
    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("generated-ts");

    let mut cmd = core_cmd();
    cmd.env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")
        .args([
            "--code-mode",
            "typescript",
            "--server-name",
            "alpha",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("TypeScript code client ready"))
        .stdout(predicate::str::contains("Generated files:"));

    assert!(output_dir.join("alpha.ts").exists());
    assert!(output_dir.join("alpha.d.ts").exists());
}

#[test]
fn rust_cli_code_mode_rejects_conflicting_mode_aliases() {
    let mut cmd = core_cmd();
    cmd.args([
        "--code-mode",
        "python",
        "--typescript-mode",
        "--server-name",
        "alpha",
        "--",
        &common::python_command(),
        common::fixture_path("alpha_server.py").to_str().unwrap(),
    ])
    .assert()
    .failure()
    .code(2)
    .stderr(predicate::str::contains("choose only one code mode"));
}

#[test]
fn rust_cli_code_mode_rejects_conflicting_runtime_modes() {
    let mut cmd = core_cmd();
    cmd.args([
        "--code-mode",
        "python",
        "--cli-mode",
        "--server-name",
        "alpha",
        "--",
        &common::python_command(),
        common::fixture_path("alpha_server.py").to_str().unwrap(),
    ])
    .assert()
    .failure()
    .code(2)
    .stderr(predicate::str::contains("choose only one of --cli-mode"));
}

#[test]
fn rust_cli_supports_version_flag() {
    let mut cmd = core_cmd();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mcp-compressor"));
}

#[test]
fn rust_cli_rejects_backend_options_before_separator() {
    let mut cmd = core_cmd();
    cmd.args([
        "--cwd",
        "/tmp",
        "--",
        &common::python_command(),
        common::fixture_path("alpha_server.py").to_str().unwrap(),
    ])
    .assert()
    .failure()
    .code(2)
    .stderr(predicate::str::contains("unexpected argument '--cwd'"));
}

#[test]
fn rust_cli_applies_cwd_and_env_to_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("bin");
    let mut cmd = core_cmd();
    cmd.env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")
        .args([
            "--cli-mode",
            "--server-name",
            "alpha",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
            "--cwd",
            tempdir.path().to_str().unwrap(),
            "--env",
            "MCP_COMPRESSOR_TEST_ENV=enabled",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("CLI ready"));
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
        .stdout(predicate::str::contains(
            "Invoke with: alpha <subcommand> [args...]",
        ));

    assert!(bin.join("alpha").exists());
}

#[test]
fn rust_cli_mode_honors_explicit_output_dir() {
    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("custom-bin");
    let expected_script = output_dir.join("alpha");

    let mut cmd = core_cmd();
    cmd.env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1")
        .args([
            "--cli-mode",
            "--server-name",
            "alpha",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--",
            &common::python_command(),
            common::fixture_path("alpha_server.py").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Generated CLI: {}",
            expected_script.display()
        )));

    assert!(expected_script.exists());
}

#[test]
fn rust_cli_mode_manual_flow_generates_script_that_invokes_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let output_dir = tempdir.path().join("generated");
    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("mcp-compressor"))
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

    let help = StdCommand::new(&script_path)
        .arg("--help")
        .output()
        .unwrap();
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
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "alpha:hello"
    );

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
fn rust_cli_contract_just_bash_transform_mode_multi_server() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_path = tempdir.path().join("mcp.json");
    std::fs::write(
        &config_path,
        common::mcp_config_json(&[("alpha", "alpha_server.py"), ("beta", "beta_server.py")]),
    )
    .unwrap();

    let mut cmd = core_cmd();
    cmd.env("MCP_COMPRESSOR_EXIT_AFTER_READY", "1");
    cmd.args(["--just-bash", "--config", config_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Just Bash ready"))
        .stdout(predicate::str::contains("Bridge URL:"))
        .stdout(predicate::str::contains("Session: bash"));
}


#[test]
fn rust_cli_mode_exits_on_ctrl_c() {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("mcp-compressor"))
            .arg("--cli-mode")
            .arg("--server-name")
            .arg("alpha")
            .arg("--output-dir")
            .arg(tempfile::tempdir().expect("tempdir").path())
            .arg("--")
            .arg(common::python_command())
            .arg(common::fixture_path("alpha_server.py"))
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn mcp-compressor");

        let stdout = child.stdout.take().expect("stdout");
        let reader = BufReader::new(stdout);
        let started = Instant::now();
        let mut saw_ready = false;
        for line in reader.lines() {
            let line = line.expect("line");
            if line.contains("Press Ctrl+C to stop.") {
                saw_ready = true;
                break;
            }
            if started.elapsed() > Duration::from_secs(20) {
                break;
            }
        }
        assert!(saw_ready, "CLI mode did not become ready");

        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }

        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                assert!(status.success(), "unexpected status: {status}");
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "process did not exit after SIGINT"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
