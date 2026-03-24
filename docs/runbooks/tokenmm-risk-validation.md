# TokenMM Risk Validation Runbook

This runbook is the operator checklist for validating TokenMM local risk, shared
portfolio risk, degraded metadata, and startup reconciliation before enabling
trading.

Use it together with `deploy/tokenmm/README.md`,
`deploy/tokenmm/strategies/README.md`, and
`apps/fluxboard/docs/tokenmm_contract.md`.

## What local risk and global risk mean

- `local risk` is the strategy-local maker-leg base exposure published by the
  strategy as `local_qty_base`.
- `global risk` is the shared TokenMM portfolio aggregate published by
  `run_portfolio` as `global_qty_base`.
- `local_qty` and `global_qty` are compatibility aliases only. They must mirror
  the `*_base` fields exactly.
- `risk_delta` is diagnostic only. It is not the canonical local risk field for
  spot inventory when `local_qty_base` exists.

## Authoritative endpoints

- `GET /api/v1/signals?profile=tokenmm`
  Authoritative for per-strategy local risk, strategy state, and the
  portfolio-global quantity metadata rendered alongside each strategy.
- `GET /api/v1/balances?profile=tokenmm`
  Authoritative for the shared portfolio snapshot, merged balances rows, shared
  totals, component diagnostics, and `global_qty_base`.
- `GET /api/v1/balances?strategy=<id>`
  Per-strategy debug surface. Use this to confirm the strategy's own published
  local balance source when a single node looks wrong.
- `GET /api/pulse/jobs`
  Authoritative for TokenMM job health and whether the restarted services are
  active, failed, or still recovering.

## Core audit commands

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?strategy=plumeusdt_bybit_perp_makerv3'
curl -fsS 'http://127.0.0.1:5022/api/pulse/jobs'
python ops/scripts/tokenmm_risk_audit.py --base-url http://127.0.0.1:5022
```

Treat `scripts/ops/tokenmm_risk_audit.py` as a compatibility shim only. The
authoritative operator entrypoint lives under `ops/scripts/`.

## Single-strategy review

1. Read the strategy row from `signals?profile=tokenmm`.
2. Confirm `local_qty_base` matches the strategy's own published balance source:
   - perp maker strategies: the per-strategy debug balances position row
   - spot maker strategies: the summed base-asset cash rows in the per-strategy
     debug balances view
3. Confirm the strategy row carries the same `global_qty_base`,
   `global_qty_base_complete`, and `aggregation_mode` as the shared profile
   response.
4. If the strategy state is `blocked_reconciliation`, treat that as a trading
   stop even if market data is present.

## Shared portfolio review

1. Read `balances?profile=tokenmm`.
2. Confirm `source = "portfolio_snapshot"` only when the shared snapshot is within `stale_after_ms`.
3. If `source = "portfolio_snapshot"` is absent, confirm the API falls back to the live per-strategy merge path instead of reusing stale snapshot data.
4. Confirm `global_qty_base` and `global_qty_base_complete` match every strategy
   row in `signals?profile=tokenmm`.
5. Confirm the `components` list contains one row per expected strategy and that
   each component `local_qty_base` matches the strategy-local value.
6. Confirm the merged balances rows and totals are the shared portfolio output,
   not independently recomputed strategy rows.
7. Confirm the API payload exposes backend-authored `risk_groups`, `risk_groups[].rows`, and row `risk_key` / `risk_label` semantics used by Fluxboard drilldown.

## Degraded metadata

When the shared or local view is incomplete, degraded metadata must explain why.

- `global_qty_base_complete = false` means the shared global quantity exists but
  is incomplete.
- `aggregation_mode = "partial"` means the shared sum uses only fresh known
  contributors.
- `missing_required` names strategies with no required component.
- `stale_required` names strategies whose required component is too old.
- `null_qty_required` names strategies whose component published no usable local
  quantity.
- `blocked_reconciliation` means startup reconciliation did not reach safe venue
  truth and the node must not trade.

Profile degradation without `missing_required`, `stale_required`, or
`null_qty_required` is an audit failure.

## Startup reconciliation mismatch triage

Use this when a node fails closed during startup with `status=78/CONFIG` and the
journal shows a netting quantity mismatch.

1. Pull the startup window from `journalctl -u flux@tokenmm-node-<strategy>.service`.
2. Check whether the mismatch includes `EXTERNAL`-linked state, such as:
   - `Order ... missing in cache for position ...-EXTERNAL`
   - `External order ... claimed by strategy ...`
   - `position net qty ... != reported net qty ...`
3. If the venue-reported quantity is already explained by non-`EXTERNAL`
   cached positions, the engine may now auto-clean stale `EXTERNAL` startup
   artifacts and continue.
4. Confirm the recovery signature in logs:
   - `Treating EXTERNAL netting positions as stale startup reconciliation artifacts`
   - `Closing stale EXTERNAL reconciliation positions`
5. If the startup window instead shows a cached open order that no longer exists
   at venue and the venue reports no remaining position for the instrument, the
   engine may now reject the stale cached order and purge the stale cached
   startup position automatically. Confirm this exact recovery signature before
   retrying:
   - `Startup targeted order-status query returned no report for ...; marking cached order as missing at venue`
   - `Reconciling ... order not found at venue, marking as REJECTED`
   - `Treating startup netting positions as stale cached positions for ...`
   - `Closing stale startup cached positions`
   - `Startup reconciliation removed stale cached positions for ...`
6. After either startup-only cleanup signature appears, restart only the
   affected node once and rerun:
   - `curl -fsS 'http://127.0.0.1:5022/api/pulse/jobs'`
   - `python ops/scripts/tokenmm_risk_audit.py --base-url http://127.0.0.1:5022`
   Do not delete Redis cache keys before this bounded retry.
