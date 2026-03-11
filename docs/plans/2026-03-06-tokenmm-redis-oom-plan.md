# TokenMM Redis Capacity And Topology Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate Redis OOMs in TokenMM and move production Redis toward a bounded, low-latency control-plane design instead of an unbounded mixed cache/history bucket.

**Architecture:** Keep the immediate production path on a dedicated Multi-AZ ElastiCache replication group, but move it to a non-burstable memory-optimized node class and enforce hard observability guardrails. Then reduce live cache residency for closed orders/positions and split durable history away from Redis so Redis stays a real-time system, not a historical database.

**Tech Stack:** AWS ElastiCache for Redis OSS, CloudWatch, SNS, Flux TokenMM runners, Nautilus live cache/message bus, Redis protocol clients, systemd/Pulse.

---

## Current State Summary

1. Production Redis is `tokenmm`, a dedicated ElastiCache replication group in `ap-southeast-1`, `cluster mode disabled`, `Multi-AZ enabled`, `cache.t4g.medium`, `default.redis7`.
2. The live TokenMM workload hit `DatabaseMemoryUsagePercentage ~= 100%` on March 6, 2026.
3. Redis keyspace is dominated by Nautilus trader cache keys, not Flux API keys.
4. Approximate live key counts captured during incident response:
   - `trader-TOKENMM-LIVE-*`: `105921`
   - `flux:v1:*`: `204`
5. The immediate code-side fix is already in place:
   - exec-engine purge settings are wired through TokenMM runners
   - live PLUME configs now enable closed-order/position/account-event purging
6. The immediate infra fix has already been requested:
   - in-place ElastiCache resize from `cache.t4g.medium` to `cache.r7g.large`
7. Baseline alarms have already been created against `arn:aws:sns:ap-southeast-1:670513421539:prod-alerts`.

## Target Operating Model

1. Redis is a low-latency control-plane/cache, not the canonical store for historical closed orders.
2. Live Redis steady-state memory target:
   - normal: `< 60%`
   - warning: `>= 70%`
   - critical: `>= 85%`
3. Use non-burstable memory-optimized nodes for live trading.
4. All high-churn datasets must have bounded retention by code, not by eviction policy.
5. Historical order/trade analytics must move to a durable analytics store or archival path outside Redis.

### Task 1: Stabilize Production Capacity

**Files:**
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_spot_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`

**Step 1: Verify the ElastiCache resize completes**

Run:
```bash
aws elasticache describe-replication-groups \
  --region ap-southeast-1 \
  --replication-group-id tokenmm
```

Expected:
- `Status=available`
- `CacheNodeType=cache.r7g.large`

**Step 2: Verify live runners survive the resize**

Run:
```bash
curl -sS http://127.0.0.1:5022/api/pulse/jobs | jq
systemctl is-active \
  flux@tokenmm-api \
  flux@tokenmm-bridge \
  flux@tokenmm-node-plumeusdt_binance_spot_makerv3 \
  flux@tokenmm-node-plumeusdt_bybit_perp_makerv3 \
  flux@tokenmm-node-plumeusdt_bybit_spot_makerv3 \
  flux@tokenmm-node-plumeusdt_okx_perp_makerv3 \
  flux@tokenmm-portfolio
```

Expected:
- all jobs `active`

**Step 3: Verify memory headroom after resize**

Run:
```bash
REDISCLI_AUTH='***' redis-cli -h 'master.tokenmm.wapqos.apse1.cache.amazonaws.com' -p 6379 --tls INFO memory
aws cloudwatch get-metric-statistics \
  --region ap-southeast-1 \
  --namespace AWS/ElastiCache \
  --metric-name DatabaseMemoryUsagePercentage \
  --dimensions Name=CacheClusterId,Value=tokenmm-001 Name=CacheNodeId,Value=0001 \
  --start-time "$(date -u -d '30 minutes ago' +%FT%TZ)" \
  --end-time "$(date -u +%FT%TZ)" \
  --period 60 \
  --statistics Average Maximum
