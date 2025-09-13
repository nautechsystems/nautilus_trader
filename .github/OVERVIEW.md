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

- **build.yml**: runs pre-commit, Rust tests, Python tests, builds wheels on multiple platforms, and uploads wheel artifacts.
- **build-docs.yml**: dispatches a repository event to trigger the documentation build on `master` and `nightly` pushes.
- **codeql-analysis.yml**: schedules and runs CodeQL security scans on pull requests and periodically via cron.
- **coverage.yml**: (optional) coverage report generation for the `nightly` branch.
- **docker.yml**: builds and pushes Docker images (`nautilus_trader`, `jupyterlab`) for `master` and `nightly` branches using Buildx and QEMU.
- **nightly-merge.yml**: automatically merges `develop` into `nightly` when the latest `develop` workflows succeed.
- **performance.yml**: runs Rust/Python performance benchmarks on the `nightly` branch and reports to CodSpeed.

## Security

- **Immutable action pinning**: all third-party actions are pinned to specific commit SHAs to guarantee immutability and reproducibility.
- **Hardened runners**: most workflows employ `step-security/harden-runner` with an `egress-policy: audit` to reduce attack surface and monitor outbound traffic.
- **Secret management**: no secrets or credentials are stored in the repo. AWS, PyPI, and other credentials are provided via GitHub Secrets and injected at runtime.
- **Code scanning**: CodeQL is enabled for continuous security analysis.
- **Dependency pinning**: key tools (pre-commit, Python versions, Rust toolchain, cargo-nextest) are locked to fixed versions or SHAs.
- **Least-privilege tokens**: workflows default the `GITHUB_TOKEN` to
  `contents: read, actions: read` and selectively elevate scopes (e.g.
  `contents: write`) only for the jobs that need to tag a release or upload
  assets. This follows the principle of least privilege and limits blast
  radius if a job is compromised.
- **Caching**: caches for sccache, pip/site-packages, pre-commit, and test data speed up workflows while preserving hermetic builds.

**Action Update Policy**: When updating GitHub Actions, only use versions that have been released for at least 2 weeks.
This allows time for the community to identify potential issues while maintaining security through timely updates.

For updates or changes to actions or workflows, please adhere to the repository's
CONTRIBUTING guidelines and maintain these security best practices.
