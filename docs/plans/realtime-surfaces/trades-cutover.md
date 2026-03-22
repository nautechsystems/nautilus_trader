# Trades Realtime Standard Cutover

Date: 2026-03-22
Branch: `lanes/task-7-rt-trades`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-7-rt-trades`

## Status

Trades is migrated in this lane to the realtime-standard controller shape for the owned UI surface:

- surface health is derived from the realtime state machine (`syncing`, `live`, `lagging`, `stale`, `recovering`) instead of the raw socket-connected bit
- HTTP replay stays dormant in healthy steady state and only runs while Trades is recovering or degraded
- replay requests now carry `stream_id` and `snapshot_revision` alongside the shared `since_seq` cursor semantics
- socket seq gaps in the active `stream_id + snapshot_revision` epoch force `recovering` and replay from the last acknowledged seq instead of advancing live state optimistically
- zero-row and zero-`last_seq` snapshots stay on the standard seq cursor path instead of switching into the legacy `after*` fallback
- the rendered table is fed by a controller-owned canonical rows array, and one-row in-place updates rerender only the changed visible row instead of invalidating the full visible window

## Notes

- The owned baseline failure in `Trades.recovery.test.tsx` was part of this lane: the test API mock now exports `deriveCanonicalNaming`, and the snapshot-refresh regression is covered.
- The default `pnpm --dir fluxboard exec vitest run ...` invocation still respects the repo's quarantined Vitest excludes, so full owned verification for the Trades bundle was run with `VITEST_FULL=1`.
- `fluxboard/e2e/realtime-cutovers/trades.spec.ts` now drives a deterministic test socket plus mocked snapshot/delta fixtures to prove the recovering-only replay contract on the actual `/trades` route.
- The shared Playwright configs still have port mismatches against Vite defaults (`playwright.config.ts` expects `:5000` while `pnpm dev` defaults to `:5173`; `playwright.prod.config.ts` expects `:5000` while `pnpm preview` defaults to `:4173`), so the recorded cutover evidence used the existing `playwright.smoke.config.ts` with an explicit preview server on `:5000`.

## Verification

- `VITEST_FULL=1 pnpm exec vitest run __tests__/trades-integration.test.tsx __tests__/trades-socket-cleanup.test.tsx __tests__/panels/trades.perf.test.tsx Trades.recovery.test.tsx components/trades/TradesTable.test.tsx`
  - Result after the rework: `5` files passed, `28` tests passed
- `pnpm --dir fluxboard exec vitest run __tests__/trades-integration.test.tsx __tests__/trades-socket-cleanup.test.tsx __tests__/panels/trades.perf.test.tsx Trades.recovery.test.tsx components/trades/TradesTable.test.tsx`
  - Result: `2` files passed, `5` tests passed
- `pnpm build:test`
  - Result: production bundle built successfully
- `VITE_PREVIEW_PORT=5000 pnpm preview -- --strictPort`
  - Result: preview server served the built SPA at `http://127.0.0.1:5000`
- `E2E_BASE_URL=http://127.0.0.1:5000 pnpm exec playwright test -c playwright.smoke.config.ts e2e/realtime-cutovers/trades.spec.ts`
  - Result: `1` cutover spec passed; it proved healthy steady state made `0` replay calls, a seq gap triggered `RECOVERING - Replaying…`, and the replay request used `since_seq=2&stream_id=trades-main&snapshot_revision=snap-1` with no `after` cursor
