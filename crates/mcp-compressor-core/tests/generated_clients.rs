mod common;

use std::{io, net::TcpListener, process::Command, thread, time::Duration};

use mcp_compressor_core::client_gen::cli::CliGenerator;
use mcp_compressor_core::client_gen::python::PythonGenerator;
use mcp_compressor_core::client_gen::typescript::TypeScriptGenerator;
use mcp_compressor_core::client_gen::{ClientGenerator, GeneratorConfig};
use mcp_compressor_core::proxy::{RunningToolProxy, ToolProxyServer};
use mcp_compressor_core::server::CompressedServer;

async fn real_backend_tools() -> Vec<mcp_compressor_core::compression::engine::Tool> {
    let compressed = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    compressed.backend_tools()
}

async fn running_proxy_config(output_dir: &std::path::Path) -> (GeneratorConfig, RunningToolProxy) {
    let compressed = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let proxy = ToolProxyServer::start(compressed).await.unwrap();

    let config = GeneratorConfig {
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
        extra_cli_bridges: Vec::new(),
    };
    (config, proxy)
}

fn generated_script_output(script: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut last_error = None;
    for attempt in 0..5 {
        match Command::new(script).args(args).output() {
            Ok(output) => return output,
            Err(error) if error.raw_os_error() == Some(26) && attempt < 4 => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(25 * (attempt + 1) as u64));
            }
            Err(error) => panic!("failed to execute {}: {error}", script.display()),
        }
    }
    let error =
        last_error.unwrap_or_else(|| io::Error::other("unknown generated script execution error"));
    panic!(
        "failed to execute {} after retrying ETXTBSY: {error}",
        script.display()
    );
}

