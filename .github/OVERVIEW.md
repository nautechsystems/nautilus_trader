<!--
  README for the .github directory: composite actions and workflow definitions.
-->
# GitHub Actions Overview

This directory contains reusable composite actions and workflow definitions for
CI/CD, testing, publishing, and automation within the NautilusTrader repository.

## Composite actions (`.github/actions`)

- **cargo-tool-install**: installs cargo tools (cargo-deny, cargo-vet) with caching.
- **common-setup**: prepares the environment (OS packages, Rust toolchain, Rust cache, Python, prek, swap space).
- **common-test-data**: caches large test data under `tests/test_data/large`.
- **common-wheel-build**: builds and installs Python wheels across Linux, macOS, and Windows for multiple Python versions.
- **install-capnp**: installs the Cap'n Proto compiler with caching across Linux, macOS, and Windows.
- **publish-wheels**: publishes built wheels to Cloudflare R2, manages old wheel cleanup and index generation.
- **upload-artifact-wheel**: uploads the latest wheel artifact to GitHub Actions.

## Workflows (`.github/workflows`)

- **build.yml**: main CI pipeline - plan, pre-commit, cargo-deny, Rust tests, Python tests, wheel builds, and artifact uploads. Uses Depot 8-core runners for Linux and Windows builds. Includes a plan step that skips builds on docs-only changes and skips Rust tests on Python-only changes.
- **build-v2.yml**: CI pipeline for the v2 Rust-native system. Uses Depot 8-core runners for Linux builds.
- **build-docs.yml**: dispatches documentation build on `master` and `nightly` pushes.
- **cli-binaries.yml**: builds and publishes CLI binaries for multiple platforms.
- **codeql-analysis.yml**: CodeQL security scans for Python and Rust on PRs and via cron.
- **copilot-setup-steps.yml**: environment setup for GitHub Copilot coding agent.
- **coverage.yml**: coverage report generation, currently paused and runs only on `workflow_dispatch`.
- **docker.yml**: builds and pushes multi-platform Docker images (`nautilus_trader`, `jupyterlab`) using Buildx and native ARM runners.
- **nightly-docs-features-check.yml**: nightly docs.rs build checks and crate feature compatibility verification.
- **nightly-merge.yml**: auto-merges `develop` into `nightly` when CI succeeds.
- **nightly-tests.yml**: extended test suites too slow for PR builds - turmoil network tests plus macOS, Windows, and Linux ARM build-and-test jobs that run daily at 12:00 UTC to give early visibility on develop before `nightly-merge` at 14:00 UTC.
- **performance.yml**: Rust/Python benchmarks on `nightly`, reporting to CodSpeed.
- **security-audit.yml**: nightly supply chain security checks (cargo-audit, cargo-deny, cargo-vet, osv-scanner).
- **trigger-reindexing.yml**: triggers documentation reindexing for search.

## Security

### Access controls

- **CODEOWNERS**: Critical infrastructure files (workflows, dependencies, build configs, scripts) require Core team review before merge.
- **Branch protection**: The develop branch requires PR reviews with CODEOWNERS enforcement and passing CI checks. External PRs must receive Core team approval before merge.
- **Least-privilege tokens**: Workflows default `GITHUB_TOKEN` to `contents: read, actions: read` and selectively elevate scopes only for jobs that need them.
- **Secret management**: No secrets or credentials are stored in the repo. Credentials are provided via GitHub Secrets and injected at runtime.

### Dependency security

- **cargo-deny**: Rust dependency auditing for security advisories (RUSTSEC/GHSA), license compliance, banned crates, and supply chain integrity. Configuration in `deny.toml`.
- **Dependency pinning**: Key tools (prek, Python versions, Rust toolchain, cargo-nextest, uv) are locked to fixed versions or SHAs. The uv version is pinned via `required-version` in `pyproject.toml` and extracted by `scripts/uv-version.sh` for CI, Docker, and local builds.
- **Dependency cooldown**: Python dependency resolution excludes packages published within the last 3 days (`exclude-newer = "3 days"` in `[tool.uv]`). This gives the community time to detect and quarantine compromised releases before they enter the lockfile.
- **Code scanning**: CodeQL is enabled for continuous security analysis of Python and Rust code on all PRs and weekly via cron.

### Build integrity

- **Build attestations**: All published artifacts include cryptographic SLSA build provenance attestations, linking each artifact to a specific commit SHA. Verify via `gh attestation verify`.
- **Immutable action pinning**: All third-party GitHub Actions are pinned to specific commit SHAs.
- **Docker image pinning**: Base images in Dockerfiles and service containers in workflows are pinned to SHA256 digests to prevent supply-chain attacks via tag mutation.
- **Caching**: Rust target directory cache (`Swatinem/rust-cache`), prek hook environments, and test data caches speed up workflows while preserving hermetic (reproducible) builds. Rust cache saves are restricted to push events to prevent PR cache pollution.
- **Concurrency**: PR CI runs are cancelled when a new push arrives to the same PR. Push events to mainline branches are never cancelled.
- **Runners**: Linux and Windows builds use Depot 8-core runners (32 GB RAM, 150 GB SSD). macOS builds use GitHub free runners. Lightweight jobs (plan, cargo-deny, cargo-vet, publish) use GitHub free runners. Custom runner labels are declared in `.github/actionlint.yaml`.

### Runtime hardening

- **Hardened runners**: All workflows employ `step-security/harden-runner` to reduce attack surface and
  monitor outbound traffic. All workflows default `egress-policy` to `block`. Set
  `STEP_SECURITY_EGRESS_POLICY=audit` only as a temporary rollback while expanding an allow list. Jobs that
  declare a GitHub Environment can override the repo or org value with an environment-scoped variable. The
  publish environments (`r2-develop`, `r2-nightly`, `release`) can use this override too. The
  `security-audit.yml` workflow also reads its allow list from GitHub Environments so it can validate
  branch changes before promoting the same settings to scheduled runs on the default branch.
- **Fork PR handling**: `build.yml` falls back to `egress-policy: audit` for fork PRs. Forks cannot
  access repo or org variables, so the allow lists would be empty and block all network access. Fork PRs
  run with read-only permissions and no access to secrets, so audit mode is safe.

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
All three variables are stored in GitHub as single-line, space-delimited values. The pinned
`step-security/harden-runner` version does not enforce newline-delimited values correctly
in `block` mode.

All workflows read these GitHub variables:

- `STEP_SECURITY_EGRESS_POLICY`: StepSecurity egress mode for the job. Workflows default to `block`. Set
  `audit` only as a temporary override while expanding an allow list.
- `COMMON_ALLOWED_ENDPOINTS`: Endpoints needed by every job (GitHub API, Ubuntu packages, tooling).
- `CI_ALLOWED_ENDPOINTS`: Extra endpoints shared by the main CI, nightly, docs, and release workflows.
- `SECURITY_AUDIT_ALLOWED_ENDPOINTS`: Extra endpoints needed by the security audit jobs.

Some workflows add job-specific endpoints inline (e.g., `upload.pypi.org:443` for publishing,
`auth.docker.io:443` and `registry-1.docker.io:443` for Docker builds).

Use the `security-audit` environment for the default branch and `master`. Use `security-audit-test` for
branch tests such as `test-security`.

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
