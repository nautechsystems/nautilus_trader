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

- **build.yml**: runs pre-commit, cargo-deny security checks, Rust tests, Python tests, builds wheels on multiple platforms, and uploads wheel artifacts.
- **build-docs.yml**: dispatches a repository event to trigger the documentation build on `master` and `nightly` pushes.
- **codeql-analysis.yml**: schedules and runs CodeQL security scans for Python and Rust code on pull requests to develop and periodically via cron.
- **coverage.yml**: (optional) coverage report generation for the `nightly` branch.
- **docker.yml**: builds and pushes Docker images (`nautilus_trader`, `jupyterlab`) for `master` and `nightly` branches using Buildx and QEMU.
- **nightly-merge.yml**: automatically merges `develop` into `nightly` when the latest `develop` workflows succeed.
- **performance.yml**: runs Rust/Python performance benchmarks on the `nightly` branch and reports to CodSpeed.

## Security

- **CODEOWNERS**: Critical infrastructure files (workflows, dependencies, build configs, scripts) require Core team review before merge. This prevents unauthorized supply chain modifications and ensures all sensitive changes receive security review.
- **Branch protection**: The develop branch requires PR reviews with CODEOWNERS enforcement and passing CI checks. External PRs must receive Core team approval before merge, while admin bypass is enabled for maintainer flexibility.
- **cargo-deny**: Rust dependency auditing for security advisories (RUSTSEC/GHSA), license compliance (LGPL-3.0 compatibility), banned crates, and supply chain integrity. Runs in CI to block vulnerable or non-compliant dependencies. Configuration in `deny.toml`.
- **Build attestations**: All published artifacts (wheels and source distributions) include cryptographic SLSA build provenance attestations. These prove artifacts were built by the official GitHub Actions workflow and link each artifact to a specific commit SHA, enabling users to verify authenticity via `gh attestation verify`. Attestations are generated for all releases, nightly builds, and develop builds published to PyPI, GitHub Releases, and the Nautech Systems package index.
- **Code scanning**: CodeQL is enabled for continuous security analysis of Python and Rust code. Scans run on all PRs to develop and weekly via cron schedule.
- **Immutable action pinning**: All third-party actions are pinned to specific commit SHAs to guarantee immutability and reproducibility.
- **Hardened runners**: Most workflows employ `step-security/harden-runner` with `egress-policy: audit` to reduce attack surface and monitor outbound traffic.
- **Secret management**: No secrets or credentials are stored in the repo. AWS, PyPI, and other credentials are provided via GitHub Secrets and injected at runtime.
- **Dependency pinning**: Key tools (pre-commit, Python versions, Rust toolchain, cargo-nextest) are locked to fixed versions or SHAs.
- **Least-privilege tokens**: Workflows default the `GITHUB_TOKEN` to
  `contents: read, actions: read` and selectively elevate scopes (e.g.
  `contents: write`) only for the jobs that need to tag a release or upload
  assets. This follows the principle of least privilege and limits blast
  radius if a job is compromised.
- **Caching**: Caches for sccache, pip/site-packages, pre-commit, and test data speed up workflows while preserving hermetic builds.

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
