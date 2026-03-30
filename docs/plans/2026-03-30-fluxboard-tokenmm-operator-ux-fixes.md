# Fluxboard TokenMM Operator UX Fixes Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Align Fluxboard operator UX with live TokenMM state by keeping warning alerts visible and removing the healthy shared-balance banner.

**Architecture:** Keep live trading behavior unchanged. Limit scope to Fluxboard presentation semantics so Signals and Balances remain the operational source of truth while Alerts stops silently hiding warning rows, and the Balances panel only surfaces the shared-account banner when reconciliation is degraded.

**Tech Stack:** React, TypeScript, Vitest

**Context Docs:**
- Design: `none` (scope was approved directly in chat because this is a small UI correction)
- PRD: `none`
- Relevant specs/runbooks: `fluxboard/docs/tokenmm_contract.md`, `docs/plans/realtime-surfaces/alerts-cutover.md`

**Decision Summary:**
- Warning alerts must remain visible until the backend snapshot removes them or the operator clears them.
- Healthy shared-account status remains available in row data, but the positive top banner is removed to reduce noise.
- No backend alert semantics or live TokenMM routing changes are in scope.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | none | `fluxboard/components/domain/alerts/AlertsTable.tsx`, `fluxboard/constants.ts`, `fluxboard/Balances.tsx`, `fluxboard/__tests__/panels/alerts.test.tsx`, `fluxboard/Balances.test.tsx`, `docs/plans/2026-03-30-fluxboard-tokenmm-operator-ux-fixes.md` | `shared` | `shared` | none | `cd fluxboard && pnpm exec vitest run __tests__/panels/alerts.test.tsx Balances.test.tsx` PASS (`35 passed`); `git diff --check` PASS | Both UI tasks completed after spec + quality review with no findings |
| Task 1: Keep warning alerts visible | completed | main | none | `fluxboard/components/domain/alerts/AlertsTable.tsx`, `fluxboard/constants.ts`, `fluxboard/__tests__/panels/alerts.test.tsx` | `shared` | `shared` | none | `cd fluxboard && pnpm exec vitest run __tests__/panels/alerts.test.tsx` PASS (`12 passed`) | Spec review and quality review found no issues |
| Task 2: Remove healthy balances banner | completed | main | Task 1: Keep warning alerts visible | `fluxboard/Balances.tsx`, `fluxboard/Balances.test.tsx` | `shared` | `shared` | none | `cd fluxboard && pnpm exec vitest run Balances.test.tsx` PASS (`23 passed`) | Spec review and quality review found no issues |

---

### Task 1: Keep warning alerts visible

**Files:**
- Modify: `fluxboard/components/domain/alerts/AlertsTable.tsx`
- Modify: `fluxboard/constants.ts`
- Modify: `fluxboard/__tests__/panels/alerts.test.tsx`

**Dependencies:** `none`

**Write Scope:** `fluxboard/components/domain/alerts/AlertsTable.tsx`, `fluxboard/constants.ts`, `fluxboard/__tests__/panels/alerts.test.tsx`

**Verification Commands:**
- `cd fluxboard && pnpm exec vitest run __tests__/panels/alerts.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Remove healthy balances banner

**Files:**
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/Balances.test.tsx`

**Dependencies:** `Task 1: Keep warning alerts visible`

**Write Scope:** `fluxboard/Balances.tsx`, `fluxboard/Balances.test.tsx`

**Verification Commands:**
- `cd fluxboard && pnpm exec vitest run Balances.test.tsx`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
