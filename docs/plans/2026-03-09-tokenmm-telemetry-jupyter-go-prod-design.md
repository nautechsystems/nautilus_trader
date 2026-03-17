# TokenMM Telemetry + Jupyter Go-Prod Design

## Overview

Merged `main` now carries the TokenMM telemetry persistence path, the localhost-only JupyterLab research surface,
and the production deploy contracts needed to replace the currently running live stack without losing the active
seven-strategy topology.

## Telemetry Flow

1. Live TokenMM services write local SQLite telemetry under `/var/lib/nautilus/telemetry/tokenmm`.
2. The core tables are `order_action`, `execution_fill`, and `quote_cycle`.
3. The optional telemetry shipper moves those local files into Postgres `telemetry.*` tables for longer retention
   and easier SQL access.

## Localhost-only JupyterLab

The JupyterLab service is intentionally separate from `flux-tokenmm.target`.

- It binds to `127.0.0.1` only.
- It uses `research/tokenmm` as the notebook root.
- The example notebook loads `orders.sqlite`, `fills.sqlite`, and `quote_cycles.sqlite`.

## Cutover

1. Bootstrap the managed sink with `sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh --apply-host-env`.
2. Install or refresh the systemd artifacts with `sudo ops/scripts/deploy/install_tokenmm_systemd.sh`.
3. Create `/var/lib/nautilus/telemetry/tokenmm`.
4. Run `ops/scripts/deploy/tokenmm_telemetry_cutover.py` once the telemetry shipper is healthy.
5. Verify local SQLite row growth and Pulse health before declaring the cutover complete.

## Operator Notes

- Keep the Flux API and localhost-only JupyterLab on loopback unless you add a secure edge.
- Use the telemetry shipper only after local SQLite persistence is confirmed healthy.
- Treat `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md` as the source of truth for Postgres shipping commands.
