# TokenMM Fluxboard Balances Upgrade Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Redesign `tokenmm.fluxboard.balances` into a TokenMM-specific live CEX inventory ledger grouped by coin, with clear venue/account drilldown, visible freshness, and no embedded risk workflow.

**Architecture:** Keep backend row composition, dedupe, canonical naming, and risk-group authorship authoritative. Add a thin TokenMM balances view-model layer in Fluxboard that derives operator-facing sections, row type labels, summary cards, and freshness/status badges from the existing payload, then split the current monolithic `Balances.tsx` into small TokenMM-specific UI components. Only add backend fields if the existing payload cannot express required status semantics without inventing fake rows.

**Tech Stack:** React, TypeScript, Zustand, Vitest, Tailwind UI tokens, Flask/Python payloads only if a minimal contract extension is required

**Context Docs:**
- Design: `none` (user supplied an inline approved PRD/design in chat on 2026-03-31)
- PRD: `none` (user supplied inline PRD in chat on 2026-03-31)
- Relevant specs/runbooks: `fluxboard/docs/tokenmm_contract.md`, `fluxboard/docs/tokenmm_socket_contract.md`, `docs/architecture/tokenmm-portfolio-inventory-semantics.md`, `docs/architecture/tokenmm-risk-source-of-truth.md`

