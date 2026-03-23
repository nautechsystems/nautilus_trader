# MarketData Realtime Standard Cutover

Date: 2026-03-23
Branch: `lanes/task-10-rt-market-balances`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-10-rt-market-balances`

## Surface Summary

MarketData now uses the shared realtime-standard bridge plus a local controller-backed render path:

- standard mode subscribes through `useWebSocket('market_update', ..., { surface: 'marketData' })`
- REST snapshots flow through `createRealtimeSurfaceController(...)` before the compatibility store mirror is updated
- freshness UI and stale-fallback detection now ride the shared `surface:marketData` viewport clock rather than ad-hoc per-widget timers
- healthy steady state disables the 5 second polling loop after the initial snapshot succeeds
- invalidation bursts are serialized: one snapshot can stay in flight, and at most one follow-up refresh is queued behind it
- fallback polling only resumes when the surface has not loaded yet, or when the last successful snapshot goes stale past `STALE_THRESHOLDS.SLOW`
- manual refresh still uses the same snapshot path, so standard and legacy modes do not diverge on visible behavior

This surface is still on the compatibility bridge over legacy `market_update`. There is no dedicated MarketData `contract_version=2` server lineage yet.

## Per-Surface Adoption Template

- surface name: `marketData`
- surface_query_key shape: `market-data:default`
- stream_id shape: `legacy:market_update:marketData` while this surface remains on the compatibility bridge; this packet should be replaced with the server-issued `stream_id` once a dedicated backend contract exists
- entity ID and delete semantics: canonical row identity is `${coin}:${exchange}`; row removal only happens through authoritative snapshot replacement, including `[]`
- authoritative ordering source: `fluxboard/MarketData.tsx` applies snapshots through `createRealtimeSurfaceController(...)` using descending timestamp order (`timestamp_ms` then `timestamp`); MarketData no longer adds its own extra memoized sort before paging, while user-facing filter/sort/paging controls and mounted-table sorting remain client-side view transforms over the current snapshot
- snapshot endpoint: `GET /api/v1/market-data/snapshot`
- live event families used: legacy `market_update` invalidation only; the current backend payload does not expose MarketData-specific deltas or recovery cursors
- recovery mode capability (`replay_supported` vs `invalidate_only`): `invalidate_only`
- replay window or invalidate-only recovery behavior: any bridge-routed `market_update` packet while the surface flag is enabled triggers a fresh snapshot fetch; there is no delta replay window, cursor, or packet-level coalescing contract from the backend yet
- row cap and overscan policy: mounted DOM is bounded by pager page size (`50` default) rather than virtualization; task-owned perf proof now shows mounted-page shared-clock freshness fanout stays bounded (`<= 52` active subscribers including mounted rows plus header/health subscribers) and preserves page anchors across clock ticks and invalidate-only refreshes. This lane still does not claim virtualization or full-dataset transform guarantees
- allowed live sorts and filter rules: symbol substring filter, exchange multi-select filter, client-side sorting, and client-side paging; live updates replace the authoritative snapshot before those transforms are reapplied
- degradation triggers and recovery thresholds: standard mode starts with polling fallback enabled until the first successful snapshot; after a successful load, fallback polling stays off until `Date.now() - lastUpdate > STALE_THRESHOLDS.SLOW`, at which point the 5 second polling fallback re-enables
- health-state UX and action rules: MarketData does not yet expose the shared `StatusPill` state machine; freshness is surfaced through shared-clock age cells plus the existing auto-refresh toggle and manual refresh button. This remains a compatibility-phase UX gap, not a hidden background contract
- rollout flag, canary scope, rollback trigger, and rollback action: frontend rollout is controlled by `fluxboard:feature:realtime-standard` plus `fluxboard:feature:realtime-standard-marketdata`; current backend capability remains the legacy `market_update` invalidation path; rollback triggers are duplicate refresh churn, stale-data regressions, or view drift between flagged and flag-off behavior; rollback action is to disable the MarketData surface flag or the global realtime flag and reload/remount the surface onto the legacy polling path
- minimum canary cohort and minimum standard-traffic thresholds required for cleanup: minimum canary cohort is one internal `/market-data` user for 7 consecutive days; minimum standard traffic is at least one flagged MarketData subscriber and at least 50 bridge-routed `market_update` packets or recovery snapshots per day during the cleanup review window
- required metrics, alert thresholds, dashboards, and rollback playbook refs: required evidence is initial snapshot count, steady-state polling-disabled confirmation, recovery snapshot count after `market_update`, stale-fallback re-enable rate, and client-visible age freshness; alert thresholds are any steady-state healthy polling, repeated snapshot storms on a single burst, or stale age persisting past `STALE_THRESHOLDS.SLOW`; refs are `systems/flux/docs/realtime-rollout.md#Observability` and `systems/flux/docs/realtime-rollout.md#Operational-guidance`
- current alert state and rollback exercise result: current alert state is green in the lane-owned verification set; rollback exercise result is pass via the task-owned flag-off remount exercise in `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, which preserved the legacy polling path when the surface flag is absent. Live mid-session kill-switch behavior is not claimed
- surface cutover readiness checkpoint results:
  - full per-surface adoption template: pass
  - surface-owned cutover verification artifact: pass via `fluxboard/e2e/realtime-cutovers/market-balances.spec.ts`
  - targeted `surface on / others off` verification: pass via `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, including explicit `MarketData on / Balances off` and `Balances on / MarketData off` cases
  - mixed-rollout compatibility evidence under the current backend: pass via `fluxboard/__tests__/realtime/legacy-adapter.test.tsx` plus the existing flag-off `MarketData.test.tsx` coverage
  - surface-specific hot-path proof or exemption: pass via `fluxboard/__tests__/panels/market-balances.perf.test.tsx`; the task-owned proof pins shared-clock timer count to one per surface, bounds freshness subscribers to the mounted page, and preserves page anchors across clock ticks and invalidate-only refreshes. MarketData also removed its own extra memoized sort before paging, but this lane does not claim mounted-table sort elimination, virtualization, or full high-cardinality row-model stability
  - explicit rollback-to-flag-off exercise: pass via `fluxboard/__tests__/realtime/market-balances-standard.test.tsx` with the surface first mounted under flag-on and then remounted under flag-off conditions
  - kill-switch, canary, and rollback actions validated against precedence rules: compatibility-path pass for frontend flag-off-on-remount fallback; live mid-session kill-switch behavior is not claimed, and dedicated backend kill-switch rehearsal is not yet applicable because MarketData has no separate server contract

