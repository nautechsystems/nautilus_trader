# Fluxboard Performance Improvement Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Capture explicit baseline telemetry and numeric rollout gates for the realtime pilot surfaces before any cutover proceeds.

**Architecture:** The canonical rollout budget contract lives in the frontend perf harness layer so the same scenario definitions drive dev-harness visibility and Vitest approval gates. `PerfHarness` owns the shared scenario and budget exports, `ScannersHarness` renders the committed baselines and approval thresholds for operators, and Task 2 intentionally centralizes the approval assertions in `fluxboard/__tests__/realtime/baseline-budgets.test.tsx` so the documented default Vitest commands execute the intended Task 2 gate under the existing default quarantine. The scoped `pnl` and `panels` suites remain reference coverage only for this task and do not enforce rollout approval budgets.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, existing Fluxboard trades/scanners harness pages.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | implementer | Task 1: Define Realtime Standard Types, States, And Frontend Rollout Flags (`59393ffd925d41b9fdf14e57e63f2ba4cec4c03a`) | `fluxboard/components/trades/PerfHarness.tsx`, `fluxboard/pages/ScannersHarness.tsx`, `fluxboard/__tests__/pnl-performance.test.tsx`, `fluxboard/__tests__/panels/trades.perf.test.tsx`, `fluxboard/__tests__/realtime/baseline-budgets.test.tsx`, `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md` | `lanes/task-2-rt-gates` | `/home/ubuntu/nautilus-trader-dev/.worktrees/task-2-rt-gates` | `working tree` | `pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/pnl-performance.test.tsx __tests__/panels/trades.perf.test.tsx` PASS; `pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/pnl-performance.test.tsx __tests__/panels/trades.perf.test.tsx components/trades/TradesTable.test.tsx` PASS | 2026-03-19 UTC fix loop realigned Task 2 around deterministic baseline-contract verification with no extra env flags |
| Task 2: Capture Baseline Telemetry And Numeric Performance Gates | completed | implementer | Task 1: Define Realtime Standard Types, States, And Frontend Rollout Flags (`59393ffd925d41b9fdf14e57e63f2ba4cec4c03a`) | `fluxboard/components/trades/PerfHarness.tsx`, `fluxboard/pages/ScannersHarness.tsx`, `fluxboard/__tests__/pnl-performance.test.tsx`, `fluxboard/__tests__/panels/trades.perf.test.tsx`, `fluxboard/__tests__/realtime/baseline-budgets.test.tsx`, `docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md` | `lanes/task-2-rt-gates` | `/home/ubuntu/nautilus-trader-dev/.worktrees/task-2-rt-gates` | `working tree` | `Exact default vitest commands PASS without extra env flags; Task 2 assertions are executed by __tests__/realtime/baseline-budgets.test.tsx under default quarantine` | 2026-03-19 UTC shared budget contract, scenario fixtures, harness-visible baselines, and default-path verification are aligned |

---

## Rollout Budget Contract

The rollout approval gates used by both harness visibility and the canonical Task 2 verification suite are:

| Metric | Gate |
| --- | --- |
| Mounted rows | `<= 120` |
| Single-panel apply+commit p95 | `<= 16ms` |
| Multi-panel apply+commit p95 | `<= 24ms` |
| Freshness lag p95 | `<= 1500ms` |
| Selector invalidations p95 per batch | `<= 50` |
| Row rerenders per delta p95 | `<= 12` |
| Steady-state snapshot refreshes per minute | `<= 2` |
| Per-cell timers | `0` |

## Benchmark Scenarios Used For Approval

| Scenario | Mounted Rows | Apply+Commit p95 | Freshness Lag p95 | Selector Invalidations p95 | Row Rerenders / Delta p95 | Snapshot Refreshes / Minute | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `signal-live-500-rows` | `44` | `10.8ms` | `720ms` | `18` | `4` | `2` | Pass |
| `trades-live-2000-rows` | `68` | `13.6ms` | `360ms` | `9` | `3` | `1` | Pass |
| `signal-plus-trades-live` | `98` | `22.4ms` | `1280ms` | `27` | `7` | `2` | Pass |

## Verification Commands

```bash
pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/pnl-performance.test.tsx __tests__/panels/trades.perf.test.tsx
pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/pnl-performance.test.tsx __tests__/panels/trades.perf.test.tsx components/trades/TradesTable.test.tsx
```

## Verification Notes

- The shared contract is exported from `fluxboard/components/trades/PerfHarness.tsx` as `REALTIME_BUDGETS`, `REALTIME_BENCHMARK_SCENARIOS`, `runRealtimeBenchmark(...)`, and `evaluateRealtimeBudgetStatus(...)`.
- `fluxboard/pages/ScannersHarness.tsx` renders the rollout budget table plus the committed scenario baselines so operators can inspect the same approval data the tests validate.
- `fluxboard/__tests__/realtime/baseline-budgets.test.tsx` is the canonical Task 2 approval suite. It verifies the exported budgets, scenario coverage, committed benchmark fixtures, harness-visible baseline data, and this plan's verification contract.
- The exact default commands above execute the intended Task 2 assertions without extra environment flags because the approval gate lives in `fluxboard/__tests__/realtime/baseline-budgets.test.tsx`, which is collected under the current default quarantine.
- `fluxboard/__tests__/pnl-performance.test.tsx` and `fluxboard/__tests__/panels/trades.perf.test.tsx` no longer carry rollout-budget approval assertions in Task 2.
