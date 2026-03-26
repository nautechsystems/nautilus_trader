# Balances Realtime Standard Cutover

Date: 2026-03-24
Branch: `lanes/task-15b-rt-balances-standard`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-15b-rt-balances-standard`

## Surface Summary

Balances now uses backend-issued realtime lineage plus a controller-backed parent-row pipeline:

- standard mode subscribes through `useStandardWebSocketSubscription(...)` using the canonical `GET /api/v1/balances?contract_version=2` lineage
- REST balance snapshots apply through `createRealtimeSurfaceController(...)` before the existing store mirror is updated
- freshness UI and stale-fallback detection now ride the shared `surface:balances` viewport clock rather than panel-local freshness timers
- healthy steady state disables the 5 second polling loop after the first successful snapshot
- invalidation bursts are serialized: one balances snapshot can stay in flight, and at most one follow-up refresh is queued behind it
- fallback polling only resumes when the surface has never loaded or when `lastOkMs` ages past `STALE_THRESHOLDS.SLOW`
- totals, risk groups, exports, and existing affordances continue to read from the compatibility store mirror, so flag-off and flag-on behavior remain aligned

Balances standard transport is `invalidate_only`. The backend emits balances `heartbeat` and `invalidate` packets on `realtime_event`; there is no balances delta replay contract.

## Per-Surface Adoption Template

- surface name: `balances`
- surface_query_key shape: backend-issued `balances|profile=<normalized-profile>|strategy_ids=<deduped-strategy-id-set>` for canonical profile-scoped subscriptions
- stream_id shape: backend-issued `balances:<normalized-profile>:<strategy-id-set>` for canonical profile-scoped subscriptions
- entity ID and delete semantics: canonical parent-row identity is `row.id`; row removal only happens through authoritative snapshot replacement, including empty snapshots
- authoritative ordering source: `fluxboard/Balances.tsx` applies snapshots through `createRealtimeSurfaceController(...)` using descending `mv_raw`; additional holdings and risk-table transforms remain client-side view transforms over the authoritative parent-row snapshot
- snapshot endpoint: `GET /api/v1/balances` with route-scoped profile query when applicable
- live event families used: standard `realtime_event` packets for `heartbeat`, `invalidate`, and standard subscribe rejection / withdrawal handling; there are no balances-specific deltas today
- recovery mode capability (`replay_supported` vs `invalidate_only`): `invalidate_only`
- replay window or invalidate-only recovery behavior: any balances `invalidate` packet while the surface flag is enabled triggers a fresh balances snapshot fetch; there is no row-delta replay window or cursor contract
- row cap and overscan policy: this packet records a bounded-cardinality exemption; balances is a holdings surface with materially lower row counts than Signal or Trades, and this lane does not claim virtualization or high-cardinality row-model guarantees
- allowed live sorts and filter rules: holdings mode supports client-side logical/stable/zero filters, column filters, and local sort order; risk mode reads snapshot-derived `risk_groups`; live updates replace the authoritative parent-row snapshot and then reapply local transforms
- degradation triggers and recovery thresholds: standard mode begins with polling fallback enabled until the first successful snapshot; after a successful load, polling remains off until `Date.now() - lastOkMs > STALE_THRESHOLDS.SLOW`, at which point fallback polling re-enables
- health-state UX and action rules: Balances does not yet expose the shared `StatusPill` state machine; freshness now remains visible through a shared-clock header indicator plus manual refresh. This is an explicit compatibility-phase UX limitation, not an implicit live contract
- rollout flag, canary scope, rollback trigger, and rollback action: frontend rollout is controlled by `fluxboard:feature:realtime-standard` plus `fluxboard:feature:realtime-standard-balances`, and backend capability is controlled by the balances standard subscribe capability in `systems/flux/flux/api/socketio.py`; rollback triggers are duplicate refresh churn, stale balances, or visible divergence between flag-on and flag-off store-driven views; rollback action is to disable the Balances surface flag or the global realtime flag and reload/remount the surface onto legacy polling
- minimum canary cohort and minimum standard-traffic thresholds required for cleanup: minimum canary cohort is one internal `/balances` user for 7 consecutive days; cleanup evidence must show at least one flagged Balances standard subscriber on 3 distinct canary days plus at least one `balances:invalidate` event during the cleanup review window. `balances:heartbeat` counts remain supporting liveness evidence only because poll-driven heartbeats scale with cadence rather than user-value
- required metrics, alert thresholds, dashboards, and rollback playbook refs: required cleanup evidence must anchor on backend `active_standard_subscribers.balances:v2`, `standard_subscribe_counts`, `standard_recovery_required_counts`, and `standard_event_counts.balances:heartbeat` / `standard_event_counts.balances:invalidate`; client-observed evidence remains supporting proof for initial balances snapshot count, steady-state polling-disabled confirmation, recovery snapshot count after `invalidate`, stale-fallback re-enable rate, and visible last-update freshness. Alert thresholds are any healthy-mode polling, repeated snapshot storms on one burst, or stale last-update age persisting past `STALE_THRESHOLDS.SLOW`; refs are `systems/flux/docs/realtime-rollout.md#Observability` and `systems/flux/docs/realtime-rollout.md#Operational-guidance`
- current alert state and rollback exercise result: current alert state is green in the lane-owned verification set; rollback exercise result is pass via the task-owned flag-off remount exercise in `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, which preserved the legacy polling path when the surface flag is absent. Live mid-session kill-switch behavior is not claimed
- surface cutover readiness checkpoint results:
  - full per-surface adoption template: pass
  - surface-owned cutover verification artifact: pass via `fluxboard/e2e/realtime-cutovers/market-balances.spec.ts`
  - targeted `surface on / others off` verification: pass via `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, including explicit `MarketData on / Balances off` and `Balances on / MarketData off` cases
  - mixed-rollout compatibility evidence under the current backend: pass via `fluxboard/__tests__/realtime/legacy-adapter.test.tsx` plus the existing flag-off `Balances.test.tsx` coverage
  - surface-specific hot-path proof or exemption: exemption recorded; balances is treated as a bounded-cardinality holdings surface in this lane and does not claim virtualization or row-delta proofs
  - explicit rollback-to-flag-off exercise: pass via `fluxboard/__tests__/realtime/market-balances-standard.test.tsx` with the surfaces first mounted under flag-on and then remounted under flag-off conditions
