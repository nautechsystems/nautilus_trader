# Repo standards

These standards define what is allowed in each repo domain and how new structure should evolve.

## Ownership standards

- `engine/` contains reusable platform capabilities.
- `systems/` contains opinionated trading systems built on the platform.
- `apps/` contains operator-facing surfaces.
- `ops/` contains deployment and runtime operations assets.
- `tooling/` contains developer, CI, and release automation.
- `research/` contains non-production work.

If a change does not clearly belong to one of these domains, stop and document the intended owner before adding it.

## Namespace standards

- Use `flux` for product, repo, deploy-lane, and operator-facing naming.
- Keep engine runtime imports under `nautilus_trader.*`.
- Keep Flux runtime imports under `flux.*`.
- Do not add new implementation modules under `nautilus_trader.flux.*`.
- Use compatibility shims only to preserve old callers during migration.
- Remove compatibility layers after callers have been migrated and documented.

## Dependency standards

- `systems/` may depend on `engine/`.
- `apps/` may depend on `systems/` contracts and engine/system APIs.
- `ops/` may reference engine/system/app entrypoints but should not own business logic.
- `tooling/` may call into engine, system, app, or ops entrypoints but should not become the owner of runtime contracts.
- `research/` may depend on any production domain for experimentation, but production domains must not depend on `research/`.

## Documentation standards

- Repo-wide structural rules go in `docs/repo/`.
- Engine internals and extension guidance go in `docs/developer_guide/`.
- System-specific behavior belongs with the owning system.
- App-specific contracts and runbooks belong with the owning app or app-specific docs.
- Tooling usage and ownership should be documented near the canonical script location.
- Structural changes must update the relevant docs in the same change.

## Naming and layout standards

- Favor domain-first placement over language-first placement.
- Favor stable, explicit names over catch-all buckets.
- Do not add placeholder top-level directories for speculative future work.
- Avoid umbrella names like `misc`, `shared`, or `common` at repo root unless there is a clear cross-domain ownership model.
- Add a README at each new ownership boundary that states what belongs there.

## Practical review checklist

Before merging a structural change, confirm:

- the code is in the right ownership domain
- the runtime namespace is still coherent
- compatibility behavior is explicit if old paths still exist
- docs were updated for the new source of truth
- `tooling/ci/check-repo-structure.sh` passes
- no new root-level sprawl was introduced without a reason
