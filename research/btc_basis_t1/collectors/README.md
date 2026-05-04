# Binance market-data collectors (Lane 0D)

Phase 0 calendar-critical work. One collector per `(venue, symbol-set)`, runs as
a systemd-managed Python service on the observability VM. Writes raw NDJSON
shards (one line per WS message) to S3 every 60s.

## Why this design

- **Raw bytes only.** No canonicalization, no schema enforcement. The collector
  cannot corrupt the archive because it does not interpret the data. Phase 1
  applies a canonical layer on top via a separate ETL.
- **systemd `Restart=always`** handles process death.
- **Prometheus freshness alert** handles process-alive-but-stuck.
- **On-disk buffering** survives process restart — completed shards eventually
  upload regardless of process lifecycle.
- **Failed-upload directory** preserves shards when S3 transiently fails.
- **No keys required.** Public WS streams only.

See `../../../ceo-plans/2026-05-03-btc-basis-t1-research.md` §8 Phase 0 Lane 0D
for the broader plan.

## Layout

```
collectors/
├── binance_collector.py             # the worker
├── systemd/
│   ├── binance-collector@.service   # template unit
│   ├── binance-um-btc.env.example   # USDM perpetual env
│   ├── binance-spot-btc.env.example # spot env
│   └── binance-um-btc-quarterly.env.example   # USDM Jun/Sep quarterly
├── alertmanager-rules.yaml          # Prometheus alerting rules
├── tests/
│   └── test_collector.py            # unit + WS-reconnect + S3-retry tests
└── README.md
```

## Local dev (laptop)

Dependencies (pinned floors):
- `boto3 >= 1.34` — S3 uploads
- `websockets >= 12.0` — Binance WS client
- `prometheus_client >= 0.19` — metrics exporter
- `pytest >= 8.0`, `pytest-asyncio >= 0.23` — tests only

```bash
cd research/btc_basis_t1/collectors
uv venv --python 3.12 .venv
uv pip install --python .venv/bin/python \
  'boto3>=1.34' 'websockets>=12.0' 'prometheus_client>=0.19' \
  'pytest>=8.0' 'pytest-asyncio>=0.23'
```

