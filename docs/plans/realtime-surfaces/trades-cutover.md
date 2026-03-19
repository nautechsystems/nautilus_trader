# Trades Realtime Standard Cutover

Date: 2026-03-19
Branch: `lanes/task-7-rt-trades`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-7-rt-trades`

## Status

Trades is migrated in this lane to the realtime-standard controller shape for the owned UI surface:

- surface health is derived from the realtime state machine (`syncing`, `live`, `lagging`, `stale`, `recovering`) instead of the raw socket-connected bit
- HTTP replay stays dormant in healthy steady state and only runs while Trades is recovering or degraded
- replay requests now carry `stream_id` and `snapshot_revision` alongside the existing cursor semantics
- the rendered table is fed by a controller-owned canonical rows array so in-place one-row updates keep stable array and row identity

## Notes

- The historical tokenmm timestamp fallback remains in place when the snapshot cursor is unusable (`last_seq <= 0`), but that path is now explicitly treated as degraded recovery instead of healthy steady state.
- The owned baseline failure in `Trades.recovery.test.tsx` was part of this lane: the test API mock now exports `deriveCanonicalNaming`, and the snapshot-refresh regression is covered.
- The default `pnpm --dir fluxboard exec vitest run ...` invocation still respects the repo's quarantined Vitest excludes, so full owned verification for the Trades bundle was run with `VITEST_FULL=1`.
- No Playwright cutover run was executed in this lane.

## Verification

- `VITEST_FULL=1 pnpm exec vitest run __tests__/trades-integration.test.tsx __tests__/trades-socket-cleanup.test.tsx __tests__/panels/trades.perf.test.tsx Trades.recovery.test.tsx components/trades/TradesTable.test.tsx`
  - Result: `5` files passed, `25` tests passed
- `pnpm --dir fluxboard exec vitest run __tests__/trades-integration.test.tsx __tests__/trades-socket-cleanup.test.tsx __tests__/panels/trades.perf.test.tsx Trades.recovery.test.tsx components/trades/TradesTable.test.tsx`
  - Result: `2` files passed, `5` tests passed
