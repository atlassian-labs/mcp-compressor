#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::compression::CompressionLevel;
    use serde_json::{json, Value};

    fn sample_tool() -> FfiTool {
        FfiTool {
            name: "echo".to_string(),
            description: Some("Echo a message.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            }),
        }
    }

    #[test]
    fn ffi_lists_and_clears_oauth_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());

        let store_dir = oauth_store_path().join("example-store");
        std::fs::create_dir_all(&store_dir).unwrap();
        remember_oauth_backend("https://example.test/mcp", "example", store_dir.clone()).unwrap();

        let entries = list_oauth_credentials().unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.backend_name == "example")
            .expect("remembered entry");
        assert_eq!(entry.backend_uri, "https://example.test/mcp");
        assert_eq!(entry.store_dir, store_dir);

        let cleared = clear_oauth_credentials(Some("example")).unwrap();
        assert!(cleared.iter().any(|path| path.ends_with("example-store")));
        assert!(!list_oauth_credentials()
            .unwrap()
            .iter()
            .any(|entry| entry.backend_name == "example"));

        if let Some(value) = previous {
            std::env::set_var("XDG_CONFIG_HOME", value);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn ffi_compresses_tool_listing() {
        let listing = compress_tool_listing(CompressionLevel::High, vec![sample_tool()]);
        assert_eq!(listing, "<tool>echo(message)</tool>");
    }

    #[test]
    fn ffi_formats_schema_response() {
        let schema = format_tool_schema_response(sample_tool());
        assert!(schema.contains("Echo a message."));
        assert!(schema.contains("message"));
    }

    #[test]
    fn ffi_parses_tool_argv() {
        let parsed = parse_tool_argv(
            sample_tool(),
            vec!["--message".to_string(), "hello".to_string()],
        )
        .unwrap();
        assert_eq!(parsed, json!({ "message": "hello" }));
    }

    fn generator_config(output_dir: &std::path::Path) -> FfiGeneratorConfig {
        FfiGeneratorConfig {
            cli_name: "ffi-server".to_string(),
            bridge_url: "http://127.0.0.1:12345".to_string(),
            token: "token".repeat(16),
            tools: vec![sample_tool()],
            session_pid: 42,
            output_dir: output_dir.to_path_buf(),
        }
    }

    #[test]
    fn ffi_generates_cli_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths =
            generate_client_artifacts(FfiClientArtifactKind::Cli, generator_config(dir.path()))
                .unwrap();
        assert_eq!(paths.len(), 1);
        let content = std::fs::read_to_string(&paths[0]).unwrap();
        assert!(content.contains("ffi-server - the ffi-server toolset"));
    }

    #[test]
    fn ffi_generates_python_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths =
            generate_client_artifacts(FfiClientArtifactKind::Python, generator_config(dir.path()))
                .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(
            paths[0].extension().and_then(|ext| ext.to_str()),
            Some("py")
        );
    }

    #[test]
    fn ffi_generates_typescript_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths = generate_client_artifacts(
            FfiClientArtifactKind::TypeScript,
            generator_config(dir.path()),
        )
        .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ts")));
        assert!(paths.iter().any(|path| path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".d.ts"))));
    }

    async fn invoke_session(
        info: &FfiCompressedSessionInfo,
        tool: &str,
        tool_name: &str,
        tool_input: Value,
    ) -> String {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/exec", info.bridge_url))
            .bearer_auth(&info.token)
            .json(&serde_json::json!({
                "tool": tool,
                "input": {
                    "tool_name": tool_name,
                    "tool_input": tool_input
                }
            }))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.text().await.unwrap()
    }

    #[tokio::test]
    async fn ffi_starts_compressed_session_and_proxy() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
            },
            vec![FfiBackendConfig {
                name: "alpha".to_string(),
                command_or_url: std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
                args: vec![fixture.to_string_lossy().into_owned()],
            }],
        )
        .await
        .unwrap();
        let info = session.info();
        assert!(info.bridge_url.starts_with("http://127.0.0.1:"));
        assert!(!info.token.is_empty());
        let invoke_tool_name = info
            .frontend_tools
            .iter()
            .find(|tool| tool.name.ends_with("invoke_tool"))
            .map(|tool| tool.name.clone())
            .expect("invoke wrapper tool");

        assert_eq!(
            invoke_session(
                &info,
                &invoke_tool_name,
                "echo",
                serde_json::json!({"message": "ffi"})
            )
            .await,
            "alpha:ffi"
        );
    }

    #[tokio::test]
    async fn ffi_starts_compressed_session_from_mcp_config_and_routes_multiple_servers() {
        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let config_json = serde_json::json!({
            "mcpServers": {
                "alpha": {
                    "command": python,
                    "args": [fixture_dir.join("alpha_server.py").to_string_lossy()]
                },
                "beta": {
                    "command": std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
                    "args": [fixture_dir.join("beta_server.py").to_string_lossy()]
                }
            }
        })
        .to_string();
        let session = start_compressed_session_from_mcp_config(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: None,
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
            },
            &config_json,
        )
        .await
        .unwrap();
        let info = session.info();
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "alpha_invoke_tool"));
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "beta_invoke_tool"));
        assert_eq!(
            invoke_session(
                &info,
                "alpha_invoke_tool",
                "add",
                serde_json::json!({"a": 2, "b": 5})
            )
            .await,
            "7"
        );
        assert_eq!(
            invoke_session(
                &info,
                "beta_invoke_tool",
                "multiply",
                serde_json::json!({"a": 3, "b": 4})
            )
            .await,
            "12"
        );
    }

    #[tokio::test]
    async fn ffi_session_can_request_cli_transform_mode() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: Some("cli".to_string()),
            },
            vec![FfiBackendConfig {
                name: "alpha".to_string(),
                command_or_url: std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
                args: vec![fixture.to_string_lossy().into_owned()],
            }],
        )
        .await
        .unwrap();
        let info = session.info();
        assert_eq!(info.frontend_tools.len(), 1);
        assert!(info.frontend_tools[0].name.ends_with("alpha_help"));
    }

    #[tokio::test]
    async fn ffi_session_can_request_just_bash_transform_mode() {
        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: None,
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: Some("just-bash".to_string()),
            },
            vec![
                FfiBackendConfig {
                    name: "alpha".to_string(),
                    command_or_url: python.clone(),
                    args: vec![fixture_dir
                        .join("alpha_server.py")
                        .to_string_lossy()
                        .into_owned()],
                },
                FfiBackendConfig {
                    name: "beta".to_string(),
                    command_or_url: python,
                    args: vec![fixture_dir
                        .join("beta_server.py")
                        .to_string_lossy()
                        .into_owned()],
                },
            ],
        )
        .await
        .unwrap();
        let info = session.info();
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "bash_tool"));
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "alpha_help"));
        assert_eq!(info.just_bash_providers.len(), 2);
        assert!(info
            .just_bash_providers
            .iter()
            .any(|provider| provider.provider_name == "alpha"));
    }

    #[test]
    fn ffi_parses_mcp_config() {
        let parsed = parse_mcp_config(
            r#"{
                "mcpServers": {
                    "my server": {
                        "command": "python3",
                        "args": ["server.py"],
                        "env": { "A": "B" }
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "my server");
        assert_eq!(parsed[0].cli_prefix, "my-server");
        assert_eq!(parsed[0].env, vec![("A".to_string(), "B".to_string())]);
    }
}