(Or `python3 -m venv .venv && .venv/bin/pip install ...` if you don't have `uv`.)

Run against a real Binance public stream + a local minio (no creds needed):

```bash
docker run -d -p 9000:9000 -p 9001:9001 \
  -e MINIO_ROOT_USER=minio -e MINIO_ROOT_PASSWORD=minio123 \
  --name minio minio/minio server /data --console-address ":9001"

mc alias set local http://localhost:9000 minio minio123
mc mb local/test-bucket

AWS_ACCESS_KEY_ID=minio AWS_SECRET_ACCESS_KEY=minio123 \
  AWS_ENDPOINT_URL=http://localhost:9000 \
  python binance_collector.py \
    --venue umfutures \
    --symbols BTCUSDT \
    --bucket test-bucket \
    --buffer-dir ./tmp/buffer \
    --failed-dir ./tmp/failed \
    --prom-port 9101
```

In another terminal:

```bash
curl -s localhost:9101/metrics | grep collector_
mc ls local/test-bucket/raw/venue=umfutures/
```

You should see `collector_messages_total` ticking up within a few seconds and
NDJSON shards appearing in S3 every 60 seconds.

## Production install (observability VM)

Run via SSM Session Manager:

```bash
aws ssm start-session --target i-<observability-vm-id> --region ap-northeast-1

# Once on the VM:
sudo useradd --system --no-create-home --shell /usr/sbin/nologin collector
sudo mkdir -p /opt/binance-collector /etc/binance-collector \
  /var/lib/binance-collector
sudo chown -R collector:collector /opt/binance-collector \
  /var/lib/binance-collector

# Copy code
# (Use scp via SSM port-forward, or fetch from S3 artifact bucket, or git clone)
sudo cp binance_collector.py /opt/binance-collector/

# Create venv and install deps
sudo python3.12 -m venv /opt/binance-collector/venv
sudo /opt/binance-collector/venv/bin/pip install \
  boto3 websockets prometheus_client

# Install systemd unit
sudo cp systemd/binance-collector@.service /etc/systemd/system/
sudo systemctl daemon-reload

# Per-instance env files (replace bucket name)
for ex in systemd/*.env.example; do
  name=$(basename "$ex" .env.example)
  sudo cp "$ex" /etc/binance-collector/$name.env
  sudo sed -i "s/CHANGEME/$(aws ssm get-parameter --name /trading/data-bucket --query Parameter.Value --output text)/g" \
    /etc/binance-collector/$name.env
  sudo chmod 640 /etc/binance-collector/$name.env
  sudo chown root:collector /etc/binance-collector/$name.env
done

# Enable + start
sudo systemctl enable --now binance-collector@binance-um-btc
sudo systemctl enable --now binance-collector@binance-spot-btc
sudo systemctl enable --now binance-collector@binance-um-btc-quarterly

# Verify
sudo systemctl status 'binance-collector@*'
sudo journalctl -u 'binance-collector@*' -f
curl -s localhost:9101/metrics | grep collector_
curl -s localhost:9102/metrics | grep collector_
curl -s localhost:9103/metrics | grep collector_
```

## Wiring Prometheus

Add to `/etc/prometheus/prometheus.yml`:

```yaml
scrape_configs:
  - job_name: binance_collectors
    static_configs:
      - targets:
          - 'localhost:9101'
          - 'localhost:9102'
          - 'localhost:9103'

rule_files:
  - /etc/prometheus/rules.d/*.yaml
```

Then:

```bash
sudo cp alertmanager-rules.yaml /etc/prometheus/rules.d/collectors.yaml
sudo killall -HUP prometheus
```

## Verifying it's working

48 hours of clean run before Phase 0 Lane 0D is "done":

```bash
# 1. systemd is happy
systemctl is-active 'binance-collector@*'   # all "active"
systemctl is-failed 'binance-collector@*'   # all "active" (i.e. not failed)

# 2. metrics flowing
curl -s localhost:9101/metrics | grep -E 'collector_messages_total|collector_last_event_ts'

# 3. shards landing in S3
aws s3 ls s3://trading-data-XXXXX/raw/venue=umfutures/year=2026/ --recursive | tail -20

# 4. no upload failures piling up
ls /var/lib/binance-collector/*/failed/ 2>/dev/null
```

Phase 0 Lane 0D exit gate: 48 hours uninterrupted with **zero alerts** in
Alertmanager.

## Daily reconciliation cron (Phase 0 Lane 0D add-on)

Every 24 hours, sample 100 random partitions, validate Parquet/NDJSON parses,
alert on schema drift. Implement in `daily_reconciliation.py` once collectors
are stable. Skeleton:

```python
# Pseudo-code
import boto3, random, json
s3 = boto3.client('s3')
shards = s3.list_objects_v2(Bucket=BUCKET, Prefix='raw/').get('Contents', [])
sample = random.sample(shards, min(100, len(shards)))
for shard in sample:
    obj = s3.get_object(Bucket=BUCKET, Key=shard['Key'])
    for line in obj['Body'].iter_lines():
        msg = json.loads(line)
        assert 'ts' in msg and 'symbol' in msg and 'stream' in msg and 'msg' in msg
# If any assertion fails → page
```

## Symbol management for quarterly contracts

Quarterly symbol format: `<base><quote>_<YYMMDD>`, e.g. `BTCUSDT_250926` for
Sep 2025 expiry.

When a new contract lists (~6 weeks before expiry), update the env file and
restart:

```bash
sudo systemctl edit binance-collector@binance-um-btc-quarterly  # or edit env file directly
sudo systemctl restart binance-collector@binance-um-btc-quarterly
```

A deferred TODO is to automate symbol discovery (T7 / E4 area).

## What this does NOT do (deferred)

- **Canonical schema**: raw NDJSON only. Phase 1 ETL adds canonical Parquet.
- **Sequence-gap detection**: detected later by the canonicalization ETL via the
  `U`/`u`/`pu` fields in depth events.
- **REST snapshot reconciliation**: same as above, in canonicalization.
- **Cross-venue collectors** (OKX, Bybit): identical pattern, separate code,
  parallel non-blocking track per the plan.
