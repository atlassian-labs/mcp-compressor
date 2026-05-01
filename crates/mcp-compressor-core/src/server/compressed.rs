//! `CompressedServer` — the top-level object that owns the backend client,
//! tool cache, and compression engine, and exposes them via a frontend MCP server.
//!
//! This file intentionally exposes the runtime API that integration tests and
//! language bindings should target. Method bodies remain `todo!()` until the
//! Phase 1 runtime is implemented.

use std::collections::HashMap;
use std::net::SocketAddr;

use serde_json::Value;

use crate::compression::engine::Tool;
use crate::compression::CompressionLevel;
use crate::Error;

/// Configuration for one upstream MCP server process reached over stdio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

impl BackendServerConfig {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: HashMap::new(),
        }
    }

    pub fn with_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.env = env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        self
    }
}

/// Compression/runtime options shared by single-server and multi-server modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedServerConfig {
    pub level: CompressionLevel,
    pub server_name: Option<String>,
    pub include_tools: Vec<String>,
    pub exclude_tools: Vec<String>,
    pub toonify: bool,
}

impl Default for CompressedServerConfig {
    fn default() -> Self {
        Self {
            level: CompressionLevel::default(),
            server_name: None,
            include_tools: Vec::new(),
            exclude_tools: Vec::new(),
            toonify: false,
        }
    }
}

/// Handle for a frontend MCP server running over streamable HTTP.
#[derive(Debug, Clone)]
pub struct RunningCompressedServer {
    addr: SocketAddr,
}

impl RunningCompressedServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Connected compressor runtime.
#[derive(Debug)]
pub struct CompressedServer;

impl CompressedServer {
    /// Connect to one upstream stdio MCP server.
    pub async fn connect_stdio(
        _config: CompressedServerConfig,
        _backend: BackendServerConfig,
    ) -> Result<Self, Error> {
        todo!()
    }

    /// Connect to multiple upstream stdio MCP servers.
    pub async fn connect_multi_stdio(
        _config: CompressedServerConfig,
        _backends: Vec<BackendServerConfig>,
    ) -> Result<Self, Error> {
        todo!()
    }

    /// Start the frontend MCP server over streamable HTTP.
    pub async fn run_http(&self, _addr: SocketAddr) -> Result<RunningCompressedServer, Error> {
        todo!()
    }

    /// Return the frontend MCP tools exposed to callers.
    pub async fn list_frontend_tools(&self) -> Result<Vec<Tool>, Error> {
        todo!()
    }

    /// Return the full backend schema for a tool via the compressed wrapper API.
    pub async fn get_tool_schema(
        &self,
        _wrapper_tool_name: &str,
        _backend_tool_name: &str,
    ) -> Result<String, Error> {
        todo!()
    }

    /// List backend tools via the max-compression `list_tools` wrapper.
    pub async fn list_backend_tools(&self, _wrapper_tool_name: &str) -> Result<String, Error> {
        todo!()
    }

    /// Invoke a backend tool via the compressed wrapper API.
    pub async fn invoke_tool(
        &self,
        _wrapper_tool_name: &str,
        _backend_tool_name: &str,
        _tool_input: Value,
    ) -> Result<String, Error> {
        todo!()
    }

    /// List frontend resources, including pass-through backend resources and
    /// compressor-owned uncompressed-tool-list resources.
    pub async fn list_resources(&self) -> Result<Vec<String>, Error> {
        todo!()
    }

    /// Read a frontend resource by URI.
    pub async fn read_resource(&self, _uri: &str) -> Result<String, Error> {
        todo!()
    }

    /// List frontend prompts passed through from backend servers.
    pub async fn list_prompts(&self) -> Result<Vec<String>, Error> {
        todo!()
    }
}
