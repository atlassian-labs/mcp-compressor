//! `CliGenerator` -- generates a CLI client for the live session: a Unix shell
//! script and, on Windows, a `.cmd` file.
//!
//! Both run one shared Python program ([`client_python_body`]); only the launcher
//! and interpreter differ. The Unix script is a thin `sh` wrapper that pipes the
//! program to `python3`; the Windows `.cmd` is a file that works as both a batch
//! file and a Python program, and runs `python` on itself. The program:
//! - Contains a `BRIDGES` map with one or more live session bridges.
//! - Maps each kebab-case subcommand to its upstream tool.
//! - Picks the bridge for the current process tree when multiple sessions share a CLI name.
//! - Passes `Authorization: Bearer <session token>` on every `POST /exec` request.
//!
//! The Unix script is marked executable (`chmod 755`).

use std::net::ToSocketAddrs;

use crate::cli::help::{self, HelpFraming};
use crate::cli::mapping::tool_name_to_subcommand;
use crate::client_gen::generator::{
    CliBridgeEntry, ClientGenerator, GeneratedArtifact, GeneratorConfig,
};
use crate::Error;

pub struct CliGenerator;

impl ClientGenerator for CliGenerator {
    fn render(&self, config: &GeneratorConfig) -> Result<Vec<GeneratedArtifact>, Error> {
        Ok(render_cli_artifacts(config))
    }
}

#[cfg(not(windows))]
fn render_cli_artifacts(config: &GeneratorConfig) -> Vec<GeneratedArtifact> {
    vec![GeneratedArtifact::new(&config.cli_name, render_unix_script(config)).executable()]
}

#[cfg(windows)]
fn render_cli_artifacts(config: &GeneratorConfig) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact::new(&config.cli_name, render_unix_script(config)).executable(),
        GeneratedArtifact::new(
            format!("{}.cmd", config.cli_name),
            render_windows_cmd(config),
        ),
    ]
}

/// The Python program both clients run: resolve a live bridge, parse arguments
/// against the tool schema, and `POST` to `/exec` with the session token. The shell
/// orchestration moves into one `main()` so the Windows `.cmd` runs the same program;
/// the resolution and parsing helpers are reused from upstream unchanged.
///
/// The session bridge is resolved by one shared process-tree walk; only the per-step
/// parent-PID lookup is platform-specific. `parent_pid` dispatches by `os.name` to
/// `unix_parent_pid` (a `/proc` or `ps` read) or `windows_parent_pid` (a process
/// snapshot), keeping the walk one implementation with each platform's lookup in its
/// own leaf.
fn client_python_body(config: &GeneratorConfig) -> String {
    let top_help = serde_json::to_string(&help::render_top_level_help(
        &config.cli_name,
        &config.cli_name,
        &config.tools,
        &HelpFraming::shell(&config.cli_name),
    ))
    .expect("top-level help should serialize");
    let bridges = bridge_map_literal(config);
    let tool_schemas = tool_schema_map_literal(config);
    let mut subcommand_map = serde_json::Map::new();
    let mut subcommand_help_map = serde_json::Map::new();
    for tool in &config.tools {
        let subcommand = tool_name_to_subcommand(&tool.name);
        subcommand_map.insert(
            subcommand.clone(),
            serde_json::Value::String(tool.name.clone()),
        );
        subcommand_help_map.insert(
            subcommand,
            serde_json::Value::String(help::render_subcommand_help(&config.cli_name, tool)),
        );
    }
    let subcommands = serde_json::Value::Object(subcommand_map).to_string();
    let subcommand_help = serde_json::Value::Object(subcommand_help_map).to_string();
    let usage = serde_json::to_string(&format!(
        "Usage: {} <subcommand> [args...]",
        config.cli_name
    ))
    .expect("usage should serialize");
    format!(
        r#"import base64, json, os, sys, urllib.error, urllib.request

TOP_HELP = {top_help}

BRIDGES = {bridges}

TOOL_SCHEMAS = json.loads(base64.b64decode({tool_schemas:?}).decode("utf-8"))

SUBCOMMANDS = {subcommands}

SUBCOMMAND_HELP = {subcommand_help}

properties = {{}}

def alive(entry):
    try:
        req = urllib.request.Request(entry["bridge"] + "/health", method="GET")
        with urllib.request.urlopen(req, timeout=1) as resp:
            return resp.status == 200
    except Exception:
        return False

def windows_parent_pid(pid):
    # Read the parent PID from a process snapshot. ctypes defaults kernel32 calls
    # to a 32-bit int return, which truncates the pointer-sized snapshot handle on
    # 64-bit Python and makes the lookup fail, so declare the real arg/return types.
    import ctypes
    from ctypes import wintypes
    TH32CS_SNAPPROCESS = 0x2
    INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
    class ProcessEntry(ctypes.Structure):
        _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD),
                    ("th32ProcessID", wintypes.DWORD),
                    ("th32DefaultHeapID", ctypes.c_void_p),
                    ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
                    ("th32ParentProcessID", wintypes.DWORD),
                    ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD),
                    ("szExeFile", ctypes.c_char * 260)]
    kernel32 = ctypes.windll.kernel32
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel32.Process32First.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel32.Process32Next.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    # The documented failure return is INVALID_HANDLE_VALUE, not NULL; guard both.
    if snapshot is None or snapshot == INVALID_HANDLE_VALUE:
        return 0
    try:
        entry = ProcessEntry()
        entry.dwSize = ctypes.sizeof(ProcessEntry)
        ok = kernel32.Process32First(snapshot, ctypes.byref(entry))
        while ok:
            if entry.th32ProcessID == pid:
                return entry.th32ParentProcessID
            ok = kernel32.Process32Next(snapshot, ctypes.byref(entry))
        return 0
    finally:
        kernel32.CloseHandle(snapshot)