```

Expected:
- immediate memory percentage materially below incident peak
- no fresh OOMs

### Task 2: Codify Redis Guardrails In Production

**Files:**
- Modify: `deploy/tokenmm/README.md`
- Create: `docs/runbooks/tokenmm-redis.md`

**Step 1: Document the production Redis SLOs**

Document:
- node class requirement: memory-optimized, non-burstable
- memory thresholds: 60/70/85
- alarm list
- failover expectations
- resize command and rollback command

**Step 2: Document the incident triage flow**

Include:
- how to inspect keyspace counts
- how to inspect CloudWatch memory/CPU/evictions
- what can be purged safely
- when to scale vertically versus stop writers

### Task 3: Reduce Live Cache Residency

**Files:**
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_spot_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Write the failing test**

Add a runner-config test proving shorter purge buffers are wired exactly.

**Step 2: Run it to verify it fails**

Run:
```bash
uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -q -k purge
```

**Step 3: Lower closed-order and account-event retention**

Recommended first cut:
- `purge_closed_orders_buffer_mins = 15`
- `purge_closed_positions_buffer_mins = 15`
- `purge_account_events_lookback_mins = 15`

Keep intervals at `10` minutes initially.

**Step 4: Run tests**

Run:
```bash
uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -q
```

**Step 5: Deploy one venue first**

Start with the least critical node, verify memory slope, then roll to all four.

### Task 4: Split Redis By Responsibility

**Files:**
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `flux/runners/tokenmm/run_portfolio.py`
- Modify: `flux/runners/tokenmm/run_bridge.py`
- Modify: `systems/flux/flux/api/app.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Introduce separate Redis config surfaces**

Add independent config blocks for:
- live trader cache / control plane
- Flux bridge/API/event surfaces

Backward compatibility:
- if only `[redis]` exists, keep current behavior

**Step 2: Route components intentionally**

Recommended split:
- `redis_hot`: Nautilus cache, params, runtime control
- `redis_flux`: Flux streams, UI snapshots, API-facing feeds

**Step 3: Write focused wiring tests**

Verify:
- cache and message bus can point at different `DatabaseConfig`s
- portfolio/bridge/API use the intended endpoint

### Task 5: Remove History From Redis

**Files:**
- Modify: `nautilus_trader/cache/cache.pyx`
- Modify: `nautilus_trader/cache/database.pyx`
- Modify: `systems/flux/flux/api/...` as needed
- Create: `docs/plans/<follow-on-history-store-plan>.md`

**Step 1: Decide the durable store**

Recommended order of preference:
1. existing operational Postgres if latency and retention are modest
2. ClickHouse if high-volume trade/order analytics are required
3. S3/Parquet archival for low-cost long-term retention

**Step 2: Keep only hot working sets in Redis**

Redis should retain:
- open orders
- inflight orders
- current positions
- recent reconciliation window only

Redis should not be the canonical home for:
- long closed-order history
- large analytics payloads
- report exports

### Task 6: Move ElastiCache To Managed Infra

**Files:**
- Create in the correct infra repository (not present in this workspace)

**Step 1: Codify**

Capture:
- replication group
- node class
- parameter group
- alarms
- SNS topic bindings

**Step 2: Add drift detection**

The production cluster should no longer rely on console-only/manual state.

## Recommended Final Production Shape

1. `tokenmm-hot`: Multi-AZ `cache.r7g.large` or larger, control-plane only.
2. `tokenmm-flux`: separate Redis for UI/event fanout if Flux traffic grows materially.
3. Durable order/trade history outside Redis.
4. CloudWatch alarms on memory, evictions, engine CPU, replication lag.
5. Regular failover drills and resize rehearsals.

## Decision Notes

1. Do not stay on `cache.t4g.medium` for live TokenMM. It is too small and burstable.
2. Do not rely on Redis eviction policy as the primary retention mechanism.
3. Do not move to cluster-mode enabled Redis until the client/config surface is intentionally designed for sharding. Vertical scale is the correct immediate move for the current cluster-mode-disabled topology.
