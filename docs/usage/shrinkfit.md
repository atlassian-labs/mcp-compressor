# Recoverable response shrinking

Use `--shrinkfit` to condense oversized tool responses in **CLI Mode** or **Just Bash mode**. It defaults to **20,000 Unicode characters**; set a different budget with `--shrinkfit=10000` (minimum 1,024).

```bash
mcp-compressor --cli-mode --server-name atlassian --shrinkfit -- \
  'https://mcp.atlassian.com/v2/mcp?tools=all'

mcp-compressor --just-bash-mode --shrinkfit=10000 -- python server.py
```

This option is off by default and cannot be combined with `--toonify`. Standard compressed MCP pass-through is unchanged; use a CLI/Just Bash transform to enable shrinking. Code Mode uses the CLI transport and also supports it.

## Output and recovery

Small successful responses retain their existing output exactly. For a supported response over the budget, the proxy:

1. Writes the **complete original MCP `CallToolResult`**, including all text/typed blocks, `structuredContent` and metadata, to a randomly named private temporary JSON file.
2. Uses shrinkfit's **condense** array strategy to produce a preview. JSON is parsed strictly; non-JSON text is treated as a string, not guessed to be YAML. Mixed/binary content without structured content bypasses shrinking.
3. Returns a JSON envelope containing `preview` and `recovery`. The character budget includes that entire envelope (not the shell's trailing newline or any outer protocol encoding).

```json
{
  "preview": {"items": [{"id": "example", "title": "Short preview"}]},
  "recovery": {
    "original_response_file": "/tmp/mcp-compressor-response-EXAMPLE.json",
    "original_format": "mcp-call-tool-result",
    "max_chars": 20000,
    "fits": true,
    "pruned": true,
    "notice": "Lossy preview; read original_response_file for the complete MCP result. Delete that private file when finished."
  }
}
```

Always check for `recovery` before assuming the original tool schema. `condense` can drop fields, shorten strings, or fall back to sampling. A preview is **not an exhaustive or schema-preserving result**. The original is saved even if shrinking only changes formatting, so nothing from the original result is lost.

For a generated command:

```bash
atlassian search-jira-issues-using-jql --json '{"cloudId":"YOUR_CLOUD_ID","jql":"assignee = currentUser()","maxResults":100}' > response.json
original=$(jq -r '.recovery.original_response_file // empty' response.json)
# When recovery is present, inspect structured content or the complete text blocks:
if [ -n "$original" ]; then
  jq '.structuredContent // .content' "$original"
fi
```

Files are on the **proxy host**, not necessarily the agent host or a Just Bash virtual filesystem. Give the consuming agent a filesystem read capability or explicitly mount the recovery location into its sandbox. This option does not grant Just Bash access to the host filesystem or expose a remote arbitrary-file endpoint. Never assume `cat` inside an isolated virtual filesystem can see a host path.

## SDK sessions

The same native implementation is available to generated-client and Just Bash hosts:

```typescript
const client = new CompressorClient({
  servers: { atlassian: "https://mcp.atlassian.com/v2/mcp?tools=all" },
  mode: "bash",
  shrinkfit: 20000,
});
```

```python
client = CompressorClient(
    servers={"atlassian": "https://mcp.atlassian.com/v2/mcp?tools=all"},
    mode="bash",
    shrinkfit=20000,
)
```

The Rust client builder exposes `.shrinkfit(20000)` with a CLI or Just Bash mode. These settings control native MCP sessions; they do not change local-tool transformation helpers.

## Failure behavior and retention

- Backend errors retain their existing nonzero/error semantics and are not shrunk or persisted. Help and schema listings are not shrunk.
- If recovery storage fails or the envelope cannot fit, the call fails rather than returning an unrecoverable preview. A failed write/fit attempt cleans up its temporary file. The backend tool may already have run: **do not automatically retry writes** because output processing failed.
- Pruning and file I/O run in a bounded blocking pool (four concurrent jobs), not on async executor workers.
- Recovery files use unpredictable names, exclusive creation and owner-only permissions on Unix. They survive proxy shutdown and are **not automatically deleted**; remove them when finished, or apply an OS/temp-directory retention policy. They may contain sensitive tenant data. Do not publish them or point the process temp directory at an untrusted/shared location.
- The limit is characters, not bytes or model tokens. Media bypasses and errors are not covered by the preview size limit. Large upstream responses are still downloaded and materialized before pruning.
