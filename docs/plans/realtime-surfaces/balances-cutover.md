# Balances Realtime Standard Cutover

Date: 2026-03-23
Branch: `lanes/task-10-rt-market-balances`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-10-rt-market-balances`

## Surface Summary

Balances now uses the shared realtime-standard bridge plus a controller-backed parent-row pipeline:

- standard mode subscribes through `useWebSocket('market_update', ..., { surface: 'balances' })`
- REST balance snapshots apply through `createRealtimeSurfaceController(...)` before the existing store mirror is updated
- freshness UI and stale-fallback detection now ride the shared `surface:balances` viewport clock rather than panel-local freshness timers
- healthy steady state disables the 5 second polling loop after the first successful snapshot
- invalidation bursts are serialized: one balances snapshot can stay in flight, and at most one follow-up refresh is queued behind it
- fallback polling only resumes when the surface has never loaded or when `lastOkMs` ages past `STALE_THRESHOLDS.SLOW`
- totals, risk groups, exports, and existing affordances continue to read from the compatibility store mirror, so flag-off and flag-on behavior remain aligned

This surface is still on the compatibility bridge over legacy `market_update`. There is no dedicated balances delta or recovery cursor contract from the backend yet.

## Per-Surface Adoption Template

- surface name: `balances`
- surface_query_key shape: `balances:<pathname-profile>` where `<pathname-profile>` is the resolved UI profile (`default`, `tokenmm`, and related route variants)
- stream_id shape: `legacy:market_update:balances:<pathname-profile>` while this surface remains on the compatibility bridge; this packet should be replaced with the server-issued `stream_id` once a dedicated balances live contract exists
- entity ID and delete semantics: canonical parent-row identity is `row.id`; row removal only happens through authoritative snapshot replacement, including empty snapshots
- authoritative ordering source: `fluxboard/Balances.tsx` applies snapshots through `createRealtimeSurfaceController(...)` using descending `mv_raw`; additional holdings and risk-table transforms remain client-side view transforms over the authoritative parent-row snapshot
- snapshot endpoint: `GET /api/v1/balances` with route-scoped profile query when applicable
- live event families used: legacy `market_update` invalidation only; there are no balances-specific deltas, cursors, or recovery-required packets today
- recovery mode capability (`replay_supported` vs `invalidate_only`): `invalidate_only`
- replay window or invalidate-only recovery behavior: any bridge-routed `market_update` packet while the surface flag is enabled triggers a fresh balances snapshot fetch; there is no row-delta replay window or cursor contract yet
- row cap and overscan policy: this packet records a bounded-cardinality exemption; balances is a holdings surface with materially lower row counts than Signal or Trades, and this lane does not claim virtualization or high-cardinality row-model guarantees
- allowed live sorts and filter rules: holdings mode supports client-side logical/stable/zero filters, column filters, and local sort order; risk mode reads snapshot-derived `risk_groups`; live updates replace the authoritative parent-row snapshot and then reapply local transforms
- degradation triggers and recovery thresholds: standard mode begins with polling fallback enabled until the first successful snapshot; after a successful load, polling remains off until `Date.now() - lastOkMs > STALE_THRESHOLDS.SLOW`, at which point fallback polling re-enables
- health-state UX and action rules: Balances does not yet expose the shared `StatusPill` state machine; freshness now remains visible through a shared-clock header indicator plus manual refresh. This is an explicit compatibility-phase UX limitation, not an implicit live contract
- rollout flag, canary scope, rollback trigger, and rollback action: frontend rollout is controlled by `fluxboard:feature:realtime-standard` plus `fluxboard:feature:realtime-standard-balances`; current backend capability remains the legacy `market_update` invalidation path; rollback triggers are duplicate refresh churn, stale balances, or visible divergence between flag-on and flag-off store-driven views; rollback action is to disable the Balances surface flag or the global realtime flag and reload/remount the surface onto legacy polling
- minimum canary cohort and minimum standard-traffic thresholds required for cleanup: minimum canary cohort is one internal `/balances` user for 7 consecutive days; minimum standard traffic is at least one flagged Balances subscriber and at least 50 bridge-routed `market_update` packets per day during the cleanup review window. Recovery snapshot counts remain supporting evidence and do not replace backend `legacy_event_counts.market_update`
- required metrics, alert thresholds, dashboards, and rollback playbook refs: required cleanup evidence must anchor on backend `legacy_event_counts.market_update` plus flagged Balances subscriber presence; client-observed evidence remains supporting proof for initial balances snapshot count, steady-state polling-disabled confirmation, recovery snapshot count after `market_update`, stale-fallback re-enable rate, and visible last-update freshness. Alert thresholds are any healthy-mode polling, repeated snapshot storms on one burst, or stale last-update age persisting past `STALE_THRESHOLDS.SLOW`; refs are `systems/flux/docs/realtime-rollout.md#Observability` and `systems/flux/docs/realtime-rollout.md#Operational-guidance`
- current alert state and rollback exercise result: current alert state is green in the lane-owned verification set; rollback exercise result is pass via the task-owned flag-off remount exercise in `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, which preserved the legacy polling path when the surface flag is absent. Live mid-session kill-switch behavior is not claimed
- surface cutover readiness checkpoint results:
  - full per-surface adoption template: pass
  - surface-owned cutover verification artifact: pass via `fluxboard/e2e/realtime-cutovers/market-balances.spec.ts`
  - targeted `surface on / others off` verification: pass via `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, including explicit `MarketData on / Balances off` and `Balances on / MarketData off` cases
  - mixed-rollout compatibility evidence under the current backend: pass via `fluxboard/__tests__/realtime/legacy-adapter.test.tsx` plus the existing flag-off `Balances.test.tsx` coverage
  - surface-specific hot-path proof or exemption: exemption recorded; balances is treated as a bounded-cardinality holdings surface in this lane and does not claim virtualization or row-delta proofs
  - explicit rollback-to-flag-off exercise: pass via `fluxboard/__tests__/realtime/market-balances-standard.test.tsx` with the surfaces first mounted under flag-on and then remounted under flag-off conditions
  - kill-switch, canary, and rollback actions validated against precedence rules: compatibility-path pass for frontend flag-off-on-remount fallback; live mid-session kill-switch behavior is not claimed, and dedicated backend kill-switch rehearsal is not yet applicable because Balances has no separate server contract