def unix_parent_pid(pid):
    try:  # Linux: read the parent PID from /proc.
        with open(f"/proc/{{pid}}/stat", "r", encoding="utf-8") as handle:
            return int(handle.read().rsplit(")", 1)[1].split()[1])
    except Exception:
        pass
    try:  # Other Unix: ask ps.
        import subprocess
        output = subprocess.check_output(["ps", "-o", "ppid=", "-p", str(pid)], text=True)
        return int(output.strip() or "0")
    except Exception:
        return 0

def parent_pid(pid):
    # The walk is shared; this per-OS parent lookup is the only platform-specific
    # part. Dispatch by os.name: Windows has no /proc or ps, and a stray ps on PATH
    # (e.g. Git for Windows) must not be consulted there.
    try:
        return windows_parent_pid(pid) if os.name == "nt" else unix_parent_pid(pid)
    except Exception:
        return 0

def ancestors():
    pid = os.getppid()
    seen = set()
    while pid and pid not in seen:
        seen.add(pid)
        yield str(pid)
        pid = parent_pid(pid)

def find_bridge():
    for pid in ancestors():
        entry = BRIDGES.get(pid)
        if entry and alive(entry):
            return entry
    # Last resort: no ancestor PID matched a known bridge (e.g. the session that
    # launched this client has exited). Return any live bridge so a lone session
    # still works.
    for entry in BRIDGES.values():
        if alive(entry):
            return entry
    return None

def schema_type(schema):
    return schema.get("type") if isinstance(schema, dict) else None

def enum_values(schema):
    if not isinstance(schema, dict):
        return []
    explicit = schema.get("enum")
    if isinstance(explicit, list):
        return [str(value) for value in explicit]
    return []

def canonical_name(value):
    return value.replace("-", "_").replace("_", "").lower()

def flag_to_property(flag):
    raw = flag[2:]
    if raw.startswith("no-"):
        raw = raw[3:]
    if raw in properties:
        return raw
    snake = raw.replace("-", "_")
    if snake in properties:
        return snake
    canonical = canonical_name(raw)
    for prop in properties:
        if canonical_name(prop) == canonical:
            return prop
    return raw

