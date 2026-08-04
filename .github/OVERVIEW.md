<!--
  README for the .github directory: composite actions and workflow definitions.
-->
# GitHub Actions Overview

This directory contains reusable composite actions and workflow definitions for
CI/CD, testing, publishing, and automation within the NautilusTrader repository.

## Composite actions (`.github/actions`)

- **attest-build-provenance-retry**: wraps GitHub build provenance attestation with bounded retries.
- **attest-sbom-retry**: wraps Docker SBOM attestation with bounded retries.
- **cargo-tool-install**: installs cargo tools (cargo-deny, cargo-vet) with caching.
- **common-setup**: prepares the environment (OS packages, Rust toolchain, Rust cache, Python, prek, swap space).
- **common-test-data**: caches large test data under `test_data/large`.
- **generate-sbom-retry**: wraps Docker SBOM generation with bounded retries.
- **install-capnp**: installs the Cap'n Proto compiler with caching across Linux, macOS, and Windows.
- **publish-wheels**: publishes built wheels to Cloudflare R2, manages old wheel cleanup and index generation.
- **upload-artifact-wheel**: uploads the latest wheel artifact to GitHub Actions.

## Workflows (`.github/workflows`)

- **build.yml**: main CI pipeline - plan, pre-commit, cargo-deny, Rust tests, Python tests,
  wheel builds, artifact uploads, release asset uploads, Trusted Publishing to PyPI and crates.io,
  release preflights, release attestations, registry verification, release checksum publication,
  final release asset verification, and final GitHub release publication and attestation
  verification. Pushes to `nightly` build and publish wheels for every supported platform.
  A dedicated Linux x86 job runs the workspace Rust suite once, and the required Python wheel jobs
  fail when that prerequisite fails. The plan step skips builds on docs-only changes and skips Rust
  tests on Python-only changes.
- **build-docs.yml**: dispatches documentation build on `master` and `nightly` pushes.
- **cli-binaries.yml**: builds and publishes CLI binaries for multiple platforms.
- **codeql-analysis.yml**: CodeQL security scans for Python and Rust on PRs to `master`, pushes to
  `nightly`, and manual dispatch.
- **docker.yml**: builds and pushes multi-platform Docker images (`nautilus_trader`, `jupyterlab`)
  using Buildx and native ARM runners.
- **dst.yml**: runs deterministic simulation smoke tests on `nightly` and manual dispatch.
- **nightly-docs-features-check.yml**: nightly docs.rs build checks and crate feature compatibility verification.
- **nightly-merge.yml**: fast-forwards `nightly` to the latest successful `develop` build.
- **nightly-miri.yml**: runs Miri against the core, model, and plugin crates each day at 13:00 UTC.
- **nightly-tests.yml**: runs standard-precision Clippy, extended turmoil network tests, and Cargo
  publish-plan and dry-run checks each day at 12:00 UTC. It gives early visibility on `develop`
  before `nightly-merge` at 14:00 UTC without repeating the platform build‑and‑test matrices.
- **performance.yml**: Rust tests and benchmarks on `nightly`.
- **security-audit.yml**: runs change-aware and scheduled supply chain checks (cargo-audit,
  cargo-deny, cargo-vet, pip-audit, osv-scanner, and Zizmor).
- **openssf-scorecard.yml**: OpenSSF Scorecard posture scan on weekly schedule and manual dispatch.
  The scheduled run publishes badge/API results; all runs upload SARIF to code scanning.

## Security

### Source and review controls

- **CODEOWNERS**: Critical infrastructure files (workflows, dependencies, build configs, scripts)
  require Core team review before merge.
- **Branch and tag rulesets**: Protected branches require signed commits and passing CI checks.
  Release tags matching `v*` are immutable after creation. External PRs must receive Core team
  approval before merge.
- **Least-privilege tokens**: Workflows default `GITHUB_TOKEN` to `contents: read, actions: read`
  and selectively elevate scopes only for jobs that need them.
