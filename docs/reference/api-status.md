# API status

The Rust migration branch is under active development. This page describes current stability expectations.

## Stable enough to build against on the migration branch

- Rust core compression behavior.
- Rust CLI standard compression mode.
- Rust CLI CLI/Python/TypeScript generation modes.
- Python `CompressorClient` high-level SDK.
- TypeScript `CompressorClient` high-level SDK.
- Rust `CompressorClient` high-level SDK.
- Explicit-header remote streamable HTTP backends.
- Generated shell/Python/TypeScript clients.

## Still being hardened

- Native OAuth flows across providers.
- Just Bash command host integration semantics.
- Cross-platform release artifacts.
- Final Python package name and cutover from `mcp-compressor-rust` to `mcp-compressor`.

## Compatibility notes

- TypeScript legacy runtime/client/server code has been removed from the migration branch.
- Python legacy top-level package has been removed from the migration branch.
- Public SDKs should not expose Rust implementation details in object names.
