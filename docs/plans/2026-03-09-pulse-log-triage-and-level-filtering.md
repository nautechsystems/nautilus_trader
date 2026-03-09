# Pulse Log Triage and Level Filtering Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make Pulse faster to triage by letting operators filter service logs by severity, open directly on the latest error line, and see better error context from the jobs table before opening the full log view.

**Architecture:** Use a hybrid approach. Keep the existing raw `/api/pulse/jobs/{job_id}/logs` text response for compatibility, but tighten the backend job-summary parser so `errors.last_seen` and `errors.preview` become more actionable in the table. Add frontend log parsing and severity filters in the Pulse modal so operators can switch between `All`, `Error`, `Warning`, and `Info` views without inventing a separate backend log format for the first iteration. Reuse the existing Fluxboard Alerts severity vocabulary instead of creating a second set of level names.

**Tech Stack:** Python/Flask Pulse API, `journalctl`, React, TypeScript, Vitest, pytest, CSS.

## Current State Snapshot

- `pulse-ui/src/App.tsx` already supports `Show only jobs with errors`, but the filter stops at the job row level.
- `pulse-ui/src/components/JobRow.tsx` already renders `errors.preview`, but it is muted, truncated to one line, and not clickable.
- `pulse-ui/src/components/LogsModal.tsx` only shows the last 300 raw log lines with refresh and close controls.
- `pulse-ui/src/api.ts` already allows `getJobLogs(jobId, lines)`, but the modal always uses the default line count.
- `systems/flux/flux/pulse/api.py` already extracts `errors.count` and `errors.preview`, but `errors.last_seen` is always `null`.
- Fluxboard Alerts already uses `ALL / CRITICAL / WARNING / INFO`; Pulse should mirror that severity model rather than inventing new operator terminology.

## Recommended Scope

1. Add modal severity filters: `All`, `Error`, `Warning`, `Info`.
2. Make the newest error easy to reach: opening from an error preview should land in an error-filtered view and jump to the latest matching line.
3. Populate and display `errors.last_seen` so the jobs table answers "is this stale or current?" without opening logs.
4. Add one small quality-of-life control for broader context: selectable line windows such as `300` and `1000`.
5. Keep the initial scope out of "mini observability platform" territory: no streaming tail, no cross-service aggregation, no full-text search, no saved views in this pass.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Completed with backend commit `28f82c852`, frontend log-triage commit `d7ba0daee`, and docs commit `0690f6066`; final verification passed: `pytest tests/unit_tests/flux/pulse/test_api.py -v`, `pnpm --dir pulse-ui test`, and `pnpm --dir pulse-ui build` |
| Task 1: Harden Pulse Error Summary Metadata | completed | main | Spec and quality review approved; `pytest tests/unit_tests/flux/pulse/test_api.py -k "error_info or list_jobs or logs" -v` -> 6 passed; committed as `28f82c852` |
| Task 2: Add Severity Filters To The Logs Modal | completed | main | Shipped in `d7ba0daee`; severity filters, line-window selection, latest-error targeting, and the no-match fallback regression fix are covered by `src/logs.test.ts` and `src/components/LogsModal.test.tsx`; `pnpm --dir pulse-ui test` passed |
| Task 3: Make Job-Row Error Previews Actionable | completed | main | Shipped in `d7ba0daee`; error previews open the modal in `Error` mode, `errors.last_seen` renders in rows, `View Logs` stays unfiltered, and the neutral no-preview row path is covered in `src/App.test.tsx`; external spec-review agents timed out, so spec was validated locally and the full frontend suite passed |
| Task 4: Document And Verify The New Triage Flow | completed | main | API docs and Pulse UI README updated in `0690f6066`; final verification passed: `pytest tests/unit_tests/flux/pulse/test_api.py -v`, `pnpm --dir pulse-ui test`, and `pnpm --dir pulse-ui build` |

---

### Task 1: Harden Pulse Error Summary Metadata

**Files:**
- Modify: `systems/flux/flux/pulse/api.py`
- Test: `tests/unit_tests/flux/pulse/test_api.py`