- kill-switch, canary, and rollback actions validated against precedence rules: compatibility-path pass for frontend flag-off-on-remount fallback; live mid-session kill-switch behavior is not claimed, but the backend balances capability and global realtime kill-switch are applicable through the shared standard contract controls

## Owned Changes

### Controller-backed parent-row snapshot path

`fluxboard/Balances.tsx` now owns a local realtime surface controller keyed by `row.id`. Incoming snapshots apply through that controller before the compatibility store mirror updates totals, risk groups, and existing downstream selectors.

### Polling fallback discipline

Legacy mode still behaves as before. Standard mode changes the contract:

- initial load may use the existing polling hook for bootstrap
- once the first snapshot succeeds, steady-state polling turns off
- polling only turns back on when the surface has never loaded or when freshness goes stale

### Standard socket subscription

Standard mode now routes through `useStandardWebSocketSubscription(...)`. The backend issues canonical balances lineage on `contract_version=2` snapshots and emits balances `heartbeat` / `invalidate` packets on `realtime_event`.

### Shared freshness clock

`fluxboard/Balances.tsx` now routes both stale-fallback detection and row/header freshness through the shared `surface:balances` viewport clock. That removes the task-local freshness interval while keeping the bounded-cardinality holdings view on the standard invalidate-only contract.

### Burst handling

`fluxboard/Balances.tsx` now serializes invalidate-only refreshes. While a balances snapshot is in flight, additional standard `invalidate` events do not get dropped; they latch one follow-up refresh instead.

## Verification

- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus-trader-dev/.worktrees/task-3-rt-backend/.venv/bin/pytest -q tests/unit_tests/flux/api/test_realtime_contract.py -k balances --confcutdir=tests/unit_tests/flux/api`
  - Result: pass with `10` tests, including default-route lineage fallback, canonical portfolio-snapshot invalidation, filtered raw-row churn regression coverage, the first-tick post-subscribe balances invalidation race, and the legacy `market_update` rollback-path regression for balances-only changes
- `VITEST_FULL=1 ../../rt-controller/fluxboard/node_modules/.bin/vitest run Balances.test.tsx api.flux.test.ts __tests__/realtime/market-balances-standard.test.tsx`
  - Result: pass with `75` tests; the balances-owned slice now pins backend-shaped lineage identities, including the default `/balances` route resolving to the backend default descriptor profile in fixtures, and preserves the same lineage object across invalidate-only refreshes while the stream identity stays stable
  - Note: existing shared `MarketData` coverage in the combined slice still emits React `act(...)` warnings; the Balances task-owned coverage itself is green
- `pnpm build:test`
  - Result: production bundle built successfully
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/market-balances.spec.ts`
  - Result: pass with `2` cutover tests; the Balances half proved backend-shaped standard subscribe lineage for the default-route fixture, exactly one initial balances snapshot in healthy steady state over a 5 second window, and exactly one recovery snapshot after a standard balances invalidation

## Exemption / Scope Notes

- Balances remains in the active cleanup wave, but it no longer needs a bridge-backed exception. Backend legacy-event cleanup is still blocked by parked `MarketData` and explicit rollback-client support.
- This packet records a bounded-cardinality exemption rather than claiming high-cardinality live-grid guarantees. The holdings surface is materially smaller than the Signal or Trades tables and still computes local transforms over full snapshots.
- Balances still uses invalidate-only recovery. The backend does not emit row-delta replay for balances, and the frontend still relies on authoritative REST refreshes after invalidation or failure.
