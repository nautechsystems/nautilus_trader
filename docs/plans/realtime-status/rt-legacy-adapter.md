# Realtime Legacy Adapter Lane Status

**Goal:** Land the shared legacy socket adapter bridge foundation in `useWebSocket` so flag-off surfaces stay on raw legacy payload handling while flag-on surfaces can route through one shared compatibility bridge path.

**Architecture:** Keep `useWebSocket(event, handler)` unchanged for all existing callsites. Add a module-level shared bridge registration path inside the hook module, allow `useWebSocket(..., { surface, ... })` to consume that shared bridge automatically, and preserve optional per-call bridge overrides for tests or exceptional wiring.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, Fluxboard Socket.IO hook.

## Progress Tracker

**Source of truth:** Update this table whenever task state, verification state, or commit state changes.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | none | `fluxboard/hooks/useWebSocket.ts`, `fluxboard/README.md`, `fluxboard/__tests__/realtime/legacy-adapter.test.tsx`, `docs/plans/realtime-status/rt-legacy-adapter.md` | `lanes/task-8-rt-legacy-adapter` | `/home/ubuntu/nautilus-trader-dev/.worktrees/task-8-rt-legacy-adapter` | Parent landed commit before this follow-up: `7d42f20516742ac257a4b7ac28291936dbe5c949`; follow-up diff adds module-level shared bridge registration/resolution, expands adapter tests, and rewrites this note to the fixed-key tracker shape | `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx` PASS (`15` tests); `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx __tests__/realtime/compatibility-matrix.test.tsx` PASS (`20` tests) | 2026-03-23 UTC follow-up review fix is implemented and verified locally; remaining work is downstream consumers registering and using the shared bridge |
| Task 1: Add failing shared-bridge registration tests | completed | main | none | `fluxboard/__tests__/realtime/legacy-adapter.test.tsx` | `lanes/task-8-rt-legacy-adapter` | `/home/ubuntu/nautilus-trader-dev/.worktrees/task-8-rt-legacy-adapter` | Diff vs parent `7d42f20516742ac257a4b7ac28291936dbe5c949` adds shared-bridge registration and override coverage | `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx` RED: exit `1`; `2` new tests failed because `registerSharedWebSocketBridge` was undefined on the lane tip | Added focused tests that require a production shared bridge path and preserve per-call override precedence |
| Task 2: Implement shared bridge registry in `useWebSocket` | completed | main | Task 1: Add failing shared-bridge registration tests | `fluxboard/hooks/useWebSocket.ts`, `fluxboard/README.md` | `lanes/task-8-rt-legacy-adapter` | `/home/ubuntu/nautilus-trader-dev/.worktrees/task-8-rt-legacy-adapter` | Diff vs parent `7d42f20516742ac257a4b7ac28291936dbe5c949` adds `registerSharedWebSocketBridge(...)`, `resetSharedWebSocketBridgeForTests()`, and shared-bridge resolution before per-call fallback | `VITEST_FULL=1 pnpm exec vitest run sockets.test.ts __tests__/realtime/legacy-adapter.test.tsx` PASS (`15` tests) | Shared bridge path now exists in production hook code; 2-argument legacy subscriptions still route directly to the socket |
| Task 3: Rewrite lane note to fixed-key status template | completed | main | Task 2: Implement shared bridge registry in `useWebSocket` | `docs/plans/realtime-status/rt-legacy-adapter.md` | `lanes/task-8-rt-legacy-adapter` | `/home/ubuntu/nautilus-trader-dev/.worktrees/task-8-rt-legacy-adapter` | Replaced the freeform note with the repo-standard Progress Tracker table and explicit verification/blocker/handoff sections | `git diff --check` PASS | This row is the required status-template conversion from the review findings |

## Blockers

- None in owned scope.

## Next Handoff

- Register one shared compatibility bridge for the first flag-on realtime surface instead of passing bespoke bridge objects at each callsite.
- Keep flag-off surfaces on the unchanged 2-argument legacy path so no standard-payload assumptions leak into legacy-only panels.
- Preserve `legacy-adapter.test.tsx` and `compatibility-matrix.test.tsx` in future rollout verification so shared-bridge regressions fail immediately.
