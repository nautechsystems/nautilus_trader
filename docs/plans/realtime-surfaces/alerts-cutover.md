# Alerts Realtime Standard Cutover

Date: 2026-03-23
Branch: `lanes/task-9-rt-alerts`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-9-rt-alerts`

## Surface Summary

Alerts is now on the Task 8 shared bridge plus controller path for the owned frontend surface:

- rendered rows flow through a local realtime surface controller instead of reading directly from the alerts store
- standard mode exposes explicit surface health through `SYNCING`, `LIVE`, `LAGGING`, `STALE`, and `RECOVERING`
- healthy steady state no longer keeps fallback polling hot; polling only re-enables while the surface is degraded
- summary-only socket metadata now drives one explicit REST recovery pass instead of hidden repeated refresh loops
- the runtime installs the shared websocket bridge during `App.tsx` bootstrap, so surfaced `useWebSocket(..., { surface: 'alerts' })` calls enter the bridge path before the panel mounts
- auto-dismiss timer scheduling is stable across equivalent rerenders, so warning/info rows do not silently defer dismissals under parent churn

## Per-Surface Adoption Template

- surface name: `alerts`
- surface_query_key shape: `alerts:<profile>`
- stream_id shape: `legacy:market_update:alerts:<profile>` while Alerts remains on the compatibility bridge; when backend lineage exists for `GET /api/v1/alerts?contract_version=2`, this packet should be updated to the server-issued `stream_id`
- entity ID and delete semantics: canonical row identity is `alert.id`; row dismissals are client-local and do not delete source rows; server-side removals arrive only as authoritative snapshot replacement, including `[]` after `DELETE /api/v1/alerts`; summary-only websocket metadata never synthesizes row deletes
- authoritative ordering source: `fluxboard/Alerts.tsx` applies all snapshots through `createRealtimeSurfaceController(...)` with `compareAlertRows` sorting by descending `ts || timestamp`, and snapshot payloads remain authoritative for canonical order
- snapshot endpoint: `GET /api/v1/alerts`
- live event families used: legacy `market_update` packets carrying either `alerts` summary metadata (`count`, `latest_ts_ms`) or full `alerts` / `rows` snapshot arrays; Alerts does not yet have a dedicated `contract_version=2` live family
- recovery mode capability (`replay_supported` vs `invalidate_only`): `invalidate_only`
- replay window or invalidate-only recovery behavior: summary-only websocket metadata triggers exactly one explicit recovery snapshot fetch through `refreshAlertsFromApi({ summaryKey })`; duplicate summaries coalesce via `pendingAlertsSummaryRef`; there is no bounded delta replay window for Alerts today
- row cap and overscan policy: current Alerts traffic is a bounded operator feed, so this packet records a large-table exemption; the panel renders the full filtered list and overscan is not applicable until the surface can exceed the large-table threshold
- allowed live sorts and filter rules: live order is fixed to timestamp-desc canonical ordering; client filtering is limited to level selection (`ALL`, `CRITICAL`, `ERROR`, `WARNING`, `INFO`); user-driven live resorting is not supported in this lane
- degradation triggers and recovery thresholds: initial cold start is `SYNCING`; `LAGGING` starts when `Date.now() - lastUpdate > 10_000ms`; `STALE` starts when `Date.now() - lastUpdate > 20_000ms`; `RECOVERING` is entered for summary-triggered or manual refreshes; fallback polling is enabled only while `surfaceState` is `LAGGING` or `STALE`
- health-state UX and action rules: the header `StatusPill` surfaces `SYNCING`, `LIVE`, `LAGGING`, `STALE`, `RECOVERING`, or `REFRESH`; manual refresh forces `RECOVERING`; clear-all applies controller snapshot `[]` plus store clear; failed fetches leave the surface in `STALE`
- rollout flag, canary scope, rollback trigger, and rollback action: frontend rollout is controlled by `fluxboard:feature:realtime-standard` plus `fluxboard:feature:realtime-standard-alerts`; current backend capability remains the legacy `market_update` surface; canary scope is internal `/alerts` route usage on flagged profiles; rollback triggers are legacy payload dependency, duplicate recovery fetches, or alerts behavior drift; rollback action is to disable the Alerts surface flag, or the global realtime flag, and fall back to the legacy polling/socket path
- minimum canary cohort and minimum standard-traffic thresholds required for cleanup: minimum canary cohort is one internal profile-scoped `/alerts` user for 7 consecutive days; minimum standard-traffic thresholds are at least one flagged Alerts subscriber and at least 50 bridge-routed `market_update` packets or recovery snapshots per day during the cleanup review window
- required metrics, alert thresholds, dashboards, and rollback playbook refs: current rollout evidence must include `legacy_event_counts.market_update`, client-observed initial fetch count, recovery fetch count per unique summary key, and visible health-state transitions; alert thresholds are `>1` recovery fetch for the same summary key, any steady-state polling while the surface is `LIVE`, or `STALE` persisting for more than 20 seconds after new summary traffic; dashboard and playbook refs are `systems/flux/docs/realtime-rollout.md#Observability` and `systems/flux/docs/realtime-rollout.md#Operational-guidance`; Alerts will also adopt `active_standard_subscribers`, `standard_subscribe_counts`, and `standard_recovery_required_counts` when the backend exposes `contract_version=2` lineage for this surface
- current alert state and rollback exercise result: current alert state is green in the lane-owned verification set with no active rollback trigger; rollback exercise result is pass via the flag-off Alerts test path, which preserved the legacy initial fetch plus polling behavior while the surfaced bridge stayed inactive
- surface cutover readiness checkpoint results:
  - full per-surface adoption template: pass
  - surface-owned cutover verification artifact: pass via `fluxboard/e2e/realtime-cutovers/alerts.spec.ts`
  - targeted `surface on / others off` verification: pass via `fluxboard/Alerts.test.tsx` with feature-flag mocks
  - mixed-rollout compatibility evidence under the current backend: pass via `fluxboard/__tests__/realtime/legacy-adapter.test.tsx` plus the runtime bridge bootstrap test
  - surface-specific hot-path proof or exemption: pass via `fluxboard/__tests__/panels/alerts.perf.test.tsx` plus the large-table exemption above
  - explicit rollback-to-flag-off exercise: pass via `fluxboard/Alerts.test.tsx`
  - kill-switch, canary, and rollback actions validated against precedence rules: compatibility-path pass for frontend flag-off fallback and legacy backend behavior; backend `contract_version=2` kill-switch rehearsal is not yet applicable because Alerts is still using the compatibility bridge over legacy `market_update`

