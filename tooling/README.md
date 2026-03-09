# Tooling

Canonical home for repo automation.

## Subdomains

- `tooling/dev/`: local developer helpers, bootstrap scripts, and regeneration utilities.
- `tooling/ci/`: CI-oriented validation and test entrypoints.
- `tooling/release/`: packaging, publishing, and release automation.

Do not add deployment-specific runtime scripts here. Those belong under `ops/`.

## Quality gates

- `tooling/ci/check-repo-structure.sh`: rejects legacy root-path regressions in active repo docs and workflow files, and enforces compatibility-only legacy trees.
- `tooling/ci/check-flux-leakage.sh`: rejects POC/chainsaw naming leakage in durable Flux docs and compatibility surfaces.
