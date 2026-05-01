//! `CompressionLevel` enum — the four verbosity tiers exposed to callers.
//!
//! Matches the Python `CompressionLevel` enum in `mcp_compressor/types.py`.

use crate::Error;
use std::fmt;
use std::str::FromStr;

/// Verbosity level used when formatting tool listings.
///
/// Higher = less token output:
///
/// ```text
/// Low  ← most verbose                    Max ← least verbose
/// Low  > Medium  > High  > Max
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// Full schema and description (least compressed).
    Low,
    /// First sentence of each tool's description only.
    #[default]
    Medium,
    /// Tool name and argument names, no descriptions.
    High,
    /// Tool name only; a `list_tools` MCP tool is added to the frontend server.
    Max,
}

impl fmt::Display for CompressionLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl FromStr for CompressionLevel {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- FromStr / parsing ---

    /// Parsing "low" (lowercase) produces Low.
    #[test]
    fn parse_low() {
        assert_eq!("low".parse::<CompressionLevel>().unwrap(), CompressionLevel::Low);
    }

    /// Parsing "medium" produces Medium.
    #[test]
    fn parse_medium() {
        assert_eq!("medium".parse::<CompressionLevel>().unwrap(), CompressionLevel::Medium);
    }

    /// Parsing "high" produces High.
    #[test]
    fn parse_high() {
        assert_eq!("high".parse::<CompressionLevel>().unwrap(), CompressionLevel::High);
    }

    /// Parsing "max" produces Max.
    #[test]
    fn parse_max() {
        assert_eq!("max".parse::<CompressionLevel>().unwrap(), CompressionLevel::Max);
    }

    /// Parsing is case-insensitive: "LOW" and "High" are accepted.
    #[test]
    fn parse_case_insensitive() {
        assert_eq!("LOW".parse::<CompressionLevel>().unwrap(), CompressionLevel::Low);
        assert_eq!("MEDIUM".parse::<CompressionLevel>().unwrap(), CompressionLevel::Medium);
        assert_eq!("HIGH".parse::<CompressionLevel>().unwrap(), CompressionLevel::High);
        assert_eq!("MAX".parse::<CompressionLevel>().unwrap(), CompressionLevel::Max);
        assert_eq!("High".parse::<CompressionLevel>().unwrap(), CompressionLevel::High);
    }

    /// An unrecognised string produces an error.
    #[test]
    fn parse_invalid() {
        assert!("invalid".parse::<CompressionLevel>().is_err());
    }

    /// An empty string produces an error.
    #[test]
    fn parse_empty() {
        assert!("".parse::<CompressionLevel>().is_err());
    }

    // --- Default ---

    /// The default level is Medium (matches the Python default).
    #[test]
    fn default_is_medium() {
        assert_eq!(CompressionLevel::default(), CompressionLevel::Medium);
    }

    // --- Display ---

    /// `Display` serialises back to the canonical lowercase form.
    #[test]
    fn display_round_trips() {
        assert_eq!(CompressionLevel::Low.to_string(), "low");
        assert_eq!(CompressionLevel::Medium.to_string(), "medium");
        assert_eq!(CompressionLevel::High.to_string(), "high");
        assert_eq!(CompressionLevel::Max.to_string(), "max");
    }

    /// A level round-trips through `Display` → `FromStr`.
    #[test]
    fn display_then_parse_is_identity() {
        for level in [
            CompressionLevel::Low,
            CompressionLevel::Medium,
            CompressionLevel::High,
            CompressionLevel::Max,
        ] {
            let serialised = level.to_string();
            let parsed: CompressionLevel = serialised.parse().unwrap();
            assert_eq!(parsed, level);
        }
    }
}
