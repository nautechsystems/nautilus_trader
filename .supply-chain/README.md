# Supply Chain Audit Coverage

This directory holds the cargo-vet store shared by the Rust manifests in
[security-audit.toml](../security-audit.toml), plus the crate publication dates used by the Cargo cooldown check.

## Exemptions and audit history

The exemptions in [config.toml](config.toml) accept specific baseline versions without complete audit coverage.
Delta audits can build on those baselines to check later changes without claiming a full review of the crate.
A passing cargo-vet check therefore includes fully audited, partially audited, and exempted dependencies.

Historical audits remain useful to consumers of older versions and to chains of delta audits.
A version's absence from the local lockfiles is not a reason to delete its certification.
Some historical entries have no review notes; their certification remains recorded, but the store does not
provide the original review rationale. Add missing rationale only from verifiable review evidence.

## Downstream imports

Importing [audits.toml](audits.toml) does not import our exemptions or our peers' audit sets.
Consumers need their own accepted baseline and direct trust relationships, as described in
[cargo-vet's import documentation](https://mozilla.github.io/cargo-vet/importing-audits.html).
Our lockfile's coverage also does not establish coverage for a consumer's different dependency versions.

Audits whose claims are limited to the enabled feature graph carry `importable = false`.
Cargo-vet does not enforce feature restrictions stated in notes, so these entries remain local rather than
certifying arbitrary downstream feature combinations. Local feature changes still require checking the recorded
scope. Publisher trust entries accept releases by the named publisher within the recorded dates; they do not
represent individual source reviews.

## Maintenance checks

The security gate uses `cargo vet --locked`, so it does not refresh imported audits.
Refresh imports separately and review the resulting changes to [imports.lock](imports.lock).
Before pruning, evaluate both configured manifests: an entry unused by the root graph may still cover the fuzz
graph. Use `cargo vet prune --no-audits` on separate copies of the store and retain coverage needed by either
graph, preserving historical local certifications. Run `make security-audit` against the resulting shared store.

The generator `scripts/check-cargo-cooldown.sh --update-db` maintains [crate-dates.json](crate-dates.json)
from all tracked Cargo lockfiles. Recorded publication dates are immutable; an older verification timestamp
alone does not make an entry stale. Advisory exceptions live in [deny.toml](../deny.toml),
[cargo-audit configuration](../.cargo/audit.toml), and [OSV configuration](../osv-scanner.toml),
separately from cargo-vet exemptions.
