<!--
  README for the .github directory: composite actions and workflow definitions.
-->
# GitHub Actions Overview

This directory contains reusable composite actions and workflow definitions for
CI/CD, testing, publishing, and automation within the NautilusTrader repository.

## Composite actions (`.github/actions`)

- **common-setup**: prepares the environment (OS packages, Rust toolchain, Python, sccache, pre-commit).
- **common-test-data**: caches large test data under `tests/test_data/large`.
- **common-wheel-build**: builds and installs Python wheels across Linux, macOS, and Windows for multiple Python versions.
- **publish-wheels**: publishes built wheels to Cloudflare R2, manages old wheel cleanup and index generation.
- **upload-artifact-wheel**: uploads the latest wheel artifact to GitHub Actions.

## Workflows (`.github/workflows`)

- **build.yml**: main CI pipeline - pre-commit, cargo-deny, Rust tests, Python tests, wheel builds, and artifact uploads.
- **build-v2.yml**: CI pipeline for the v2 Rust-native system.
- **build-docs.yml**: dispatches documentation build on `master` and `nightly` pushes.
- **cli-binaries.yml**: builds and publishes CLI binaries for multiple platforms.
- **codeql-analysis.yml**: CodeQL security scans for Python and Rust on PRs and via cron.
- **copilot-setup-steps.yml**: environment setup for GitHub Copilot coding agent.
- **coverage.yml**: coverage report generation for the `nightly` branch.
- **docker.yml**: builds and pushes Docker images (`nautilus_trader`, `jupyterlab`) using Buildx and QEMU.
- **nightly-merge.yml**: auto-merges `develop` into `nightly` when CI succeeds.
- **performance.yml**: Rust/Python benchmarks on `nightly`, reporting to CodSpeed.
- **trigger-reindexing.yml**: triggers documentation reindexing for search.

## Security

### Access controls

- **CODEOWNERS**: Critical infrastructure files (workflows, dependencies, build configs, scripts) require Core team review before merge.
- **Branch protection**: The develop branch requires PR reviews with CODEOWNERS enforcement and passing CI checks. External PRs must receive Core team approval before merge.
- **Least-privilege tokens**: Workflows default `GITHUB_TOKEN` to `contents: read, actions: read` and selectively elevate scopes only for jobs that need them.
- **Secret management**: No secrets or credentials are stored in the repo. Credentials are provided via GitHub Secrets and injected at runtime.

### Dependency security

- **cargo-deny**: Rust dependency auditing for security advisories (RUSTSEC/GHSA), license compliance, banned crates, and supply chain integrity. Configuration in `deny.toml`.
- **Dependency pinning**: Key tools (pre-commit, Python versions, Rust toolchain, cargo-nextest) are locked to fixed versions or SHAs.
- **Code scanning**: CodeQL is enabled for continuous security analysis of Python and Rust code on all PRs and weekly via cron.

### Build integrity

- **Build attestations**: All published artifacts include cryptographic SLSA build provenance attestations, linking each artifact to a specific commit SHA. Verify via `gh attestation verify`.
- **Immutable action pinning**: All third-party GitHub Actions are pinned to specific commit SHAs.
- **Docker image pinning**: Base images in Dockerfiles are pinned to SHA256 digests to prevent supply-chain attacks via tag mutation.
- **Caching**: Caches for sccache, pip/site-packages, pre-commit, and test data speed up workflows while preserving hermetic (reproducible) builds.

### Runtime hardening

- **Hardened runners**: Most workflows employ `step-security/harden-runner` with `egress-policy: audit` to reduce attack surface and monitor outbound traffic.

### Allowed network endpoints

The `step-security/harden-runner` action restricts network access to approved endpoints.
Common endpoints are maintained in the variable `COMMON_ALLOWED_ENDPOINTS`:

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
astral.sh:443                                # UV/Ruff tooling
```

Job-specific endpoints (e.g., `pypi.org:443` for publishing jobs) are added inline within each workflow.

**Action Update Policy**: When updating GitHub Actions, only use versions that have been released for at least 2 weeks.
This allows time for the community to identify potential issues while maintaining security through timely updates.

For updates or changes to actions or workflows, please adhere to the repository's
CONTRIBUTING guidelines and maintain these security best practices.