#[test]
fn generated_cli_help_renders_camel_case_properties_as_kebab_case_flags() {
    let tempdir = tempfile::tempdir().unwrap();
    let config = GeneratorConfig {
        cli_name: "atlassian".to_string(),
        bridge_url: "http://127.0.0.1:1".to_string(),
        token: "token".to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "searchJiraIssuesUsingJql",
            Some("Search issues with JQL".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "cloudId": { "type": "string", "description": "Cloud ID" },
                    "jql": { "type": "string", "description": "JQL query" },
                    "maxResults": { "type": "number", "description": "Max results" },
                    "nextPageToken": { "type": "string", "description": "Page token" }
                },
                "required": ["cloudId", "jql"]
            }),
        )],
        session_pid: std::process::id(),
        output_dir: tempdir.path().to_path_buf(),
        extra_cli_bridges: Vec::new(),
    };
    CliGenerator.generate(&config).unwrap();
    let script = tempdir.path().join("atlassian");
    let output = generated_script_output(&script, &["search-jira-issues-using-jql", "--help"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--cloud-id <string>"), "stdout: {stdout}");
    assert!(stdout.contains("--jql <string>"), "stdout: {stdout}");
    assert!(
        stdout.contains("--max-results <number>"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("--next-page-token <string>"),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("--cloudId"), "stdout: {stdout}");
    assert!(!stdout.contains("--maxResults"), "stdout: {stdout}");
}

#[test]
fn generated_cli_schema_blob_allows_literal_backslash_unicode_fragments() {
    let tempdir = tempfile::tempdir().unwrap();
    let config = GeneratorConfig {
        cli_name: "slack".to_string(),
        bridge_url: "http://127.0.0.1:1".to_string(),
        token: "token".to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "slackmcp_slack_search_public",
            Some("Searches public channels. Example malformed user text can include literal \\u fragments.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "A query string. Literal text may contain C:\\users\\tesler or incomplete \\u escape fragments from docs."
                    }
                },
                "required": ["query"]
            }),
        )],
        session_pid: std::process::id(),
        output_dir: tempdir.path().to_path_buf(),
        extra_cli_bridges: Vec::new(),
    };
    CliGenerator.generate(&config).unwrap();
    let script = tempdir.path().join("slack");

    let output = generated_script_output(
        &script,
        &["slackmcp-slack-search-public", "--query", "hello"],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mcp-compressor proxy is not running"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("SyntaxError"), "stderr: {stderr}");
    assert!(!stderr.contains("unicodeescape"), "stderr: {stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_typescript_module_reports_stopped_proxy_without_fetch_noise() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, proxy) = running_proxy_config(tempdir.path()).await;
    let mut config = config;
    config.tools = real_backend_tools().await;
    let paths = TypeScriptGenerator.generate(&config).unwrap();
    let module = paths
        .iter()
        .find(|path| path.file_name().unwrap() == "alpha.ts")
        .unwrap();

    drop(proxy);

    let output = Command::new("bun")
        .arg("--eval")
        .arg(format!(
            "import {{ echo }} from {module:?}; await echo('hello')",
            module = module.display().to_string()
        ))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mcp-compressor proxy is not running"),
        "stderr: {stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_cli_script_handles_structured_json_arguments() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, _proxy) = running_proxy_config(tempdir.path()).await;
    let mut config = config;
    config.tools = real_backend_tools().await;

    CliGenerator.generate(&config).unwrap();
    let script = tempdir.path().join("alpha");

    let output = std::process::Command::new(&script)
        .args([
            "summarize-payload",
            "--items",
            "[\"one\",\"two\"]",
            "--metadata",
            "{\"source\":\"generated-cli\",\"ok\":true}",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("item_count"));
    assert!(stdout.contains("generated-cli"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_cli_script_matches_legacy_argument_parser_features() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, _proxy) = running_proxy_config(tempdir.path()).await;
    let mut config = config;
    config.tools = real_backend_tools().await;

    CliGenerator.generate(&config).unwrap();
    let script = tempdir.path().join("alpha");

    let repeated = std::process::Command::new(&script)
        .args([
            "summarize-payload",
            "--items",
            "one",
            "--items",
            "two",
            "--metadata",
            "{\"source\":\"repeat\"}",
        ])
        .output()
        .unwrap();
    assert!(
        repeated.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    let stdout = String::from_utf8_lossy(&repeated.stdout);
    assert!(stdout.contains("repeat"));
    assert!(stdout.contains("one"));
    assert!(stdout.contains("two"));

    let json_escape = std::process::Command::new(&script)
        .args([
            "summarize-payload",
            "--json",
            "{\"items\":[\"json\"],\"metadata\":{\"source\":\"escape\"}}",
        ])
        .output()
        .unwrap();
    assert!(
        json_escape.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&json_escape.stderr)
    );
    let stdout = String::from_utf8_lossy(&json_escape.stdout);
    assert!(stdout.contains("escape"));
    assert!(stdout.contains("json"));

    let unknown = generated_script_output(&script, &["echo", "--unknown", "value"]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown flag"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_cli_script_invokes_real_proxy_and_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, _proxy) = running_proxy_config(tempdir.path()).await;
    let mut config = config;
    config.tools = real_backend_tools().await;
    let paths = CliGenerator.generate(&config).unwrap();
    let script = paths
        .iter()
        .find(|path| path.file_name().unwrap() == "alpha")
        .unwrap();

    let output = generated_script_output(script, &["echo", "--message", "hello"]);

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
async fn generated_cli_script_reports_stopped_proxy_without_traceback() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, proxy) = running_proxy_config(tempdir.path()).await;
    let mut config = config;
    config.tools = real_backend_tools().await;
    let paths = CliGenerator.generate(&config).unwrap();
    let script = paths
        .iter()
        .find(|path| path.file_name().unwrap() == "alpha")
        .unwrap();

    drop(proxy);

    let output = generated_script_output(script, &["echo", "--message", "hello"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mcp-compressor proxy is not running"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("Traceback"), "stderr: {stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_python_module_invokes_real_proxy_and_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, _proxy) = running_proxy_config(tempdir.path()).await;
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
async fn generated_python_module_reports_stopped_proxy_without_urllib_traceback() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, proxy) = running_proxy_config(tempdir.path()).await;
    let mut config = config;
    config.tools = real_backend_tools().await;
    let paths = PythonGenerator.generate(&config).unwrap();
    let module = paths
        .iter()
        .find(|path| path.file_name().unwrap() == "alpha.py")
        .unwrap();

    drop(proxy);

    let output = Command::new(common::python_command())
        .arg("-c")
        .arg(format!(
            "import sys; sys.path.insert(0, {dir:?}); import alpha; alpha.echo('hello')",
            dir = module.parent().unwrap().display().to_string()
        ))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mcp-compressor proxy is not running"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("urllib.request"), "stderr: {stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_typescript_module_invokes_real_proxy_and_backend() {
    let tempdir = tempfile::tempdir().unwrap();
    let (config, _proxy) = running_proxy_config(tempdir.path()).await;
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

fn golden(relative_path: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("testdata/golden")
            .join(relative_path),
    )
    .unwrap()
    .trim_end()
    .to_string()
}

#[test]
fn generated_cli_help_documents_and_validates_schema_enums() {
    let bridge = start_json_result_bridge("unused");
    let tempdir = tempfile::tempdir().unwrap();
    let config = GeneratorConfig {
        cli_name: "slack".to_string(),
        bridge_url: bridge.url.clone(),
        token: "token".to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "slackmcp_slack_search_public_and_private",
            Some("Search Slack.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "sort": { "type": "string", "enum": ["score", "timestamp"], "description": "Sort by relevance or date." }
                },
                "required": ["query"]
            }),
        )],
        session_pid: std::process::id(),
        output_dir: tempdir.path().to_path_buf(),
        extra_cli_bridges: Vec::new(),
    };
    CliGenerator.generate(&config).unwrap();
    let script = tempdir.path().join("slack");

    let help = run_generated_script(
        &script,
        &["slackmcp-slack-search-public-and-private", "--help"],
    );
    assert!(help.contains("--sort"), "help: {help}");
    assert!(help.contains("<score|timestamp>"), "help: {help}");
    assert!(help.contains("values: score, timestamp"), "help: {help}");

    let output = generated_script_output(
        &script,
        &[
            "slackmcp-slack-search-public-and-private",
            "--query",
            "hello",
            "--sort",
            "date",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value for --sort: date (expected one of: score, timestamp)"),
        "stderr: {stderr}"
    );
}

#[test]
fn generated_cli_prints_raw_host_bridge_result() {
    let bridge = start_json_result_bridge("{\"results\":\"ok\"}");
    let tempdir = tempfile::tempdir().unwrap();
    let config = GeneratorConfig {
        cli_name: "alpha".to_string(),
        bridge_url: bridge.url.clone(),
        token: "token".to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "echo",
            Some("Echo.".to_string()),
            serde_json::json!({"type":"object","properties":{"message":{"type":"string"}}}),
        )],
        session_pid: std::process::id(),
        output_dir: tempdir.path().to_path_buf(),
        extra_cli_bridges: Vec::new(),
    };
    CliGenerator.generate(&config).unwrap();
    let script = tempdir.path().join("alpha");

    assert_eq!(
        run_generated_script(&script, &["echo", "--message", "hello"]),
        "{\"results\":\"ok\"}"
    );
}

/// The generated CLI client must route to the bridge owned by its own session --
/// the entry keyed under an ancestor PID -- even when another live bridge exists.
/// This is the multi-session disambiguation the process-tree walk provides; on
/// Windows it depends on the snapshot-based parent lookup climbing past `cmd.exe`.
///
/// The decoy bridge is keyed under a non-ancestor PID (`u32::MAX`) and inserted
/// first, so it is the fallback's pick. A walk that fails to reach the session PID
/// would fall back and return `wrong-session`. Both bridges are live, so the
/// fallback cannot skip the decoy and pass by accident.
#[test]
fn generated_cli_routes_to_ancestor_session_bridge_over_live_decoy() {
    let correct = start_json_result_bridge("correct-session");
    let decoy = start_json_result_bridge("wrong-session");
    let tempdir = tempfile::tempdir().unwrap();
    let config = GeneratorConfig {
        cli_name: "alpha".to_string(),
        bridge_url: correct.url.clone(),
        token: "token".to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "echo",
            Some("Echo.".to_string()),
            serde_json::json!({"type":"object","properties":{"message":{"type":"string"}}}),
        )],
        session_pid: std::process::id(),
        output_dir: tempdir.path().to_path_buf(),
        extra_cli_bridges: vec![mcp_compressor_core::client_gen::generator::CliBridgeEntry {
            session_pid: u32::MAX,
            bridge_url: decoy.url.clone(),
            token: "token".to_string(),
        }],
    };
    CliGenerator.generate(&config).unwrap();
    // The walk only matters where the client runs as a real child of this test
    // process: the Unix `sh` script (no extension) or, on Windows, the `.cmd`
    // launched through cmd.exe -- which is the path that exercises the ctypes walk.
    let script_name = if cfg!(windows) { "alpha.cmd" } else { "alpha" };
    let script = tempdir.path().join(script_name);

    assert_eq!(
        run_generated_script(&script, &["echo", "--message", "hello"]),
        "correct-session"
    );
}

