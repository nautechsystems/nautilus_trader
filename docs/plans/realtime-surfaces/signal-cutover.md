# Signal Realtime Cutover

## Status

`2026-03-19`: local frontend cutover is implemented for the Signal surface in `lanes/task-6-rt-signal`.

This is not a blanket claim that the entire live rollout is finished. It means the owned Signal panel path now matches the realtime-standard frontend shape closely enough for the local task gate:

- rendered table state is driven through the shared realtime surface controller
- row age ticking uses the shared viewport clock instead of per-row timers or visibility observers
- changed-id socket payloads schedule one-shot invalidation recovery instead of immediate snapshot thrash
- Maker V4 rows reconcile in place through `liveDataVersion` so a single-row update does not force a new mapped data array

## What Changed

### Standard controller path

`fluxboard/components/domain/signal/SignalTable.tsx` now keeps the zustand signal store as the raw merge source for compatibility, but the displayed `EnrichedRow[]` is synchronized into `createRealtimeSurfaceController(...)` and consumed through `useRealtimeSurfaceController(...)`.

Implications:

- steady-state row deltas patch the controller in place
- the no-filter hot path keeps the visible data array stable
- custom user sorts intentionally fall back to snapshot rebuilds so TanStack Table can recompute ordering correctly with the current `DataTable` contract

### Recovery semantics

The old overlap of polling + watchdog + immediate changed-id refresh is removed from the Signal surface. Recovery now uses `useRecoveryScheduler(...)`:

- initial load still fetches a snapshot
- socket connect triggers a sync snapshot
- socket disconnect, connect errors, reconnect attempts, and changed-id invalidations schedule one recovery fetch
- repeated invalidations while recovery is already pending do not queue repeated snapshot fetches

This is intentionally invalidate-only recovery, not continuous fallback polling.

### Shared clock

`fluxboard/components/domain/signal/useVisibleNowMs.ts` now delegates to the shared viewport clock. The Signal surface no longer allocates per-row `IntersectionObserver`s or per-row timers in the large-table path.

## Verification

Local task verification passed with:

```bash
VITEST_FULL=1 pnpm --dir fluxboard exec vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/SignalTable.sourceOfTruth.test.tsx components/domain/signal/SignalTable.age-ticking.test.tsx components/domain/signal/SignalTable.store.test.ts tests/signal/MakerV4SignalTable.test.tsx __tests__/panels/signal.test.tsx
```

## Known Limits

- `fluxboard/e2e/realtime-cutovers/signal.spec.ts` is present only as a `test.fixme(...)`. The app does not yet expose a deterministic Signal realtime fixture or a supported socket/debug inspection surface for Playwright, so pretending this browser cutover is covered would be dishonest.
- Existing test runs still emit React `act(...)` warnings from shared tooltip behavior in unrelated UI plumbing. The owned task verification passes despite those warnings.
