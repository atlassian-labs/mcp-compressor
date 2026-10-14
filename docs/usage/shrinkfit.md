# Recoverable response shrinking

Enable `--shrinkfit` in **any MCP proxy mode**: standard compressed MCP, CLI, Code Mode, or Just Bash. It defaults to **20,000 Unicode characters**; set `--shrinkfit=10000` for another limit (minimum 1,024). It is off by default.

```bash
# MCP clients recover by calling the invocation wrapper again with full=true.
mcp-compressor --shrinkfit -- 'https://mcp.atlassian.com/v2/mcp?tools=all'

# CLI/Code/Just Bash clients recover through a local file.
mcp-compressor --cli-mode --server-name atlassian --shrinkfit -- \
  'https://mcp.atlassian.com/v2/mcp?tools=all'
```

The shared native implementation applies to stdio and streamable-HTTP frontends, generated clients, and SDK sessions. It does not change the TypeScript-only local-function `compressTools(...)` helpers. Shrinkfit takes precedence over `--toonify` for tool results while enabled, so recovery metadata stays JSON. Tool listings/help can still use their configured formats.

## Existing object shape plus `truncation`

Object results keep their existing top-level fields. There is **no `preview` wrapper**. Fields inside the object may be removed or shortened by shrinkfit's `condense` strategy.

```json
{
  "data": {
    "issues": [{"key": "PROJ-123", "summary": "Fix authentication"}],
    "nextPageToken": "opaque-token"
  },
  "truncation": {
    "message": "This tool response has been automatically shrunk to limit token consumption. An attempt has been made to retain important information. If the fully hydrated response is required, it can be read from /tmp/mcp-compressor-response-a7b3c9.json",
    "report": {
      "pruned": true,
      "fits": true,
      "items_omitted": 0,
      "keys_omitted": 4,
      "strings_truncated": 2
    },
    "original_response_file": "/tmp/mcp-compressor-response-a7b3c9.json"
  }
}
```

The small report consists of shrinkfit's actual counters, not model-generated claims. The unbounded per-field path list is intentionally omitted so report metadata cannot overwhelm the budget. The budget includes the output and truncation instructions. Characters are not bytes or tokens.

Small outputs and backend errors remain unchanged. JSON is parsed strictly; malformed JSON/prose is treated as text, not guessed to be YAML. Arrays/scalars cannot accept a sibling key: their value type is retained, with recovery in MCP `_meta.truncation`. Standard MCP additionally gets a text notice for clients that don't display `_meta`; string transports serialize the MCP result envelope for these cases. A backend-owned `truncation` field is never overwritten; recovery uses the same metadata fallback. Media without structured content bypasses condensation.

If `structuredContent` exists, it and the primary text output are updated consistently. The complete original envelope is retained for recovery. Condensed output is explicitly lossy and is **not guaranteed to satisfy the backend's original output schema** or preserve every identity.

## CLI, Code Mode and Just Bash: read the original file

When shrinking is triggered in filesystem-oriented modes, a complete original MCP `CallToolResult` is written to a randomly named private JSON temp file. This includes typed content blocks, structured content and metadata—not only the flattened response body. Small outputs/errors do not create files.

Read `truncation.original_response_file` (or `_meta.truncation.original_response_file` for a non-object payload). The message also contains the path so an agent can recover without special parsing logic.

```bash
original=$(jq -r '.truncation.original_response_file // ._meta.truncation.original_response_file // empty' response.json)
if [ -n "$original" ]; then
  jq '.structuredContent // .content' "$original"
fi
```

Files reside on the **proxy host**, survive shutdown, and require explicit cleanup or OS retention policy. Unix permissions are 0600. They may contain sensitive data. Sandboxed Just Bash hosts need an explicit mount or host read capability; this option does not grant arbitrary filesystem access or introduce a remote file-read API.

## Non-CLI MCP: recall with `full`

With shrinking enabled, the compressor's `*_invoke_tool` schema adds an optional boolean **`full`**, default false. Its description discourages use unless the condensed response lacks information needed for the task. The argument belongs to the compressor wrapper, **outside `tool_input`**:

```json
{
  "tool_name": "searchJiraIssuesUsingJql",
  "tool_input": {"cloudId": "YOUR_CLOUD_ID", "jql": "assignee = currentUser()", "maxResults": 100},
  "full": true
}
```

The `truncation.message` explains this recovery route instead of referring to a filesystem path. Keep the same tool name, arguments and request context:

- A cache hit returns the complete original MCP result **without calling the backend again** and without shrinkfit/TOON transformation.
- A cache miss executes the backend with the original arguments and **bypasses shrinking for that call**, including a first-ever `full=true` call.
- Ordinary `full=false` invocations always call the backend. This is a recovery cache, not general request memoization.
- A tool's own `full` parameter remains inside `tool_input` and is forwarded unchanged. The compressor boolean is never forwarded.
- Invalid non-boolean `full` values fail validation.

