//! Generic HTTP tool proxy server.
//!
//! Binds to `127.0.0.1:<port>` (random free port by default), generates a
//! `SessionToken` at startup, and routes `/health` and `/exec` requests.
//!
//! This module intentionally exposes the API that integration tests and client
//! generators should target. Method bodies remain `todo!()` until the Phase 1
//! proxy implementation is built.

use crate::proxy::auth::SessionToken;
use crate::server::compressed::CompressedServer;
use crate::Error;

#[derive(Debug)]
pub struct ToolProxyServer;

#[derive(Debug, Clone)]
pub struct RunningToolProxy {
    bridge_url: String,
    token: SessionToken,
}

impl ToolProxyServer {
    pub async fn start(_server: CompressedServer) -> Result<RunningToolProxy, Error> {
        todo!()
    }
}

impl RunningToolProxy {
    pub fn bridge_url(&self) -> &str {
        &self.bridge_url
    }

    pub fn token(&self) -> &SessionToken {
        &self.token
    }

    pub fn token_value(&self) -> &str {
        self.token.value()
    }

    pub fn health_url(&self) -> String {
        format!("{}/health", self.bridge_url)
    }

    pub fn exec_url(&self) -> String {
        format!("{}/exec", self.bridge_url)
    }
}
