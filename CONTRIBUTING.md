# Contributing to `mcp-compressor`

Contributions are welcome, and they are greatly appreciated!
Every little bit helps, and credit will always be given.

Pull requests, issues and comments are welcome. For pull requests, please:

* Add tests for new features and bug fixes
* Follow the existing style
* Separate unrelated changes into multiple pull requests

For bigger changes, please make sure you start a discussion first by creating an issue and explaining the intended change.

You can contribute in many ways:

# Types of Contributions

## Report Bugs

Report bugs at https://github.com/atlassian-labs/mcp-compressor/issues

If you are reporting a bug, please include:

- Your operating system name and version.
- Any details about your local setup that might be helpful in troubleshooting.
- Detailed steps to reproduce the bug.

## Fix Bugs

Look through the GitHub issues for bugs.
Anything tagged with "bug" and "help wanted" is open to whoever wants to implement a fix for it.

## Implement Features

Look through the GitHub issues for features.
Anything tagged with "enhancement" and "help wanted" is open to whoever wants to implement it.

## Write Documentation

mcp-compressor could always use more documentation, whether as part of the official docs, in docstrings, or even on the web in blog posts, articles, and such.

## Submit Feedback

The best way to send feedback is to file an issue at https://github.com/atlassian-labs/mcp-compressor/issues.

If you are proposing a new feature:

- Explain in detail how it would work.
- Keep the scope as narrow as possible, to make it easier to implement.
- Remember that this is a volunteer-driven project, and that contributions
  are welcome :)

# Get Started!

Ready to contribute? Here's how to set up `mcp-compressor` for local development.
Please note this documentation assumes you already have `uv`, `Git`, Rust/Cargo, and Bun installed and ready to go.

1. Fork the `mcp-compressor` repo on GitHub.

2. Clone your fork locally:

```bash
cd <directory_in_which_repo_should_be_created>
git clone git@github.com:YOUR_NAME/mcp-compressor.git
```

3. Now install the repository dependencies. Navigate into the directory:

```bash
cd mcp-compressor
```

Install the Python environment and TypeScript dependencies with:

```bash
uv sync
cd typescript && bun install --frozen-lockfile && cd ..
```

4. Install pre-commit to run linters/formatters at commit time:

```bash
uv run pre-commit install
```

5. Create a branch for local development:

```bash
git checkout -b name-of-your-bugfix-or-feature
```

Now you can make your changes locally.

6. Don't forget to add test cases for your added functionality in the relevant package test directory (`tests/`, `crates/mcp-compressor-core/tests/`, or `typescript/tests/`).

7. When you're done making changes, run the relevant checks. For the full workspace:

```bash
make check
make test
```

For targeted checks while iterating:

```bash
uv run pytest -q tests/test_main.py
cargo test -p mcp-compressor-core
cd typescript && bun run check:ci
```

8. Before raising a pull request that affects Python compatibility, you can also run tox.
   This runs the Python tests across supported Python versions:

```bash
tox
```

This requires you to have multiple supported Python versions installed.
This step is also triggered in CI, so you can skip it locally when CI coverage is enough.

9. Commit your changes and push your branch to GitHub:

```bash
git add .
git commit -m "Your detailed description of your changes."
git push origin name-of-your-bugfix-or-feature
```

10. Submit a pull request through the GitHub website.

# Pull Request Guidelines

Before you submit a pull request, check that it meets these guidelines:

1. The pull request should include tests.

2. If the pull request adds functionality, the docs should be updated.
   Put your new functionality into a function with a docstring, and add the feature to the list in `README.md`.
