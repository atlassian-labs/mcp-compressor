mod common;

use std::process::Command;

use mcp_compressor_core::client_gen::cli::CliGenerator;
use mcp_compressor_core::client_gen::python::PythonGenerator;
use mcp_compressor_core::client_gen::typescript::TypeScriptGenerator;
use mcp_compressor_core::client_gen::{ClientGenerator, GeneratorConfig};
use mcp_compressor_core::proxy::ToolProxyServer;
use mcp_compressor_core::server::CompressedServer;

async fn running_proxy_config(output_dir: &std::path::Path) -> GeneratorConfig {
    let compressed = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let proxy = ToolProxyServer::start(compressed).await.unwrap();

    GeneratorConfig {
        cli_name: "alpha".to_string(),
        bridge_url: proxy.bridge_url().to_string(),
        token: proxy.token_value().to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "echo",
            Some("Echo a message from alpha.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "required": ["message"]
            }),
        )],
        session_pid: std::process::id(),
        output_dir: output_dir.to_path_buf(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_cli_script_invokes_real_proxy_and_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let config = running_proxy_config(tempdir.path()).await;
    let paths = CliGenerator.generate(&config).unwrap();
    let script = paths
        .iter()
        .find(|path| path.file_name().unwrap() == "alpha")
        .unwrap();

    let output = Command::new(script)
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_python_module_invokes_real_proxy_and_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let config = running_proxy_config(tempdir.path()).await;
    PythonGenerator.generate(&config).unwrap();

    let code = "import alpha; print(alpha.echo('hello'))";
    let output = Command::new(common::python_command())
        .env("PYTHONPATH", tempdir.path())
        .args(["-c", code])
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_typescript_module_invokes_real_proxy_and_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let config = running_proxy_config(tempdir.path()).await;
    TypeScriptGenerator.generate(&config).unwrap();

    let module_path = tempdir.path().join("alpha.ts");
    let output = Command::new("bun")
        .arg("--eval")
        .arg(format!(
            "import {{ echo }} from '{}'; console.log(await echo('hello'));",
            module_path.display()
        ))
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
}
