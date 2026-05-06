use serde_json::Value;

use crate::cli::parser::parse_argv;
use crate::compression::engine::{CompressionEngine, Tool};
use crate::compression::CompressionLevel;
use crate::config::topology::MCPConfig;
use crate::Error;

use super::dto::{FfiMcpServer, FfiTool};

pub fn compress_tool_listing(level: CompressionLevel, tools: Vec<FfiTool>) -> String {
    let tools = tools.into_iter().map(Into::into).collect::<Vec<Tool>>();
    CompressionEngine::new(level).format_listing(&tools)
}

pub fn format_tool_schema_response(tool: FfiTool) -> String {
    CompressionEngine::format_schema_response(&tool.into())
}

pub fn parse_tool_argv(tool: FfiTool, argv: Vec<String>) -> Result<Value, Error> {
    parse_argv(&argv, &tool.into())
}

pub fn parse_mcp_config(config_json: &str) -> Result<Vec<FfiMcpServer>, Error> {
    let config = MCPConfig::from_json(config_json)?;
    Ok(config
        .server_names()
        .into_iter()
        .filter_map(|name| {
            let server = config.server(&name)?;
            Some(FfiMcpServer {
                cli_prefix: config.cli_prefix(&name),
                name,
                command: server.command.clone(),
                args: server.args.clone(),
                env: server
                    .env
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            })
        })
        .collect())
}
