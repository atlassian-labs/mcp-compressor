# Release process

This page documents how maintainers publish mcp-compressor release artifacts.

A GitHub Release is the normal release entrypoint. The release tag determines all package versions; versions should not be committed into source files manually.

## Version tags

Use a tag beginning with `v`.

Stable releases use SemVer:

```text
v1.2.3
```

Python prerelease-style tags are also supported and are converted for Rust/npm:

| Release tag | Python version | Rust/npm version | npm dist tag |
| --- | --- | --- | --- |
| `v0.15.0a1` | `0.15.0a1` | `0.15.0-alpha.1` | `next` |
| `v0.15.0b2` | `0.15.0b2` | `0.15.0-beta.2` | `next` |
| `v0.15.0rc3` | `0.15.0rc3` | `0.15.0-rc.3` | `next` |
| `v1.2.3` | `1.2.3` | `1.2.3` | `latest` |

## Release workflow

The release workflow is:

```text
.github/workflows/on-release-main.yml
```

It runs when a GitHub Release is published and can also be run manually with `workflow_dispatch`.

It publishes:

- Python package to PyPI,
- Rust crates to crates.io,
- TypeScript package to Atlassian npm-public,
- documentation to GitHub Pages.

## Python publish

Python publishing uses PyPI trusted publishing from:

```text
.github/workflows/on-release-main.yml
```

The workflow patches the package version from the release tag, builds one abi3 wheel per platform for Linux x64, Linux ARM64, macOS arm64, macOS x64, and Windows that supports CPython 3.11 and newer, builds Linux wheels in a manylinux image for broad distro compatibility, labels wheel jobs by target platform, builds one source distribution, and publishes them with:

```bash
uv publish --trusted-publishing always dist/*
```

No PyPI token should be needed when the trusted publisher is configured on PyPI for this repository/workflow.

## Rust publish

Rust publishing uses crates.io token auth with the repository secret:

```text
CARGO_REGISTRY_TOKEN
```

The workflow patches Cargo versions from the release tag, publishes the implementation crate first, waits for the crates.io index, then publishes the public crate.

The public crate is:

```text
mcp-compressor
```

It includes the installable binary:

```bash
cargo install mcp-compressor
mcp-compressor --help
```

## TypeScript publish

TypeScript publishing uses Atlassian's npm-public Artifactory flow.

The main release workflow does **not** publish TypeScript directly from the tag ref. Artifactory npm-public publishing requires branch-based GitHub OIDC claims, so the release workflow:

1. checks out the release tag,
2. writes `.release-tag`,
3. force-updates the dedicated `release` branch,
4. dispatches:

   ```text
   .github/workflows/publish-typescript-package.yml
   ```

   on the `release` branch,
5. waits for the dispatched workflow to complete.

The TypeScript workflow reads the tag from the workflow input, with `.release-tag` as a fallback, converts it to an npm-compatible version, builds native addons on Linux x64, Linux ARM64, macOS arm64, macOS x64, and Windows using the documented GitHub-hosted macOS Intel runner for x64 artifacts, bundles those native addons into the package, packs the package, smoke-tests the tarball, then publishes with the npm dist tag derived from the release tag.

For prereleases, npm publish uses:

```text
--tag next
```

For stable releases, it uses:

```text
--tag latest
```

### Required npm-public configuration

The package must be configured in Atlassian Artifactory npm-public allowlists to enable Artifactory forwarding to npmjs.

The workflow uses:

```yaml
atlassian-labs/artifact-publish-token@v1.0.1
```

with `output-modes: npm` to create `.npmrc-public`.

## Docs publish

Docs are deployed by the release workflow with:

```bash
uv run mkdocs gh-deploy --force
```

The docs job has `contents: write` permission so it can push to `gh-pages`.

## Validation before releasing

Before creating a release, run:

```bash
make check
make docs-test
cargo check -p mcp-compressor
cargo check -p mcp-compressor-core
cargo check -p mcp-compressor-python
cargo check -p mcp-compressor-node
```

For a high-confidence release, also verify the release artifact smoke workflows are green on `main`.
