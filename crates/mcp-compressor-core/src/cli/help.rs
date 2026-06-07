//! Shared CLI help renderer — the single source of truth for help text used by
//! every transform mode (generated CLI script, Just Bash commands, and the
//! `*_help` MCP tool description).
//!
//! Historically there were three separate implementations of "render the
//! top-level help" and "render a subcommand's help" — one embedded in the
//! generated CLI shell script ([`crate::client_gen::cli`]), one in the host
//! transform plan ([`crate::ffi::host_transform`]), and one in the TypeScript
//! Just Bash command builder. They drifted (different column widths, footers,
//! and the Just Bash path was missing subcommand `--help` entirely).
//!
//! This module centralizes all of that logic so that:
//!
//! - The top-level `<cli> --help` output and the `<cli>_help` tool description
//!   share an **identical body** and differ only by an explicit
//!   [`HelpFraming`] prefix/footer.
//! - The rich per-subcommand `<cli> <subcommand> --help` output is rendered
//!   from one place and is therefore identical across CLI and Just Bash modes.

use std::collections::HashSet;

use crate::cli::mapping::tool_name_to_subcommand;
use crate::compression::engine::Tool;

/// The TOON-format note shown in every top-level help body.
pub const TOON_NOTE: &str =
    "When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.";

/// Framing applied around the shared top-level help body. The body itself is
/// always identical; only this prefix/footer may differ between the shell
/// `--help` output and the `*_help` tool description.
#[derive(Debug, Clone, Default)]
pub struct HelpFraming {
    /// Optional lines prepended before the shared body (followed by a blank
    /// line). Used by the `*_help` tool description to tell the model to use
    /// the CLI rather than calling the tool.
    pub prefix: Vec<String>,
    /// Optional lines appended after the shared body (preceded by a blank
    /// line).
    pub footer: Vec<String>,
}

impl HelpFraming {
    /// Framing for the generated shell script's top-level `--help`.
    pub fn shell(command: &str) -> Self {
        Self {
            prefix: Vec::new(),
            footer: vec![format!(
                "Run '{command} <subcommand> --help' for subcommand usage."
            )],
        }
    }

    /// Framing for the `<cli>_help` MCP tool description.
    pub fn help_tool(command: &str, cli_name: &str) -> Self {
        Self {
            prefix: vec![format!(
                "Functionality associated with the {cli_name} toolset is provided via the `{command}` CLI. Do not call this tool - use the CLI instead."
            )],
            footer: vec![
                format!("Run '{command} --help' in the shell for usage."),
                format!("Run '{command} <subcommand> --help' for per-command help."),
                format!("Run '{command} <subcommand> [options]' to invoke a tool."),
            ],
        }
    }
}

/// Render the shared top-level help body (without framing): the toolset header,
/// the TOON note, the USAGE line, and the SUBCOMMANDS listing with concise
/// per-subcommand summaries.
pub fn render_top_level_body(command: &str, cli_name: &str, tools: &[Tool]) -> String {
    let mut lines = vec![
        format!("{cli_name} - the {cli_name} toolset"),
        String::new(),
        TOON_NOTE.to_string(),
        String::new(),
        "USAGE:".to_string(),
        format!("  {command} <subcommand> [options]"),
        String::new(),
        "SUBCOMMANDS:".to_string(),
    ];
    lines.extend(render_subcommands_block(tools));
    lines.join("\n")
}

/// Render the full top-level help text: framing prefix, the shared body, then
/// framing footer.
pub fn render_top_level_help(
    command: &str,
    cli_name: &str,
    tools: &[Tool],
    framing: &HelpFraming,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !framing.prefix.is_empty() {
        lines.extend(framing.prefix.iter().cloned());
    }
    lines.push(render_top_level_body(command, cli_name, tools));
    if !framing.footer.is_empty() {
        lines.push(String::new());
        lines.extend(framing.footer.iter().cloned());
    }
    lines.join("\n")
}

