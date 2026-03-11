# TokenMM Telemetry RDS Runbook

This runbook covers shipping local TokenMM SQLite telemetry into Postgres.

## Local sources

Expected local files under `/var/lib/nautilus/telemetry/tokenmm`:

- `orders.sqlite` with `order_action`
- `fills.sqlite` with `execution_fill`
- `quote_cycles.sqlite` with `quote_cycle`

Smoke check them before shipping:

```bash
sqlite3 /var/lib/nautilus/telemetry/tokenmm/orders.sqlite "SELECT COUNT(*) FROM order_action;"
sqlite3 /var/lib/nautilus/telemetry/tokenmm/fills.sqlite "SELECT COUNT(*) FROM execution_fill;"
sqlite3 /var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite "SELECT COUNT(*) FROM quote_cycle;"
```

## Shipper config

Create a local config file such as `deploy/tokenmm/tokenmm.telemetry_shipper.toml`:

```toml
[telemetry_shipper]
enabled = true
source_profile = "tokenmm"
orders_db_path = "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"
fills_db_path = "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"
quote_cycles_db_path = "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite"
state_db_path = "/var/lib/nautilus/telemetry/tokenmm/shipper_state.sqlite"
poll_interval_ms = 1000
max_batch_size = 1000
prune_retention_hours = 168
```

Export the Postgres connection settings:

```bash
export NAUTILUS_TELEMETRY_PG_HOST=<rds-host>
export NAUTILUS_TELEMETRY_PG_PORT=5432
export NAUTILUS_TELEMETRY_PG_DATABASE=<database>
export NAUTILUS_TELEMETRY_PG_SCHEMA=telemetry
export NAUTILUS_TELEMETRY_PG_USERNAME=<user>
export NAUTILUS_TELEMETRY_PG_PASSWORD=<password>
export NAUTILUS_TELEMETRY_PG_SSLMODE=require
export POSTGRES_URL="postgresql://${NAUTILUS_TELEMETRY_PG_USERNAME}:${NAUTILUS_TELEMETRY_PG_PASSWORD}@${NAUTILUS_TELEMETRY_PG_HOST}:${NAUTILUS_TELEMETRY_PG_PORT}/${NAUTILUS_TELEMETRY_PG_DATABASE}?sslmode=${NAUTILUS_TELEMETRY_PG_SSLMODE}"
```

## Bootstrap and run

Bootstrap the `telemetry` schema and tables:

```bash
python -m nautilus_trader.persistence.shipper.run \
  --config deploy/tokenmm/tokenmm.telemetry_shipper.toml \
  --bootstrap-postgres
```

Run one ship pass:

```bash
python -m nautilus_trader.persistence.shipper.run \
  --config deploy/tokenmm/tokenmm.telemetry_shipper.toml \
  --once
```

Run continuously:

```bash
python -m nautilus_trader.persistence.shipper.run \
  --config deploy/tokenmm/tokenmm.telemetry_shipper.toml
```

## Verify in Postgres

```bash
psql "$POSTGRES_URL" -c "SELECT COUNT(*) FROM telemetry.order_action;"
psql "$POSTGRES_URL" -c "SELECT COUNT(*) FROM telemetry.execution_fill;"
psql "$POSTGRES_URL" -c "SELECT COUNT(*) FROM telemetry.quote_cycle;"
```

Useful spot checks:

```bash
psql "$POSTGRES_URL" -c "SELECT strategy_id, action_type, reason_code, ts_event FROM telemetry.order_action ORDER BY ts_event DESC LIMIT 20;"
psql "$POSTGRES_URL" -c "SELECT strategy_id, trade_id, quote_cycle_id, ts_event FROM telemetry.execution_fill ORDER BY ts_event DESC LIMIT 20;"
psql "$POSTGRES_URL" -c "SELECT strategy_id, quote_cycle_id, quote_cycle_event, reason_code FROM telemetry.quote_cycle ORDER BY created_at DESC LIMIT 20;"
```