- **Secret management**: No secrets or credentials are stored in the repo. Credentials are provided
  via GitHub Secrets and injected at runtime.

### Dependency intake controls

- **Dependency pinning**: Key tools (prek, Python versions, Rust toolchain, cargo-nextest, uv) are
  locked to fixed versions or SHAs. The uv version is pinned via `required-version` in
  `python/pyproject.toml` and extracted by `scripts/uv-version.sh` for CI, Docker, and local builds.
  Release and audit helper Python CLIs are pinned in `tools.toml`.
- **Dependency cooldown**: Python dependency resolution excludes packages published within the last
  3 days (`exclude-newer = "3 days"` in `[tool.uv]`). This gives the community time to detect and
  quarantine compromised releases before they enter the lockfile.

### Pre-merge and scheduled scanning

- **cargo-deny**: Rust dependency auditing for security advisories (RUSTSEC/GHSA), license
  compliance, banned crates, and supply chain integrity. Configuration in `deny.toml`.
- **Code scanning**: CodeQL analyzes Python and Rust code on PRs to `master`, pushes to `nightly`,
  and manual dispatch. Zizmor runs in `security-audit.yml` and uploads SARIF when token
  permissions allow it.
- **OpenSSF Scorecard**: `openssf-scorecard.yml` publishes repository posture results for the public
  badge/API and uploads SARIF to code scanning.

### Build and publish controls

- **Immutable action pinning**: All third-party GitHub Actions are pinned to specific commit SHAs.
- **Docker image pinning**: Base images in Dockerfiles and service containers in workflows are
  pinned to SHA256 digests to prevent supply-chain attacks via tag mutation.
- **Build attestations**: Python wheels and sdists receive GitHub artifact attestations and PyPI
  publish attestations. Docker images receive cosign signatures and SPDX SBOM attestations. Verify
  Python artifacts via `gh attestation verify` and container images via `cosign verify`.
- **Release sequencing**: Stable releases create a draft GitHub release first, attach wheel and
  sdist assets, publish to package indexes (`packages.nautechsystems.io`, PyPI, crates.io), verify
  registries, attach final integrity assets, then publish the GitHub release. This keeps the GitHub
  release as the anchor for downstream registry publishing while staying compatible with GitHub
  release immutability.
- **Release checksums**: GitHub releases attach `SHA256SUMS`, per-asset `.sha256` files,
  `dist-manifest.json`, and per-artifact `.sigstore` / `.intoto.jsonl` provenance bundle siblings
  for Python artifacts. The release body also includes a generated artifact checksum table and
  provenance verification command.
- **PyPI Trusted Publishing**: `publish-wheels-pypi` and `publish-sdist-pypi` upload to PyPI via
  OIDC trusted publishing rather than a long-lived API token. The trusted publisher on PyPI is
  bound to repo `nautechsystems/nautilus_trader`, workflow `build.yml`, and environment `release`;
  `uv publish --trusted-publishing automatic` mints a short-lived token at publish time. No
  `PYPI_*` secret is required.
- **crates.io Trusted Publishing**: `publish-cargo-crates` publishes Cargo crates via crates.io
  OIDC trusted publishing. The trusted publisher on crates.io must be configured per crate for
  repo `nautechsystems/nautilus_trader`, workflow `build.yml`, and environment `release`; the
  job uses a short-lived token from `rust-lang/crates-io-auth-action` and no long-lived cargo token.
- **Post-publish verification**: `publish-release-integrity` generates the release manifest, then
  verifies PyPI files against `dist-manifest.json`, verifies PyPI provenance publisher metadata, and
  verifies crates.io entries were trusted-published by this repository before attaching checksum
  assets to the draft release. These verifier calls retry transient Sigstore/Rekor/TUF lag, while
  provenance and identity mismatches fail fast. The job records whether each crate matches the
  release commit, was already published, or matched an explicit
  `CRATES_IO_MANUAL_PUBLISH_EXCEPTIONS` `crate@version` entry for emergency token-publish recovery.
  Manual entries are recorded in `crates-manifest.json` with
  `release_status: "manual_token_publish"`. Malformed or unused exception entries fail the job. The
  job uploads `crates-manifest.json`, attaches attestation siblings, and cleans up release workflow
  artifacts. `publish-github-release` verifies the final draft asset set, publishes the draft
  release, and verifies GitHub's release attestation.
