//! Terminal startup banner with compression statistics.

use crate::compression::engine::Tool;
use crate::compression::{CompressionEngine, CompressionLevel};
use crate::server::compressed::INVOKE_TOOL_INPUT_SCHEMA_DESCRIPTION;

const TITLE: &str = "\
\x1b[32m█▀▄▀█ █▀▀ █▀█   █▀▀ █▀█ █▀▄▀█ █▀█ █▀█ █▀▀ █▀▀ █▀▀ █▀█ █▀█\x1b[0m
\x1b[32m█ ▀ █ █▄▄ █▀▀   █▄▄ █▄█ █ ▀ █ █▀▀ █▀▄ ██▄ ▄▄█ ▄▄█ █▄█ █▀▄\x1b[0m";

/// Compute compression statistics for all levels given a set of backend tools.
pub fn compression_stats(tools: &[Tool]) -> CompressionStats {
    let original: usize = tools
        .iter()
        .map(|t| {
            let name_len = t.name.len();
            let desc_len = t.description.as_deref().unwrap_or("").len();
            let schema_len = t
                .input_schema
                .get("properties")
                .and_then(|p| serde_json::to_string(p).ok())
                .map(|s| s.len())
                .unwrap_or(0);
            name_len + desc_len + schema_len
        })
        .sum();

    let levels = [
        CompressionLevel::Low,
        CompressionLevel::Medium,
        CompressionLevel::High,
        CompressionLevel::Max,
    ];

    let compressed: Vec<(CompressionLevel, usize)> = levels
        .iter()
        .map(|level| (level.clone(), compressed_frontend_size(tools, level)))
        .collect();

    CompressionStats {
        original_size: original,
        compressed,
    }
}

pub struct CompressionStats {
    pub original_size: usize,
    pub compressed: Vec<(CompressionLevel, usize)>,
}

fn compressed_frontend_size(tools: &[Tool], level: &CompressionLevel) -> usize {
    let engine = CompressionEngine::new(level.clone());
    let listing = engine.format_listing(tools);

    let get_tool_schema_description = format!(
        "Get the complete schema and description for one backend tool. Available tools:\n{listing}"
    );
    let invoke_tool_description = "Invoke one backend tool by name with JSON input.";
    let list_tools_description = "List backend tools available through this compressed MCP server.";

    let schema_wrapper = serde_json::json!({
        "type": "object",
        "properties": {
            "tool_name": {"type": "string", "description": "Name of the backend tool"}
        },
        "required": ["tool_name"]
    });
    let invoke_wrapper = serde_json::json!({
        "type": "object",
        "properties": {
            "tool_name": {"type": "string", "description": "Name of the backend tool"},
            "tool_input": {
                "type": "object",
                "description": INVOKE_TOOL_INPUT_SCHEMA_DESCRIPTION,
                "properties": {},
                "additionalProperties": true
            }
        },
        "required": ["tool_name", "tool_input"]
    });
    let list_wrapper = serde_json::json!({
        "type": "object",
        "properties": {}
    });

    let mut size = get_tool_schema_description.len()
        + invoke_tool_description.len()
        + schema_wrapper.to_string().len()
        + invoke_wrapper.to_string().len();

    if *level == CompressionLevel::Max {
        size += list_tools_description.len() + list_wrapper.to_string().len();
    }

    size
}

/// Print the startup banner with compression chart to stderr.
pub fn print_banner(
    server_name: Option<&str>,
    transport_type: &str,
    active_level: &CompressionLevel,
    tools: &[Tool],
    cli_info: Option<CliInfo<'_>>,
) {
    let columns = terminal_width().min(80);
    if columns < 63 {
        return;
    }

    let content_width = columns - 6;
    let header = format!("╭{}╮", "─".repeat(columns - 2));
    let footer = format!("╰{}╯", "─".repeat(columns - 2));
    let separator = format!("├{}┤", "─".repeat(columns - 2));
    let blank = format!("│{}│", " ".repeat(columns - 2));

    let stats = compression_stats(tools);

    let mut lines = vec![header.clone(), blank.clone()];
    for title_line in TITLE.lines() {
        lines.push(pad_line(title_line, content_width, true));
    }
    lines.push(blank.clone());
    lines.push(pad_line(
        "https://atlassian-labs.github.io/mcp-compressor/",
        content_width,
        true,
    ));
    if let Some(name) = server_name {
        lines.push(blank.clone());
        lines.push(pad_line(
            &format!("\x1b[32m●\x1b[0m Backend server name: {name}"),
            content_width,
            false,
        ));
    }
    lines.push(pad_line(
        &format!(
            "\x1b[32m●\x1b[0m Backend server transport: {}",
            transport_type.to_uppercase()
        ),
        content_width,
        false,
    ));
    lines.push(blank.clone());
    lines.push(separator.clone());
    lines.push(blank.clone());

    lines.push(pad_line(
        &format!(
            "📊 Compression Statistics (current = {}):",
            capitalize(active_level)
        ),
        content_width - 1,
        false,
    ));
    lines.push(blank.clone());
    lines.extend(format_chart(&stats, content_width, active_level));

    if let Some(info) = cli_info {
        lines.push(blank.clone());
        lines.push(separator.clone());
        lines.push(blank.clone());
        if let Some(script) = info.script_path {
            lines.push(pad_line(
                &format!("Script:  {script}"),
                content_width,
                false,
            ));
        }
        if let Some(bridge) = info.bridge_url {
            lines.push(pad_line(
                &format!("Bridge:  {bridge}"),
                content_width,
                false,
            ));
        }
        if let Some(invoke) = info.invoke_prefix {
            lines.push(pad_line(
                &format!("Run:     {invoke} --help"),
                content_width,
                false,
            ));
        }
    }

    lines.push(blank.clone());
    lines.push(footer);

    eprintln!("{}", lines.join("\n"));
}

pub struct CliInfo<'a> {
    pub script_path: Option<&'a str>,
    pub bridge_url: Option<&'a str>,
    pub invoke_prefix: Option<&'a str>,
}

fn format_chart(
    stats: &CompressionStats,
    width: usize,
    active_level: &CompressionLevel,
) -> Vec<String> {
    let chart_width = width.saturating_sub(16);
    let original = stats.original_size;
    let mut lines = Vec::new();

    // Original bar (100%)
    let bar = "█".repeat(chart_width);
    lines.push(pad_line(&format!("Original {bar} 100.0%"), width, false));

    // Each compression level
    let levels = [
        CompressionLevel::Low,
        CompressionLevel::Medium,
        CompressionLevel::High,
        CompressionLevel::Max,
    ];
    for level in &levels {
        let size = stats
            .compressed
            .iter()
            .find(|(l, _)| l == level)
            .map(|(_, s)| *s)
            .unwrap_or(0);
        let ratio = if original > 0 {
            size as f64 / original as f64
        } else {
            0.0
        };
        let filled = (ratio * chart_width as f64).round() as usize;
        let filled = filled.min(chart_width);
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(chart_width - filled));
        let pct = ratio * 100.0;
        let label = format!("{:<8}", capitalize(level));
        let mut line = pad_line(&format!("{label} {bar} {pct:5.1}%"), width, false);

        if level == active_level {
            line = highlight_bar(&line);
        }
        lines.push(line);
    }
    lines
}

