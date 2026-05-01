//! Frontend MCP server registration for compressed wrapper tools.

use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    Annotated, CallToolRequestParams, CallToolResult, Content, ErrorCode, GetPromptRequestParams,
    GetPromptResult, InitializeResult, ListPromptsResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, Prompt,
    RawResource, ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
    ServerCapabilities, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer};
use serde_json::{Map, Value};

use crate::server::CompressedServer;

/// Dynamic frontend MCP service that exposes compressed wrapper tools and
/// delegates their calls to [`CompressedServer`].
#[derive(Debug)]
pub struct FrontendServer {
    compressed: Arc<CompressedServer>,
}

impl FrontendServer {
    pub fn new(compressed: CompressedServer) -> Self {
        Self {
            compressed: Arc::new(compressed),
        }
    }

    pub fn from_arc(compressed: Arc<CompressedServer>) -> Self {
        Self { compressed }
    }
}

impl ServerHandler for FrontendServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_instructions("Compressed MCP frontend server")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .compressed
            .list_frontend_tools()
            .await
            .map_err(mcp_error)?
            .into_iter()
            .map(convert_tool)
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let wrapper_name = request.name.to_string();
        let arguments = request.arguments.unwrap_or_default();
        let output = if wrapper_name.ends_with("get_tool_schema") {
            let tool_name = required_string(&arguments, "tool_name")?;
            self.compressed
                .get_tool_schema(&wrapper_name, &tool_name)
                .await
        } else if wrapper_name.ends_with("invoke_tool") {
            let tool_name = required_string(&arguments, "tool_name")?;
            let tool_input = arguments
                .get("tool_input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            self.compressed
                .invoke_tool(&wrapper_name, &tool_name, tool_input)
                .await
        } else if wrapper_name.ends_with("list_tools") {
            self.compressed.list_backend_tools(&wrapper_name).await
        } else {
            Err(crate::Error::ToolNotFound(wrapper_name))
        }
        .map_err(mcp_error)?;

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resources = self
            .compressed
            .list_resources()
            .await
            .map_err(mcp_error)?
            .into_iter()
            .map(convert_resource)
            .collect();
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let text = self
            .compressed
            .read_resource(&request.uri)
            .await
            .map_err(mcp_error)?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, request.uri),
        ]))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let prompts = self
            .compressed
            .list_prompts()
            .await
            .map_err(mcp_error)?
            .into_iter()
            .map(|name| Prompt::new(name, Option::<String>::None, None))
            .collect();
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        self.compressed
            .get_prompt(&request.name, request.arguments)
            .await
            .map_err(mcp_error)
    }

    fn get_tool(&self, _name: &str) -> Option<Tool> {
        None
    }
}

fn convert_tool(tool: crate::compression::engine::Tool) -> Tool {
    let input_schema = match tool.input_schema {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    Tool::new(
        tool.name,
        tool.description.unwrap_or_default(),
        Arc::new(input_schema),
    )
}

fn convert_resource(uri: String) -> Resource {
    Annotated::new(RawResource {
        name: uri.clone(),
        uri,
        title: None,
        description: None,
        mime_type: None,
        icons: None,
        size: None,
        meta: None,
    }, None)
}

fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String, McpError> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| McpError::new(ErrorCode::INVALID_PARAMS, format!("missing {name}"), None))
}

fn mcp_error(error: crate::Error) -> McpError {
    McpError::new(ErrorCode::INTERNAL_ERROR, error.to_string(), None)
}