- **Caching**: The dedicated Linux x86 Rust job restores its action cache for untrusted PRs and
  produces it from trusted `develop` and `test-ci` pushes. Its other self-hosted runs use a
  persistent target. Linux x86 wheel jobs disable action caching and use persistent targets for
  trusted pushes. Other wheel-matrix Rust caches save only on pushes. Prek hook environments use a
  separate cache. The active large Parquet fixtures save after the Rust tests on a cache miss.
- **Concurrency**: PR CI runs are cancelled when a new push arrives to the same PR. Push events to
  mainline branches are never cancelled.
- **Runners**: Trusted Linux x86 build and test jobs, including `test-ci`, use the self-hosted build
  pools. Untrusted PRs use GitHub-hosted runners under the policy below. Depot 8-core runners cover
  platforms without self-hosted capacity, such as Linux ARM and Windows cross-platform builds.
  macOS and lightweight jobs use GitHub runners. The scheduled `nightly-tests.yml` workflow uses no
  Depot runners. Custom runner labels are declared in `.github/actionlint.yaml`.

### Runtime hardening

- **Hardened runners**: Workflows use `step-security/harden-runner` on supported runners to reduce
  attack surface and monitor outbound traffic. Depot Windows wheel jobs omit it because its post step
  currently assumes `C:\agent` exists. Workflows default `egress-policy` to `block`. Set
  `STEP_SECURITY_EGRESS_POLICY=audit` only as a temporary rollback while expanding an allow list.
  Jobs that declare a GitHub Environment can override the repo or org value with an
  environment-scoped variable. The publish environments (`r2-develop`, `r2-nightly`, `release`) can
  use this override too. Security audit jobs read repo and org variables directly and run in audit
  mode for fork PRs when variables are absent.
- **Untrusted PR handling**: `build.yml` uses self‑hosted runners only for same‑repository,
  non‑Dependabot PRs with a known author. Fork and missing‑origin PRs use GitHub‑hosted runners with
  `egress-policy: audit` because they cannot read the repository or organization endpoint variables.
  Dependabot and missing‑author PRs also use GitHub‑hosted
  runners, but retain the configured egress policy, which defaults to `block`. These jobs run with
  read‑only permissions and no access to Actions secrets.

### Security gate override

The `security-gate-nightly` job runs `cargo audit` and `osv-scanner` to catch vulnerabilities
before publishing. Occasionally, upstream events outside our control (transitive dependency
advisories, crate yanks for non-security reasons) can block the nightly pipeline with no
actionable fix on our side.

The repo-scoped variable `SECURITY_GATE_OVERRIDE` holds an ISO 8601 UTC timestamp
(e.g. `2026-03-28T02:00:00Z`). When the current time is before the timestamp, the security
gate is skipped. When the timestamp passes, the gate re-enables automatically with no manual
reset. The variable will be left unset for normal operations.

A repo admin will thoroughly assess all flagged items before setting the timestamp, and will
scope it to the minimum window needed for the blocked build to complete:

```
date -u -d '+2 hours' --iso-8601=seconds  # e.g. 2 hour window
```

Modifying repo variables requires admin access. An attacker with that level of access can
already disable workflows or push directly, so the override does not widen the attack surface.

`cargo audit` catches CVEs and unsound code advisories independent of yank status. A crate
yanked for non-security reasons (MSRV mistakes, broken builds, accidental publishes) produces
a warning but does not indicate a vulnerability.

### Allowed network endpoints

The `step-security/harden-runner` action restricts network access to approved endpoints.
Endpoint variables are stored in GitHub as single-line, space-delimited values. The pinned
`step-security/harden-runner` version does not enforce newline-delimited values correctly
in `block` mode.

