# 2026-03-05 TokenMM multi-strategy deployment (Phase 1 execution notes)

## Task 5 decision: publisher completeness

Decision for Phase 1:

- Deferred (optional task). No change to `nautilus_trader/flux/strategies/makerv3/publisher.py`.
- Portfolio correctness for TokenMM balances is provided by API-side aggregation/staleness semantics
  in Tasks 3–4 and does not rely on publisher completeness.

Follow-up trigger:

- Revisit publisher fanout only if operators need full multi-instrument position payloads directly from
  strategy-emitted balance snapshots and performance budget confirms acceptable payload growth.

## Task 7 decision: Signals/Trades/Alerts scope

Decision for Phase 1:

- `GET /api/v1/signals`, `GET /api/v1/trades`, `GET /api/v1/alerts`, and Socket.IO profile streams remain
  **per-strategy** surfaces.
- TokenMM portfolio behavior is enabled for balances only (`/api/v1/balances?profile=tokenmm`).
- Operators should use strategy selection for non-balance views until Phase 2.

Rationale:

- Keeps Phase 1 bounded to balances portfolio correctness, explicit allowlisting, and operational safety.
- Avoids widening API/socket fanout behavior during the 5-node production rollout.

## Follow-up plan (separate PR)

If multi-strategy Signals/Trades/Alerts are required:

1. Add explicit fanout contracts for `profile=tokenmm` on REST (`signals`, `trades`, `alerts`), mirroring
   the allowlist + required-set model used for balances.
2. Extend Socket.IO profile emitter to emit bounded per-strategy deltas for all allowlisted TokenMM
   strategy IDs.
3. Add staleness/degraded semantics and required-component reporting for these fanout surfaces.
4. Add dedicated API/socket regression tests for multi-strategy ordering, dedupe, pagination, and
   reconnect cursor behavior.

Out of scope for Phase 1:

- `flux:v2:*` stream schema changes.
- New strategy families (hedgers/rebalancers/arbitrage/equities).
