# Equities Signal Source Badge Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Show a configured source badge on the equities Fluxboard Signal page for the reference leg only.

**Architecture:** Keep this frontend-only. Reuse the existing signal payload fields and derive the configured source from `route` or `instrument_id`, then render it only on the equities Signal page so other profiles remain unchanged.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, Fluxboard UI components.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | none | `fluxboard/**`, `docs/plans/2026-03-16-equities-signal-source-badge*` | shared | `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr` | none | `vitest PASS`, `git diff --check PASS` | Equities-only source badge implemented and verified |
| Task 1: Add equities-only source badge tests | completed | main | none | `fluxboard/tests/signal/**` | shared | `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr` | none | `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesSignalSourceBadge.test.tsx PASS` | Red/green test added for equities-only source badge behavior |
| Task 2: Implement source badge rendering | completed | main | Task 1: Add equities-only source badge tests | `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/types.ts` | shared | `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr` | none | `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesSignalSourceBadge.test.tsx PASS` | Added configured-source derivation and reference-leg badge rendering |
| Task 3: Verify scoped behavior | completed | main | Task 2: Implement source badge rendering | `fluxboard/tests/signal/**` | shared | `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr` | none | `pnpm --dir fluxboard exec vitest run tests/signal/SignalTable.audit.test.tsx tests/signal/EquitiesSignalSourceBadge.test.tsx PASS; git diff --check PASS` | Confirmed equities-only behavior without breaking active SignalTable suite |

---

### Task 1: Add equities-only source badge tests

**Files:**
- Create: `fluxboard/tests/signal/EquitiesSignalSourceBadge.test.tsx`

**Dependencies:** `none`

**Write Scope:** `fluxboard/tests/signal/EquitiesSignalSourceBadge.test.tsx`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesSignalSourceBadge.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Implement source badge rendering

**Files:**
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/types.ts`

**Dependencies:** `Task 1: Add equities-only source badge tests`

**Write Scope:** `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/types.ts`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesSignalSourceBadge.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Verify scoped behavior

**Files:**
- Modify: `docs/plans/2026-03-16-equities-signal-source-badge.md`

**Dependencies:** `Task 2: Implement source badge rendering`

**Write Scope:** `fluxboard/tests/signal/EquitiesSignalSourceBadge.test.tsx`, `docs/plans/2026-03-16-equities-signal-source-badge.md`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run tests/signal/EquitiesSignalSourceBadge.test.tsx`
- `git diff --check`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