Workflows use these GitHub variables by role:

- `STEP_SECURITY_EGRESS_POLICY`: StepSecurity egress mode for the job. Workflows default to `block`. Set
  `audit` only as a temporary override while expanding an allow list.
- `COMMON_ALLOWED_ENDPOINTS`: Endpoints needed by every job (GitHub API, Ubuntu packages, tooling).
- `CI_ALLOWED_ENDPOINTS`: Extra endpoints shared by the main CI, nightly, docs, and release workflows.
- `SECURITY_AUDIT_ALLOWED_ENDPOINTS`: Extra endpoints needed by the security audit jobs.

Some workflows add job-specific endpoints inline (e.g., `upload.pypi.org:443` for publishing,
`auth.docker.io:443` and `registry-1.docker.io:443` for Docker builds, and Scorecard publishing
plus lookup endpoints such as `api.scorecard.dev:443`, `fulcio.sigstore.dev:443`, and
`tuf-repo-cdn.sigstore.dev:443`). The Windows CLI build also permits GlobalSign OCSP and CRL
endpoints so Schannel can verify GitHub release certificates.

Security audit jobs do not use deployment environments. They do not need environment secrets, and
environment branch policies block same-repo contributor PRs before the audit steps can start.

#### `COMMON_ALLOWED_ENDPOINTS`

```
api.github.com:443                           # GitHub API
github.com:443                               # GitHub main site
artifacts.githubusercontent.com:443          # GitHub Actions artifacts
codeload.github.com:443                      # GitHub code downloads
raw.githubusercontent.com:443                # Raw file access
uploads.github.com:443                       # GitHub uploads
objects.githubusercontent.com:443            # GitHub objects storage
pipelines.actions.githubusercontent.com:443  # Actions pipelines
tokens.actions.githubusercontent.com:443     # Actions tokens
github-cloud.githubusercontent.com:443       # GitHub cloud content
github-cloud.s3.amazonaws.com:443            # GitHub S3 storage
media.githubusercontent.com:443              # GitHub media content
archive.ubuntu.com:443                       # Ubuntu package archives
security.ubuntu.com:443                      # Ubuntu security updates
azure.archive.ubuntu.com:443                 # Azure Ubuntu mirrors
ports.ubuntu.com:443                         # Ubuntu ports archives
changelogs.ubuntu.com:443                    # Ubuntu changelogs
esm.ubuntu.com:443                           # Ubuntu ESM (extended security)
motd.ubuntu.com:443                          # Ubuntu MOTD updates
astral.sh:443                                # UV/Ruff tooling
proxy.golang.org:443                         # Go module proxy (shfmt pre-commit hook)
sum.golang.org:443                           # Go checksum database
storage.googleapis.com:443                   # Go module downloads (via proxy)
registry.npmjs.org:443                       # npm packages (actionlint hook)
api.snapcraft.io:443                         # Ubuntu snap API (runner infra)
static.rust-lang.org:443                     # Rust toolchain downloads
crates.io:443                                # Rust crate registry
index.crates.io:443                          # Rust crate index
static.crates.io:443                         # Rust crate downloads
fulcio.sigstore.dev:443                      # Sigstore certificate authority
rekor.sigstore.dev:443                       # Sigstore transparency log
www.bestpractices.dev:443                    # OpenSSF Best Practices
oss-fuzz-build-logs.storage.googleapis.com:443  # OSS-Fuzz build logs
api.osv.dev:443                              # OSV vulnerability database
api.deps.dev:443                             # deps.dev API
tuf-repo-cdn.sigstore.dev:443                # Sigstore TUF repository
tuf-repo.github.com:443                      # GitHub TUF repository
tmaproduction.blob.core.windows.net:443      # GitHub attestation bundles
production.cloudfront.docker.com:443         # Docker image content
deb.debian.org:80                            # Debian packages
security.debian.org:80                       # Debian security updates
releases.astral.sh:443                       # Astral tool releases
registry-1.docker.io:443                     # Docker registry
auth.docker.io:443                           # Docker authentication
pkg-containers.githubusercontent.com:443     # GitHub container packages
timestamp.sigstore.dev:443                   # Sigstore timestamp authority
go.dev:443                                   # Go tool downloads
dl.google.com:443                            # Google tool downloads
```