#[test]
fn generated_python_module_returns_raw_host_bridge_result() {
    let bridge = start_json_result_bridge("python-ok");
    let tempdir = tempfile::tempdir().unwrap();
    let config = simple_generator_config("alpha", &bridge.url, tempdir.path());
    PythonGenerator.generate(&config).unwrap();

    let output = Command::new(common::python_command())
        .env("PYTHONPATH", tempdir.path())
        .args(["-c", "import alpha; print(alpha.echo('hello'))"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "python-ok");
}

#[test]
fn generated_typescript_module_returns_raw_host_bridge_result() {
    let bridge = start_json_result_bridge("typescript-ok");
    let tempdir = tempfile::tempdir().unwrap();
    let config = simple_generator_config("alpha", &bridge.url, tempdir.path());
    TypeScriptGenerator.generate(&config).unwrap();

    let code = format!(
        "import * as alpha from {module:?}; console.log(await alpha.echo('hello'));",
        module = tempdir.path().join("alpha.ts").display().to_string()
    );
    let output = Command::new("bun")
        .arg("--eval")
        .arg(&code)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "typescript-ok"
    );
}

struct TestBridge {
    url: String,
}

fn start_json_result_bridge(result: &str) -> TestBridge {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let result = result.to_string();
    thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let mut stream = stream.unwrap();
            let mut buffer = [0_u8; 4096];
            let read = std::io::Read::read(&mut stream, &mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]);
            let body = if request.starts_with("GET /health") {
                "ok".to_string()
            } else {
                serde_json::json!({ "result": result }).to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
        }
    });
    TestBridge {
        url: format!("http://{addr}"),
    }
}

