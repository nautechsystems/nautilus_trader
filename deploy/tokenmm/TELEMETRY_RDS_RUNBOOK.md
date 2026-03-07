# TokenMM Telemetry RDS Runbook

This runbook provisions and bootstraps the production execution telemetry sink for TokenMM.

## Topology

- Trading nodes persist local telemetry to SQLite under `/var/lib/nautilus/telemetry/tokenmm`.
- `tokenmm-telemetry-shipper` reads those local SQLite files and ships rows into one RDS PostgreSQL sink.
- Redis remains latest-only for live operational state; historical portfolio reconciliation comes
  from the shipped RDS tables.
- Local TokenMM telemetry files include `fills.sqlite`, `orders.sqlite`, `quote_cycles.sqlite`,
  `balance_snapshots.sqlite`, and `portfolio_inventory.sqlite`.
- TokenMM and future equities profiles share one physical Postgres database. Segment analysis by
  `source_profile`, `strategy_id`, `trader_id`, and `account_id`.
- Sink-side idempotency is keyed by `source_profile` plus the local event identity, so rows from
  different profiles do not collide in the shared database.

## RDS target

- Engine: `PostgreSQL 16`
- Database: `nautilus_telemetry`
- Schema: `telemetry`
- Connectivity: private only, security-group allowlist limited to trading hosts
- Availability: Multi-AZ enabled
- Backups: automated backups enabled
- TLS: required

## Host configuration

Add these to `/etc/flux/common.env`:

```env
NAUTILUS_TELEMETRY_PG_HOST=<rds-endpoint>
NAUTILUS_TELEMETRY_PG_PORT=5432
NAUTILUS_TELEMETRY_PG_DATABASE=nautilus_telemetry
NAUTILUS_TELEMETRY_PG_SCHEMA=telemetry
NAUTILUS_TELEMETRY_PG_USERNAME=<db-user>
NAUTILUS_TELEMETRY_PG_PASSWORD=<db-password>
NAUTILUS_TELEMETRY_PG_SSLMODE=require
```

Store the username/password in AWS Secrets Manager and inject them into `/etc/flux/common.env`
through your normal host bootstrap flow.

## Bootstrap

Run the schema bootstrap once from the repo root on the target host:

```bash
set -a
source /etc/flux/common.env
set +a
python3 -m nautilus_trader.persistence.shipper.run \
  --config deploy/tokenmm/tokenmm.live.toml \
  --bootstrap-postgres
```

Then install or refresh the systemd surfaces:

```bash
sudo ops/scripts/deploy/install_tokenmm_systemd.sh
sudo systemctl daemon-reload
sudo systemctl restart flux-tokenmm.target
```

## Checks

- `systemctl status flux@tokenmm-telemetry-shipper`
- `journalctl -u flux@tokenmm-telemetry-shipper -f`
- `sudo systemctl status flux-tokenmm.target`
- Confirm the local SQLite files exist under `/var/lib/nautilus/telemetry/tokenmm`
- Confirm `deploy/tokenmm/tokenmm.live.toml` sets:
  - `balance_snapshots_db_path`
  - `portfolio_inventory_db_path`
- Confirm the sink tables exist:
  - `telemetry.execution_fill`
  - `telemetry.order_action`
  - `telemetry.quote_cycle`
  - `telemetry.flux_balance_snapshot`
  - `telemetry.flux_balance_snapshot_row`
  - `telemetry.portfolio_inventory_snapshot`

## Notes

- The shipper is best-effort and off the trading hot path.
- Trading nodes do not write directly to RDS.
- Local SQLite retention is 7 days after successful shipping.
- Use one physical Postgres database for TokenMM and future equities. Separate clients and analyses
  by `source_profile` plus the existing strategy/account identifiers.