#### `CI_ALLOWED_ENDPOINTS`

```
artifactcache.actions.githubusercontent.com:443              # Actions cache
github-releases.githubusercontent.com:443                    # GitHub release downloads
launch.actions.githubusercontent.com:443                     # Actions launch
results-receiver.actions.githubusercontent.com:443           # Actions results
release-assets.githubusercontent.com:443                     # Release assets
hosted-compute-request-orchestrator-prod-iad-01.githubapp.com:443  # Runner orchestration
hosted-compute-request-orchestrator-prod-iad-02.githubapp.com:443  # Runner orchestration
hosted-compute-watchdog-prod-iad-01.githubapp.com:443        # Runner watchdog
hosted-compute-watchdog-prod-iad-02.githubapp.com:443        # Runner watchdog
packages.microsoft.com:443                                   # Microsoft packages
sh.rustup.rs:443                                             # Rust toolchain installer
static.rust-lang.org:443                                     # Rust toolchain downloads
crates.io:443                                                # Rust crate registry
index.crates.io:443                                          # Rust crate index
static.crates.io:443                                         # Rust crate downloads
pypi.org:443                                                 # Python packages
files.pythonhosted.org:443                                   # Python package files
capnproto.org:443                                            # Cap'n Proto compiler
packages.nautechsystems.io:443                               # Nautech packages
test-data.nautechsystems.io:443                              # Nautech test data
formulae.brew.sh:443                                         # Homebrew formulae
community.chocolatey.org:443                                 # Chocolatey community
chocolatey.org:443                                           # Chocolatey packages
packages.chocolatey.org:443                                  # Chocolatey downloads
archive.ubuntu.com:80                                        # Ubuntu archives (HTTP)
security.ubuntu.com:80                                       # Ubuntu security (HTTP)
azure.archive.ubuntu.com:80                                  # Azure Ubuntu (HTTP)
ports.ubuntu.com:80                                          # Ubuntu ports (HTTP)
fulcio.sigstore.dev:443                                      # Sigstore certificate authority
rekor.sigstore.dev:443                                       # Sigstore transparency log
codspeed.io:443                                              # CodSpeed benchmarking
api.codspeed.io:443                                          # CodSpeed API
prod-codspeed-storagestack-reportbucket06758118-r1k0it05uytl.s3.eu-west-1.amazonaws.com:443  # CodSpeed reports
api.osv.dev:443                                              # OSV vulnerability database
d2glxqk2uabbnd.cloudfront.net:443                            # CI download CDN
d5l0dvt14r5h8.cloudfront.net:443                             # CI download CDN
```

#### `SECURITY_AUDIT_ALLOWED_ENDPOINTS`

```
static.rust-lang.org:443                     # Rust toolchain downloads
crates.io:443                                # Rust crate registry
index.crates.io:443                          # Rust crate index
static.crates.io:443                         # Rust crate downloads
pypi.org:443                                 # Python packages
files.pythonhosted.org:443                   # Python package files
api.osv.dev:443                              # OSV vulnerability database
release-assets.githubusercontent.com:443     # GitHub release assets
```

#### Azure runner infrastructure

GitHub-hosted runners contact Azure infrastructure at fixed IPs that are allowed by default
at the VM level and do not need to be in the allow lists:

- `168.63.129.16:80` -- Azure IMDS/wireserver (DHCP, DNS forwarding, health probes)
- `168.63.129.16:53` -- Azure DNS resolver

**Action Update Policy**: When updating GitHub Actions, only use versions that have been released for at least 2 weeks.
This allows time for the community to identify potential issues while maintaining security through timely updates.

For updates or changes to actions or workflows, please adhere to the repository's
CONTRIBUTING guidelines and maintain these security best practices.