**Decision Summary:**
- `Balances` should become TokenMM-only holdings UI; remove the in-page risk mode rather than trying to share one surface between holdings and risk.
- Backend canonical naming, row collapse, and snapshot semantics remain source of truth; frontend heuristics stay fallback-only.
- Add a dedicated Fluxboard view-model/helper layer instead of bloating `Balances.tsx` further.
- Use two sections, `Stables` and `Trading Assets`, driven by stable classification already present in parent rows.
- Derive row-level `OK` and `STALE` from timestamps first; treat `PARTIAL` and `MISSING` as page or group-level states unless the backend exposes expected-entity metadata.
- Do not keep dead controls (`logicalOnly`, `stableOnly`, chain, wallet, risk subview) on the TokenMM balances page.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `fluxboard/Balances.tsx`, `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/Balances.test.tsx`, `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, `fluxboard/components/balances/*`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/_payloads_balances.py` | `codex/tokenmm-fluxboard-balances-upgrade` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-fluxboard-balances-upgrade` | none | not_run | 2026-03-31 UTC: execution started in dedicated worktree; branch-local plan copied from canonical repo because the handoff file was present there as an untracked working-tree document, not in branch history |
| Task 1: Add typed TokenMM balances metadata and view-model scaffolding | in_progress | main | none | `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/components/balances/tokenmmBalancesModel.ts`, `fluxboard/components/balances/tokenmmBalancesModel.test.ts` | `codex/tokenmm-fluxboard-balances-upgrade` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-fluxboard-balances-upgrade` | working tree dirty | `cd fluxboard && pnpm vitest run api.flux.test.ts components/balances/tokenmmBalancesModel.test.ts` -> FAIL (`components/balances/tokenmmBalancesModel.test.ts`: missing `./tokenmmBalancesModel` import; RED confirmed after `pnpm install --frozen-lockfile`) | 2026-03-31 UTC: added RED transport/model coverage; existing balances transport normalization is green, and the new helper module is the only failing gap |
| Task 2: Implement freshness, type, section, and summary derivation | not_started | unassigned | Task 1: Add typed TokenMM balances metadata and view-model scaffolding | `fluxboard/components/balances/tokenmmBalancesModel.ts`, `fluxboard/components/balances/tokenmmBalancesModel.test.ts` | `shared` | `shared` | none | not_run | Plan created |
| Task 3: Rebuild the Balances page around TokenMM-specific holdings UI | not_started | unassigned | Task 2: Implement freshness, type, section, and summary derivation | `fluxboard/Balances.tsx`, `fluxboard/components/balances/BalanceStatusBadge.tsx`, `fluxboard/components/balances/TokenMMBalancesStatusStrip.tsx`, `fluxboard/components/balances/TokenMMBalancesSummary.tsx`, `fluxboard/components/balances/TokenMMBalancesToolbar.tsx`, `fluxboard/components/balances/TokenMMBalancesTable.tsx`, `fluxboard/Balances.test.tsx` | `shared` | `shared` | none | not_run | Plan created |
| Task 4: Cover live-update behavior and remove dead balances affordances | not_started | unassigned | Task 3: Rebuild the Balances page around TokenMM-specific holdings UI | `fluxboard/Balances.test.tsx`, `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, `fluxboard/components/panels/BalancesPanel.test.tsx`, `fluxboard/components/balances/BalanceGroup.tsx`, `fluxboard/components/balances/BalanceRow.tsx` | `shared` | `shared` | none | not_run | Plan created |
| Task 5: Add minimal backend status hints only if frontend derivation is insufficient | not_started | unassigned | Task 3: Rebuild the Balances page around TokenMM-specific holdings UI | `systems/flux/flux/api/_payloads_balances.py`, `systems/flux/flux/api/app.py`, `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/Balances.test.tsx` | `shared` | `shared` | none | not_run | Optional gated task; only execute if row-level status requirements cannot be met from current payload |

---

### Task 1: Add typed TokenMM balances metadata and view-model scaffolding

**Files:**
- Create: `fluxboard/components/balances/tokenmmBalancesModel.ts`
- Create: `fluxboard/components/balances/tokenmmBalancesModel.test.ts`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/api.flux.test.ts`

**Dependencies:** `none`

**Write Scope:** `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/components/balances/tokenmmBalancesModel.ts`, `fluxboard/components/balances/tokenmmBalancesModel.test.ts`

**Verification Commands:**
- `cd fluxboard && pnpm vitest run api.flux.test.ts components/balances/tokenmmBalancesModel.test.ts`

**Step 1: Write failing API normalization coverage**

In `fluxboard/api.flux.test.ts`, add balances-focused tests that prove `api.getBalances()` preserves the metadata needed by the redesign:
- `degraded`
- `scope_status`
- `source`
- `stale_after_ms`
- `components`
- `missing_required`
- `stale_required`
- `null_qty_required`
- `aggregation_mode`

These tests should also assert that backend-authored canonical naming survives unchanged when present.

**Step 2: Add TokenMM balances payload/view-model types**

In `fluxboard/types.ts`, extend `BalancesPayload` with the optional metadata above and add local TokenMM-only display types for:
- parent display status
- child display status
- row type (`spot`, `perp`, `cash`)
- stable/trading section keys
- summary-card data
- toolbar filter state

Keep these types UI-specific. Do not overload backend `risk_groups` or invent new portfolio semantics inside shared transport types.

**Step 3: Create the view-model scaffolding**

Create `fluxboard/components/balances/tokenmmBalancesModel.ts` with exported pure helpers for:
- classifying child rows into `spot`, `perp`, or `cash`
- building search tokens from coin, venue, readable instrument labels, and account labels
- grouping rows into `stables` and `trading`
- deriving venue options from current child rows

Do not implement the full status/sort behavior yet. This task should produce the stable API surface that the UI will consume.

**Step 4: Add failing model tests**

In `fluxboard/components/balances/tokenmmBalancesModel.test.ts`, write focused cases for:
- type classification from canonical/product fields
- stable vs trading section split
- venue option extraction
- readable naming preference order: backend display name, then normalized fallback

Use fixtures modeled after current `Balances.test.tsx` payload shape so the helper and page tests share the same data assumptions.

**Step 5: Implement the minimal API/type/model changes**

Update `fluxboard/api.ts` to retain the extra payload metadata. Implement the minimal helper exports and types required to make the new tests pass, without changing `Balances.tsx` yet.

**Step 6: Run targeted tests**

Run the verification command above and confirm the new balances transport/model tests pass before moving on.

**Step 7: Commit**

```bash
git add fluxboard/types.ts fluxboard/api.ts fluxboard/api.flux.test.ts fluxboard/components/balances/tokenmmBalancesModel.ts fluxboard/components/balances/tokenmmBalancesModel.test.ts
git commit -m "feat: scaffold tokenmm balances view model"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Implement freshness, type, section, and summary derivation

**Files:**
- Modify: `fluxboard/components/balances/tokenmmBalancesModel.ts`
- Modify: `fluxboard/components/balances/tokenmmBalancesModel.test.ts`

**Dependencies:** `Task 1: Add typed TokenMM balances metadata and view-model scaffolding`

**Write Scope:** `fluxboard/components/balances/tokenmmBalancesModel.ts`, `fluxboard/components/balances/tokenmmBalancesModel.test.ts`

**Verification Commands:**
- `cd fluxboard && pnpm vitest run components/balances/tokenmmBalancesModel.test.ts`

**Step 1: Write failing status and sort tests**

Expand `fluxboard/components/balances/tokenmmBalancesModel.test.ts` with failing coverage for:
- parent sorting order: stale/error first, then highest absolute MV, then alphabetical coin
- child sorting order: highest absolute MV, then venue name, then type
- summary values: total MV, stable MV, non-stable MV, non-zero coin count, stale row count
- `hideZero` default behavior
- `search`, `venue`, and `type` filters
- persistent expansion compatibility when live payloads change but parent ids remain stable

**Step 2: Implement row freshness/status derivation**

In `tokenmmBalancesModel.ts`, derive display statuses using:
- row `last_ts` when present
- payload `generated_at`
- payload `stale_after_ms` with fallback to the current Fluxboard balances threshold
- payload-level degraded/scope metadata for header and section messaging

Use these rules:
- `OK` and `STALE` are row-level defaults
- `PARTIAL` is allowed for a parent/group when children are mixed fresh/stale or when scope degradation is active
- `MISSING` is page/scope-level only unless Task 5 adds explicit backend expected-entity hints

Do not synthesize fake child rows just to show `MISSING`.

**Step 3: Implement inventory composition derivation**

Add helpers that compute, for each parent row:
- net qty
- net MV
- mark
- spot qty subtotal
- perp qty subtotal
- venue count and short venue list
- freshest displayed timestamp
- parent status badge

For each child row, derive:
- readable primary label such as `Bybit Spot` or `OKX Swap`
- separate account label
- muted raw symbol/instrument text
- child status badge

**Step 4: Implement filter and section outputs**

Add a single pure function that takes rows plus the TokenMM toolbar filter state and returns:
- stable section rows
- trading section rows
- venue options
- summary cards
- stale/degraded counters
- expandability info

This function should be deterministic and side-effect free so `Balances.tsx` can stay thin.

**Step 5: Run targeted tests**

Run the verification command above and confirm the full model behavior is green before wiring UI components to it.

**Step 6: Commit**

```bash
git add fluxboard/components/balances/tokenmmBalancesModel.ts fluxboard/components/balances/tokenmmBalancesModel.test.ts
git commit -m "feat: derive tokenmm balances display model"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Rebuild the Balances page around TokenMM-specific holdings UI

**Files:**
- Create: `fluxboard/components/balances/BalanceStatusBadge.tsx`
- Create: `fluxboard/components/balances/TokenMMBalancesStatusStrip.tsx`
- Create: `fluxboard/components/balances/TokenMMBalancesSummary.tsx`
- Create: `fluxboard/components/balances/TokenMMBalancesToolbar.tsx`
- Create: `fluxboard/components/balances/TokenMMBalancesTable.tsx`
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/Balances.test.tsx`

**Dependencies:** `Task 2: Implement freshness, type, section, and summary derivation`

**Write Scope:** `fluxboard/Balances.tsx`, `fluxboard/components/balances/BalanceStatusBadge.tsx`, `fluxboard/components/balances/TokenMMBalancesStatusStrip.tsx`, `fluxboard/components/balances/TokenMMBalancesSummary.tsx`, `fluxboard/components/balances/TokenMMBalancesToolbar.tsx`, `fluxboard/components/balances/TokenMMBalancesTable.tsx`, `fluxboard/Balances.test.tsx`

**Verification Commands:**
- `cd fluxboard && pnpm vitest run Balances.test.tsx`

**Step 1: Write failing page-level tests for the new holdings UX**

In `fluxboard/Balances.test.tsx`, replace or extend expectations so they cover:
- no `Risk` tab/button inside `Balances`
- status strip text for live/snapshot/degraded state
- summary cards limited to inventory context
- TokenMM toolbar controls: search, venue filter, type filter, hide zero, expand/collapse all
- `Stables` and `Trading Assets` sections
- parent columns: coin, net qty, net MV, mark, spot qty, perp qty, venues, updated, status
- child columns: venue, account, type, symbol/instrument, qty, MV, mark, updated, status

**Step 2: Extract the new small UI components**

Create components for:
- page status strip
- summary card row
- toolbar
- status badge
- grouped table

Keep them presentational and feed them precomputed data from `tokenmmBalancesModel.ts`. Avoid moving filtering logic back into the components.

**Step 3: Simplify `Balances.tsx`**

Refactor `fluxboard/Balances.tsx` so it:
- keeps REST bootstrap and realtime subscription behavior unchanged
- removes `mode`, `riskSearch`, `riskNonZeroOnly`, `selectedRiskKey`, `selectedRiskLabel`
- removes `TableFilter`, `logicalOnly`, and `stableOnly`
- uses the new TokenMM filter state and model helpers
- preserves expanded parent ids across live updates when ids stay stable
- keeps deterministic sort/filter behavior under websocket churn

Do not touch websocket lineage behavior unless a test proves the redesign broke it.

**Step 4: Match the PRD’s table semantics**

In the new table component:
- render two sections: `Stables` and `Trading Assets`
- default to grouped parent rows by coin
- sort parents by status severity, then absolute MV, then alphabetical coin
- sort children by absolute MV, venue name, then type
- right-align all numeric columns
- show muted raw symbol/instrument text under the readable primary label
- make expansion immediate and controlled by the page state

**Step 5: Add page-level degraded and empty states**

Ensure the page renders:
- loading skeleton or clear loading placeholder on first fetch
- explicit empty state for no balances
- degraded/shared-snapshot banner when payload metadata indicates fallback or scope degradation
- explicit status text without leaking backend-debug schema into the default path

**Step 6: Run targeted tests**

Run the verification command above and fix any page regressions before moving on.

**Step 7: Commit**

```bash
git add fluxboard/Balances.tsx fluxboard/components/balances/BalanceStatusBadge.tsx fluxboard/components/balances/TokenMMBalancesStatusStrip.tsx fluxboard/components/balances/TokenMMBalancesSummary.tsx fluxboard/components/balances/TokenMMBalancesToolbar.tsx fluxboard/components/balances/TokenMMBalancesTable.tsx fluxboard/Balances.test.tsx
git commit -m "feat: redesign tokenmm balances holdings UI"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Cover live-update behavior and remove dead balances affordances

**Files:**
- Modify: `fluxboard/Balances.test.tsx`
- Modify: `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`
- Modify: `fluxboard/components/panels/BalancesPanel.test.tsx`
- Modify or Delete: `fluxboard/components/balances/BalanceGroup.tsx`
- Modify or Delete: `fluxboard/components/balances/BalanceRow.tsx`

**Dependencies:** `Task 3: Rebuild the Balances page around TokenMM-specific holdings UI`

**Write Scope:** `fluxboard/Balances.test.tsx`, `fluxboard/__tests__/realtime/market-balances-standard.test.tsx`, `fluxboard/components/panels/BalancesPanel.test.tsx`, `fluxboard/components/balances/BalanceGroup.tsx`, `fluxboard/components/balances/BalanceRow.tsx`

**Verification Commands:**
- `cd fluxboard && pnpm vitest run Balances.test.tsx __tests__/realtime/market-balances-standard.test.tsx components/panels/BalancesPanel.test.tsx`

**Step 1: Add failing live-update coverage**

In the realtime balances tests, add coverage proving:
- expanded rows stay expanded after snapshot replacement and invalidate-driven refresh
- row order does not thrash when non-sort fields update
- websocket updates only reorder rows when the sort key materially changes

**Step 2: Add panel wiring coverage**

Update `fluxboard/components/panels/BalancesPanel.test.tsx` so any embedded balances panel continues to render the new status strip, summary row, and holdings table correctly.

**Step 3: Remove or repurpose dead component code**

`fluxboard/components/balances/BalanceGroup.tsx` and `fluxboard/components/balances/BalanceRow.tsx` appear unused by the current page. Either:
- delete them and any now-dead supporting code, or
- repurpose them to back the new table if that truly reduces duplication

Do not leave parallel unused balances implementations in the tree after the redesign lands.

**Step 4: Run targeted tests**

Run the verification command above and make sure the redesigned page remains stable under standard realtime tests.

**Step 5: Commit**

```bash
git add fluxboard/Balances.test.tsx fluxboard/__tests__/realtime/market-balances-standard.test.tsx fluxboard/components/panels/BalancesPanel.test.tsx fluxboard/components/balances/BalanceGroup.tsx fluxboard/components/balances/BalanceRow.tsx
git commit -m "test: cover tokenmm balances live behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Add minimal backend status hints only if frontend derivation is insufficient

**Files:**
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/api.flux.test.ts`
- Modify: `fluxboard/Balances.test.tsx`

**Dependencies:** `Task 3: Rebuild the Balances page around TokenMM-specific holdings UI`

**Write Scope:** `systems/flux/flux/api/_payloads_balances.py`, `systems/flux/flux/api/app.py`, `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/Balances.test.tsx`

**Verification Commands:**
- `cd fluxboard && pnpm vitest run api.flux.test.ts Balances.test.tsx`
- `pytest systems/flux/flux/api -k balances`

**Step 1: Prove the gap before changing the backend**

Only start this task if Task 3 leaves one of these unresolved:
- no reliable way to distinguish row/group `PARTIAL` from simple `STALE`
- no reliable way to communicate `MISSING` without fabricating absent rows
- no reliable way to display an operator-facing type label from current canonical naming fields

Document the exact failing UI behavior in tests first.

**Step 2: Add the smallest explicit hint possible**

Prefer adding one or both of:
- row/group `status` plus optional `status_reason`
- explicit readable `type_label` when canonical naming is ambiguous

Do not add risk analytics or new portfolio recomputation. Keep the backend change limited to payload annotation.

**Step 3: Normalize the new hints in Fluxboard**

Update Fluxboard transport types and normalizers to pass the new hints through untouched and make the view-model prefer them over heuristics.

**Step 4: Run targeted frontend and backend tests**

Run the verification commands above. If the Python side does not already have balances payload tests, add only the smallest regression coverage needed for the new fields.

**Step 5: Commit**

```bash
git add systems/flux/flux/api/_payloads_balances.py systems/flux/flux/api/app.py fluxboard/types.ts fluxboard/api.ts fluxboard/api.flux.test.ts fluxboard/Balances.test.tsx
git commit -m "feat: add explicit tokenmm balances status hints"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

## Execution Notes

1. Keep the backend authoritative for row composition, canonical naming, risk groups, and collapse/dedupe. The redesign is a presentation-layer tightening unless Task 5 is explicitly triggered.
2. Do not reuse the existing `RiskTable` inside `Balances`. If product later wants cross-linking, add a link out to the risk page in a separate change.
3. Treat `MISSING` carefully. A row that does not exist cannot be rendered honestly without backend expected-entity metadata. Do not fabricate child rows just to satisfy the badge matrix.
4. Bump the balances filter local-storage key when replacing the current `logicalOnly`/`stableOnly` shape so stale preferences cannot poison the new toolbar.
5. Preserve standard realtime bootstrap and invalidate/recovery behavior from the current `Balances.tsx`; the redesign should not re-open transport work already done for contract v2.

## Recommended Execution Order

1. Execute Tasks 1-4 serially in one shared worktree.
2. Only execute Task 5 if Task 3 leaves a real contract gap.
3. After implementation, run the repo’s normal balances-focused frontend verification and only then decide whether broader smoke coverage is warranted.