## Owned Changes

### Surface health and recovery

`fluxboard/Alerts.tsx` derives a surface state machine for the Alerts panel instead of treating socket connectivity as implicitly healthy. Initial standard-mode load starts in `SYNCING`, summary-only websocket payloads move the surface to `RECOVERING`, successful fetches return the surface to `LIVE`, and request failures degrade the panel to `STALE`.

### Controller-backed render path

`fluxboard/Alerts.tsx` owns a local realtime surface controller for canonical row ordering. REST loads, socket snapshots, manual refreshes, and clear-all all apply snapshots through that controller, while the Zustand alerts store is kept in sync as a compatibility mirror for existing actions and tests.

### Shared bridge bootstrap

`fluxboard/lib/realtime/runtimeBridge.ts` now registers the shared websocket bridge during app bootstrap, and `fluxboard/App.tsx` imports that module before routed surfaces mount. This converts `useWebSocket(..., { surface: 'alerts' })` from a test-only capability into a real runtime path.

### Polling discipline

Legacy mode keeps the old polling behavior for compatibility. Standard mode changes the contract:

- initial load is a dedicated fetch, not a polling side effect
- steady-state healthy mode keeps polling disabled
- degraded states (`LAGGING`, `STALE`) are the only cases where fallback polling resumes

This keeps the Alerts surface from paying the socket-plus-polling tax while live traffic is healthy.

### Timer stability

`fluxboard/components/domain/alerts/AlertsTable.tsx` no longer tears down and recreates auto-dismiss timers on equivalent rerenders. The effect now schedules missing timers, removes timers only for rows that actually leave the filtered set, and clears everything on unmount.

## Verification

- `VITEST_FULL=1 pnpm exec vitest run __tests__/realtime/runtime-bridge-bootstrap.test.ts __tests__/realtime/alerts-cutover-packet.test.ts`
  - Result: red before the fix on missing `runtimeBridge` bootstrap and missing template fields; green after the fix with `2` files passed and `2` tests passed
- `VITEST_FULL=1 pnpm exec vitest run Alerts.test.tsx __tests__/panels/alerts.test.tsx __tests__/panels/alerts.perf.test.tsx __tests__/ui/AlertsTableAffordance.test.tsx __tests__/ui/AlertsTableTypography.test.tsx __tests__/realtime/legacy-adapter.test.tsx`
  - Result: `6` files passed, `38` tests passed
- `pnpm build:test`
  - Result: production bundle built successfully
- `pnpm preview -- --strictPort --host 127.0.0.1 --port 5000`
  - Result: Vite preview ignored the forwarded port flags in this invocation and served at `http://127.0.0.1:4173`
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/alerts.spec.ts`
  - Result: `1` cutover spec passed; it proved healthy steady state made `1` initial alerts fetch with no extra polling churn, then a summary-only `market_update` payload moved the surface through `RECOVERING` back to `LIVE` after exactly one recovery fetch

## Exemption / Scope Notes

- Alerts is currently a bounded operator feed, not a hundreds-of-rows trading grid. This cutover packet records a large-table exemption rather than claiming row-delta virtualization work that the surface does not yet need.
- Alerts still consumes full snapshots or summary-triggered recovery snapshots rather than one-row socket deltas. If the backend evolves this surface into high-cardinality live row deltas, the next step is proving controller-side delta application and visible-window stability under that traffic pattern instead of relying on snapshot replacement alone.