**Step 1: Write the failing backend tests**

Add tests that prove:

1. `_extract_error_info(...)` returns the newest matching timestamp in `last_seen` when the journal summary output is parseable.
2. `/api/pulse/jobs` still returns the same job shape, but now includes a non-null `errors.last_seen` for jobs with current matching error lines.
3. Benign restart noise that is already ignored does not accidentally populate `last_seen`.

Run:

```bash
pytest tests/unit_tests/flux/pulse/test_api.py -k "error_info or list_jobs" -v
```

Expected: FAIL because `last_seen` is currently always `null`.

**Step 2: Implement the minimal backend change**

Update the Pulse summary path to make timestamps parseable and reusable:

1. Keep `/api/pulse/jobs/{job_id}/logs` returning plain text for compatibility.
2. Change the summary-only `journalctl` call inside `_service_payload(...)` to a stable, parseable output format such as `short-iso`.
3. Extend `_extract_error_info(...)` so it captures both the newest matching line text (`preview`) and the timestamp of that newest match (`last_seen`).
4. Preserve the current ignore list for known benign noise.

Expected: job rows gain meaningful `last_seen` values without changing the raw logs endpoint contract.

**Step 3: Run the backend tests to green**

Run:

```bash
pytest tests/unit_tests/flux/pulse/test_api.py -k "error_info or list_jobs or logs" -v
```

Expected: PASS for the touched Pulse API tests, including the existing logs route behavior.

**Step 4: Commit**

```bash
git add \
  systems/flux/flux/pulse/api.py \
  tests/unit_tests/flux/pulse/test_api.py
git commit -m "feat(pulse): add error last-seen metadata"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Add Severity Filters To The Logs Modal

**Files:**
- Create: `pulse-ui/src/logs.ts`
- Create: `pulse-ui/src/logs.test.ts`
- Create: `pulse-ui/src/components/LogsModal.test.tsx`
- Modify: `pulse-ui/src/components/LogsModal.tsx`
- Modify: `pulse-ui/src/index.css`

**Step 1: Write the failing frontend tests**

Cover these behaviors:

1. raw log lines are classified into `ERROR`, `WARNING`, `INFO`, or `OTHER` with a small deterministic parser
2. the modal renders filter controls and only shows matching lines when a filter is active
3. the modal reports match counts per level so operators can tell what is hidden
4. changing the line window from `300` to `1000` triggers a refetch with the selected limit

Run:

```bash
pnpm --dir pulse-ui test -- src/logs.test.ts src/components/LogsModal.test.tsx
```

Expected: FAIL because the parser and modal controls do not exist yet.

**Step 2: Implement the parsing and filtering flow**

Add a small log utility module that:

1. splits raw `journalctl` text into individual lines
2. derives a display severity from the line content using the same operator-facing categories as Fluxboard Alerts
3. returns stable line records for rendering and counting

Then update `LogsModal.tsx` to:

1. render severity filter chips or buttons for `All`, `Error`, `Warning`, and `Info`
2. render match counts so hidden lines are obvious
3. support line window selection using the existing `getJobLogs(jobId, lines)` helper
4. keep the raw all-lines view one click away so filtered mode never traps the operator

Expected: operators can collapse noisy info lines and focus on the severity bucket they care about.

**Step 3: Add quick navigation to the latest error**

Extend the modal behavior so that when it opens in an error-focused mode it:

1. scrolls the latest matching error into view
2. visually distinguishes the active target line
3. falls back cleanly to the unfiltered view if there are no matches in the fetched window

Expected: opening a broken service gets the operator to the newest error line immediately instead of at the top of a long log block.

**Step 4: Run frontend verification**

Run:

```bash
pnpm --dir pulse-ui test -- src/logs.test.ts src/components/LogsModal.test.tsx src/App.test.tsx
```

Expected: PASS for the new parser and modal coverage, with no regressions in the existing app flow.

**Step 5: Commit**

```bash
git add \
  pulse-ui/src/logs.ts \
  pulse-ui/src/logs.test.ts \
  pulse-ui/src/components/LogsModal.tsx \
  pulse-ui/src/components/LogsModal.test.tsx \
  pulse-ui/src/index.css
