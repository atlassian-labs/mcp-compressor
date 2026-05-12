//! Terminal startup banner with compression statistics.

use crate::compression::engine::Tool;
use crate::compression::{CompressionEngine, CompressionLevel};

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
        .map(|level| {
            let engine = CompressionEngine::new(level.clone());
            let listing = engine.format_listing(tools);
            (level.clone(), listing.len())
        })
        .collect();

    // CLI mode size: just tool names as subcommands
    let cli_size: usize = tools.iter().map(|t| t.name.len() + 1).sum();

    CompressionStats {
        original_size: original,
        compressed,
        cli_size,
    }
}

pub struct CompressionStats {
    pub original_size: usize,
    pub compressed: Vec<(CompressionLevel, usize)>,
    pub cli_size: usize,
}

/// Print the startup banner with compression chart to stderr.
pub fn print_banner(
    server_name: Option<&str>,
    transport_type: &str,
    active_level: &CompressionLevel,
    tools: &[Tool],
    cli_mode: bool,
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

    if cli_mode {
        lines.push(pad_line(
            "📊 Compression Statistics (current = CLI mode):",
            content_width - 1,
            false,
        ));
    } else {
        lines.push(pad_line(
            &format!(
                "📊 Compression Statistics (current = {}):",
                capitalize(active_level)
            ),
            content_width - 1,
            false,
        ));
    }
    lines.push(blank.clone());
    lines.extend(format_chart(&stats, content_width, active_level, cli_mode));

    if let Some(info) = cli_info {
        lines.push(blank.clone());
        lines.push(separator.clone());
        lines.push(blank.clone());
        if let Some(script) = info.script_path {
            lines.push(pad_line(&format!("Script:  {script}"), content_width, false));
        }
        if let Some(bridge) = info.bridge_url {
            lines.push(pad_line(&format!("Bridge:  {bridge}"), content_width, false));
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
    cli_mode: bool,
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
        let bar = format!(
            "{}{}",
            "█".repeat(filled),
            "░".repeat(chart_width - filled)
        );
        let pct = ratio * 100.0;
        let label = format!("{:<8}", capitalize(level));
        let mut line = pad_line(&format!("{label} {bar} {pct:5.1}%"), width, false);

        if level == active_level && !cli_mode {
            // Highlight active level in green
            if let Some(fade_pos) = line.find('░') {
                line = format!(
                    "{}\x1b[1;32m{}\x1b[0m{}",
                    &line[..2],
                    &line[2..fade_pos],
                    &line[fade_pos..]
                );
            }
        }
        lines.push(line);
    }

    // CLI mode bar
    let cli_ratio = if original > 0 {
        stats.cli_size as f64 / original as f64
    } else {
        0.0
    };
    let filled = (cli_ratio * chart_width as f64).round() as usize;
    let filled = filled.min(chart_width);
    let bar = format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(chart_width - filled)
    );
    let pct = cli_ratio * 100.0;
    let mut line = pad_line(&format!("CLI mode {bar} {pct:5.1}%"), width, false);
    if cli_mode {
        if let Some(fade_pos) = line.find('░') {
            line = format!(
                "{}\x1b[1;32m{}\x1b[0m{}",
                &line[..2],
                &line[2..fade_pos],
                &line[fade_pos..]
            );
        }
    }
    lines.push(line);

    lines
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
