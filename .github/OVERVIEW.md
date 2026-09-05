# GitHub Actions Overview

The `.github` directory contains NautilusTrader's composite actions and workflows for continuous
integration, scheduled checks, and publication.

## Composite actions (`.github/actions`)

- [`attest-build-provenance-retry`](actions/attest-build-provenance-retry/action.yml): retries
  GitHub build provenance attestation after transient failures.
- [`attest-sbom-retry`](actions/attest-sbom-retry/action.yml): retries Docker SBOM attestation after
  transient failures.
- [`cargo-tool-install`](actions/cargo-tool-install/action.yml): installs a version-pinned Cargo
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
- [`docker.yml`](workflows/docker.yml): builds, publishes, signs, and attests the multi-platform
  `nautilus_trader` and `jupyterlab` images.
- [`dst.yml`](workflows/dst.yml): runs deterministic simulation smoke tests.
- [`nightly-docs-features-check.yml`](workflows/nightly-docs-features-check.yml): checks docs.rs
  builds, crate feature combinations, and example targets.
- [`nightly-merge.yml`](workflows/nightly-merge.yml): fast-forwards `nightly` to the latest
  successful `develop` commit.
- [`nightly-miri.yml`](workflows/nightly-miri.yml): runs Miri against selected crates.
- [`nightly-tests.yml`](workflows/nightly-tests.yml): runs Rust doctests, Python memory leak tests,
  standard-precision Clippy, extended network tests, and Cargo publication checks.
- [`openssf-scorecard.yml`](workflows/openssf-scorecard.yml): publishes OpenSSF Scorecard results
  and uploads SARIF.
- [`performance.yml`](workflows/performance.yml): runs Rust tests and registered benchmarks on
  `nightly`, plus selected CodSpeed benchmarks on `develop`, `test-performance`, and pull requests
  targeting `develop`.
- [`security-audit.yml`](workflows/security-audit.yml): provides the change-aware supply-chain audit
  used by `build.yml`, plus non-`develop` pull request, scheduled, manual, and `test-security` runs.
- [`test.yml`](workflows/test.yml): runs pre-commit, Python tests, and Rust tests on Linux x86 with
  Python 3.14 for pushes to the protected `test` branch.

## Security

The [security architecture](../docs/developer_guide/security.md) covers the release threat model,
artifact integrity records, and verification flow. This section records CI-specific constraints.

### Change and dependency controls

- [`CODEOWNERS`](CODEOWNERS) requires Core team review for workflows, composite actions,
  dependencies, build configuration, and scripts. Repository rulesets require signed commits,
  reviews, and required checks on protected branches, and prevent release tag mutation.
- External actions include their canonical source URL, use a full commit SHA, and record the
  corresponding release tag. Adopt an action release only after it has been available for at least
  two weeks.
- Docker base images and workflow service containers use immutable digest pins.
- Tool and dependency versions are pinned in the repository. Python dependency resolution applies
  the seven-day publication cooldown defined in `python/pyproject.toml`, and Rust crate updates
  observe the three-day cooldown defined in `Cargo.toml`.
- `security-audit.yml` runs `cargo audit`, `cargo-deny`, `cargo-vet`, `pip-audit`, OSV-Scanner, and
  Zizmor. CodeQL and OpenSSF Scorecard run in dedicated workflows.

### Execution boundaries

- Dedicated script tests must pass in CI before pre-commit begins, so important repository scripts
  are tested before pre-commit uses them.
- Workflows default `GITHUB_TOKEN` to `contents: read` and `actions: read`. Individual jobs add only
  the permissions required for their operation.
- The script tests, pre-commit, Linux x86 wheel, and Linux x86 Rust jobs in `build.yml` use the
  self-hosted build pool only for same-repository, non-Dependabot pull requests with a known author.
  Fork, Dependabot, or incomplete pull request metadata routes these jobs to GitHub-hosted runners.
  Untrusted pull request jobs remain read-only and do not receive Actions secrets.
- `test.yml` accepts only pushes to `test`, which the `test` ruleset restricts to
  trusted repository maintainers before the workflow reaches the self-hosted build pool.
- `build.yml` cancels superseded pull request runs. Push runs use commit-specific concurrency groups,
  so a later push cannot replace an earlier candidate's result.

### Publication integrity

- PyPI and crates.io publication uses short-lived OpenID Connect identities through Trusted
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
allow lists come from GitHub configuration variables, and workflows declare job-specific endpoints
inline.

- `STEP_SECURITY_EGRESS_POLICY`: selects the egress mode. Use `audit` only as a temporary fallback
  while expanding an allow list.
- `COMMON_ALLOWED_ENDPOINTS`: provides endpoints shared across workflows.
- `CI_ALLOWED_ENDPOINTS`: adds endpoints used by build, documentation, container, and scheduled
  test workflows.
- `SECURITY_AUDIT_ALLOWED_ENDPOINTS`: adds endpoints used by security audit jobs and the nightly
  publication gate.

Store endpoint variables as single-line, space-delimited values. The pinned Harden Runner version
does not enforce newline-delimited values correctly in `block` mode.

Jobs that declare a GitHub Environment can override the repository or organization egress policy
with an environment-scoped variable. Security audit jobs read repository and organization variables
directly and do not use deployment environments or environment secrets.

Fork pull requests and their called security audit jobs in `build.yml` use `audit` mode because they
cannot read the endpoint variables. Other workflows retain the configured policy and default to
`block`.

### Security gate override

The `SECURITY_GATE_OVERRIDE` environment-scoped configuration variable permits a reviewed security
gate failure for one commit. Configure it on both `r2-develop` and `r2-nightly`, set it to `disabled`
during normal operation, and remove any repository-scoped variable with the same name. Only
repository admins can configure environment variables, while users with write access can configure
repository variables.

An active value uses `<UTC expiry>@<full commit SHA>`, for example:

```text
2026-08-08T12:00:00Z@0123456789abcdef0123456789abcdef01234567
```

The expiry must use the exact `YYYY-MM-DDTHH:MM:SSZ` format and be no more than two hours in the
future. The SHA must match the publication commit. Missing, malformed, expired, overlong, or
mismatched values fail closed. Cancelled, skipped, and other incomplete gate results cannot be
overridden.

Security audits run as part of `build.yml`. The Zizmor and supply-chain jobs remain path-scoped for
ordinary pull requests and branch pushes, while `test-ci` and `test-pre-commit` force both jobs. The
development publication and stable release tag gate depend directly on the same build's audit
result. Nightly builds do not repeat the full audit; the nightly security gate completes its scans
before publication. Scheduled and manual audits run independently, so the repository is still
audited when no build runs. The override does not suppress pull request, scheduled, manual, or
stable release audits.

To approve a blocked development or nightly publication:

1. Review the failed audit and confirm that publishing the affected commit is acceptable.
1. Set `SECURITY_GATE_OVERRIDE` on the matching `r2-develop` or `r2-nightly` environment.
1. Re-run the failed build jobs for the same commit.
1. Reset the environment variable to `disabled` after the publication completes.
