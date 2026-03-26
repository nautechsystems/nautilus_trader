# Signal Realtime Standard Cutover

Date: 2026-03-23
Branch: `lanes/task-14-rt-standard-transport`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-14-rt-standard-transport`

## Status

Signal now uses the backend standard Socket.IO contract in the flag-on path:

- the panel requests `contract_version=2` on the canonical `/api/v1/signals` snapshot
- the frontend subscribes with backend lineage via `subscribe`
- live standard packets arrive on `realtime_event`
- teardown uses `unsubscribe`
- the legacy `market_update` and `signal_delta` listeners are no longer part of the flag-on steady state

Local rollback still exists through the per-surface feature flag. If the backend rejects the subscribe or withdraws capability mid-session, the panel fails closed into `manual_refresh_required` rather than silently dropping back to legacy transport.

## Behavioral Contract

- Signal standard recovery remains `invalidate_only`.
- Matching `delta_batch` packets merge row updates into the existing panel state and advance the standard cursor.
- `heartbeat` and `invalidate` packets also advance the standard cursor so reconnect resume does not fall behind during quiet or invalidate-only periods.
- `invalidate` schedules one bounded snapshot refresh instead of immediate snapshot thrash.
- Lineage mismatches are ignored.
- Reconnect resubscribe uses the latest acknowledged standard cursor, not the original snapshot cursor.
- Socket disconnect/reconnect keeps invalidate-only recovery armed instead of canceling the scheduled snapshot.
- `manual_refresh_required` is sticky across reconnects; the panel does not silently auto-recover until the user refreshes.
- Recovery snapshots that started before `manual_refresh_required` are discarded and cannot silently clear the fail-closed state.

## Rollout Notes

- Steady-state live traffic now runs through standard Socket.IO `subscribe` / `realtime_event` / `unsubscribe`.
- The advertised capability still reports `transport_mode = polling_only` and `replay_supported = false` because recovery remains REST snapshot based and replay is not available yet.
- Backend legacy event removal is still blocked by bridge-backed surfaces and explicit flag-off rollback support.
- Signal is ready for frontend duplicate-path cleanup review, not backend legacy-event cleanup.

## Verification

- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run sockets.test.ts __tests__/realtime/standard-socket-client.test.tsx __tests__/panels/signal.test.tsx __tests__/trades-integration.test.tsx __tests__/trades-socket-cleanup.test.tsx`
  - Result: `21` suites passed, `76` tests passed
- `pnpm --dir fluxboard build:test`
  - Result: production bundle built successfully
- `E2E_BASE_URL=http://127.0.0.1:4173 pnpm --dir fluxboard exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/signal.spec.ts`
  - Result: `1` cutover spec passed; it proved `subscribe` carried signal lineage, no legacy steady-state listeners handled `market_update` / `signal_delta`, standard `delta_batch` updated the live row state, reconnect preserved invalidate-only recovery, `invalidate` forced exactly one additional recovery snapshot, and route teardown emitted `unsubscribe`

## Known Limits

- The browser harness proves the frontend contract against a deterministic test socket, not against a live backend environment.
- Existing Signal Vitest runs still emit shared `act(...)` warnings from unrelated tooltip plumbing.
