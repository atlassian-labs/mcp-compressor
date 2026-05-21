use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client_gen::cli::CliGenerator;
use crate::client_gen::generator::{
    artifact_map, write_artifacts, ClientGenerator, GeneratorConfig,
};
use crate::client_gen::python::PythonGenerator;
use crate::client_gen::typescript::TypeScriptGenerator;
use crate::ffi::client_gen::FfiClientArtifactKind;
use crate::ffi::dto::{FfiGeneratorConfig, FfiTool};
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FfiHostTransformKind {
    Cli,
    JustBash,
    Python,
    #[serde(rename = "typescript")]
    TypeScript,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FfiHostTransformConfig {
    pub kind: FfiHostTransformKind,
    pub server_name: String,
    pub tools: Vec<FfiTool>,
    pub output_dir: Option<PathBuf>,
    pub command_name: Option<String>,
    pub bridge_url: Option<String>,
    pub token: Option<String>,
    pub session_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FfiHostTransformPlan {
    pub help_tool_name: String,
    pub help_description: String,
    pub output_dir: Option<PathBuf>,
    pub files: BTreeMap<String, String>,
    pub paths: Vec<PathBuf>,
    pub environment: BTreeMap<String, String>,
    pub just_bash: Option<FfiHostJustBashPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FfiHostJustBashPlan {
    pub provider_name: String,
    pub command_name: String,
    pub help_tool_name: String,
    pub commands: Vec<FfiHostJustBashCommandPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FfiHostJustBashCommandPlan {
    pub command_name: String,
    pub backend_tool_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

pub fn build_host_transform_plan(
    config: FfiHostTransformConfig,
) -> Result<FfiHostTransformPlan, Error> {
    let server_name = normalize_server_name(Some(config.server_name));
    let help_tool_name = format!("{server_name}_help");
    match config.kind {
        FfiHostTransformKind::JustBash => {
            let help_description =
                shell_tool_help_description(&server_name, &server_name, &config.tools);
            let commands = config
                .tools
                .iter()
                .map(|tool| FfiHostJustBashCommandPlan {
                    command_name: cli_subcommand_name(&tool.name),
                    backend_tool_name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.input_schema.clone(),
                })
                .collect();
            Ok(FfiHostTransformPlan {
                help_tool_name: help_tool_name.clone(),
                help_description,
                output_dir: None,
                files: BTreeMap::new(),
                paths: Vec::new(),
                environment: BTreeMap::new(),
                just_bash: Some(FfiHostJustBashPlan {
                    provider_name: server_name.clone(),
                    command_name: server_name,
                    help_tool_name,
                    commands,
                }),
            })
        }
        FfiHostTransformKind::Cli => {
            let output_dir = config.output_dir.unwrap_or_else(default_cli_output_dir);
            let command_name = config.command_name.unwrap_or_else(|| server_name.clone());
            let generator_config = generator_config(
                &server_name,
                &output_dir,
                config.tools.clone(),
                config.bridge_url,
                config.token,
                config.session_pid,
            );
            let artifacts = CliGenerator.render(&generator_config)?;
            let files = artifact_map(&artifacts);
            let paths = write_artifacts(&artifacts, &output_dir)?;
            let help_description =
                shell_tool_help_description(&command_name, &server_name, &config.tools);
            Ok(FfiHostTransformPlan {
                help_tool_name,
                help_description,
                output_dir: Some(output_dir.clone()),
                files,
                paths,
                environment: path_environment(&output_dir),
                just_bash: None,
            })
        }
        FfiHostTransformKind::Python | FfiHostTransformKind::TypeScript => {
            let output_dir = config.output_dir.unwrap_or_else(|| PathBuf::from("./dist"));
            let artifact_kind = match config.kind {
                FfiHostTransformKind::Python => FfiClientArtifactKind::Python,
                FfiHostTransformKind::TypeScript => FfiClientArtifactKind::TypeScript,
                _ => unreachable!(),
            };
            let generator_config = generator_config(
                &server_name,
                &output_dir,
                config.tools.clone(),
                config.bridge_url,
                config.token,
                config.session_pid,
            );
            let artifacts = match artifact_kind {
                FfiClientArtifactKind::Python => PythonGenerator.render(&generator_config)?,
                FfiClientArtifactKind::TypeScript => {
                    TypeScriptGenerator.render(&generator_config)?
                }
                FfiClientArtifactKind::Cli => unreachable!(),
            };
            let files = artifact_map(&artifacts);
            let paths = write_artifacts(&artifacts, &output_dir)?;
            let help_description =
                code_help_description(artifact_kind, &server_name, &output_dir, &config.tools);
            let environment = match config.kind {
                FfiHostTransformKind::Python => {
                    let mut env = BTreeMap::new();
                    env.insert(
                        "PYTHONPATH".to_string(),
                        output_dir.to_string_lossy().into_owned(),
                    );
                    env
                }
                _ => BTreeMap::new(),
            };
            Ok(FfiHostTransformPlan {
                help_tool_name,
                help_description,
                output_dir: Some(output_dir),
                files,
                paths,
                environment,
                just_bash: None,
            })
        }
    }
}

pub fn normalize_host_tool_result(value: Value, toonify: bool) -> String {
    let output = value_to_string(&value);
    if toonify {
        crate::ffi::client_gen::maybe_toonify_output(&output)
    } else {
        output
    }
}

fn generator_config(
    server_name: &str,
    output_dir: &std::path::Path,
    tools: Vec<FfiTool>,
    bridge_url: Option<String>,
    token: Option<String>,
    session_pid: Option<u32>,
) -> GeneratorConfig {
    FfiGeneratorConfig {
        cli_name: server_name.to_string(),
        bridge_url: bridge_url.unwrap_or_else(|| "http://127.0.0.1:0".to_string()),
        token: token.unwrap_or_default(),
        tools,
        session_pid: session_pid.unwrap_or_else(std::process::id),
        output_dir: output_dir.to_path_buf(),
    }
    .into()
}

fn shell_tool_help_description(command: &str, cli_name: &str, tools: &[FfiTool]) -> String {
    let mut lines = vec![
        format!(
            "Functionality associated with the {cli_name} toolset is provided via the `{command}` CLI. Do not call this tool - use the CLI instead."
        ),
        format!("{cli_name} - the {cli_name} toolset"),
        String::new(),
        "When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.".to_string(),
        String::new(),
        "USAGE:".to_string(),
        format!("  {command} <subcommand> [options]"),
        String::new(),
        "SUBCOMMANDS:".to_string(),
    ];
    lines.extend(format_subcommands(tools, cli_subcommand_name));
    lines.extend([
        String::new(),
        format!("Run '{command} --help' in the shell for usage."),
        format!("Run '{command} <subcommand> --help' for per-command help."),
        format!("Run '{command} <subcommand> [options]' to invoke a tool."),
    ]);
    lines.join("\n")
}

fn code_help_description(
    kind: FfiClientArtifactKind,
    server_name: &str,
    output_dir: &std::path::Path,
    tools: &[FfiTool],
) -> String {
    let (language, language_lower, module_name) = match kind {
        FfiClientArtifactKind::Python => ("Python", "python", format!("{server_name}.py")),
        FfiClientArtifactKind::TypeScript => {
            ("TypeScript", "typescript", format!("{server_name}.ts"))
        }
        FfiClientArtifactKind::Cli => unreachable!(),
    };
    let source_path = output_dir.join(&module_name).to_string_lossy().into_owned();
    let mut lines = vec![
        format!(
            "Functionality associated with the {server_name} toolset is provided via a {language} module. Do not call this tool - import and use the {language_lower} functionality instead."
        ),
        format!("{server_name} - the {server_name} toolset"),
        String::new(),
        format!("{language} source code is available in {source_path}"),
        String::new(),
        "Available functions:".to_string(),
    ];
    let signatures = tools
        .iter()
        .map(|tool| code_function_signature(kind, tool))
        .collect::<Vec<_>>();
    let max_signature_len = signatures.iter().map(String::len).max().unwrap_or(0);
    for (tool, signature) in tools.iter().zip(signatures) {
        let description = compact_description(tool.description.as_deref());
        lines.push(
            format!(
                "  {signature:<width$}{description}",
                width = max_signature_len + 2
            )
            .trim_end()
            .to_string(),
        );
    }
    lines.push(String::new());
    match kind {
        FfiClientArtifactKind::Python => lines.extend([
            "For details on a specific function, run:".to_string(),
            "```python".to_string(),
            format!("from {server_name} import <function>"),
            "print(help(<function>))".to_string(),
            "```".to_string(),
        ]),
        FfiClientArtifactKind::TypeScript => lines.extend([
            "For details on a specific function, inspect the TypeScript declarations or editor hover documentation.".to_string(),
            format!(
                "Primary declarations: {}",
                output_dir.join(format!("{server_name}.d.ts")).to_string_lossy()
            ),
        ]),
        FfiClientArtifactKind::Cli => unreachable!(),
    }
    lines.join("\n")
}

fn code_function_signature(kind: FfiClientArtifactKind, tool: &FfiTool) -> String {
    let name = match kind {
        FfiClientArtifactKind::Python => to_snake_case(&tool.name),
        FfiClientArtifactKind::TypeScript => snake_to_camel(&tool.name),
        FfiClientArtifactKind::Cli => unreachable!(),
    };
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object);
    let Some(properties) = properties else {
        return format!("{name}()");
    };
    let required = tool
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut args = Vec::new();
    for key in properties.keys() {
        let function_arg = match kind {
            FfiClientArtifactKind::Python => to_snake_case(key),
            FfiClientArtifactKind::TypeScript => key.to_string(),
            FfiClientArtifactKind::Cli => unreachable!(),
        };
        if required.contains(key.as_str()) {
            args.push(function_arg);
        } else if kind == FfiClientArtifactKind::Python {
            args.push(format!("{function_arg}=None"));
        } else {
            args.push(format!("{function_arg}?"));
        }
    }
    format!("{name}({})", args.join(", "))
}

fn format_subcommands(tools: &[FfiTool], name_for_tool: fn(&str) -> String) -> Vec<String> {
    let names = tools
        .iter()
        .map(|tool| name_for_tool(&tool.name))
        .collect::<Vec<_>>();
    let max_name_len = names.iter().map(String::len).max().unwrap_or(0);
    tools
        .iter()
        .zip(names)
        .map(|(tool, name)| {
            let description = compact_description(tool.description.as_deref());
            format!("  {name:<width$}{description}", width = max_name_len + 2)
                .trim_end()
                .to_string()
        })
        .collect()
}

fn cli_subcommand_name(tool_name: &str) -> String {
    crate::cli::mapping::tool_name_to_subcommand(tool_name)
}

fn compact_description(description: Option<&str>) -> String {
    description
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}


fn to_snake_case(name: &str) -> String {
    let mut output = String::new();
    let mut previous_was_separator = false;
    for (index, ch) in name.chars().enumerate() {
        if ch == '-' || ch == ' ' {
            if !output.is_empty() && !previous_was_separator {
                output.push('_');
            }
            previous_was_separator = true;
        } else if ch.is_ascii_uppercase() {
            if index > 0 && !previous_was_separator {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else {
            output.push(ch);
            previous_was_separator = ch == '_';
        }
    }
    output
}

fn snake_to_camel(name: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = false;
    for ch in name.chars() {
        if ch == '_' || ch == '-' {
            uppercase_next = true;
        } else if uppercase_next {
            out.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn normalize_server_name(name: Option<String>) -> String {
    name.unwrap_or_else(|| "mcp".to_string())
}

fn default_cli_output_dir() -> PathBuf {
    if let Ok(value) = std::env::var("MCP_COMPRESSOR_CLI_OUTPUT_DIR") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".local/bin");
        }
    }
    PathBuf::from("./dist")
}

fn path_environment(output_dir: &std::path::Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".to_string(),
        format!("{}:$PATH", output_dir.to_string_lossy()),
    );
    env
}

fn value_to_string(value: &Value) -> String {
    if let Some(value) = value.as_str() {
        return value.to_string();
    }
    if let Some(map) = value.as_object() {
        if map.len() == 1 && map.contains_key("result") {
            return value_to_string(&map["result"]);
        }
        if let Some(text) = mcp_text_content_to_string(map.get("content")) {
            return text;
        }
    }
    value.to_string()
}

fn mcp_text_content_to_string(content: Option<&Value>) -> Option<String> {
    let content = content?.as_array()?;
    let parts = content
        .iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            if object.get("type")?.as_str()? == "text" {
                object.get("text")?.as_str().map(ToOwned::to_owned)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, description: &str) -> FfiTool {
        FfiTool {
            name: name.to_string(),
            description: Some(description.to_string()),
            input_schema: json!({"type":"object","properties":{}}),
        }
    }

    #[test]
    fn cli_plan_contains_subcommand_help() {
        let plan = build_host_transform_plan(FfiHostTransformConfig {
            kind: FfiHostTransformKind::Cli,
            server_name: "alpha".to_string(),
            tools: vec![tool("echo_message", "Echo a message.")],
            output_dir: Some(PathBuf::from("./target/tmp-host-plan-cli")),
            command_name: Some("alpha".to_string()),
            bridge_url: Some("http://127.0.0.1:1".to_string()),
            token: Some("token".to_string()),
            session_pid: Some(1),
        })
        .unwrap();
        assert_eq!(plan.help_tool_name, "alpha_help");
        assert!(plan.help_description.contains(
            "Functionality associated with the alpha toolset is provided via the `alpha` CLI."
        ));
        assert!(plan
            .help_description
            .contains("  echo-message  Echo a message."));
    }

    #[test]
    fn cli_plan_uses_kebab_case_for_camel_case_tool_names() {
        let plan = build_host_transform_plan(FfiHostTransformConfig {
            kind: FfiHostTransformKind::Cli,
            server_name: "atlassian".to_string(),
            tools: vec![
                tool("atlassianUserInfo", "Get current user info."),
                tool(
                    "getAccessibleAtlassianResources",
                    "Get accessible resources.",
                ),
            ],
            output_dir: Some(PathBuf::from("./target/tmp-host-plan-camel-cli")),
            command_name: Some("atlassian".to_string()),
            bridge_url: Some("http://127.0.0.1:1".to_string()),
            token: Some("token".to_string()),
            session_pid: Some(1),
        })
        .unwrap();
        assert!(plan.help_description.contains("  atlassian-user-info"));
        assert!(plan
            .help_description
            .contains("  get-accessible-atlassian-resources"));
        assert!(!plan.help_description.contains("atlassianUserInfo"));
        assert!(!plan
            .help_description
            .contains("getAccessibleAtlassianResources"));
    }

    #[test]
    fn normalizes_mcp_text_content_results() {
        let value = json!({"content":[{"type":"text","text":"{\"ok\":true}"}]});
        assert_eq!(normalize_host_tool_result(value, false), "{\"ok\":true}");
    }
}
