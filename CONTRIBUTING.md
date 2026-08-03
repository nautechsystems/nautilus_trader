# Contributing to NautilusTrader

Contributions from the trading community help drive NautilusTrader forward. This guide covers how to
pick something up, set up your environment, and get a PR merged. If you get stuck at any point, ask
on [Discord](https://discord.gg/NautilusTrader).

> [!IMPORTANT]
>
> Never report a security vulnerability as a public issue. Follow [SECURITY.md](SECURITY.md) instead.

## Start with an issue

Open a GitHub issue to discuss your proposed changes or enhancements. Early feedback is the quickest
way to avoid work that can't be merged.

Check the [open-source scope](ROADMAP.md#open-source-scope) first so your idea fits what the project
maintains, and read the [Code of Conduct](CODE_OF_CONDUCT.md).

You also need to sign the [Contributor License Agreement](CLA.md) before we can merge your work.
[CLA Assistant](https://cla-assistant.io/) prompts you automatically on your first PR.

> [!NOTE]
>
> **New integrations** are a major undertaking for the project and require discussion and approval
> before any PR is opened. See
> [ROADMAP: Community-contributed integrations](ROADMAP.md#community-contributed-integrations) for
> the process, and [ADAPTERS.md](ADAPTERS.md) for adapter tiers, community listings, and support
> boundaries.

## Find the right package

The Rust workspace lives under `crates/`, and the PyO3 Python package lives under `python/`. See
[MIGRATION_V2.md](MIGRATION_V2.md) when porting code from the legacy v1 package on `develop_v1`.
If you aren't sure where a change belongs, ask in the issue.

## Set up your environment

Fork the repository and branch from `develop`, merging upstream changes regularly to keep your fork
current. Then follow the [Environment setup guide](docs/developer_guide/environment_setup.md) for
Rust, Python, and uv. With those in place, install the pinned development tools. This includes
[prek](https://github.com/j178/prek), which runs pre-commit checks, formatters, and linters before
each commit:

```bash
cargo install cargo-binstall --locked  # one-off prerequisite
make install-tools
prek install
```

`make install-tools` reads pinned versions from `Cargo.toml`, `tools.toml`, and
`python/pyproject.toml`. See
[Install development tools](docs/developer_guide/environment_setup.md#2-install-development-tools)
for the full list, what each tool does, and which tools install separately.

## Make your change

Include tests that cover your change. Running the relevant target locally first saves a round trip:

- `make cargo-test` runs the Rust tests.
- `make pytest` runs the Python tests, building the extension and stubs first.

Rust tests use `#[rstest]` rather than `#[test]`, including non-parameterized ones, and pre-commit
enforces this (`#[tokio::test]` is fine for async tests). See
[Testing](docs/developer_guide/testing.md) for the wider conventions.

Follow the established coding practices in the
[Developer Guide](https://nautilustrader.io/docs/latest/developer_guide/). For documentation changes,
follow the style guide in `docs/developer_guide/docs.md` (use sentence case for headings H2 and
below).

## Before you open a PR

These are the checks that most often send a PR back:

- Run `make format` and `make pre-commit` so CI passes on the first attempt.
- If you changed PyO3 bindings or the Rust docs behind them, run `make py-stubs` and commit the
  generated output. These stubs are generated rather than hand-edited, and CI fails on drift. See
  [Generated Python artifacts](docs/developer_guide/rust.md#generated-python-artifacts).
- Give new Rust files the standard copyright header. See
  [File header requirements](docs/developer_guide/rust.md#file-header-requirements).
- Do not update `RELEASES.md`. Maintainers keep it current to avoid frequent merge conflicts.
- Do not use [Conventional Commits](https://www.conventionalcommits.org/) syntax for commit messages
  or PR titles. Follow [Commit messages](docs/developer_guide/coding_standards.md#commit-messages)
  instead. PR titles matter because a squash merge turns the PR title into the commit subject.

Open the PR against `develop` with a summary comment and a reference to any relevant GitHub issue.
Keep it small and focused, which makes review much faster.

We typically respond to PRs within a couple of days, and will let you know if anything needs changing
before merging.
