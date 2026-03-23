# Alerts Realtime Standard Cutover

Date: 2026-03-23
Branch: `lanes/task-9-rt-alerts`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-9-rt-alerts`

## Status

Alerts is migrated in this lane to the realtime-standard surface shape for the owned frontend path:

- rendered rows now flow through a local realtime surface controller instead of reading directly from the alerts store
- standard mode exposes explicit surface health through `SYNCING`, `LIVE`, `LAGGING`, `STALE`, and `RECOVERING`
- healthy steady state no longer keeps fallback polling hot; polling only re-enables while the surface is degraded
- summary-only socket metadata now drives one explicit REST recovery pass instead of hidden repeated refresh loops
- websocket subscriptions are surface-tagged with `surface: 'alerts'` so the shared bridge path can take ownership when the runtime bridge registers
- auto-dismiss timer scheduling is stable across equivalent rerenders, so warning/info rows do not silently defer dismissals under parent churn

## What Changed

### Surface health and recovery

`fluxboard/Alerts.tsx` now derives a surface state machine for the Alerts panel instead of treating "socket connected" as implicitly healthy. Initial standard-mode load starts in `SYNCING`, summary-only websocket payloads move the surface to `RECOVERING`, successful fetches return the surface to `LIVE`, and request failures degrade the panel to `STALE`.

### Controller-backed render path

`fluxboard/Alerts.tsx` now owns a local realtime surface controller for canonical row ordering. REST loads, socket snapshots, manual refreshes, and clear-all all apply snapshots through that controller, while the Zustand alerts store is kept in sync as a compatibility mirror for existing actions and tests.

### Polling discipline

Legacy mode keeps the old polling behavior for compatibility. Standard mode changes the contract:

- initial load is a dedicated fetch, not a polling side effect
- steady-state healthy mode keeps polling disabled
- degraded states (`LAGGING`, `STALE`) are the only cases where fallback polling resumes

This keeps the Alerts surface from paying the socket-plus-polling tax while live traffic is healthy.

### Timer stability

`fluxboard/components/domain/alerts/AlertsTable.tsx` no longer tears down and recreates auto-dismiss timers on equivalent rerenders. The effect now schedules missing timers, removes timers only for rows that actually leave the filtered set, and clears everything on unmount.

## Verification

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
