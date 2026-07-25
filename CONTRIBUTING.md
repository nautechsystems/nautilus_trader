# Contributing to NautilusTrader

We highly value involvement from the trading community, and all contributions are greatly
appreciated as they help us continually improve NautilusTrader!

Before starting, please read our [Code of Conduct](CODE_OF_CONDUCT.md) and check the
[open-source scope](ROADMAP.md#open-source-scope) so your work aligns with what the project
maintains.

> [!NOTE]
>
> **Integrations:**
> New integrations are a major undertaking for the project and therefore require additional
> discussion and approval before opening any PRs.
> Please see the [ROADMAP: Community-contributed integrations](ROADMAP.md#community-contributed-integrations)
> for details on the process, and [ADAPTERS.md](ADAPTERS.md) for adapter tiers, community listings,
> and support boundaries.

> [!IMPORTANT]
>
> **Security:**
> Never report a security vulnerability as a public issue. Follow [SECURITY.md](SECURITY.md) instead.

## Steps

To contribute, follow these steps:

1. Open an issue on GitHub to discuss your proposed changes or enhancements.

2. Read the [Contributor License Agreement (CLA)](CLA.md). You are required to sign it before we
   can merge your work, which is administered automatically through
   [CLA Assistant](https://cla-assistant.io/).

3. Fork the repository, branch from `develop`, and keep your fork up to date by regularly merging
   upstream changes.

4. Set up your development environment by following the
   [Environment setup guide](docs/developer_guide/environment_setup.md), which covers Rust, Python,
   and uv. With those prerequisites in place, install the pinned development tools. This includes
   [prek](https://github.com/j178/prek), which runs pre-commit checks, formatters, and linters
   before each commit:

   ```bash
   cargo install cargo-binstall --locked  # one-off prerequisite
   make install-tools
   prek install
   ```

   `make install-tools` installs the pinned development tools from `Cargo.toml`, `tools.toml`, and
   `pyproject.toml`. See [Install development tools](docs/developer_guide/environment_setup.md#2-install-development-tools)
   for the full list, what each tool does, and which tools install separately.

5. Open a pull request (PR) against the `develop` branch with a summary comment and a reference to
   any relevant GitHub issue(s).

6. The CI system runs the full test suite on your code including all unit and integration tests, so
   include appropriate tests with the PR.

7. We will review your code as quickly as possible and provide feedback if any changes are needed
   before merging.

## Requirements

- Run `make format` and `make pre-commit` before opening a PR so CI passes on the first attempt.
- Do NOT use [Conventional Commits](https://www.conventionalcommits.org/) syntax for commit messages
  or PR titles. Follow [Commit messages](docs/developer_guide/coding_standards.md#commit-messages)
  instead. PR titles matter because a squash merge turns the PR title into the commit subject.
- Do not update `RELEASES.md` in a pull request. Maintainers keep it current to avoid frequent merge
  conflicts.
- For v2 PyO3 bindings or wrapped Rust docs, run `make py-stubs-v2` and commit the generated output.
  See [Generated Python artifacts](docs/developer_guide/rust.md#generated-python-artifacts).
- Give new Rust files the standard copyright header. See
  [File header requirements](docs/developer_guide/rust.md#file-header-requirements).

## Tips

- Follow the established coding practices in the
  [Developer Guide](https://nautilustrader.io/docs/latest/developer_guide/).
- For documentation changes, follow the style guide in `docs/developer_guide/docs.md` (use sentence
  case for headings H2 and below).
- Keep PRs small and focused for easier review.
- Reference the relevant GitHub issue(s) in your PR comment.