**Cache misses can repeat side effects.** Only request full when needed; do not retry a write unless repeating it is safe. There is no guarantee a cached entry will remain available.

### Cache limits and isolation

The cache is in-memory per compressor session, with a five-minute TTL, at most 32 entries and 32 MiB of serialized results/keys. Keys include backend, tool, canonical arguments and effective request metadata. Oversized entries are not retained. Changing relevant metadata or arguments misses the cache. Results expire/evict and are cleared on shutdown.

Sessions with dynamic credential refresh disable cached recovery before the first refresh hook runs, preventing reuse across a potentially changed identity (including concurrent requests). Their full=true requests always re-execute without shrinking. Session users must not share credentials/proxy tokens across unrelated principals; this cache does not introduce a multi-tenant identity system.

## SDKs

Python:

```python
client = CompressorClient(servers={"atlassian": "https://mcp.atlassian.com/v2/mcp?tools=all"}, shrinkfit=20000)
# proxy obtained from the client context/session:
preview = proxy.invoke("some_tool", args)
original = proxy.invoke("some_tool", args, full=True)  # only when necessary
```

TypeScript:

```typescript
const client = new CompressorClient({
  servers: { atlassian: "https://mcp.atlassian.com/v2/mcp?tools=all" },
  shrinkfit: 20000,
});
const proxy = await client.connect();
const preview = await proxy.invoke("some_tool", args);
const original = await proxy.invoke("some_tool", args, { full: true });
```

Rust client builders support `.shrinkfit(20000)`; the proxy exposes `invoke_with_full(server, tool, input, true)`. CLI/Just Bash settings still choose filesystem recovery; normal MCP mode chooses cached recall.

## Failure and performance

Backend errors retain their existing semantics and are not shrunk/cached. File-write or impossible-budget failures fail rather than returning an unrecoverable preview; temporary unsuccessful files are removed. A backend write may have already succeeded before output processing fails—do not retry blindly.

Pruning and file I/O run in a bounded blocking pool (four jobs) rather than async executor workers. Upstream results are still fully fetched and materialized. Cache bounds count serialized bytes, not exact allocator overhead. Large media bypasses and errors are not subject to the preview size cap. Full responses intentionally bypass that cap.

## Host-owned TypeScript tools

Alta and other hosts that own their MCP connections can opt in without moving
connection management into the compressor:

```typescript
const transformed = await transformToolsForCliMode(tools, {
  serverName: "inventory",
  shrinkfit: 6000,
});
```

`shrinkfit` is also accepted by `compressTools`, `planToolsForJustBash`,
`transformToolsForJustBash`, `transformToolsForCodeMode`, and
`createExecutableToolBridge`. It is a positive integer character budget, not a
byte or token budget. Omit it to retain existing behavior.

These APIs use the same Rust shrinker, on a native worker thread. Host APIs do
not write response files: consumers may be running in a remote sandbox or an
in-memory filesystem. Instead, the toolset gains `compressor_recover_response`
(with a collision-safe suffix if necessary). It is exposed through the same
surface as the other tools: a generated CLI/Just Bash subcommand, Python or
TypeScript function, or the compressed `invoke_tool` wrapper. Recovery details
are included in each shortened response and the recovery tool's help/schema.
For example:

```sh
inventory compressor-recover-response --response-id <id-from-truncation-notice>
```

For generated TypeScript, use `compressorRecoverResponse(responseId)`; generated
Python uses `compressor_recover_response(response_id=...)`. Check generated help
if an upstream tool collided with that name.

Recovery returns the exact original **normalized string**, not the upstream MCP
transport envelope. It never re-executes a tool, even on a cache miss. Originals
are scoped to the transformed toolset and bounded to 128 entries / 64 MiB, with
a five-minute lifetime. Expired/evicted/unknown IDs fail explicitly. Call
`close()` on bridge/generated/Just Bash transform results to clear their caches
and release their existing resources. `compressTools` follows its existing
closure lifetime; discard the toolset to release its bounded cache.

For hosts that need to compose transformations or route tool calls through their
own sandbox callback bridge, `shrinkfitTools(tools, { shrinkfit: 6000 })` returns
`{ tools, close }`. Transform and advertise **that complete tool map**, including
its recovery tool, and use the same map for callback dispatch. Do not shrink the
same tools twice or apply TOON afterward: the recovery metadata is JSON and the
recovered string must remain unchanged. Convenience APIs disable their TOON
option when shrinkfit is active.

Error and multimodal envelopes bypass shrinking. If a budget cannot accommodate
a meaningful preview plus recovery notice, shrinking fails rather than returning
an over-budget result or dropping recovery instructions. Such failures occur
**after** the upstream invocation: never blindly retry writes. A single original
larger than the recovery cache capacity similarly fails rather than advertising
an unavailable original. No defaults or upstream tool schemas are changed.