def coerce_value(flag, schema, raw_value, forced_bool=None):
    if forced_bool is not None:
        return forced_bool
    typ = schema_type(schema)
    if typ == "boolean":
        if raw_value is None:
            return True
        if raw_value == "true":
            return True
        if raw_value == "false":
            return False
        raise SystemExit(f"invalid boolean value for {{flag}}: {{raw_value}} (expected true or false)")
    if typ == "integer":
        try:
            return int(raw_value)
        except Exception:
            raise SystemExit(f"invalid integer value for {{flag}}: {{raw_value}}")
    if typ == "number":
        try:
            return float(raw_value)
        except Exception:
            raise SystemExit(f"invalid number value for {{flag}}: {{raw_value}}")
    if typ == "array":
        try:
            parsed = json.loads(raw_value)
            if isinstance(parsed, list):
                return parsed
        except Exception:
            pass
        return coerce_value(flag, schema.get("items", {{}}), raw_value)
    try:
        return json.loads(raw_value or "")
    except Exception:
        return raw_value or ""

def validate_value(flag, schema, value):
    allowed = enum_values(schema)
    if not allowed:
        return
    values = value if isinstance(value, list) else [value]
    for candidate in values:
        if str(candidate) not in allowed:
            raise SystemExit(
                f"invalid value for {{flag}}: {{candidate}} (expected one of: {{', '.join(allowed)}})"
            )

def insert_value(output, key, schema, value):
    if schema_type(schema) == "array":
        values = value if isinstance(value, list) else [value]
        output.setdefault(key, []).extend(values)
    else:
        output[key] = value

def parse_args(argv):
    if argv and argv[0] == "--json":
        if len(argv) < 2:
            raise SystemExit("--json requires a value")
        if len(argv) > 2:
            raise SystemExit("--json cannot be combined with other arguments")
        return json.loads(argv[1])
    output = {{}}
    index = 0
    while index < len(argv):
        flag = argv[index]
        if not flag.startswith("--") or flag == "--":
            raise SystemExit(f"unexpected positional argument: {{flag}}")
        prop = flag_to_property(flag)
        if prop not in properties:
            raise SystemExit(f"unknown flag: {{flag}}")
        schema = properties[prop]
        typ = schema_type(schema)
        forced_bool = False if flag.startswith("--no-") else None
        if forced_bool is False:
            if typ != "boolean":
                raise SystemExit(f"{{flag}} can only be used with boolean properties")
            raw_value = None
            consumed = 1
        elif typ == "boolean":
            if index + 1 < len(argv) and not argv[index + 1].startswith("--"):
                raw_value = argv[index + 1]
                consumed = 2
            else:
                raw_value = None
                consumed = 1
        else:
            if index + 1 >= len(argv) or argv[index + 1].startswith("--"):
                raise SystemExit(f"{{flag}} requires a value")
            raw_value = argv[index + 1]
            consumed = 2
        value = coerce_value(flag, schema, raw_value, forced_bool)
        validate_value(flag, schema, value)
        insert_value(output, prop, schema, value)
        index += consumed
    return output

def unwrap_proxy_response(body):
    try:
        parsed = json.loads(body)
    except Exception:
        return body
    if isinstance(parsed, dict) and set(parsed.keys()) == {{"result"}}:
        result = parsed["result"]
        return result if isinstance(result, str) else json.dumps(result, separators=(",", ":"))
    return body

