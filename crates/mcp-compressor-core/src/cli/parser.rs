//! CLI argument parser: `argv → tool_input`.
//!
//! Parses a list of CLI arguments (everything after the subcommand) into a
//! `serde_json::Value` dict that can be passed directly as `tool_input` to the
//! backend MCP server.
//!
//! # Argument conventions (mirrors Python `parse_argv_to_tool_input`)
//!
//! | Syntax | Produces |
//! |---|---|
//! | `--flag value` | `{"flag": "value"}` (string) |
//! | `--flag` | `{"flag": true}` (boolean) |
//! | `--no-flag` | `{"flag": false}` (boolean) |
//! | `--flag true` / `--flag false` | explicit bool |
//! | `--flag 5` (integer prop) | `{"flag": 5}` |
//! | `--flag 0.5` (number prop) | `{"flag": 0.5}` |
//! | `--tag a --tag b` (array prop) | `{"tag": ["a","b"]}` |
//! | `--json '{"k":"v"}'` | `{"k": "v"}` (raw JSON escape-hatch) |
//! | `--page-id 123` (kebab flag) | `{"page_id": "123"}` (snake prop) |
//!
//! Unknown flags and positional arguments are errors.
//! Missing required arguments are errors.

use serde_json::{Map, Number, Value};

use crate::compression::engine::Tool;
use crate::Error;

/// Parse CLI `argv` (everything after the subcommand itself) into a JSON
/// object suitable for use as `tool_input`.
///
/// The `tool`'s `input_schema` drives type coercion and required-argument
/// checking.
pub fn parse_argv(argv: &[String], tool: &Tool) -> Result<serde_json::Value, Error> {
    if argv.first().is_some_and(|arg| arg == "--json") {
        let json = argv
            .get(1)
            .ok_or_else(|| Error::Parse("--json requires a value".to_string()))?;
        if argv.len() > 2 {
            return Err(Error::Parse("--json cannot be combined with other arguments".to_string()));
        }
        return Ok(serde_json::from_str(json)?);
    }

    let properties = schema_properties(tool);
    let required = required_properties(tool);
    let mut output = Map::new();
    let mut index = 0;

    while index < argv.len() {
        let arg = &argv[index];
        if !arg.starts_with("--") || arg == "--" {
            return Err(Error::Parse(format!("unexpected positional argument: {arg}")));
        }

        let (property_name, forced_bool) = parse_flag_name(arg);
        let schema = properties
            .get(&property_name)
            .ok_or_else(|| Error::Parse(format!("unknown flag: {arg}")))?;
        let schema_type = schema_type(schema);

        let (raw_value, consumed) = if forced_bool == Some(false) {
            if schema_type != Some("boolean") {
                return Err(Error::Parse(format!("{arg} can only be used with boolean properties")));
            }
            (None, 1)
        } else if schema_type == Some("boolean") {
            match argv.get(index + 1) {
                Some(next) if !next.starts_with("--") => (Some(next.as_str()), 2),
                _ => (None, 1),
            }
        } else {
            let value = argv
                .get(index + 1)
                .filter(|next| !next.starts_with("--"))
                .ok_or_else(|| Error::Parse(format!("{arg} requires a value")))?;
            (Some(value.as_str()), 2)
        };

        let value = coerce_value(&property_name, schema, raw_value, forced_bool)?;
        insert_value(&mut output, &property_name, schema, value);
        index += consumed;
    }

    for property in required {
        if !output.contains_key(&property) {
            return Err(Error::Validation(format!("missing required argument: {property}")));
        }
    }

    Ok(Value::Object(output))
}

fn schema_properties(tool: &Tool) -> Map<String, Value> {
    tool.input_schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn required_properties(tool: &Tool) -> Vec<String> {
    tool.input_schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_flag_name(flag: &str) -> (String, Option<bool>) {
    let name = flag.trim_start_matches("--");
    if let Some(name) = name.strip_prefix("no-") {
        (flag_to_property_name(name), Some(false))
    } else {
        (flag_to_property_name(name), None)
    }
}

fn flag_to_property_name(flag: &str) -> String {
    flag.replace('-', "_")
}

fn schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str)
}

fn array_item_schema(schema: &Value) -> Option<&Value> {
    schema.get("items")
}

fn coerce_value(
    property_name: &str,
    schema: &Value,
    raw_value: Option<&str>,
    forced_bool: Option<bool>,
) -> Result<Value, Error> {
    if let Some(value) = forced_bool {
        return Ok(Value::Bool(value));
    }

    match schema_type(schema) {
        Some("boolean") => coerce_bool(property_name, raw_value),
        Some("integer") => coerce_integer(property_name, raw_value),
        Some("number") => coerce_number(property_name, raw_value),
        Some("array") => {
            let item_schema = array_item_schema(schema).unwrap_or(&Value::Null);
            coerce_value(property_name, item_schema, raw_value, None)
        }
        _ => Ok(Value::String(raw_value.unwrap_or_default().to_string())),
    }
}