fn simple_generator_config(
    cli_name: &str,
    bridge_url: &str,
    output_dir: &std::path::Path,
) -> GeneratorConfig {
    GeneratorConfig {
        cli_name: cli_name.to_string(),
        bridge_url: bridge_url.to_string(),
        token: "token".to_string(),
        tools: vec![mcp_compressor_core::compression::engine::Tool::new(
            "echo",
            Some("Echo.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "required": ["message"]
            }),
        )],
        session_pid: std::process::id(),
        output_dir: output_dir.to_path_buf(),
        extra_cli_bridges: Vec::new(),
    }
}

fn alpha_golden_tools() -> Vec<mcp_compressor_core::compression::engine::Tool> {
    vec![
        mcp_compressor_core::compression::engine::Tool::new(
            "echo",
            Some("Echo a message.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": { "message": { "type": "string", "description": "Message to echo" } },
                "required": ["message"]
            }),
        ),
        mcp_compressor_core::compression::engine::Tool::new(
            "add",
            Some("Add two integers.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "integer", "description": "Left operand" },
                    "b": { "type": "integer", "description": "Right operand" }
                },
                "required": ["a", "b"]
            }),
        ),
        mcp_compressor_core::compression::engine::Tool::new(
            "summarize_payload",
            Some("Summarize a structured payload.".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "items": { "type": "array", "items": { "type": "string" }, "description": "Items to summarize" },
                    "metadata": { "type": "object", "description": "Arbitrary metadata" },
                    "include_details": { "type": "boolean", "description": "Include detailed rows" }
                },
                "required": ["items"]
            }),
        ),
    ]
}

fn atlassian_like_golden_tools() -> Vec<mcp_compressor_core::compression::engine::Tool> {
    vec![mcp_compressor_core::compression::engine::Tool::new(
        "searchJiraIssuesUsingJql",
        Some("Search issues with JQL".to_string()),
        serde_json::json!({
            "type": "object",
            "properties": {
                "cloudId": { "type": "string", "description": "Cloud ID" },
                "jql": { "type": "string", "description": "JQL query" },
                "maxResults": { "type": "number", "description": "Max results" },
                "nextPageToken": { "type": "string", "description": "Page token" }
            },
            "required": ["cloudId", "jql"]
        }),
    )]
}

fn generate_cli_script(
    cli_name: &str,
    tools: Vec<mcp_compressor_core::compression::engine::Tool>,
) -> tempfile::TempDir {
    let tempdir = tempfile::tempdir().unwrap();
    let config = GeneratorConfig {
        cli_name: cli_name.to_string(),
        bridge_url: "http://127.0.0.1:1".to_string(),
        token: "token".to_string(),
        tools,
        session_pid: std::process::id(),
        output_dir: tempdir.path().to_path_buf(),
        extra_cli_bridges: Vec::new(),
    };
    CliGenerator.generate(&config).unwrap();
    tempdir
}

fn run_generated_script(script: &std::path::Path, args: &[&str]) -> String {
    let output = generated_script_output(script, args);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

#[test]
fn rust_generated_alpha_cli_matches_shared_golden_help() {
    let tempdir = generate_cli_script("alpha", alpha_golden_tools());
    let script = tempdir.path().join("alpha");
    assert_eq!(
        run_generated_script(&script, &["--help"]),
        golden("agent-facing/cli/alpha-help.txt")
    );
    assert_eq!(
        run_generated_script(&script, &["echo", "--help"]),
        golden("agent-facing/cli/alpha-echo-help.txt")
    );
}

#[test]
fn rust_generated_atlassian_like_cli_matches_shared_golden_help() {
    let tempdir = generate_cli_script("atlassian", atlassian_like_golden_tools());
    let script = tempdir.path().join("atlassian");
    assert_eq!(
        run_generated_script(&script, &["--help"]),
        golden("agent-facing/atlassian-like/atlassian-help.txt")
    );
    assert_eq!(
        run_generated_script(&script, &["search-jira-issues-using-jql", "--help"]),
        golden("agent-facing/atlassian-like/search-jira-issues-using-jql-help.txt")
    );
}