/// Highlight the filled █ portion of a bar line in green using char-safe splits.
fn highlight_bar(line: &str) -> String {
    // Find the first '░' (dim block) char position using char indices
    if let Some(fade_byte) = line.char_indices().find(|(_, c)| *c == '░').map(|(i, _)| i) {
        // Find the last '│' before the bar content — the prefix up to and including "│  "
        // We work purely with byte positions from char_indices, so this is safe.
        let prefix_end = line
            .char_indices()
            .take_while(|(_, c)| *c != '█' && *c != '░')
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!(
            "{}\x1b[1;32m{}\x1b[0m{}",
            &line[..prefix_end],
            &line[prefix_end..fade_byte],
            &line[fade_byte..]
        )
    } else {
        // No dim blocks — whole bar is filled, highlight entirely
        format!("\x1b[1;32m{line}\x1b[0m")
    }
}

fn capitalize(level: &CompressionLevel) -> String {
    let s = level.to_string();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn pad_line(line: &str, total_width: usize, center: bool) -> String {
    // Strip ANSI codes for width calculation
    let clean: String = strip_ansi(line);
    let clean_width = clean.chars().count();

    if center {
        let padding_total = total_width.saturating_sub(clean_width);
        let padding_left = padding_total / 2;
        let padding_right = padding_total - padding_left;
        format!(
            "│  {}{}{}  │",
            " ".repeat(padding_left),
            line,
            " ".repeat(padding_right)
        )
    } else {
        let padding_right = total_width.saturating_sub(clean_width);
        format!("│  {}{}  │", line, " ".repeat(padding_right))
    }
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }
    result
}

fn terminal_width() -> usize {
    // Try to get terminal width; fall back to 80
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        unsafe {
            let mut ws = MaybeUninit::<libc::winsize>::zeroed();
            if libc::ioctl(2, libc::TIOCGWINSZ, ws.as_mut_ptr()) == 0 {
                let ws = ws.assume_init();
                if ws.ws_col > 0 {
                    return ws.ws_col as usize;
                }
            }
        }
    }
    80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_bar_handles_unicode_box_border() {
        let line =
            "│  Medium   █████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  15.5%  │";
        let highlighted = highlight_bar(line);
        assert!(highlighted.contains("\x1b[1;32m"));
        assert!(highlighted.contains("Medium"));
        assert_eq!(strip_ansi(&highlighted), line);
    }

    #[test]
    fn format_chart_only_shows_compression_levels() {
        let stats = CompressionStats {
            original_size: 129,
            compressed: vec![
                (CompressionLevel::Low, 80),
                (CompressionLevel::Medium, 20),
                (CompressionLevel::High, 10),
                (CompressionLevel::Max, 5),
            ],
        };

        let lines = format_chart(&stats, 80, &CompressionLevel::Medium);
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().any(|line| line.contains("Original")));
        assert!(lines.iter().any(|line| line.contains("Low")));
        assert!(lines.iter().any(|line| line.contains("Medium")));
        assert!(lines.iter().any(|line| line.contains("High")));
        assert!(lines.iter().any(|line| line.contains("Max")));
        assert!(!lines.iter().any(|line| line.contains("CLI mode")));
        let medium = lines.iter().find(|line| line.contains("Medium")).unwrap();
        assert!(medium.contains("\x1b[1;32m"));
    }

    #[test]
    fn max_compression_stat_includes_wrapper_schema_surface() {
        let tools = vec![Tool::new(
            "echo",
            Some("Echo a message".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {"message": {"type": "string"}},
                "required": ["message"]
            }),
        )];
        let stats = compression_stats(&tools);
        let max = stats
            .compressed
            .iter()
            .find(|(level, _)| *level == CompressionLevel::Max)
            .map(|(_, size)| *size)
            .unwrap();
        assert!(max > 0);
    }
}
