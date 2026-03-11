# Ops

Deployment assets, environment templates, operational scripts, and runbooks live here.

- `ops/scripts/deploy/`: canonical deploy and local stack orchestration scripts.
- `deploy/`: configuration assets, templates, and operator-facing deployment documentation.
- `docs/runbooks/production-host-disk-recovery.md`: recorded March 9, 2026 host cleanup and before/after capacity measurements.
- `docs/runbooks/ec2-host-baseline.md`: expected journald, Docker, CloudWatch, and Fluent Bit baseline for Flux EC2 boxes.
- `docs/runbooks/ec2-log-and-disk-rollout.md`: rollout order and alert thresholds for applying the host baseline.
