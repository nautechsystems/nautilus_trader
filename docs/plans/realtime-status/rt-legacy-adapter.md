# Realtime Legacy Adapter Foundation Status

Date: 2026-03-23
Branch: `lanes/task-8-rt-legacy-adapter`
Worktree: `/home/ubuntu/nautilus-trader-dev/.worktrees/task-8-rt-legacy-adapter`

## Current Commit / Diff

- Base commit before this lane commit: `8eaae3342b7c2fdc2bbb49a9edf9eaaaa1b8d770`
- Owned file changes in this lane:
  - `fluxboard/hooks/useWebSocket.ts` modified
  - `fluxboard/README.md` modified
  - `fluxboard/__tests__/realtime/legacy-adapter.test.tsx` added
- Tracked diff summary before commit: `2 files changed, 67 insertions(+), 9 deletions(-)`
- `fluxboard/sockets.test.ts` stayed unchanged; the existing socket state-machine coverage remains part of the verification bundle.

## TDD Verification

### RED

- Command: `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx`
- Result: exit `1`
- Failure summary:
  - `sockets.test.ts` passed (`8` tests)
  - `__tests__/realtime/legacy-adapter.test.tsx` failed `4` of `5` tests
  - The failures were the intended seam failures: `resolveMode` had `0` calls, the shared `bridge.subscribe` path had `0` calls, and the injected legacy subscribe path had `0` calls because `useWebSocket` only supported the legacy 2-argument subscription shape.

### GREEN

- Command: `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx`
- Result: exit `0`; `2` files passed, `13` tests passed
- Command: `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
- Result: exit `0`; `3` files passed, `18` tests passed

## What Changed

- `useWebSocket` now accepts an optional third argument with:
  - `surface`
  - injected legacy `subscribe`
  - shared `bridge.resolveMode(...)`
  - shared `bridge.subscribe(...)`
- The default `useWebSocket(event, handler)` path still subscribes directly to the legacy socket and forwards the raw payload without standard-contract assumptions.
- When the resolved mode is `standard`, the hook routes through the shared bridge seam instead of the direct socket subscriber.
- The hook still uses a handler ref, so handler-only rerenders do not create duplicate subscriptions.
- Mode flips clean up the previous legacy or bridge subscription before subscribing again.
- `fluxboard/README.md` now documents the third-argument adapter seam and the expectation that flag-on surfaces reuse a shared bridge.

## Blockers

- None in owned scope.
- Follow-on surfaces still need to provide one shared bridge implementation and choose when `resolveMode(...)` returns `standard`.

## Next Handoff

- Reuse the new `useWebSocket(..., options)` seam for the first flag-on panel instead of introducing per-panel bridge glue.
- Keep the bridge implementation shared and inject it through the hook so flag-off surfaces continue to use the untouched legacy path.
- Retain the new adapter regression suite plus `compatibility-matrix.test.tsx` in subsequent rollout verification so bridge wiring cannot regress silently.
