# Contributing to NautilusTrader

Thank you for helping improve NautilusTrader. High‑quality contributions from the trading
community are welcome. This guide explains how to prepare a complete, review‑ready pull request.
For questions, ask in
[GitHub Discussions](https://github.com/nautechsystems/nautilus_trader/discussions) or on
[Discord](https://discord.gg/NautilusTrader).

> [!IMPORTANT]
>
> Never report a security vulnerability as a public issue. Follow [SECURITY.md](SECURITY.md) instead.

## Review standard

**NautilusTrader can execute live trades involving real capital. Errors can cause financial loss, so
all pull requests are held to a very high standard for correctness, reliability, testing, clarity,
and maintainability.**

Maintainers appreciate the time and care a high‑quality pull request requires, with or without AI.
They review complete, locally validated pull requests thoroughly. Merge decisions still depend on
evidence that the change meets this standard. When a contribution's expected review and maintenance
cost is disproportionate to its value to the project, maintainers may decline it. Contributors must
resolve blocking feedback before a pull request merges, either by updating the change or agreeing
with a maintainer on another resolution.

## Use of AI

If you use AI tools, you remain responsible for every submission. Read and follow the
[AI Policy](AI_POLICY.md), which explains the requirements for human direction, review,
communication, and attribution.

## Start with an issue

Before starting a substantial change, such as a new feature, integration, or design change,
**open a GitHub issue or comment on a relevant existing issue, then wait for a maintainer to agree
on the problem and approach**. **Small, self‑contained fixes, such as typos, obvious documentation
corrections, or narrowly scoped bug fixes with focused tests, do not need prior agreement.** Pull
requests for substantial changes without prior discussion and agreement may be closed without
review. Early agreement is the quickest way to avoid work that can't be merged.

**Before starting work, check the issue and any open pull requests for an implementation already under
review. Do not submit a competing implementation.** Instead, add useful context or an alternative
approach to the existing issue so contributors and maintainers can coordinate. If an existing pull
request appears inactive, ask on the pull request or its linked issue and wait for a maintainer to
confirm that the work is available before starting.

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
[prek](https://github.com/j178/prek), which runs file checks before each commit and validates the
commit message before Git records it:

```bash
cargo install cargo-binstall --locked  # one-off prerequisite
make install-tools
prek install
```

`prek install` installs both hook types configured by the repository.

`make install-tools` reads pinned versions from `Cargo.toml` and `tools.toml`. See
[Install development tools](docs/developer_guide/environment_setup.md#2-install-development-tools)
for the full list, what each tool does, and which tools install separately.

## Make your change

Include tests that cover changed behavior or logic. Running the relevant target locally first saves
a round trip:

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

### Prepare a review‑ready change

Open a pull request only when the change is complete, locally validated, and ready for maintainer
review, unless a maintainer asks for an early draft. Do not use draft or work‑in‑progress pull
requests as a development workspace. Develop and iterate on your branch before opening the pull
request.

### Run local checks and follow repository rules

Complete these requirements before opening or updating a pull request:

- Run `make format`, then run `make pre-commit` locally and confirm it passes.
- Run all tests relevant to the change locally. You may summarize relevant validation in the pull
  request when it helps reviewers, but exact commands and full output are not required.
- If you changed PyO3 bindings or the Rust docs behind them, run `make py-stubs` and commit the
  generated output. These stubs are generated rather than hand-edited, and CI fails on drift. See
  [Generated Python artifacts](docs/developer_guide/rust.md#generated-python-artifacts).
- Give new Rust files the standard copyright header. See
  [File header requirements](docs/developer_guide/rust.md#file-header-requirements).
- Do not update `RELEASES.md`. Maintainers keep it current to avoid frequent merge conflicts.
- Do not use [Conventional Commits](https://www.conventionalcommits.org/) syntax for commit messages
  or PR titles. Follow [Commit messages](docs/developer_guide/coding_standards.md#commit-messages)
  instead. PR titles matter because a squash merge turns the PR title into the commit subject.

For higher assurance, you can also run `make pre-flight`, which performs the project's broad local
validation suite. It does not replace `make pre-commit`.

### Use CI responsibly

Project CI confirms a change that you have already validated locally. **Do not rely on an open pull
request as the primary development loop.** Each push starts another CI run. Frequent incremental
pushes consume compute, cancel work in progress, and make the Actions history harder to read, which
can obscure meaningful failures.

Batch related corrections into coherent updates. After review feedback, make the changes locally,
rerun the relevant checks and tests, then push the complete update. If a platform, access, or local
resource constraint prevents a relevant check, discuss the limitation with a maintainer before
requesting review and state it clearly in the pull request.

### Open the pull request

Open the PR against `develop` with a concise summary and a reference to any relevant GitHub issue.
Keep it small and focused, which makes review much faster.

Make the PR description accurate, specific, and easy to review. Remove generic or bloated prose
that could hide important details.

By opening a PR, you confirm that you have read this guide and, if you used AI, the
[AI Policy](AI_POLICY.md). You also confirm that you understand and can explain every submitted
change and all information in the PR description.

We typically respond to pull requests within a couple of days.
