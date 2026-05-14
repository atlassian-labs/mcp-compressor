# API status

`mcp-compressor` is published as a multi-language library and CLI with aligned public APIs across Python, TypeScript, and Rust.

## Stable public surfaces

- CLI standard MCP compression mode.
- CLI Mode for generated shell commands.
- Code Mode for generated Python and TypeScript clients.
- Python `CompressorClient` high-level SDK.
- TypeScript `CompressorClient` high-level SDK.
- Rust `CompressorClient` high-level SDK.
- TypeScript local in-process tool compression with `compressTools`.
- Remote streamable HTTP MCP backends.
- OAuth and explicit-header authentication.
- Dynamic SDK auth providers.
- Generated shell/Python/TypeScript clients.

## Still being hardened

- Native OAuth compatibility across a wider range of MCP providers.
- Cross-platform binary/package release automation.
- Generated API reference pages for Python and TypeScript.

## Compatibility notes

- Public imports should use `mcp_compressor`, `@atlassian/mcp-compressor`, and `mcp_compressor` for Rust.
- Implementation crate/module names are internal details and should not appear in user-facing examples.
- Deprecated CLI aliases `--python-mode` and `--typescript-mode` remain available, but `--code-mode python` and `--code-mode typescript` are preferred.
