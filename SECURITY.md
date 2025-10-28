# Security Policy

At NautilusTrader, we take security seriously and appreciate your efforts in
helping us identify and fix any vulnerabilities. If you have discovered a
security vulnerability, follow the guidelines outlined below.

## Reporting a Vulnerability

**Preferred method:** [GitHub Security Advisories](https://github.com/nautechsystems/nautilus_trader/security/advisories/new)

This allows private disclosure and coordination before public release. You'll
receive credit in the security advisory and release notes.

**Alternative:** Email <info@nautechsystems.io>

For sensitive reports via email, you may request our PGP key for encrypted communication.

## Response Timeline

We commit to:

- **Initial response**: Within 48 hours of report submission
- **Status update**: Within 7 days with initial assessment
- **Fix timeline**: Critical vulnerabilities patched within 30 days; other issues within 90 days
- **Coordinated disclosure**: We'll work with you to agree on a public disclosure date

## Responsible Disclosure

We encourage responsible disclosure of any security vulnerabilities you may
discover. Please provide us with a reasonable amount of time to fix the issue
before disclosing it publicly. We will acknowledge your contribution in our
security advisories and release notes unless you prefer to remain anonymous.

## Supported Versions

We only support the latest version of NautilusTrader. If you are using an older
version, it is possible that vulnerabilities may have been fixed in a later
release.

## Bug Bounty Program

At this time, we do not have a formal bug bounty program. We
appreciate any efforts to help us improve the security of our platform and will
do our best to properly recognize and credit your contributions.

## Security Infrastructure

NautilusTrader employs multiple layers of security to protect against supply
chain attacks and vulnerabilities:

- **Dependency auditing**: Automated security scanning via cargo-deny (Rust) and Dependabot alerts (Python)
- **Code scanning**: CodeQL static analysis for Python and Rust code
- **CODEOWNERS**: Critical infrastructure files require Core team review before merge
- **Branch protection**: Develop branch requires PR reviews and passing CI checks
- **Immutable action pinning**: GitHub Actions pinned to commit SHAs for reproducibility
- **Hardened runners**: Network egress monitoring and least-privilege tokens
- **License compliance**: Automated checks ensuring LGPL-3.0 compatibility

For detailed security practices, see [.github/OVERVIEW.md](.github/OVERVIEW.md#security).