## Owned Changes

### Controller-backed parent-row snapshot path

`fluxboard/Balances.tsx` now owns a local realtime surface controller keyed by `row.id`. Incoming snapshots apply through that controller before the compatibility store mirror updates totals, risk groups, and existing downstream selectors.

### Polling fallback discipline

Legacy mode still behaves as before. Standard mode changes the contract:

- initial load may use the existing polling hook for bootstrap
- once the first snapshot succeeds, steady-state polling turns off
- polling only turns back on when the surface has never loaded or when freshness goes stale

### Shared bridge subscription

Standard mode now routes through `useWebSocket(..., { surface: 'balances' })`. The runtime bridge keeps this surface on the compatibility path over legacy `market_update`, so socket traffic is currently invalidate-only rather than row-delta based.

### Shared freshness clock

`fluxboard/Balances.tsx` now routes both stale-fallback detection and row/header freshness through the shared `surface:balances` viewport clock. That removes the task-local freshness interval while keeping the bounded-cardinality holdings view on the compatibility contract.

### Burst handling

`fluxboard/Balances.tsx` now serializes invalidate-only refreshes. While a balances snapshot is in flight, additional `market_update` invalidations do not get dropped; they latch one follow-up refresh instead.

## Verification

- `pnpm exec vitest run __tests__/realtime/market-balances-standard.test.tsx`
  - Result: pass with `7` tests, including shared-clock coverage, explicit `MarketData on / Balances off`, explicit `Balances on / MarketData off`, legacy-off baseline, and flag-off remount rollback coverage
- `VITEST_FULL=1 pnpm exec vitest run __tests__/panels/market-balances.perf.test.tsx`
  - Result: pass with `3` tests, including the mounted-page fanout and page-anchor proof for a 200-row MarketData snapshot
- `VITEST_FULL=1 pnpm exec vitest run MarketData.test.tsx Balances.test.tsx __tests__/realtime/market-balances-standard.test.tsx __tests__/panels/market-balances.perf.test.tsx __tests__/realtime/legacy-adapter.test.tsx`
  - Result: pass with `5` files passed and `48` tests passed
  - Note: existing `MarketData.test.tsx` coverage still emits React `act(...)` warnings in the shared slice; the Balances task-owned coverage itself is green
- `pnpm build:test`
  - Result: production bundle built successfully
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/market-balances.spec.ts`
  - Result: pass with `2` cutover tests; the Balances half proved exactly one initial balances snapshot in healthy steady state over a 5 second window, then exactly one recovery snapshot after a bridge-routed `market_update`

## Exemption / Scope Notes

- Balances remains in the active cleanup wave. This packet does not grant backend-cleanup readiness on its own because the surface is still bridge-backed over legacy `market_update`; Task 15 still requires either a dedicated backend standard contract for Balances or an explicit retirement decision.
- This packet records a bounded-cardinality exemption rather than claiming high-cardinality live-grid guarantees. The holdings surface is materially smaller than the Signal or Trades tables and still computes local transforms over full snapshots.
- The backend does not yet emit balances-specific invalidation lineage. Standard mode therefore uses generic `market_update` invalidation over the compatibility bridge. This lane now serializes burst invalidations locally, but the real long-term fix is backend lineage plus bridge-side surface filtering, not reintroducing permanent polling.
