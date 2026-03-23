# Trades Realtime Standard Cutover

Date: 2026-03-23
Branch: `lanes/task-14-rt-standard-transport`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-14-rt-standard-transport`

## Status

Trades now uses the backend standard Socket.IO contract in the canonical live view:

- the canonical first-page unfiltered descending snapshot requests `contract_version=2`
- the frontend subscribes with lineage metadata via `subscribe`
- matching `realtime_event` `delta_batch` packets drive steady-state live updates
- subscribe rejection and capability withdrawal fail closed into `manual_refresh_required`
- the legacy `trade_update` listener is retained only for explicit local flag-off rollback

Non-canonical trades views remain REST-only and do not advertise `data.realtime`.

## Behavioral Contract

- the canonical standard live view is `page=1`, `page_size=50`, `sort=ts_desc`, with no filters
- healthy standard steady state does not run parallel HTTP delta polling
- trade row ordering still uses the inner trade row `seq`; the standard envelope `seq` is tracked separately as the surface cursor
- reconnect and resubscribe use the latest acknowledged standard cursor
- standard envelope seq gaps trigger bounded HTTP delta recovery from the last acknowledged seq
- `recovery_required` with `reason=trade_gap` triggers a canonical snapshot recovery; recoverable subscribe lineage drift does the same instead of failing closed
- `manual_refresh_required` is sticky across reconnects; the panel does not silently auto-recover until the user refreshes
- queued recovery snapshots that started before `manual_refresh_required` are discarded and cannot silently clear the fail-closed state
- returning from a non-canonical view waits for a fresh canonical snapshot before the standard subscription is re-armed
- legacy events without epoch metadata remain compatible in flag-off mode and do not spuriously trigger snapshot refreshes

## Rollout Notes

- The current backend transport is still `polling_only`.
- Backend `trade_update` removal remains blocked until Task 15 and Task 13 resolve the remaining cleanup boundary.
- Trades is ready for frontend duplicate-path cleanup review, not backend legacy-event cleanup.

## Verification

- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run sockets.test.ts __tests__/realtime/standard-socket-client.test.tsx __tests__/panels/signal.test.tsx __tests__/trades-integration.test.tsx __tests__/trades-socket-cleanup.test.tsx`
  - Result: `21` suites passed, `76` tests passed
- `pnpm --dir fluxboard build:test`
  - Result: production bundle built successfully
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm --dir fluxboard exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/trades.spec.ts`
  - Result: `1` cutover spec passed; it proved `subscribe` carried trades lineage, no legacy steady-state `trade_update` listener handled client traffic, standard `delta_batch` updated the live table, healthy steady state made `0` delta replay requests, and `capability_withdrawn` remained fail-closed across reconnect while emitting `unsubscribe`

## Known Limits

- The browser harness proves the frontend contract against a deterministic test socket, not against a live backend environment.
- The backend still exposes legacy event traffic for rollback clients and bridge-backed surfaces.
