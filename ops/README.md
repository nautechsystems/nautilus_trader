# Ops

Deployment assets, environment templates, operational scripts, and runbooks live here.

At the repo level, `flux` is the canonical deploy and operator-facing name. Runtime engine imports remain `nautilus_trader`.

Operational naming contract:

- deploy lanes and release roots use `flux` naming, for example `~/releases/<lane>/<stack>/current`
- service groups and Pulse surfaces use Flux-facing names such as `tokenmm`, `equities`, and `equities-pilot`
- deploy scripts must launch engine and system entrypoints from immutable release roots, not from mutable repo checkouts

- `ops/scripts/deploy/`: canonical deploy and local stack orchestration scripts.
- `deploy/`: configuration assets, templates, and operator-facing deployment documentation.
- `docs/runbooks/deploy-lanes.md`: canonical lane contract for `dev`, `pilot`, and `prod`.
- `docs/runbooks/equities-pilot-rollout.md`: equities-specific pilot workflow and rollout contract.
- `docs/runbooks/production-host-disk-recovery.md`: recorded March 9, 2026 host cleanup and before/after capacity measurements.
- `docs/runbooks/ec2-host-baseline.md`: expected journald, Docker, CloudWatch, and Fluent Bit baseline for Flux EC2 boxes.
- `docs/runbooks/ec2-log-and-disk-rollout.md`: rollout order and alert thresholds for applying the host baseline.