7. Recovery is complete only if the restarted node is active in Pulse and the
   risk audit no longer reports `blocked_reconciliation` for that strategy.
8. If the node still exits after one restart, treat the mismatch as genuine venue
   drift or missing history and keep trading disabled until the account/cache
   state is reconciled manually.

When the failure is clearly due to missing venue order/fill history after prolonged
downtime, operators may apply a per-strategy `exec_reconciliation_lookback_mins`
override and retry startup. Do this only on nodes whose startup reconciliation is
scoped to their configured instrument set, and record the override plus recovery
evidence in the incident notes.

If the widened lookback now causes startup to fail with `Execution reconciliation timed out`
while mass status reports are still being gathered, raise that node's
`timeout_reconciliation` before retrying. Keep the timeout scoped to the affected
strategy rather than changing the shared default.

Important:

- This cleanup is startup-only.
- Continuous/background position discrepancy checks stay strict and must still
  surface quantity drift after startup.

## Data unavailable vs true zero

- `true zero` means the canonical quantity field is explicitly zero.
- `data unavailable` means the canonical quantity is missing or degraded and the
  diagnostics explain why.
- A fresh flat perp report may suppress the maker position row in
  `balances?strategy=<id>` while `signals?profile=tokenmm` and the portfolio
  component still show `local_qty_base = 0`. Treat that as true zero.
- Do not treat an absent position row by itself as data unavailable if the
  canonical local quantity is explicitly zero.
- Do not treat a missing canonical quantity with degraded metadata as zero.

## Post-restart checks before enabling trading

Run these checks after a Pulse restart and before enabling trading:

1. Restart the TokenMM group through Pulse and confirm the jobs are active.
2. Confirm `signals?profile=tokenmm` returns all expected strategies.
3. Confirm `balances?profile=tokenmm` is sourced from `portfolio_snapshot`.
4. Confirm no strategy remains in `blocked_reconciliation`.
5. Confirm the local strategy view, per-strategy debug balances view, and shared
   portfolio component agree for each strategy.
6. Confirm degraded metadata is either absent or explicitly explained.
7. For any node that previously failed startup reconciliation, confirm the
   journal shows either a clean startup with no mismatch or the startup-only
   stale-`EXTERNAL` cleanup signature above.

## Rollout and acceptance gates

Required production sign-off:

1. all targeted unit tests green
2. TokenMM group restarted cleanly through Pulse
3. `signals`, `balances(profile=tokenmm)`, and `balances(strategy=<id>)` agree
   for each strategy according to contract
4. partial vs strict `global_qty` semantics are visible and documented
5. startup reconciliation block/degrade behavior is verified intentionally at
   least once
