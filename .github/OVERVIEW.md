# GitHub Actions Overview

The `.github` directory contains NautilusTrader's composite actions and workflows for continuous
integration, scheduled checks, and publication.

## Composite actions (`.github/actions`)

- [`attest-build-provenance-retry`](actions/attest-build-provenance-retry/action.yml): retries
  GitHub build provenance attestation after transient failures.
- [`attest-sbom-retry`](actions/attest-sbom-retry/action.yml): retries Docker SBOM attestation after
  transient failures.
- [`cargo-tool-install`](actions/cargo-tool-install/action.yml): installs a version‑pinned Cargo
  tool with caching.
- [`common-setup`](actions/common-setup/action.yml): configures system packages, Rust and Python
  toolchains, caches, and optional disk or swap preparation.
- [`common-test-data`](actions/common-test-data/action.yml): caches the large test data set.
- [`generate-sbom-retry`](actions/generate-sbom-retry/action.yml): retries SPDX SBOM generation
  after transient failures.
- [`install-capnp`](actions/install-capnp/action.yml): installs the Cap'n Proto compiler across
  supported platforms.
- [`publish-wheels`](actions/publish-wheels/action.yml): publishes wheels and maintains the
  Cloudflare R2 package index.
- [`upload-artifact-wheel`](actions/upload-artifact-wheel/action.yml): uploads wheel artifacts to
  GitHub Actions.

## Workflows (`.github/workflows`)

- [`build.yml`](workflows/build.yml): plans CI scope, runs lint and test jobs, builds wheels, and
  publishes Python packages, Rust crates, and stable release assets.
- [`build-docs.yml`](workflows/build-docs.yml): dispatches documentation builds from `master` and
  `nightly`.
- [`cli-binaries.yml`](workflows/cli-binaries.yml): builds and publishes CLI archives for Linux,
  macOS, and Windows.
- [`codeql-analysis.yml`](workflows/codeql-analysis.yml): runs CodeQL analysis for Python and Rust.
- [`docker.yml`](workflows/docker.yml): builds, publishes, signs, and attests the multi‑platform
  `nautilus_trader` and `jupyterlab` images.
- [`dst.yml`](workflows/dst.yml): runs deterministic simulation smoke tests.
- [`nightly-docs-features-check.yml`](workflows/nightly-docs-features-check.yml): checks docs.rs
  builds, crate feature combinations, and example targets.
- [`nightly-merge.yml`](workflows/nightly-merge.yml): fast‑forwards `nightly` to the latest
  successful `develop` commit.
- [`nightly-miri.yml`](workflows/nightly-miri.yml): runs Miri against selected crates.
- [`nightly-tests.yml`](workflows/nightly-tests.yml): runs standard‑precision Clippy, extended
  network tests, and Cargo publication checks.
- [`openssf-scorecard.yml`](workflows/openssf-scorecard.yml): publishes OpenSSF Scorecard results
  and uploads SARIF.
- [`performance.yml`](workflows/performance.yml): runs Rust tests and benchmarks on `nightly`.
- [`security-audit.yml`](workflows/security-audit.yml): runs change‑aware and scheduled supply‑chain
  audits.

## Security

The [security architecture](../docs/developer_guide/security.md) covers the release threat model,
artifact integrity records, and verification flow. This section records CI‑specific constraints.

### Change and dependency controls

- [`CODEOWNERS`](CODEOWNERS) requires Core team review for workflows, composite actions,
  dependencies, build configuration, and scripts. Repository rulesets require signed commits,
  reviews, and required checks on protected branches, and prevent release tag mutation.
- External actions include their canonical source URL, use a full commit SHA, and record the
  corresponding release tag. Adopt an action release only after it has been available for at least
  two weeks.
- Docker base images and workflow service containers use immutable digest pins.
- Tool and dependency versions are pinned in the repository. Python dependency resolution applies
  the publication cooldowns defined in `python/pyproject.toml`, including a three‑day default.
- `security-audit.yml` runs `cargo audit`, `cargo-deny`, `cargo-vet`, `pip-audit`, OSV‑Scanner, and
  Zizmor. CodeQL and OpenSSF Scorecard run in dedicated workflows.

### Execution boundaries

- Workflows default `GITHUB_TOKEN` to `contents: read` and `actions: read`. Individual jobs add only
  the permissions required for their operation.
- The pre‑commit, Linux x86 wheel, and Linux x86 Rust jobs in `build.yml` use the self‑hosted build
  pool only for same‑repository, non‑Dependabot pull requests with a known author. Fork, Dependabot,
  or incomplete pull request metadata routes these jobs to GitHub‑hosted runners. Untrusted pull
  request jobs remain read‑only and do not receive Actions secrets.
- `build.yml` cancels superseded pull request runs. Push runs use commit‑specific concurrency groups,
  so a later push cannot replace an earlier candidate's result.

### Publication integrity

- PyPI and crates.io publication uses short‑lived OpenID Connect identities through Trusted
  Publishing. These jobs are bound to the protected `release` environment.
- Stable releases remain drafts until package indexes have been published and verified against the
  release assets. The workflow then attaches checksums, manifests, and provenance records before
  publishing the GitHub release.
- Container images receive keyless cosign signatures and SPDX SBOM attestations. The Docker
  workflow verifies both against the published image digest.

See [Releases](../docs/developer_guide/releases.md) for the stable release ordering constraints and
[Security Policy](../SECURITY.md) for artifact verification commands.

### Network egress

Supported jobs use `step-security/harden-runner` with `egress-policy: block` by default. Shared
allow lists come from GitHub configuration variables, and workflows declare job‑specific endpoints
inline.

- `STEP_SECURITY_EGRESS_POLICY`: selects the egress mode. Use `audit` only as a temporary fallback
  while expanding an allow list.
- `COMMON_ALLOWED_ENDPOINTS`: provides endpoints shared across workflows.
- `CI_ALLOWED_ENDPOINTS`: adds endpoints used by build, documentation, container, and scheduled
  test workflows.
- `SECURITY_AUDIT_ALLOWED_ENDPOINTS`: adds endpoints used by security audit jobs and the nightly
  publication gate.

Store endpoint variables as single‑line, space‑delimited values. The pinned Harden Runner version
does not enforce newline‑delimited values correctly in `block` mode.

Jobs that declare a GitHub Environment can override the repository or organization egress policy
with an environment‑scoped variable. Security audit jobs read repository and organization variables
directly and do not use deployment environments or environment secrets.

Fork pull requests in `build.yml` and `security-audit.yml` use `audit` mode because they cannot read
the endpoint variables. Other workflows retain the configured policy and default to `block`.

### Security gate override

The `SECURITY_GATE_OVERRIDE` repository variable accepts an ISO 8601 UTC expiry timestamp. While the
timestamp is in the future, it skips the nightly publication security gate and forced security
audits for development wheel publication pushes. It does not suppress pull request, scheduled, or
manual audits, and it expires without a separate reset.

Leave the variable unset during normal operation. Set it only after reviewing the blocked audit and
limit the expiry to the minimum time needed for the affected publication run.