fn coerce_bool(property_name: &str, raw_value: Option<&str>) -> Result<Value, Error> {
    match raw_value {
        None => Ok(Value::Bool(true)),
        Some("true") => Ok(Value::Bool(true)),
        Some("false") => Ok(Value::Bool(false)),
        Some(value) => Err(Error::Parse(format!(
            "invalid boolean value for {property_name}: {value}"
        ))),
    }
}

fn coerce_integer(property_name: &str, raw_value: Option<&str>) -> Result<Value, Error> {
    let value = raw_value.ok_or_else(|| Error::Parse(format!("{property_name} requires a value")))?;
    let parsed = value
        .parse::<i64>()
        .map_err(|_| Error::Parse(format!("invalid integer value for {property_name}: {value}")))?;
    Ok(Value::Number(Number::from(parsed)))
}

fn coerce_number(property_name: &str, raw_value: Option<&str>) -> Result<Value, Error> {
    let value = raw_value.ok_or_else(|| Error::Parse(format!("{property_name} requires a value")))?;
    let parsed = value
        .parse::<f64>()
        .map_err(|_| Error::Parse(format!("invalid number value for {property_name}: {value}")))?;
    let number = Number::from_f64(parsed)
        .ok_or_else(|| Error::Parse(format!("invalid number value for {property_name}: {value}")))?;
    Ok(Value::Number(number))
}

