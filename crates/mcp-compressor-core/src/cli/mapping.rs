//! CLI name-mapping utilities.
//!
//! Mirrors `mcp_compressor/cli_tools.py`:
//! - [`tool_name_to_subcommand`] — converts MCP tool names to kebab-case CLI subcommands
//! - [`sanitize_cli_name`] — sanitizes arbitrary strings into safe CLI command names

/// Convert a `snake_case` or `camelCase` MCP tool name to a `kebab-case` CLI subcommand.
///
/// # Rules
///
/// 1. Insert a hyphen before each uppercase-to-lowercase camelCase transition.
/// 2. Replace all underscores with hyphens.
/// 3. Lowercase the entire result.
///
/// # Examples
///
/// | Input | Output |
/// |---|---|
/// | `get_confluence_page` | `get-confluence-page` |
/// | `getConfluencePage` | `get-confluence-page` |
/// | `createJiraIssue` | `create-jira-issue` |
/// | `fetch` | `fetch` |
/// | `getjiraissue` | `getjiraissue` |
pub fn tool_name_to_subcommand(tool_name: &str) -> String {
    let mut out = String::new();
    let mut previous_was_lower_or_digit = false;

    for ch in tool_name.chars() {
        if ch == '_' {
            out.push('-');
            previous_was_lower_or_digit = false;
        } else if ch.is_ascii_uppercase() {
            if previous_was_lower_or_digit {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
            previous_was_lower_or_digit = false;
        } else {
            out.push(ch.to_ascii_lowercase());
            previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }

    out
}

/// Convert a kebab-case CLI subcommand back to a `snake_case` MCP tool name.
pub fn subcommand_to_tool_name(subcommand: &str) -> String {
    subcommand.replace('-', "_")
}

/// Sanitize an arbitrary string into a safe CLI command / script name.
///
/// # Rules (applied in order)
///
/// 1. Lowercase the entire string.
/// 2. Replace every character not in `[a-z0-9_-]` with `-`.
/// 3. Collapse consecutive `[-_]` sequences into a single `-`.
/// 4. Strip leading and trailing `-` and `_`.
/// 5. If the result is empty, use `"mcp"`.
/// 6. If the result starts with a digit, prepend `"mcp-"`.
///
/// # Examples
///
/// | Input | Output |
/// |---|---|
/// | `"My Server!"` | `"my-server"` |
/// | `"atlassian-labs"` | `"atlassian-labs"` |
/// | `"  spaces  "` | `"spaces"` |
/// | `""` | `"mcp"` |
/// | `"123abc"` | `"mcp-123abc"` |
/// | `"multi  spaces"` | `"multi-spaces"` |
pub fn sanitize_cli_name(name: &str) -> String {
    let mut out = String::new();
    let mut pending_separator: Option<char> = None;

    for ch in name.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if let Some(separator) = pending_separator.take() {
                if !out.is_empty() {
                    out.push(separator);
                }
            }
            out.push(ch);
        } else if ch == '_' || ch == '-' {
            pending_separator = Some(match pending_separator {
                Some(_) => '-',
                None => ch,
            });
        } else {
            pending_separator = Some(match pending_separator {
                Some(_) => '-',
                None => '-',
            });
        }
    }

    let mut sanitized = out.trim_matches(['-', '_']).to_string();
    if sanitized.is_empty() {
        sanitized = "mcp".to_string();
    }
    if sanitized.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        sanitized = format!("mcp-{sanitized}");
    }
    sanitized
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // tool_name_to_subcommand
    // ------------------------------------------------------------------

    /// snake_case → kebab-case (basic case, most common input).
    #[test]
    fn subcommand_snake_to_kebab() {
        assert_eq!(tool_name_to_subcommand("get_confluence_page"), "get-confluence-page");
    }

    /// A single-word tool name is unchanged.
    #[test]
    fn subcommand_single_word() {
        assert_eq!(tool_name_to_subcommand("fetch"), "fetch");
    }

    /// Two-word snake_case name.
    #[test]
    fn subcommand_two_word_snake() {
        assert_eq!(tool_name_to_subcommand("list_resources"), "list-resources");
    }

    /// Trailing version numbers in snake_case are preserved.
    #[test]
    fn subcommand_snake_with_version() {
        assert_eq!(tool_name_to_subcommand("my_tool_v2"), "my-tool-v2");
    }

    /// camelCase → kebab-case (two-word).
    #[test]
    fn subcommand_camel_two_word() {
        assert_eq!(tool_name_to_subcommand("getConfluencePage"), "get-confluence-page");
    }

    /// camelCase → kebab-case (three-word with acronym-like capitalisation).
    #[test]
    fn subcommand_camel_three_word() {
        assert_eq!(tool_name_to_subcommand("createJiraIssue"), "create-jira-issue");
    }

    /// An all-lowercase string with no separators is returned unchanged.
    /// (No camelCase transitions → no splits.)
    #[test]
    fn subcommand_all_lowercase_no_splits() {
        assert_eq!(tool_name_to_subcommand("getjiraissue"), "getjiraissue");
    }

    /// An already-lowercase tool name with a trailing number is left intact.
    #[test]
    fn subcommand_snake_trailing_number() {
        assert_eq!(tool_name_to_subcommand("list_resources_v2"), "list-resources-v2");
    }

    // ------------------------------------------------------------------
    // subcommand_to_tool_name (inverse)
    // ------------------------------------------------------------------

    /// kebab-case → snake_case round-trip.
    #[test]
    fn inverse_kebab_to_snake() {
        assert_eq!(subcommand_to_tool_name("get-confluence-page"), "get_confluence_page");
    }

    /// Single-word round-trip.
    #[test]
    fn inverse_single_word() {
        assert_eq!(subcommand_to_tool_name("fetch"), "fetch");
    }

    // ------------------------------------------------------------------
    // sanitize_cli_name
    // ------------------------------------------------------------------

    /// Spaces and special characters are replaced with hyphens.
    #[test]
    fn sanitize_spaces_and_special_chars() {
        assert_eq!(sanitize_cli_name("My Server!"), "my-server");
    }

    /// Already-valid hyphen-separated names are unchanged.
    #[test]
    fn sanitize_already_valid() {
        assert_eq!(sanitize_cli_name("atlassian-labs"), "atlassian-labs");
    }

    /// Leading and trailing whitespace is stripped (becomes hyphens, then stripped).
    #[test]
    fn sanitize_leading_trailing_spaces() {
        assert_eq!(sanitize_cli_name("  spaces  "), "spaces");
    }

    /// An empty string yields the fallback "mcp".
    #[test]
    fn sanitize_empty_yields_mcp() {
        assert_eq!(sanitize_cli_name(""), "mcp");
    }

    /// A string composed entirely of invalid characters yields "mcp".
    #[test]
    fn sanitize_all_invalid_yields_mcp() {
        assert_eq!(sanitize_cli_name("!!!"), "mcp");
    }

    /// A name starting with a digit gets the "mcp-" prefix.
    #[test]
    fn sanitize_digit_start_gets_prefix() {
        assert_eq!(sanitize_cli_name("123abc"), "mcp-123abc");
    }

    /// Multiple consecutive spaces collapse to a single hyphen.
    #[test]
    fn sanitize_multiple_spaces_collapse() {
        assert_eq!(sanitize_cli_name("multi  spaces"), "multi-spaces");
    }

    /// A single underscore is preserved as an underscore.
    #[test]
    fn sanitize_single_underscore_preserved() {
        assert_eq!(sanitize_cli_name("hello_world"), "hello_world");
    }

    /// Consecutive underscores collapse to a single hyphen.
    #[test]
    fn sanitize_consecutive_underscores_collapse() {
        assert_eq!(sanitize_cli_name("hello__world"), "hello-world");
    }

    /// Mixed consecutive separators (underscore + hyphen) collapse.
    #[test]
    fn sanitize_mixed_consecutive_separators() {
        assert_eq!(sanitize_cli_name("hello_-world"), "hello-world");
    }

    /// Upper-case letters are lowercased.
    #[test]
    fn sanitize_uppercase_lowercased() {
        assert_eq!(sanitize_cli_name("Hello_World"), "hello_world");
    }
}