## Owned Changes

### Controller-backed snapshot path

`fluxboard/MarketData.tsx` now owns a local realtime surface controller keyed by `${coin}:${exchange}`. REST snapshots apply through that controller first, then mirror into the existing Zustand store so current UI reads and tests continue to work.

### Polling fallback discipline

Legacy mode still behaves as before. Standard mode changes the contract:

- initial load may use the existing polling hook for bootstrap
- once the first snapshot succeeds, steady-state polling turns off
- polling only turns back on when the surface has never loaded or when freshness goes stale

### Shared bridge subscription

Standard mode now routes through `useWebSocket(..., { surface: 'marketData' })`. The shared runtime bridge keeps this surface on the compatibility path over legacy `market_update`, so socket traffic is currently invalidate-only rather than delta-based.

### Shared freshness clock

`fluxboard/MarketData.tsx` now routes both stale-fallback detection and age-cell freshness through the shared `surface:marketData` viewport clock. That removes the task-local `setInterval` freshness loop and bounds clock fanout to the mounted page.

### Burst handling

`fluxboard/MarketData.tsx` now serializes invalidate-only refreshes. While a snapshot request is in flight, additional `market_update` invalidations do not start concurrent fetches; they latch one follow-up refresh instead.

## Verification

- `pnpm exec vitest run __tests__/realtime/market-balances-standard.test.tsx`
  - Result: pass with `7` tests, including shared-clock coverage, explicit `MarketData on / Balances off`, explicit `Balances on / MarketData off`, legacy-off baseline, and flag-off remount rollback coverage
- `VITEST_FULL=1 pnpm exec vitest run __tests__/panels/market-balances.perf.test.tsx`
  - Result: pass with `3` tests, including the mounted-page fanout and page-anchor proof for a 200-row MarketData snapshot
- `VITEST_FULL=1 pnpm exec vitest run MarketData.test.tsx Balances.test.tsx __tests__/realtime/market-balances-standard.test.tsx __tests__/panels/market-balances.perf.test.tsx __tests__/realtime/legacy-adapter.test.tsx`
  - Result: pass with `5` files passed and `48` tests passed
  - Note: existing `MarketData.test.tsx` coverage still emits React `act(...)` warnings; this lane did not introduce new failing assertions around those pre-existing warnings
- `pnpm build:test`
  - Result: production bundle built successfully
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/market-balances.spec.ts`
  - Result: pass with `2` cutover tests; the MarketData half proved exactly one initial snapshot in healthy steady state over a 5 second window, then exactly one recovery snapshot after a bridge-routed `market_update`

## Scope Notes

- This packet now carries mounted-page hot-path proof rather than a blanket large-table exemption. MarketData removed its own extra memoized sort before paging, but mounted-table sorting, filtered views, and user-sorted views still compute in memory and this lane does not claim virtualization or full high-cardinality live-grid guarantees.
- The backend does not yet emit MarketData-specific live deltas or invalidate keys. Standard mode therefore uses generic `market_update` invalidation over the compatibility bridge. This lane now serializes burst invalidations locally, but the real long-term fix is backend lineage plus bridge-side surface filtering, not more UI-local polling.
