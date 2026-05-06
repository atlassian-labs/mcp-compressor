use std::path::PathBuf;

/// Select where generated CLI scripts should be written.
///
/// Returns `(directory, on_path)` where `on_path` indicates whether the
/// directory is already on the user's `PATH`.
pub fn cli_output_dir() -> std::io::Result<(PathBuf, bool)> {
    if let Some(path) = std::env::var_os("MCP_COMPRESSOR_CLI_OUTPUT_DIR") {
        return Ok((PathBuf::from(path), true));
    }

    let path_dirs = path_dirs();
    for candidate in candidate_script_dirs() {
        let resolved = candidate.canonicalize().unwrap_or(candidate.clone());
        if resolved.is_dir() && path_dirs.iter().any(|path_dir| path_dir == &resolved) {
            return Ok((resolved, true));
        }
    }

    Ok((std::env::current_dir()?, false))
}

fn candidate_script_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if cfg!(windows) {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("WindowsApps"),
            );
        }
        if let Some(home) = &home {
            candidates.push(home.join(".local").join("bin"));
        }
    } else {
        if let Some(home) = &home {
            candidates.push(home.join(".local").join("bin"));
            candidates.push(home.join("bin"));
        }
        candidates.push(PathBuf::from("/usr/local/bin"));
        candidates.push(PathBuf::from("/opt/homebrew/bin"));
    }
    candidates
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|entry| entry.canonicalize().unwrap_or(entry))
                .collect()
        })
        .unwrap_or_default()
}