/// Render the `SUBCOMMANDS:` listing lines (kebab-case subcommand + concise
/// summary), dynamically aligned to the longest subcommand name.
pub fn render_subcommands_block(tools: &[Tool]) -> Vec<String> {
    let names = tools
        .iter()
        .map(|tool| tool_name_to_subcommand(&tool.name))
        .collect::<Vec<_>>();
    let max_name_len = names.iter().map(String::len).max().unwrap_or(0);
    tools
        .iter()
        .zip(names)
        .map(|(tool, name)| {
            let description = short_tool_description(tool.description.as_deref());
            format!("  {name:<width$}{description}", width = max_name_len + 2)
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Render the rich per-subcommand help: description block, usage line, and the
/// REQUIRED / OPTIONAL / COMPLEX / GLOBAL option sections derived from the
/// tool's input schema.
pub fn render_subcommand_help(cli_name: &str, tool: &Tool) -> String {
    let subcommand = tool_name_to_subcommand(&tool.name);
    let mut help = format!(
        "{cli_name} {subcommand}\n\n{}\n\nUSAGE:\n  {cli_name} {subcommand} [options]\n",
        tool_description_block(tool.description.as_deref().unwrap_or("Invoke this tool.")),
    );

    let options = tool_options(tool);
    let required = options
        .iter()
        .filter(|option| option.required && !option.is_complex)
        .collect::<Vec<_>>();
    let optional = options
        .iter()
        .filter(|option| !option.required && !option.is_complex)
        .collect::<Vec<_>>();
    let complex = options
        .iter()
        .filter(|option| option.is_complex)
        .collect::<Vec<_>>();

    append_option_section(&mut help, "REQUIRED", &required);
    append_option_section(&mut help, "OPTIONAL", &optional);
    append_option_section(&mut help, "COMPLEX / JSON OPTIONS", &complex);

    help.push_str("\nGLOBAL OPTIONS:\n");
    help.push_str("  --json <json object>\n");
    help.push_str("      Provide the entire tool input as JSON. This bypasses flag parsing.\n");
    help.push_str("\n  --help\n");
    help.push_str("      Show this help message.\n");

    help
}

// ---------------------------------------------------------------------------
// Concise summary helpers (shared by top-level help and code-mode help).
// ---------------------------------------------------------------------------

/// Produce a concise one-line summary of a tool description: the first
/// sentence (unless under 10 chars, in which case the first non-empty line),
/// cleanly truncated to 200 characters.
pub fn short_tool_description(description: Option<&str>) -> String {
    let trimmed = description.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let first_sentence = first_sentence(trimmed);
    let candidate = if first_sentence.chars().count() >= 10 {
        first_sentence
    } else {
        first_non_empty_line(trimmed)
    };
    truncate_clean(candidate, 200)
}

fn first_sentence(value: &str) -> &str {
    for (index, ch) in value.char_indices() {
        if matches!(ch, '.' | '!' | '?') {
            return value[..=index].trim();
        }
    }
    first_non_empty_line(value)
}

fn first_non_empty_line(value: &str) -> &str {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

fn truncate_clean(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let limit = max_chars.saturating_sub(3);
    let mut end = 0;
    for (count, (index, ch)) in compact.char_indices().enumerate() {
        if count >= limit {
            break;
        }
        end = index + ch.len_utf8();
    }
    let mut prefix = compact[..end]
        .trim_end_matches(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';' || ch == ':')
        .to_string();
    if let Some(space) = prefix.rfind(' ') {
        if space >= max_chars / 2 {
            prefix.truncate(space);
        }
    }
    prefix.push_str("...");
    prefix
}

/// Collapse internal whitespace runs into single spaces, keeping the text on a
/// single line.
pub fn full_description(description: &str) -> String {
    description.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render a tool's top-level description while preserving its original line
/// structure. Each line is trimmed and has internal whitespace runs collapsed,
/// but newlines (including blank lines between paragraphs) are kept so the
/// description does not get flattened into a single massive line. Leading and
/// trailing blank lines are removed, and runs of 3+ blank lines are collapsed
/// to a single blank line.
pub fn tool_description_block(description: &str) -> String {
    let lines: Vec<String> = description
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();

    // Collapse multiple consecutive blank lines into a single blank line.
    let mut collapsed: Vec<String> = Vec::with_capacity(lines.len());
    let mut previous_blank = false;
    for line in lines {
        let is_blank = line.is_empty();
        if is_blank && previous_blank {
            continue;
        }
        previous_blank = is_blank;
        collapsed.push(line);
    }

    // Trim leading/trailing blank lines.
    while collapsed.first().is_some_and(|line| line.is_empty()) {
        collapsed.remove(0);
    }
    while collapsed.last().is_some_and(|line| line.is_empty()) {
        collapsed.pop();
    }

    if collapsed.is_empty() {
        return "Invoke this tool.".to_string();
    }

    collapsed.join("\n")
}

// ---------------------------------------------------------------------------
// Per-subcommand option rendering.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolOption {
    name: String,
    ty: String,
    required: bool,
    description: Option<String>,
    default: Option<String>,
    enum_values: Vec<String>,
    minimum: Option<String>,
    maximum: Option<String>,
    min_length: Option<String>,
    max_length: Option<String>,
    min_items: Option<String>,
    max_items: Option<String>,
    is_complex: bool,
    complex_hint: Option<String>,
}

fn tool_options(tool: &Tool) -> Vec<ToolOption> {
    let required = tool
        .input_schema
        .get("required")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    tool.input_schema
        .get("properties")
        .and_then(|value| value.as_object())
        .map(|properties| {
            properties
                .iter()
                .map(|(name, schema)| {
                    let (ty, is_complex, complex_hint) = schema_type_details(schema);
                    ToolOption {
                        name: name.clone(),
                        ty,
                        required: required.contains(name),
                        description: schema
                            .get("description")
                            .and_then(|value| value.as_str())
                            .map(full_description),
                        default: schema.get("default").map(default_value_label),
                        enum_values: schema_enum_values(schema),
                        minimum: schema_number_constraint(schema, "minimum"),
                        maximum: schema_number_constraint(schema, "maximum"),
                        min_length: schema_number_constraint(schema, "minLength"),
                        max_length: schema_number_constraint(schema, "maxLength"),
                        min_items: schema_number_constraint(schema, "minItems"),
                        max_items: schema_number_constraint(schema, "maxItems"),
                        is_complex,
                        complex_hint,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn append_option_section(help: &mut String, title: &str, options: &[&ToolOption]) {
    if options.is_empty() {
        return;
    }
    help.push('\n');
    help.push_str(title);
    help.push_str(":\n");
    for option in options {
        help.push_str(&format_tool_option_help(option));
    }
}

fn format_tool_option_help(option: &ToolOption) -> String {
    let flag = format!("--{}", tool_name_to_subcommand(&option.name));
    let mut output = format!("  {flag} <{}>\n", option.ty);
    let mut details = Vec::new();
    if option.required {
        details.push("Required.".to_string());
    }
    if let Some(description) = &option.description {
        details.push(description.clone());
    }
    if !option.enum_values.is_empty() {
        details.push(format!(
            "Allowed values: {}.",
            option.enum_values.join(", ")
        ));
    }
    if let Some(default) = &option.default {
        details.push(format!("Default: {default}."));
    }
    if let Some(minimum) = &option.minimum {
        details.push(format!("Minimum: {minimum}."));
    }
    if let Some(maximum) = &option.maximum {
        details.push(format!("Maximum: {maximum}."));
    }
    if let Some(min_length) = &option.min_length {
        details.push(format!("Minimum length: {min_length}."));
    }
    if let Some(max_length) = &option.max_length {
        details.push(format!("Maximum length: {max_length}."));
    }
    if let Some(min_items) = &option.min_items {
        details.push(format!("Minimum items: {min_items}."));
    }
    if let Some(max_items) = &option.max_items {
        details.push(format!("Maximum items: {max_items}."));
    }
    if option.ty == "boolean" {
        details.push("Accepted values: true, false.".to_string());
        details.push(format!(
            "Also supports: {flag} and --no-{}.",
            tool_name_to_subcommand(&option.name)
        ));
    }
    if let Some(hint) = &option.complex_hint {
        details.push(hint.clone());
    }
    for detail in details {
        output.push_str(&wrap_indented(&detail, 6, 100));
    }
    output
}

fn schema_number_constraint(schema: &serde_json::Value, key: &str) -> Option<String> {
    schema.get(key).map(default_value_label)
}

fn schema_type_details(schema: &serde_json::Value) -> (String, bool, Option<String>) {
    let labels = schema_enum_values(schema);
    if !labels.is_empty() {
        return (labels.join("|"), false, None);
    }
    if schema.get("oneOf").is_some()
        || schema.get("anyOf").is_some()
        || schema.get("allOf").is_some()
    {
        return (
            "json".to_string(),
            true,
            Some("Schema contains oneOf/anyOf/allOf; use --json for complex input.".to_string()),
        );
    }
    match schema.get("type").and_then(|value| value.as_str()) {
        Some("integer") => ("integer".to_string(), false, None),
        Some("number") => ("number".to_string(), false, None),
        Some("boolean") => ("boolean".to_string(), false, None),
        Some("array") => match schema.get("items") {
            Some(items) => {
                let (item_ty, item_complex, _) = schema_type_details(items);
                if item_complex
                    || matches!(
                        items.get("type").and_then(|value| value.as_str()),
                        Some("object" | "array")
                    )
                {
                    (
                        "json array".to_string(),
                        true,
                        Some("Pass as a JSON array string.".to_string()),
                    )
                } else {
                    (
                        format!("{item_ty}[]"),
                        false,
                        Some("Repeat this flag or pass a JSON array.".to_string()),
                    )
                }
            }
            None => (
                "json array".to_string(),
                true,
                Some("Pass as a JSON array string.".to_string()),
            ),
        },
        Some("object") => (
            "json object".to_string(),
            true,
            Some("Pass as a JSON object string.".to_string()),
        ),
        Some("string") | None => ("string".to_string(), false, None),
        Some(other) => (other.to_string(), false, None),
    }
}

fn schema_enum_values(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("enum")
        .and_then(|value| value.as_array())
        .map(|values| values.iter().map(default_value_label).collect())
        .unwrap_or_default()
}

fn default_value_label(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn wrap_indented(value: &str, indent: usize, width: usize) -> String {
    let prefix = " ".repeat(indent);
    let mut output = String::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width.saturating_sub(indent) {
            output.push_str(&prefix);
            output.push_str(line.trim_end());
            output.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        output.push_str(&prefix);
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, description: &str) -> Tool {
        Tool {
            name: name.to_string(),
            description: Some(description.to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." },
                    "sort": {
                        "type": "string",
                        "description": "Sort order.",
                        "enum": ["score", "timestamp"]
                    }
                },
                "required": ["query"]
            }),
        }
    }

    /// The `*_help` tool description must equal the shell `--help` body plus the
    /// documented framing prefix/footer — nothing else may differ.
    #[test]
    fn help_tool_description_equals_shell_help_modulo_framing() {
        let tools = vec![tool(
            "search",
            "Search things. Extra detail after the sentence.",
        )];
        let command = "atlassian";
        let cli_name = "atlassian";

        let body = render_top_level_body(command, cli_name, &tools);
        let shell = render_top_level_help(command, cli_name, &tools, &HelpFraming::shell(command));
        let help_tool = render_top_level_help(
            command,
            cli_name,
            &tools,
            &HelpFraming::help_tool(command, cli_name),
        );

        // Both renderings contain the identical shared body.
        assert!(shell.contains(&body), "shell help must contain shared body");
        assert!(
            help_tool.contains(&body),
            "help-tool description must contain the identical shared body"
        );

        // The help-tool description is exactly: prefix + body + footer.
        let framing = HelpFraming::help_tool(command, cli_name);
        let expected = format!(
            "{}\n{}\n\n{}",
            framing.prefix.join("\n"),
            body,
            framing.footer.join("\n")
        );
        assert_eq!(help_tool, expected);
    }

    #[test]
    fn top_level_help_uses_concise_summaries() {
        let long = "First sentence is short. ".to_string() + &"word ".repeat(80);
        let tools = vec![tool("search", &long)];
        let body = render_top_level_body("svc", "svc", &tools);
        // Concise: only the first sentence should appear, not the 80-word tail.
        assert!(body.contains("First sentence is short."));
        assert!(!body.contains("word word word word word word"));
    }

    #[test]
    fn subcommand_help_includes_enum_and_required_sections() {
        let help = render_subcommand_help("svc", &tool("search", "Search things."));
        assert!(help.contains("REQUIRED:"));
        assert!(help.contains("--query"));
        assert!(help.contains("OPTIONAL:"));
        assert!(help.contains("--sort"));
        assert!(help.contains("Allowed values: score, timestamp."));
        assert!(help.contains("GLOBAL OPTIONS:"));
        assert!(help.contains("--json"));
    }

    #[test]
    fn tool_description_block_preserves_line_structure() {
        let input = "  First line.  \n\nSecond  paragraph    with   spaces.\nThird line.\n\n\n";
        let rendered = tool_description_block(input);
        assert_eq!(
            rendered,
            "First line.\n\nSecond paragraph with spaces.\nThird line."
        );
        assert!(rendered.contains('\n'));
    }
}
