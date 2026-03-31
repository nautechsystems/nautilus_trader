# TokenMM Telemetry RDS Runbook

This runbook covers the current lean default of `S3 + Athena` archival plus the optional
RDS path for environments that still need PostgreSQL.

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

The supported production path is the shared live config plus the managed bootstrap script:

```bash
export TOKENMM_DEPLOY_ROOT=/absolute/path/to/deploy-root
cd "${TOKENMM_DEPLOY_ROOT}"
sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" \
  ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh --apply-host-env
```

That updates `/etc/flux/common.env` with `TOKENMM_AWS_REGION`,
`NAUTILUS_TELEMETRY_PG_SECRET_ID`, and the active endpoint metadata. The shipper
loads the actual credentials from AWS Secrets Manager at runtime.

The live shared config keeps local SQLite as a short spool and tags the archive policy:

```toml
[telemetry_shipper]
enabled = true
source_profile = "tokenmm"
durable_sink = "postgres"
orders_db_path = "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"
fills_db_path = "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"
quote_cycles_db_path = "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite"
state_db_path = "/var/lib/nautilus/telemetry/tokenmm/shipper_state.sqlite"
poll_interval_ms = 1000
max_batch_size = 1000
prune_retention_hours = 48
raw_quote_cycle_local_hours = 48
raw_quote_cycle_s3_days = 7
core_history_s3_days = 365
```

Reference env fragment:

```bash
cat deploy/tokenmm/systemd/tokenmm-telemetry-rds.env.example
```

## Bootstrap and run

Bootstrap the `telemetry` schema and tables:

```bash
ops/scripts/deploy/run_tokenmm_telemetry_shipper.sh \
  --config deploy/tokenmm/tokenmm.live.toml \
  --bootstrap-postgres
```

Run a one-time cutover after the sink is healthy:

```bash
sudo systemctl start flux@tokenmm-telemetry-shipper.service
sudo .venv/bin/python ops/scripts/deploy/tokenmm_telemetry_cutover.py \
  --wait-for-catchup \
  --archive-quote-cycles \
  --archive-s3-bucket tokenmm-telemetry-archive
```

That stages Parquet locally, uploads it to `S3`, creates the Athena table if needed, and
registers the exact partition written by the cutover so queries work without manual repair.

## Verify in Athena

```bash
aws athena start-query-execution \
  --work-group primary \
  --query-string "SELECT strategy_id, action_type, reason_code, ts_event FROM nautilus_telemetry.tokenmm_order_action ORDER BY ts_event DESC LIMIT 20;" \
  --result-configuration OutputLocation=s3://tokenmm-telemetry-archive/nautilus/telemetry/tokenmm/athena-query-results/
```

```bash
aws athena start-query-execution \
  --work-group primary \
  --query-string "SELECT strategy_id, quote_cycle_id, quote_cycle_event, reason_code, created_at FROM nautilus_telemetry.tokenmm_quote_cycle ORDER BY created_at DESC LIMIT 20;" \
  --result-configuration OutputLocation=s3://tokenmm-telemetry-archive/nautilus/telemetry/tokenmm/athena-query-results/
```

If you need an ad hoc `POSTGRES_URL` for `psql`, load the secret first:

```bash
raw="$(aws secretsmanager get-secret-value \
  --region "${TOKENMM_AWS_REGION:-ap-southeast-1}" \
  --secret-id "${NAUTILUS_TELEMETRY_PG_SECRET_ID}" \
  --query SecretString \
  --output text)"
export NAUTILUS_TELEMETRY_PG_HOST="$(printf '%s' "${raw}" | jq -r '.host')"
export NAUTILUS_TELEMETRY_PG_PORT="$(printf '%s' "${raw}" | jq -r '.port')"
export NAUTILUS_TELEMETRY_PG_DATABASE="$(printf '%s' "${raw}" | jq -r '.database')"
export NAUTILUS_TELEMETRY_PG_SCHEMA="$(printf '%s' "${raw}" | jq -r '.schema')"
export NAUTILUS_TELEMETRY_PG_USERNAME="$(printf '%s' "${raw}" | jq -r '.username')"
export NAUTILUS_TELEMETRY_PG_PASSWORD="$(printf '%s' "${raw}" | jq -r '.password')"
export NAUTILUS_TELEMETRY_PG_SSLMODE="$(printf '%s' "${raw}" | jq -r '.sslmode // "require"')"
export POSTGRES_URL="postgresql://${NAUTILUS_TELEMETRY_PG_USERNAME}:${NAUTILUS_TELEMETRY_PG_PASSWORD}@${NAUTILUS_TELEMETRY_PG_HOST}:${NAUTILUS_TELEMETRY_PG_PORT}/${NAUTILUS_TELEMETRY_PG_DATABASE}?sslmode=${NAUTILUS_TELEMETRY_PG_SSLMODE}"
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
