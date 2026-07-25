# Pre-Initialize Custom JSON-RPC Request Issue

## Problem Description

Clients like Antigravity CLI that send custom JSON-RPC discovery requests (e.g., `server/discover`) before the `initialize` request cause mcp-compressor to reject the request and terminate the stdio connection.

## Root Cause

The underlying rmcp library (v1.7.0) only handles `ping` requests before `initialize`. Custom requests cause it to return an `ExpectedInitializeRequest` error and close the connection.

## Investigation Summary

1. **Identified the issue**: rmcp's `serve_server_with_ct_inner` function (lines 170-198 in `service/server.rs`) has a loop that waits for either `ping` or `initialize` requests. Any custom request causes it to return an error.

2. **Attempted fixes**:
   - Transport wrapper: Complex lifetime issues make this impractical
   - Custom initialization function: Would require copying private rmcp internals (`serve_inner`)

3. **Upgrade**: Upgraded rmcp from 1.6.0 to 1.7.0 for improved error handling

## Recommended Solution

This should be fixed upstream in the rmcp library. The fix would be to modify the initialization loop (lines 170-198 in rmcp's `src/service/server.rs`) to handle custom requests similar to how it handles ping - by responding with a "Method not found" error but continuing to wait for initialize.

## Workarounds for Users

### Option 1: Avoid Pre-Initialize Custom Requests
Configure clients to send `initialize` first, before any custom discovery requests.

### Option 2: Use Streamable HTTP Transport
The streamable HTTP transport may have different handling for pre-initialize messages.

## Files to Reference

- Reproduction test: `tests/test_pre_initialize_custom_request.py`
- rmcp source: `~/.cargo/registry/src/index.crates.io-*/rmcp-1.7.0/src/service/server.rs` (lines 170-198)
- Issue: #241
