//! `CompressionEngine` — pure, stateless formatter for tool listings and schemas.
//!
//! All methods are pure functions: no I/O, no side-effects, no async.
//! This makes them trivially testable in isolation.
//!
//! # Format rules (mirrors Python `_format_tool_description`)
//!
//! | Level  | Output shape |
//! |--------|--------------|
//! | Max    | `<tool>name</tool>` |
//! | High   | `<tool>name(arg1, arg2)</tool>` |
//! | Medium | `<tool>name(arg1, arg2): First sentence of description</tool>` |
//! | Low    | `<tool>name(arg1, arg2): Full description</tool>` |
//!
//! `format_listing` at `Max` always returns an empty string; the frontend server
//! instead exposes a dedicated `list_tools` MCP tool for that level.

use crate::compression::CompressionLevel;

/// A single MCP tool as seen by the compression engine.
///
/// Mirrors `mcp.types.Tool` and `fastmcp.tools.Tool`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tool {
    /// Canonical tool name (e.g. `"get_confluence_page"`).
    pub name: String,
    /// Human-readable description.  May be absent for tools without docs.
    pub description: Option<String>,
    /// JSON Schema object describing the accepted input (the `properties` key
    /// holds named parameters; `required` lists mandatory ones).
    pub input_schema: serde_json::Value,
}

impl Tool {
    /// Convenience constructor used in tests.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<Option<String>>,
        input_schema: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }

    /// Return the ordered list of parameter names from `input_schema.properties`.
    pub fn param_names(&self) -> Vec<String> {
        self.input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// Stateless compression engine.
///
/// Instantiated with a [`CompressionLevel`]; all formatting calls borrow the
/// tool slice from the caller rather than owning it.
#[derive(Debug, Clone)]
pub struct CompressionEngine {
    level: CompressionLevel,
}

impl CompressionEngine {
    pub fn new(level: CompressionLevel) -> Self {
        Self { level }
    }

    /// Format the listing of *all* tools at the engine's compression level.
    ///
    /// Returns an empty string when the level is `Max` — callers should expose
    /// a `list_tools` MCP tool instead.
    /// Otherwise joins individual [`format_tool`] results with `"\n"`.
    pub fn format_listing(&self, tools: &[Tool]) -> String {
        if self.level == CompressionLevel::Max {
            return String::new();
        }

        tools
            .iter()
            .map(|tool| self.format_tool(tool))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format a *single* tool at the engine's compression level.
    ///
    /// See module-level doc for the format rules.
    pub fn format_tool(&self, tool: &Tool) -> String {
        format_tool_at_level(tool, &self.level)
    }

    /// Look up a tool by name in the provided slice.
    ///
    /// Returns `None` when the name is not found.
    pub fn get_schema<'a>(&self, tools: &'a [Tool], name: &str) -> Option<&'a Tool> {
        tools.iter().find(|tool| tool.name == name)
    }

    /// Build the full schema response string for a tool.
    ///
    /// Always uses `Low` verbosity regardless of the engine's configured level —
    /// schema lookup is meant to give complete information.
    ///
    /// Format:
    /// ```text
    /// <tool>name(arg1, arg2): Full description</tool>
    ///
    /// {
    ///   "type": "object",
    ///   "properties": { ... },
    ///   ...
    /// }
    /// ```
    pub fn format_schema_response(tool: &Tool) -> String {
        let tool_description = format_tool_at_level(tool, &CompressionLevel::Low);
        let schema = serde_json::to_string_pretty(&tool.input_schema)
            .unwrap_or_else(|_| tool.input_schema.to_string());
        format!("{tool_description}\n\n{schema}")
    }
}

fn format_tool_at_level(tool: &Tool, level: &CompressionLevel) -> String {
    match level {
        CompressionLevel::Max => format!("<tool>{}</tool>", tool.name),
        CompressionLevel::High => format!("<tool>{}({})</tool>", tool.name, format_args(tool)),
        CompressionLevel::Medium => format_with_description(tool, first_sentence_description(tool)),
        CompressionLevel::Low => format_with_description(tool, tool.description.as_deref()),
    }
}

fn format_with_description(tool: &Tool, description: Option<&str>) -> String {
    let signature = format!("{}({})", tool.name, format_args(tool));
    match description.map(str::trim).filter(|description| !description.is_empty()) {
        Some(description) => format!("<tool>{signature}: {description}</tool>"),
        None => format!("<tool>{signature}</tool>"),
    }
}

fn format_args(tool: &Tool) -> String {
    tool.param_names().join(", ")
}