def main():
    global properties
    argv = sys.argv[1:]
    if not argv or argv[0] in ("--help", "-h", "help"):
        print(TOP_HELP)
        return 0
    subcommand = argv[0]
    if subcommand not in SUBCOMMANDS:
        print({usage}, file=sys.stderr)
        return 2
    rest = argv[1:]
    if rest and rest[0] in ("--help", "-h", "help"):
        print(SUBCOMMAND_HELP[subcommand])
        return 0
    tool_name = SUBCOMMANDS[subcommand]
    entry = find_bridge()
    if entry is None:
        print("mcp-compressor proxy is not running; restart the mcp-compressor CLI-mode process and try again.", file=sys.stderr)
        return 1
    bridge = entry["bridge"]
    token = entry["token"]
    tool_schema = TOOL_SCHEMAS[tool_name]
    properties = tool_schema.get("inputSchema") or tool_schema.get("input_schema") or {{}}
    properties = properties.get("properties", {{}}) if isinstance(properties, dict) else {{}}
    tool_input = parse_args(rest)
    payload = json.dumps({{"tool": tool_name, "input": tool_input}}).encode()
    req = urllib.request.Request(
        bridge + "/exec",
        data=payload,
        headers={{"Content-Type": "application/json", "Authorization": "Bearer " + token}},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            sys.stdout.write(unwrap_proxy_response(resp.read().decode()))
    except urllib.error.HTTPError as exc:
        message = exc.read().decode(errors="replace") or exc.reason
        print(f"mcp-compressor proxy returned HTTP {{exc.code}}: {{message}}", file=sys.stderr)
        return 1
    except urllib.error.URLError as exc:
        print(
            "mcp-compressor proxy is not running; restart the mcp-compressor CLI-mode process and try again.",
            file=sys.stderr,
        )
        print(f"details: {{exc.reason}}", file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
"#,
        top_help = top_help,
        bridges = bridges,
        tool_schemas = tool_schemas,
        subcommands = subcommands,
        subcommand_help = subcommand_help,
        usage = usage,
    )
}

/// Unix client: a thin `sh` wrapper that runs the shared program through `python3`.
fn render_unix_script(config: &GeneratorConfig) -> String {
    format!(
        "#!/usr/bin/env sh\n# generated by mcp-compressor -- do not edit manually\nexec python3 - \"$@\" <<'PY'\n{body}\nPY\n",
        body = client_python_body(config),
    )
}

/// Windows `.cmd` client: the same shared program in a file that works as both a
/// Windows batch file and a Python program. Run by Windows, the batch lines at the
/// top start `python` on the file itself and then exit with Python's exit code. Run
/// by Python, the first line (`@echo off`) is skipped by `python -x`, and the batch
/// launcher lines sit inside a Python string, so Python ignores them and runs the
/// shared program. Uses `python`, since Windows usually has no `python3`.
#[cfg(windows)]
fn render_windows_cmd(config: &GeneratorConfig) -> String {
    format!(
        "@echo off\nREM = r'''\npython -x \"%~f0\" %*\nexit /b %errorlevel%\n'''\n{body}",
        body = client_python_body(config),
    )
}

pub fn read_live_bridge_entries(script: &str) -> Vec<CliBridgeEntry> {
    let Some(line) = script
        .lines()
        .find(|line| line.trim_start().starts_with("BRIDGES = "))
    else {
        return Vec::new();
    };
    let Some(json) = line.split_once('=').map(|(_, value)| value.trim()) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(pid, entry)| {
            let session_pid = pid.parse().ok()?;
            Some(CliBridgeEntry {
                session_pid,
                bridge_url: entry.get("bridge")?.as_str()?.to_string(),
                token: entry.get("token")?.as_str()?.to_string(),
            })
        })
        .filter(|entry| bridge_is_live(&entry.bridge_url))
        .collect()
}

