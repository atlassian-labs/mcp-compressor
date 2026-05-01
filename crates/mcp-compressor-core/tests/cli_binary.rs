mod common;

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
    .stdout(predicate::str::contains("alpha_help"));
}

#[test]
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
