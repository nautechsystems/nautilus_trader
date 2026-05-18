# Security Policy

Security is a priority for the NautilusTrader project, and we value the work of
those who help identify and resolve vulnerabilities. If you have found a
security issue, please follow the guidelines below.

For our full security policies, see <https://nautilustrader.io/security/>.

## Scope

This policy covers:

- NautilusTrader open-source software and official repositories.
- Nautech Systems websites (nautilustrader.io).

Third-party services, exchanges, and data providers are excluded.

## Reporting a Vulnerability

**Preferred method:** [GitHub Security Advisories](https://github.com/nautechsystems/nautilus_trader/security/advisories/new)

This allows private disclosure and coordination before public release. You'll
receive credit in the security advisory and release notes.

**Alternative:** Email <security@nautechsystems.io>

For sensitive reports via email, you may request our PGP key for encrypted communication.

Please include: vulnerability description, reproduction steps, affected versions,
and suggested remediation if available.

## Response Timeline

We commit to:

- **Initial response**: Within 48 hours of report submission.
- **Status update**: Within 7 days with initial assessment.
- **Fix timeline**: Critical vulnerabilities patched within 30 days; other issues within 90 days.
- **Coordinated disclosure**: We'll work with you to agree on a public disclosure date.

## Responsible Disclosure

We encourage responsible disclosure of any security vulnerabilities you may
discover. When reporting, we ask that you:

- Do not publicly disclose the vulnerability before a fix is available.
- Only exploit the issue to the extent necessary to demonstrate it.
- Do not access unauthorized data or disrupt systems.
- Comply with all applicable laws.

We will acknowledge your contribution in our security advisories and release
notes unless you prefer to remain anonymous.

## Supported Versions

We only support the latest version of NautilusTrader. If you are using an older
version, it is possible that vulnerabilities may have been fixed in a later
release.

## Bug Bounty Program

At this time, we do not have a formal bug bounty program. We
appreciate any efforts to help us improve the security of our platform and will
do our best to properly recognize and credit your contributions.

## Security Infrastructure

NautilusTrader employs multiple layers of security across the development and
release lifecycle:

### Source and review controls

- **CODEOWNERS**: Critical infrastructure files, dependency manifests, and lock files require Core
  team review before merge.
- **Branch and tag rulesets**: Protected branches require signed commits and passing CI checks.
  Release tags matching `v*` are immutable after creation.
- **Source restrictions**: Rust packages are sourced exclusively from crates.io. Git dependencies
  and unknown registries are prohibited.

### Dependency intake controls

- **Version pinning and lock files**: Rust dependencies are pinned in `Cargo.lock` with
  cryptographic checksums. Python dependencies are pinned in `uv.lock` and `python/uv.lock` with
  integrity hashes. Wildcard version requirements are prohibited.
- **Dependency and tool cooldown**: Python dependency resolution excludes packages published
  within the last 3 days via `exclude-newer` in `pyproject.toml`. Development tools are pinned to
  explicit versions across `tools.toml`, `Cargo.toml`, and related manifests, and version bumps are
  reviewed during security audits. Rust crate updates are reviewed through our cargo-vet audit
  process and policy. The cooldown gives the community time to detect and quarantine compromised
  releases.
- **Wheel-only Python installs**: The `no-build-package` list in `[tool.uv]` enumerates every
  third-party package locked in `uv.lock` and forbids `uv` from building any of them from source.
  In normal operation uv prefers wheels, so the setting is a no-op; it kicks in only if a listed
  upstream stops publishing wheels for the target platform, in which case `uv lock` fails instead
  of silently building from an sdist. The local workspace package is intentionally absent because it
  must be built by the workspace's own build backend. The `check-no-build-packages` pre-commit hook
  verifies the list stays in lock-step with `uv.lock` on every commit that touches the lock or the
  manifest.
- **Toolchain pinning**: The uv package manager version is pinned via `required-version` in
  `pyproject.toml` and enforced across CI, Docker, and local development. Release and audit tool
  Python CLIs are pinned in `tools.toml`.
- **License compliance**: Automated checks ensure LGPL-3.0-or-later compatibility.

### Pre-merge and scheduled scanning

- **Pre-commit security**: Gitleaks credential screening, private key detection, Zizmor GitHub
  Actions auditing, and Unicode control character detection run before changes land.
- **Dependency auditing**: Automated security scanning runs via cargo-audit, cargo-deny, cargo-vet,
  and OSV Scanner (Rust), pip-audit (Python), and Zizmor (GitHub Actions).
- **Supply chain provenance**: cargo-vet verifies Rust dependency provenance by importing trusted
  audit data from organizations including Bytecode Alliance, Google, Mozilla, and Embark Studios.
- **Code scanning**: CodeQL static analysis covers Python and Rust code on PRs to `master`,
  pushes to `nightly`, and manual dispatch. The scheduled security audit also uploads Zizmor SARIF
  results for GitHub Actions workflow findings when token permissions allow it.

### Build and publish controls

- **Build integrity**: SLSA build provenance attestations for Python release artifacts, GitHub
  release checksum manifests, immutable GitHub release attestations, immutable GitHub Actions
  pinned to commit SHAs, container digest pinning, Docker image signing via Sigstore cosign, SPDX
  SBOM generation and Sigstore attestation for container images, and hardened CI runners with
  network egress blocked to an explicit allow-list.
- **Release sequencing**: Stable releases create a draft GitHub release and attach wheel and sdist
  assets before publishing to package indexes (`packages.nautechsystems.io`, PyPI, crates.io).
  CI verifies the registries, attaches final checksum and provenance assets, then publishes the
  GitHub release and verifies its release attestation. This keeps the GitHub release and checksum
  manifest as the anchor for downstream registry verification while staying compatible with GitHub
  release immutability.
- **Deployment environments**: Release and package publishing jobs use scoped GitHub deployment
  environments (`release`, `r2-develop`, and `r2-nightly`) so publishing credentials and OIDC
  trusted-publisher identities stay isolated from test, lint, and build-only jobs.
- **Publish authentication**: PyPI and crates.io uploads use Trusted Publishing (OIDC) bound to the
  `release` GitHub Environment, eliminating long-lived API tokens. Each publish mints a short-lived
  token scoped to the specific repo, workflow, and environment.
- **GHCR authentication**: Container image pushes to GitHub Container Registry use the short-lived
  `GITHUB_TOKEN` scoped to the workflow run, not a long-lived personal access token.
- **Post-publish verification**: CI verifies PyPI wheels and sdists against the GitHub release
  manifest and expected PyPI publisher identity, verifies crates.io entries were trusted-published
  by this repository, records whether each crate matches the release commit or was already
  published, verifies the final GitHub release attestation, and verifies container image signatures
  and SBOM attestations against the expected GitHub Actions workflow identities after publishing.

### Runtime cryptography

- **Cryptography**: All TLS and cryptographic operations use
  [aws-lc-rs](https://github.com/aws/aws-lc-rs), the Rust binding for AWS-LC. The library runs in
  non-FIPS mode because the FIPS 140-3 module (`aws-lc-fips-sys`) requires the Go toolchain as a
  build dependency. The underlying cryptographic primitives (AES-GCM, SHA-2, ECDSA,
  ChaCha20-Poly1305) are identical in both modes; the FIPS module adds runtime self-tests and
  module boundary enforcement required for federal certification.

For our full supply chain security policy, see <https://nautilustrader.io/security/supply-chain/>.

For detailed CI/CD security practices, see [.github/OVERVIEW.md](.github/OVERVIEW.md#security).

## Known vulnerability management

Where a known advisory exists in a transitive dependency without an available
fix, we document the risk assessment, context, and mitigation in our audit
configuration. Accepted risks are categorized by severity, scope (direct vs.
transitive, runtime vs. dev-time), and monitored for upstream resolution.

When a new vulnerability is identified in a dependency:

- The nightly security audit flags the advisory automatically.
- The Core team assesses severity and exposure within the NautilusTrader context.
- Critical vulnerabilities in direct dependencies are patched or mitigated within 30 days.
- Users are notified through release notes and, where appropriate, security advisories.

Users who build NautilusTrader from source or extend it with additional
dependencies are responsible for auditing their own dependency trees. The
controls described in this policy apply to official NautilusTrader releases and
the canonical repository.

## Advisories addressed

Third-party security advisories we have addressed via dependency upgrades.
Detection lag for each upgrade is gated by the `exclude-newer` cooldown described
above; the cooldown can be bypassed when a CVE warrants immediate response.

- **1.227.0**:
  - [GHSA-mf9v-mfxr-j63j](https://github.com/urllib3/urllib3/security/advisories/GHSA-mf9v-mfxr-j63j):
    `urllib3` decompression-bomb safeguard bypass via `HTTPResponse.drain_conn()` after partial
    decompression, and via the second `read(amt=N)`/`stream(amt=N)` call when Brotli-decompressed.
    Upgraded `urllib3` to v2.7.0.
  - [GHSA-qccp-gfcp-xxvc](https://github.com/urllib3/urllib3/security/advisories/GHSA-qccp-gfcp-xxvc):
    `urllib3` pools created via `ProxyManager.connection_from_url` not stripping headers listed in
    `Retry.remove_headers_on_redirect` when redirecting cross-host. Upgraded `urllib3` to v2.7.0.

## Verifying releases

Python release artifacts and Docker images are signed or attested via Sigstore-backed workflows.
Cargo crates are published through crates.io Trusted Publishing. The release verifier records
whether each crate version was published by the current release commit or already existed from an
earlier trusted-published commit in this repository. You can independently verify artifacts before
installing them.

### Python wheels and sdist

GitHub releases include a generated checksum table, an aggregate `SHA256SUMS`
file, per-asset `.sha256` files, and a machine-readable `dist-manifest.json`
for Python wheels and the sdist. Check the downloaded artifact against one of
those checksum sources before installation.

GitHub releases also include `.sigstore` Sigstore bundles and `.intoto.jsonl`
DSSE envelope siblings for each Python artifact. The GitHub CLI fetches
attestations from the GitHub API by default; use `--bundle <artifact>.sigstore`
to verify against a downloaded Sigstore bundle instead.

After downloading from PyPI or the GitHub release, verify each artifact with the
GitHub CLI. The `--cert-identity-regex` and `--cert-oidc-issuer` flags bind
verification to the `build.yml` release workflow, not just the repository:

```sh
ISSUER=https://token.actions.githubusercontent.com
IDENTITY='^https://github\.com/nautechsystems/nautilus_trader/\.github/workflows/build\.yml@refs/heads/(master|nightly)$'

# `gh attestation verify` takes one subject per call, so loop over wheels
for whl in nautilus_trader-*.whl; do
  gh attestation verify "$whl" \
    --repo nautechsystems/nautilus_trader \
    --cert-identity-regex "$IDENTITY" \
    --cert-oidc-issuer "$ISSUER"
done

gh attestation verify nautilus_trader-*.tar.gz \
  --repo nautechsystems/nautilus_trader \
  --cert-identity-regex "$IDENTITY" \
  --cert-oidc-issuer "$ISSUER"
```

### Docker images

Resolve the mutable tag to an immutable digest first so every check, the
subsequent `docker pull`, and the `docker run` operate on the same image:

```sh
# Use crane (or `docker buildx imagetools inspect <ref> --format '{{.Manifest.Digest}}'`)
DIGEST=$(crane digest ghcr.io/nautechsystems/nautilus_trader:latest)
IMAGE=ghcr.io/nautechsystems/nautilus_trader@${DIGEST}
ISSUER=https://token.actions.githubusercontent.com
IDENTITY='^https://github\.com/nautechsystems/nautilus_trader/\.github/workflows/docker\.yml@refs/heads/(master|nightly)$'
```

Verify the cosign signature, which proves the image was produced by the
NautilusTrader CI workflow:

```sh
cosign verify "$IMAGE" \
  --certificate-identity-regexp "$IDENTITY" \
  --certificate-oidc-issuer "$ISSUER"
```

Verify the SPDX SBOM attestation is bound to the same image digest:

```sh
cosign verify-attestation --type https://spdx.dev/Document/v2.3 "$IMAGE" \
  --certificate-identity-regexp "$IDENTITY" \
  --certificate-oidc-issuer "$ISSUER"
```

The GitHub CLI can also verify the SBOM attestation, but does not check the
cosign image signature, so use it in addition to `cosign verify` above:

```sh
gh attestation verify "oci://${IMAGE}" \
  --repo nautechsystems/nautilus_trader \
  --predicate-type https://spdx.dev/Document/v2.3 \
  --cert-identity-regex "$IDENTITY" \
  --cert-oidc-issuer "$ISSUER"
```
