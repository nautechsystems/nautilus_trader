# Fluxboard Params Bulk Draft Commit Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make Fluxboard bulk-row edits for `qty` and other non-toggle params apply consistently to the current filtered strategies without requiring a separate Enter commit step before `Save All` or `Save Selected`.

**Architecture:** Keep `bulkDrafts` as transient input state while the operator is typing, but introduce a single commit/flush path that promotes pending bulk drafts into `paramValues`, `dirtyParams`, `errorParams`, and undo metadata before save actions and when a bulk input is explicitly committed by blur/keyboard. Preserve the current immediate-apply behavior for the `bot_on` toggle, and make the typed-input path use the same shared mutation helper so filtered-row updates, validation, undo, and persistence stay aligned.

**Tech Stack:** React 18, TypeScript, Vite, Vitest, React Testing Library, sonner.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Task 1: Lock the regression with failing tests | completed | main | Reviewed red-phase qty regressions landed; focused bulk-row suite failed with the 2 intended bulk-draft cases before implementation |
| Task 2: Unify bulk draft commit behavior in Params | completed | main | Spec and quality reviews passed; local green runs confirmed for bulk-row suite 9/9, keyboard shortcuts 2/2, and status filter 3/3 |
| Task 3: Verify the fix and decide whether to unquarantine coverage | completed | main | Moved bulk-row regression to `fluxboard/__tests__/ParamsBulkApplyAllRow.test.tsx`; default-path verification passed (`pnpm test:run __tests__/ParamsBulkApplyAllRow.test.tsx`, 9/9) and nearby keyboard suite passed (2/2); README and plan docs updated to match the new path |

---

## Investigation Summary

- `fluxboard/Params.tsx` keeps typed bulk-row values in `bulkDrafts`, but `handleSaveAll`, `saveAllSelected`, and `collectBulkUpdates` only persist data already present in `paramValues`/`dirtyParams`.
- `bot_on` is the exception because its bulk toggle path calls `applyBulkDraft('bot_on', value)` immediately on change.
- Existing regression coverage only proves the Enter-driven path (`cex_bid_edge`) and the immediate toggle path (`bot_on`). The bulk component test is also excluded from the default Vitest run unless `VITEST_FULL=1`, which is why this inconsistency can slip through normal local verification.
- On clean `main`, `VITEST_FULL=1 pnpm test:run __tests__/components/ParamsBulkApplyAllRow.test.tsx` currently passes. The user-reported `qty` failure is therefore most likely the uncommitted-draft path rather than a general failure in `applyBulkDraft`.

### Task 1: Lock the regression with failing tests

**Files:**
- Modify: `fluxboard/__tests__/ParamsBulkApplyAllRow.test.tsx`
- Reference: `fluxboard/Params.tsx`
- Reference: `fluxboard/vite.config.ts`

**Step 1: Add a failing bulk-draft regression for `qty`**

Add a test that:
- renders `Params`
- filters to two strategies
- types a bulk `qty` value into the sticky bulk row
- clicks `Save All` without pressing Enter first
- expects both filtered strategies to be saved with the new `qty`

Use a schema shape that includes `qty` as a numeric param so the test matches the reported behavior.

**Step 2: Add a failing UI-state regression for filtered row updates**

Add a second test that:
- types a bulk `qty` value
- commits it via the chosen UX event (`blur`, `Enter`, or save flush)
- verifies the visible filtered row cells show the updated `qty`

This keeps the fix honest on both persistence and visible-row refresh.

**Step 3: Run the focused regression test to verify it fails on current code**

Run: `VITEST_FULL=1 pnpm test:run __tests__/ParamsBulkApplyAllRow.test.tsx`

Expected before implementation:
- the new `qty` regression fails because `updateParams` is never called with the typed bulk draft unless Enter already applied it

**Step 4: Record the failure in the Progress Tracker**

Update Task 1 to `completed` only after the regression is failing for the intended reason.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Unify bulk draft commit behavior in Params

**Files:**
- Modify: `fluxboard/Params.tsx`
- Reference: `fluxboard/components/params/ParamCell.tsx`

**Step 1: Extract a shared bulk-commit helper**

Refactor the current `applyBulkDraft` logic into a reusable helper that accepts:
- `paramKey`
- committed value
- target strategy IDs
- whether the operation should create undo metadata / toast feedback

That helper must remain the single place that mutates:
- `paramValues`
- `dirtyParams`
- `errorParams`
- `remoteUpdatedRows`
- `lastBulkChangeOp`

**Step 2: Add a flush path for pending typed drafts**

Introduce a function that inspects the current `bulkDrafts` and current `bulkTargetIds`, then commits any pending typed drafts before save collection runs.

Minimum integration points:
- `handleSaveAll`
- `saveAllSelected`

Preferred UX addition if it stays clean:
- commit the active bulk draft on blur for non-toggle inputs so filtered rows visibly update before save

**Step 3: Preserve current toggle semantics**

Keep `bot_on` on the immediate-apply path, but switch it to the same shared helper so toggle and typed inputs no longer diverge in behavior.

**Step 4: Keep undo behavior coherent**

Only record undo metadata for real committed bulk changes, not every keystroke while typing in a bulk input.

If blur-based commit is added, ensure:
- a simple click-away creates one undoable operation
- repeated typing before blur does not create partial operations

**Step 5: Re-run the focused regression test**

Run: `VITEST_FULL=1 pnpm test:run __tests__/ParamsBulkApplyAllRow.test.tsx`

Expected after implementation:
- all bulk apply tests pass, including the new `qty` regression

**Step 6: Update the Progress Tracker**

Mark Task 2 `completed` only after the focused regression passes.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Verify the fix and decide whether to unquarantine coverage

**Files:**
- Modify: `fluxboard/vite.config.ts` (only if the test should be unquarantined)
- Modify: `fluxboard/__tests__/ParamsBulkApplyAllRow.test.tsx` (if test location/content changes)
- Reference: `fluxboard/README.md`

**Step 1: Run the focused bulk-row regression suite**

Run: `pnpm test:run __tests__/ParamsBulkApplyAllRow.test.tsx`

Expected:
- PASS

**Step 2: Run a nearby Params regression suite that exercises save behavior**

Run: `pnpm test:run __tests__/ParamsKeyboardShortcuts.test.tsx`

Expected:
- PASS

**Step 3: Decide whether this coverage should remain quarantined**

If `ParamsBulkApplyAllRow.test.tsx` is stable and fast after the fix:
- remove or narrow the `__tests__/components/**` exclusion in `fluxboard/vite.config.ts`, or
- move the specific regression to a non-quarantined test path

If it is still too heavy:
- keep the quarantine
- document that the implementation relies on `VITEST_FULL=1` coverage for this path

**Step 4: Update the Progress Tracker and summarize verification evidence**

Capture the exact commands and pass/fail outcomes in the tracker notes.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