fn tool_schema_map_literal(config: &GeneratorConfig) -> String {
    let map = config
        .tools
        .iter()
        .map(|tool| (tool.name.clone(), tool.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let json = serde_json::to_string(&map).expect("tool schema map should serialize");
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, json)
}

fn bridge_map_literal(config: &GeneratorConfig) -> String {
    let mut entries = config.extra_cli_bridges.clone();
    entries.retain(|entry| entry.session_pid != config.session_pid);
    entries.push(CliBridgeEntry {
        session_pid: config.session_pid,
        bridge_url: config.bridge_url.clone(),
        token: config.token.clone(),
    });
    let mut map = serde_json::Map::new();
    for entry in entries {
        map.insert(
            entry.session_pid.to_string(),
            serde_json::json!({ "bridge": entry.bridge_url, "token": entry.token }),
        );
    }
    serde_json::Value::Object(map).to_string()
}

fn bridge_is_live(bridge_url: &str) -> bool {
    let Some(address) = bridge_url.strip_prefix("http://") else {
        return false;
    };
    let Some(host_port) = address.split('/').next() else {
        return false;
    };
    host_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .is_some_and(|address| {
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(200))
                .is_ok()
        })
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
        assert!(
            !paths.is_empty(),
            "expected at least one generated artifact"
        );
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
        assert!(
            content.starts_with("#!"),
            "script must start with shebang, got: {content:?}"
        );
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
        assert!(
            content.contains(&config.bridge_url),
            "bridge URL not found in script"
        );
    }

    #[test]
    #[cfg(unix)]
    fn unix_script_contains_multi_session_bridge_map() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = make_config(dir.path());
        config.extra_cli_bridges = vec![CliBridgeEntry {
            session_pid: 111,
            bridge_url: "http://127.0.0.1:1".to_string(),
            token: "old-token".to_string(),
        }];
        let artifacts = CliGenerator.render(&config).unwrap();
        let content = &artifacts[0].contents;
        assert!(content.contains("BRIDGES = "));
        assert!(content.contains("\"111\""));
        assert!(content.contains("old-token"));
        assert!(content.contains(&format!("\"{}\"", config.session_pid)));
        assert!(content.contains(&config.token));
        // Guard that the process-tree resolver is still emitted (cheap, fast
        // failure if the shared body loses it; behavior is covered by the e2e test).
        assert!(content.contains("def ancestors():"));
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

    /// Top-level help includes legacy-style title, usage, and subcommands.
    #[test]
    #[cfg(unix)]
    fn unix_script_contains_top_level_help() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        assert!(content.contains("my-server - the my-server toolset"));
        assert!(content.contains("USAGE:"));
        assert!(content.contains("SUBCOMMANDS:"));
        assert!(content.contains("Run 'my-server <subcommand> --help'"));
    }

    /// Subcommand help includes usage and option names.
    #[test]
    #[cfg(unix)]
    fn unix_script_contains_subcommand_help() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        assert!(content.contains("my-server fetch"));
        assert!(content.contains("OPTIONS:"));
        assert!(content.contains("--url <string>"));
        assert!(content.contains("--url"));
        assert!(content.contains("--url <string>"));
        assert!(content.contains("Required."));
        assert!(content.contains("URL to fetch."));
        assert!(content.contains("--timeout"));
        assert!(content.contains("--timeout <integer>"));
        assert!(content.contains("Timeout in seconds."));
        assert!(content.contains("Default: 30."));
        assert!(content.contains("--method"));
        assert!(content.contains("Allowed values: GET, POST."));
    }

    /// Subcommand help no longer emits the redundant OUTPUT section.
    #[test]
    #[cfg(unix)]
    fn unix_script_subcommand_help_has_no_output_section() {
        let dir = tempfile::tempdir().unwrap();
        let config = make_config(dir.path());
        let paths = CliGenerator.generate(&config).unwrap();
        let unix_script = paths.iter().find(|p| p.extension().is_none()).unwrap();
        let content = fs::read_to_string(unix_script).unwrap();
        assert!(!content.contains("OUTPUT:"));
        assert!(!content.contains("Prints the upstream tool result directly."));
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
            paths
                .iter()
                .any(|p| p.extension() == Some(OsStr::new("cmd"))),
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
        let cmd = paths
            .iter()
            .find(|p| p.extension() == Some(OsStr::new("cmd")))
            .unwrap();
        let content = fs::read_to_string(cmd).unwrap();
        assert!(content.contains(&config.token));
    }
}