fn insert_value(output: &mut Map<String, Value>, property_name: &str, schema: &Value, value: Value) {
    if schema_type(schema) == Some("array") {
        output
            .entry(property_name.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("array property should be stored as array")
            .push(value);
    } else {
        output.insert(property_name.to_string(), value);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Helper: build a Tool with just a name and a given JSON Schema.
    fn tool_with_schema(schema: serde_json::Value) -> Tool {
        Tool::new("test_tool", None::<String>, schema)
    }

    // Helper: args vec from string literals.
    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    // ------------------------------------------------------------------
    // String arguments
    // ------------------------------------------------------------------

    /// A simple `--flag value` pair produces a string in the output dict.
    #[test]
    fn string_arg() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "url": { "type": "string" } },
            "required": ["url"]
        }));
        let result = parse_argv(&args(&["--url", "https://example.com"]), &tool).unwrap();
        assert_eq!(result, json!({ "url": "https://example.com" }));
    }

    /// Multiple string flags are captured independently.
    #[test]
    fn multiple_string_args() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": {
                "url":    { "type": "string" },
                "method": { "type": "string" }
            }
        }));
        let result =
            parse_argv(&args(&["--url", "https://example.com", "--method", "GET"]), &tool).unwrap();
        assert_eq!(result, json!({ "url": "https://example.com", "method": "GET" }));
    }

    // ------------------------------------------------------------------
    // Boolean arguments
    // ------------------------------------------------------------------

    /// A bare `--flag` (no value following) produces `true`.
    #[test]
    fn boolean_flag_bare() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "verbose": { "type": "boolean" } }
        }));
        let result = parse_argv(&args(&["--verbose"]), &tool).unwrap();
        assert_eq!(result, json!({ "verbose": true }));
    }

    /// `--flag true` produces `true`.
    #[test]
    fn boolean_flag_explicit_true() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "verbose": { "type": "boolean" } }
        }));
        let result = parse_argv(&args(&["--verbose", "true"]), &tool).unwrap();
        assert_eq!(result, json!({ "verbose": true }));
    }

    /// `--flag false` produces `false`.
    #[test]
    fn boolean_flag_explicit_false() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "verbose": { "type": "boolean" } }
        }));
        let result = parse_argv(&args(&["--verbose", "false"]), &tool).unwrap();
        assert_eq!(result, json!({ "verbose": false }));
    }

    /// `--no-flag` produces `false` for a boolean property.
    #[test]
    fn no_prefix_produces_false() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "verbose": { "type": "boolean" } }
        }));
        let result = parse_argv(&args(&["--no-verbose"]), &tool).unwrap();
        assert_eq!(result, json!({ "verbose": false }));
    }

    // ------------------------------------------------------------------
    // Integer and number arguments
    // ------------------------------------------------------------------

    /// An `integer` property is coerced from the string value.
    #[test]
    fn integer_arg() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "count": { "type": "integer" } }
        }));
        let result = parse_argv(&args(&["--count", "5"]), &tool).unwrap();
        assert_eq!(result, json!({ "count": 5 }));
    }

    /// A `number` property is coerced to a float.
    #[test]
    fn number_arg_float() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "ratio": { "type": "number" } }
        }));
        let result = parse_argv(&args(&["--ratio", "0.5"]), &tool).unwrap();
        assert_eq!(result, json!({ "ratio": 0.5 }));
    }

    /// Passing a non-numeric string to an integer property is an error.
    #[test]
    fn integer_arg_invalid_value() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "count": { "type": "integer" } }
        }));
        assert!(parse_argv(&args(&["--count", "notanumber"]), &tool).is_err());
    }

    // ------------------------------------------------------------------
    // Array arguments (repeated flag)
    // ------------------------------------------------------------------

    /// Repeating a flag for an array property accumulates values.
    #[test]
    fn array_arg_repeated_flag() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        }));
        let result = parse_argv(&args(&["--tags", "a", "--tags", "b"]), &tool).unwrap();
        assert_eq!(result, json!({ "tags": ["a", "b"] }));
    }

    /// A single-element array works correctly.
    #[test]
    fn array_arg_single_element() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        }));
        let result = parse_argv(&args(&["--tags", "only"]), &tool).unwrap();
        assert_eq!(result, json!({ "tags": ["only"] }));
    }

    // ------------------------------------------------------------------
    // kebab-case → snake_case flag mapping
    // ------------------------------------------------------------------

    /// A kebab-case CLI flag maps to the corresponding snake_case property.
    #[test]
    fn kebab_flag_maps_to_snake_prop() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "page_id": { "type": "string" } },
            "required": ["page_id"]
        }));
        let result = parse_argv(&args(&["--page-id", "ABC123"]), &tool).unwrap();
        assert_eq!(result, json!({ "page_id": "ABC123" }));
    }

    /// The snake_case version of a flag name is also accepted directly.
    #[test]
    fn snake_flag_also_accepted() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "page_id": { "type": "string" } },
            "required": ["page_id"]
        }));
        let result = parse_argv(&args(&["--page_id", "ABC123"]), &tool).unwrap();
        assert_eq!(result, json!({ "page_id": "ABC123" }));
    }

    // ------------------------------------------------------------------
    // Required argument validation
    // ------------------------------------------------------------------

    /// A missing required argument is an error.
    #[test]
    fn missing_required_arg_is_error() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "url": { "type": "string" } },
            "required": ["url"]
        }));
        assert!(parse_argv(&[], &tool).is_err());
    }

    /// Optional arguments may be omitted without error.
    #[test]
    fn optional_arg_may_be_omitted() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": {
                "url":     { "type": "string" },
                "timeout": { "type": "number" }
            },
            "required": ["url"]
        }));
        let result = parse_argv(&args(&["--url", "https://example.com"]), &tool).unwrap();
        assert_eq!(result, json!({ "url": "https://example.com" }));
    }

    // ------------------------------------------------------------------
    // Error cases
    // ------------------------------------------------------------------

    /// An unknown flag is an error.
    #[test]
    fn unknown_flag_is_error() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "url": { "type": "string" } }
        }));
        assert!(parse_argv(&args(&["--unknown", "value"]), &tool).is_err());
    }

    /// A positional argument (no `--` prefix) is an error.
    #[test]
    fn positional_arg_is_error() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "url": { "type": "string" } }
        }));
        assert!(parse_argv(&args(&["positional"]), &tool).is_err());
    }

    /// A flag missing its value (end of argv) is an error.
    #[test]
    fn flag_missing_value_is_error() {
        let tool = tool_with_schema(json!({
            "type": "object",
            "properties": { "url": { "type": "string" } }
        }));
        assert!(parse_argv(&args(&["--url"]), &tool).is_err());
    }

    // ------------------------------------------------------------------
    // --json escape hatch
    // ------------------------------------------------------------------

    /// `--json '{"k":"v"}'` passes the raw JSON object through unchanged.
    #[test]
    fn json_escape_hatch() {
        let tool = tool_with_schema(json!({ "type": "object", "properties": {} }));
        let result =
            parse_argv(&args(&["--json", r#"{"key": "val"}"#]), &tool).unwrap();
        assert_eq!(result, json!({ "key": "val" }));
    }

    /// `--json` with no following value is an error.
    #[test]
    fn json_escape_hatch_requires_value() {
        let tool = tool_with_schema(json!({ "type": "object", "properties": {} }));
        assert!(parse_argv(&args(&["--json"]), &tool).is_err());
    }

    /// `--json` accepts a JSON array (not just objects).
    #[test]
    fn json_escape_hatch_array() {
        let tool = tool_with_schema(json!({ "type": "object", "properties": {} }));
        let result = parse_argv(&args(&["--json", "[1,2,3]"]), &tool).unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    // ------------------------------------------------------------------
    // Empty arguments
    // ------------------------------------------------------------------

    /// An empty argv with no required args succeeds with an empty dict.
    #[test]
    fn empty_argv_no_required() {
        let tool = tool_with_schema(json!({ "type": "object", "properties": {} }));
        let result = parse_argv(&[], &tool).unwrap();
        assert_eq!(result, json!({}));
    }
}
