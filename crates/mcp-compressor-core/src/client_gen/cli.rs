//! `CliGenerator` — generates a Unix shell script and/or Windows `.cmd` file.
//!
//! The generated script:
//! - Has a `#!/usr/bin/env sh` shebang on Unix.
//! - Contains a `TOKEN` constant with the session bearer token.
//! - Contains a `BRIDGE` constant with the proxy URL.
//! - Has one `case` branch per upstream tool, each mapped to a kebab-case subcommand.
//! - Passes `Authorization: Bearer $TOKEN` on every `POST /exec` request.
//! - Is marked executable (Unix `chmod 755`).

use std::path::PathBuf;

use crate::client_gen::generator::{ClientGenerator, GeneratorConfig};
use crate::Error;

pub struct CliGenerator;

impl ClientGenerator for CliGenerator {
    fn generate(&self, config: &GeneratorConfig) -> Result<Vec<PathBuf>, Error> {
        todo!()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_gen::generator::test_helpers::{make_config, make_config_multiword_tool};
    use crate::client_gen::ClientGenerator;
    use std::fs;

    // ------------------------------------------------------------------
    // File creation
    // ------------------------------------------------------------------

    /// generate() returns at least one path.
    #[test]
    fn generate_returns_non_empty_paths() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        assert!(!paths.is_empty(), "expected at least one generated artifact");
    }

    /// All returned paths actually exist on disk.
    #[test]
    fn generated_paths_exist() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        for path in &paths {
            assert!(path.exists(), "path does not exist: {path:?}");
        }
    }

    /// All returned paths are inside the configured `output_dir`.
    #[test]
    fn generated_paths_inside_output_dir() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        for path in &paths {
            assert!(
                path.starts_with(dir.path()),
                "path {path:?} is outside output_dir {:?}",
                dir.path(),
            );
        }
    }

    // ------------------------------------------------------------------
    // Unix script content
    // ------------------------------------------------------------------

    /// On Unix the primary script has a shebang line.
    #[test]
    #[cfg(unix)]
    fn unix_script_has_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        assert!(content.starts_with("#!"), "script must start with shebang, got: {content:?}");
    }

    /// On Unix the primary script is named after `cli_name`.
    #[test]
    #[cfg(unix)]
    fn unix_script_named_after_cli_name() {
        use std::ffi::OsStr;
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        // The Unix script has no extension
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        assert_eq!(unix_script.file_name(), Some(OsStr::new("my-server")));
    }

    /// The script embeds the session token verbatim.
    #[test]
    #[cfg(unix)]
    fn unix_script_contains_token() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        assert!(content.contains(&config.token), "token not found in script");
    }

    /// The script embeds the bridge URL.
    #[test]
    #[cfg(unix)]
    fn unix_script_contains_bridge_url() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        assert!(content.contains(&config.bridge_url), "bridge URL not found in script");
    }

    /// Each upstream tool name appears as a subcommand in the script.
    #[test]
    #[cfg(unix)]
    fn unix_script_contains_all_tool_subcommands() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        // Tool names appear either as-is or in kebab-case
        assert!(content.contains("fetch"), "subcommand 'fetch' not found");
        assert!(content.contains("search"), "subcommand 'search' not found");
    }

    /// A multi-word tool name appears as its kebab-case subcommand.
    #[test]
    #[cfg(unix)]
    fn unix_script_kebab_case_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config_multiword_tool(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        // "get_confluence_page" → "get-confluence-page"
        assert!(
            content.contains("get-confluence-page"),
            "expected kebab-case subcommand in script",
        );
    }

    /// On Unix the generated script is executable.
    #[test]
    #[cfg(unix)]
    fn unix_script_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let perms = fs::metadata(unix_script).unwrap().permissions();
        assert_ne!(perms.mode() & 0o111, 0, "script must be executable");
    }

    // ------------------------------------------------------------------
    // Windows .cmd content
    // ------------------------------------------------------------------

    /// On Windows a `.cmd` file is generated.
    #[test]
    #[cfg(windows)]
    fn windows_cmd_generated() {
        use std::ffi::OsStr;
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        assert!(
            paths.iter().any(|p| p.extension() == Some(OsStr::new("cmd"))),
            "expected a .cmd file on Windows",
        );
    }

    /// The Windows `.cmd` file contains the session token.
    #[test]
    #[cfg(windows)]
    fn windows_cmd_contains_token() {
        use std::ffi::OsStr;
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let cmd = paths.iter().find(|p| p.extension() == Some(OsStr::new("cmd"))).unwrap();
        let content = fs::read_to_string(cmd).unwrap();
        assert!(content.contains(&config.token));
    }
}