fn first_sentence_description(tool: &Tool) -> Option<&str> {
    let description = tool.description.as_deref()?;
    let first_line = description.lines().next().unwrap_or_default();
    Some(first_line.split('.').next().unwrap_or_default().trim())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// A tool with two string parameters and a multi-sentence description.
    fn fetch_tool() -> Tool {
        Tool::new(
            "fetch",
            Some("Fetch a URL. Returns the raw content.".into()),
            json!({
                "type": "object",
                "properties": {
                    "url":     { "type": "string", "description": "Target URL" },
                    "timeout": { "type": "number", "description": "Timeout in seconds" }
                },
                "required": ["url"]
            }),
        )
    }

    /// A tool with a multi-line description.
    fn multiline_tool() -> Tool {
        Tool::new(
            "multiline",
            Some("First line description.\nSecond line continuation.".into()),
            json!({ "type": "object", "properties": { "x": { "type": "string" } } }),
        )
    }

    /// A tool with no description.
    fn no_desc_tool() -> Tool {
        Tool::new(
            "ping",
            None::<String>,
            json!({ "type": "object", "properties": { "host": { "type": "string" } } }),
        )
    }

    /// A tool with no parameters.
    fn no_args_tool() -> Tool {
        Tool::new(
            "health",
            Some("Check server health.".into()),
            json!({ "type": "object", "properties": {} }),
        )
    }

    // ------------------------------------------------------------------
    // format_tool — Max
    // ------------------------------------------------------------------

    /// At Max, only the tool name is rendered (no arguments, no description).
    #[test]
    fn format_tool_max_name_only() {
        let engine = CompressionEngine::new(CompressionLevel::Max);
        assert_eq!(engine.format_tool(&fetch_tool()), "<tool>fetch</tool>");
    }

    /// At Max, a tool with no description is still just the name.
    #[test]
    fn format_tool_max_no_description() {
        let engine = CompressionEngine::new(CompressionLevel::Max);
        assert_eq!(engine.format_tool(&no_desc_tool()), "<tool>ping</tool>");
    }

    // ------------------------------------------------------------------
    // format_tool — High
    // ------------------------------------------------------------------

    /// At High, arguments are listed but descriptions are omitted.
    #[test]
    fn format_tool_high_name_and_args() {
        let engine = CompressionEngine::new(CompressionLevel::High);
        assert_eq!(engine.format_tool(&fetch_tool()), "<tool>fetch(url, timeout)</tool>");
    }

    /// At High, a tool with no args shows an empty arg list.
    #[test]
    fn format_tool_high_no_args() {
        let engine = CompressionEngine::new(CompressionLevel::High);
        assert_eq!(engine.format_tool(&no_args_tool()), "<tool>health()</tool>");
    }

    // ------------------------------------------------------------------
    // format_tool — Medium
    // ------------------------------------------------------------------

    /// At Medium, the first sentence of the description is included.
    /// "Fetch a URL. Returns the raw content." → only "Fetch a URL" is kept.
    #[test]
    fn format_tool_medium_first_sentence() {
        let engine = CompressionEngine::new(CompressionLevel::Medium);
        let out = engine.format_tool(&fetch_tool());
        assert_eq!(out, "<tool>fetch(url, timeout): Fetch a URL</tool>");
    }

    /// At Medium, only the first *line* of the description is considered
    /// before splitting on ".".
    #[test]
    fn format_tool_medium_first_line_of_multiline() {
        let engine = CompressionEngine::new(CompressionLevel::Medium);
        let out = engine.format_tool(&multiline_tool());
        // "First line description.\nSecond line..." → first line → before "." → "First line description"
        assert_eq!(out, "<tool>multiline(x): First line description</tool>");
    }

    /// At Medium, a tool with no description renders without a description suffix.
    #[test]
    fn format_tool_medium_no_description() {
        let engine = CompressionEngine::new(CompressionLevel::Medium);
        assert_eq!(engine.format_tool(&no_desc_tool()), "<tool>ping(host)</tool>");
    }

    // ------------------------------------------------------------------
    // format_tool — Low
    // ------------------------------------------------------------------

    /// At Low, the complete description is included verbatim.
    #[test]
    fn format_tool_low_full_description() {
        let engine = CompressionEngine::new(CompressionLevel::Low);
        assert_eq!(
            engine.format_tool(&fetch_tool()),
            "<tool>fetch(url, timeout): Fetch a URL. Returns the raw content.</tool>",
        );
    }

    /// At Low, a multi-line description is included in full (not truncated).
    #[test]
    fn format_tool_low_multiline_description_kept() {
        let engine = CompressionEngine::new(CompressionLevel::Low);
        let out = engine.format_tool(&multiline_tool());
        assert!(out.contains("First line description."));
        assert!(out.contains("Second line continuation."));
    }

    /// At Low, a tool with no args shows an empty arg list.
    #[test]
    fn format_tool_low_no_args() {
        let engine = CompressionEngine::new(CompressionLevel::Low);
        assert_eq!(engine.format_tool(&no_args_tool()), "<tool>health(): Check server health.</tool>");
    }

    // ------------------------------------------------------------------
    // format_listing
    // ------------------------------------------------------------------

    /// At Max, format_listing always returns an empty string.
    /// (The frontend server registers a list_tools MCP tool instead.)
    #[test]
    fn format_listing_max_returns_empty() {
        let engine = CompressionEngine::new(CompressionLevel::Max);
        assert_eq!(engine.format_listing(&[fetch_tool(), no_desc_tool()]), "");
    }

    /// An empty tool slice at any non-Max level returns an empty string.
    #[test]
    fn format_listing_empty_tools() {
        for level in [CompressionLevel::Low, CompressionLevel::Medium, CompressionLevel::High] {
            let engine = CompressionEngine::new(level);
            assert_eq!(engine.format_listing(&[]), "");
        }
    }

    /// Multiple tools are joined with newlines.
    #[test]
    fn format_listing_multiple_tools_joined_with_newline() {
        let engine = CompressionEngine::new(CompressionLevel::High);
        let tools = vec![fetch_tool(), no_args_tool()];
        let listing = engine.format_listing(&tools);
        let lines: Vec<&str> = listing.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "<tool>fetch(url, timeout)</tool>");
        assert_eq!(lines[1], "<tool>health()</tool>");
    }

    /// A single tool listing has no trailing newline.
    #[test]
    fn format_listing_single_tool_no_trailing_newline() {
        let engine = CompressionEngine::new(CompressionLevel::High);
        let listing = engine.format_listing(&[fetch_tool()]);
        assert!(!listing.ends_with('\n'));
    }

    // ------------------------------------------------------------------
    // get_schema
    // ------------------------------------------------------------------

    /// get_schema returns Some(&tool) when the name matches.
    #[test]
    fn get_schema_found() {
        let engine = CompressionEngine::new(CompressionLevel::Medium);
        let tools = vec![fetch_tool()];
        let result = engine.get_schema(&tools, "fetch");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "fetch");
    }

    /// get_schema returns None for an unknown name.
    #[test]
    fn get_schema_not_found() {
        let engine = CompressionEngine::new(CompressionLevel::Medium);
        let tools = vec![fetch_tool()];
        assert!(engine.get_schema(&tools, "nonexistent").is_none());
    }

    /// get_schema on an empty tool list always returns None.
    #[test]
    fn get_schema_empty_list() {
        let engine = CompressionEngine::new(CompressionLevel::Medium);
        assert!(engine.get_schema(&[], "fetch").is_none());
    }

    // ------------------------------------------------------------------
    // format_schema_response
    // ------------------------------------------------------------------

    /// Schema response includes the Low-detail tool description.
    #[test]
    fn format_schema_response_contains_low_description() {
        let tool = fetch_tool();
        let response = CompressionEngine::format_schema_response(&tool);
        assert!(response.contains("<tool>fetch(url, timeout):"), "got: {response}");
        assert!(response.contains("Fetch a URL. Returns the raw content."));
    }

    /// Schema response includes the pretty-printed JSON input schema.
    #[test]
    fn format_schema_response_contains_json_schema() {
        let tool = fetch_tool();
        let response = CompressionEngine::format_schema_response(&tool);
        assert!(response.contains("\"properties\""), "got: {response}");
        assert!(response.contains("\"url\""));
    }

    /// Schema response separates the description and schema with a blank line.
    #[test]
    fn format_schema_response_blank_line_separator() {
        let tool = fetch_tool();
        let response = CompressionEngine::format_schema_response(&tool);
        assert!(response.contains("\n\n"), "expected blank-line separator, got: {response}");
    }

    // ------------------------------------------------------------------
    // param_names
    // ------------------------------------------------------------------

    /// param_names returns parameter names in schema insertion order.
    #[test]
    fn param_names_returns_ordered_params() {
        let tool = fetch_tool();
        let names = tool.param_names();
        // "url" appears before "timeout" in the schema definition
        assert_eq!(names, vec!["url", "timeout"]);
    }

    /// param_names on a tool with no properties returns an empty vec.
    #[test]
    fn param_names_empty_schema() {
        let tool = no_args_tool();
        assert_eq!(tool.param_names(), Vec::<String>::new());
    }
}