git commit -m "feat(pulse-ui): add log severity filters"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Make Job-Row Error Previews Actionable

**Files:**
- Modify: `pulse-ui/src/App.tsx`
- Modify: `pulse-ui/src/App.test.tsx`
- Modify: `pulse-ui/src/components/JobRow.tsx`
- Modify: `pulse-ui/src/index.css`

**Step 1: Write the failing app-level tests**

Add tests that prove:

1. clicking an error preview opens the logs modal in an error-focused mode
2. clicking the normal `View Logs` button still opens the unfiltered modal
3. a populated `errors.last_seen` is visible in the job row
4. job rows without error previews keep the current neutral rendering

Run:

```bash
pnpm --dir pulse-ui test -- src/App.test.tsx
```

Expected: FAIL because the row preview is currently plain text and the modal has no initial filter state.

**Step 2: Implement the triage-first row affordance**

Update the job row and app state so that:

1. `errors.preview` becomes a clickable affordance when present
2. the preview opens the modal with an initial severity filter of `Error`
3. the existing `View Logs` action remains available for the full raw view
4. `errors.last_seen` is rendered near the preview or badge so operators can judge recency at a glance

Expected: the jobs table becomes a real triage surface instead of just a launch point for a modal.

**Step 3: Run UI verification**

Run:

```bash
pnpm --dir pulse-ui test -- src/App.test.tsx src/index.css.test.ts
pnpm --dir pulse-ui build
```

Expected: PASS for the updated app behavior and CSS smoke tests, plus a successful production build.

**Step 4: Commit**

```bash
git add \
  pulse-ui/src/App.tsx \
  pulse-ui/src/App.test.tsx \
  pulse-ui/src/components/JobRow.tsx \
  pulse-ui/src/index.css
git commit -m "feat(pulse-ui): make error previews actionable"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Document And Verify The New Triage Flow

**Files:**
- Modify: `systems/flux/docs/api.md`
- Modify: `pulse-ui/README.md`

**Step 1: Update the API contract docs**

Document:

1. that `/api/pulse/jobs` error summaries now populate `errors.last_seen` when a matching line is available
2. that `/api/pulse/jobs/{job_id}/logs` remains a raw text endpoint and the severity filtering is a Pulse UI behavior layered on top

Expected: the contract stays explicit and operators do not assume a new structured logs API exists.

**Step 2: Update the Pulse UI README**

Document the new operator workflow:

1. `Show only jobs with errors` narrows the table
2. clicking an error preview opens logs directly on the newest error
3. the modal supports severity filters and larger line windows for extra context

Expected: local testing and rollout docs match the shipped behavior.

**Step 3: Run full verification**

Run:

```bash
pytest tests/unit_tests/flux/pulse/test_api.py -v
pnpm --dir pulse-ui test
pnpm --dir pulse-ui build
```

Expected: PASS across backend Pulse API tests and the Pulse UI test/build pipeline.

**Step 4: Commit**

```bash
git add \
  systems/flux/docs/api.md \
  pulse-ui/README.md
git commit -m "docs(pulse): document log triage workflow"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Notes For The Implementer

1. `systems/flux/flux/pulse/api.py` and `flux/pulse/api.py` are hardlinked in this worktree; edit the canonical `systems/...` path and verify the shared inode remains intact.
2. Keep the first pass deliberately small. A good outcome is "operators can isolate errors immediately" rather than "Pulse becomes Kibana."
3. If the frontend severity parser proves too noisy, tighten the regexes to favor fewer false positives over overly aggressive classification.
4. Do not change the default raw logs endpoint shape unless a later task explicitly opts into a new API contract.

## Explicitly Deferred

1. live streaming or tail-following logs
2. full-text log search
3. cross-service aggregated log views
4. saving filter state in the URL
5. a new Pulse-native alerts page, because Fluxboard Alerts already owns that surface
