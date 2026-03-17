# Equities Signal Family And Qty Alias Prune Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Add a separate Signal family dropdown for the equities Fluxboard view and remove legacy `global_qty` / `local_qty` alias fields from the equities MakerV4 signal stack while keeping canonical `*_base` inventory fields.

**Architecture:** Keep the UI change localized to [`fluxboard/components/domain/signal/SignalTable.tsx`](/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/fluxboard/components/domain/signal/SignalTable.tsx) by mirroring the Params family-control pattern: strategy text search stays separate and family selection becomes an explicit dropdown that can lock itself when only one family is present. Keep the backend cleanup scoped to the equities MakerV4 path: stop emitting duplicate alias field names on strategy state and API signal payloads, preserve canonical `local_qty_base` / `global_qty_base` plus completeness metadata, and avoid changing MakerV3/tokenmm inventory behavior.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, Python, pytest, Flux API signal payload builders, Markdown deploy docs.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | Execution started in current session; beginning Task 1 |
| Task 1: Add failing Signal family-control tests | in_progress | implementer | Worker starting failing Signal family-control tests for equities Signal |
| Task 2: Implement Signal family dropdown behavior | not_started | unassigned | Make family selection explicit on Signal, including equities |
| Task 3: Add failing MakerV4 qty-alias contract tests | not_started | unassigned | Lock expected omission of `global_qty` / `local_qty` on equities MakerV4 |
| Task 4: Implement equities MakerV4 qty-alias pruning | not_started | unassigned | Remove legacy alias fields while preserving canonical base fields |
| Task 5: Update docs and run verification | not_started | unassigned | Sync equities docs and run targeted frontend/backend checks |

---

### Task 1: Add failing Signal family-control tests

**Files:**
- Create: `fluxboard/tests/signal/SignalFamilyFilter.test.tsx`
- Modify: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`

**Step 1: Write the failing test**

Add focused tests that render [`SignalTable.tsx`](/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/fluxboard/components/domain/signal/SignalTable.tsx) on `/equities/signal` and assert:

```tsx
expect(screen.getByLabelText('Signal family')).toBeInTheDocument();
expect(screen.getByLabelText('Signal family')).toHaveValue('maker_v4');
expect(screen.getByLabelText('Signal family')).toBeDisabled();
expect(screen.getByPlaceholderText(/Strategy ID/i)).toBeInTheDocument();
```

Also add one regression assertion that the dedicated MakerV4 table still renders after the family control becomes visible.

**Step 2: Run test to verify it fails**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/SignalFamilyFilter.test.tsx tests/signal/MakerV4SignalTable.test.tsx`
Expected: FAIL because maker-suite Signal currently hides the family dropdown.

**Step 3: Write minimal implementation**

No implementation in this task.

**Step 4: Run test to verify it still fails for the expected reason**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/SignalFamilyFilter.test.tsx`
Expected: FAIL with missing `Signal family` control.

**Step 5: Commit**

```bash
git add fluxboard/tests/signal/SignalFamilyFilter.test.tsx fluxboard/tests/signal/MakerV4SignalTable.test.tsx
git commit -m "test: cover signal family control behavior"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Implement Signal family dropdown behavior

**Files:**
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`

**Step 1: Implement Params-style family-control state**

Mirror the existing Params flow from [`fluxboard/Params.tsx`](/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/fluxboard/Params.tsx):

- derive visible family counts from profile-compatible rows
- compute available families and `lockedSingleFamily`
- keep strategy text filter separate from family selection
- expose `aria-label="Signal family"`
- default equities to `maker_v4`, but still show the control

The intended control shape is:

```tsx
<select
  value={effectiveFamilyScope}
  onChange={(event) => setFamilyScope(event.target.value as SignalFamilyScope)}
  disabled={Boolean(lockedSingleFamily)}
  aria-label="Signal family"
>
```

**Step 2: Keep table routing behavior intact**

Ensure the current dedicated MakerV4 route still wins when the effective family scope is `maker_v4` or the filtered dataset is entirely MakerV4. Do not regress `showQuoted` visibility or generic-table fallbacks for non-equities profiles.

**Step 3: Run frontend tests**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/SignalFamilyFilter.test.tsx tests/signal/MakerV4SignalTable.test.tsx`
Expected: PASS

**Step 4: Run one broader Signal regression slice**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/SignalTable.audit.test.tsx`
Expected: PASS

**Step 5: Commit**

```bash
git add fluxboard/components/domain/signal/SignalTable.tsx fluxboard/tests/signal/SignalFamilyFilter.test.tsx fluxboard/tests/signal/MakerV4SignalTable.test.tsx
git commit -m "feat: add explicit signal family control"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Add failing MakerV4 qty-alias contract tests

**Files:**
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write failing strategy-state assertions**

Update MakerV4 inventory-contract tests to assert equities MakerV4 state keeps:

```python
assert payload["local_qty_base"] == "12"
assert payload["global_qty_base"] == "37"
assert "local_qty" not in payload
assert "global_qty" not in payload
```

Do not remove assertions for completeness flags or aggregation metadata unless the implementation intentionally changes them.

**Step 2: Write failing signal-payload assertions**

Add/adjust payload tests so the equities MakerV4 signal response keeps canonical base fields but omits duplicate aliases:

```python
assert row["local_qty_base"] == 12.0
assert row["global_qty_base"] == 37.0
assert "local_qty" not in row
assert "global_qty" not in row
```

Also add one unit-level assertion in [`test_payloads.py`](/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/tests/unit_tests/flux/api/test_payloads.py) so the payload builder does not silently reintroduce those alias keys for the equities MakerV4 branch.

**Step 3: Run tests to verify they fail**

Run: `pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k "global_qty or local_qty or makerv4" -q`
Expected: FAIL because the current MakerV4 stack still emits the legacy alias fields.

**Step 4: Document the branch assumption in the test names/comments**

Make it explicit that this change is scoped to the equities MakerV4 branch and preserves canonical `*_base` quantities.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "test: lock equities makerv4 qty alias contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Implement equities MakerV4 qty-alias pruning

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`

**Step 1: Stop emitting legacy alias keys from MakerV4 state**

Update [`systems/flux/flux/strategies/makerv4/strategy.py`](/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/systems/flux/flux/strategies/makerv4/strategy.py) so the state snapshot keeps canonical `local_qty_base` / `global_qty_base` but no longer duplicates them into `local_qty` / `global_qty` for the equities MakerV4 branch.

Do not change:

- portfolio inventory component publishing
- `local_position_qty_*`
- `global_qty_base_complete` / `global_qty_complete`
- `aggregation_mode`

**Step 2: Prune alias keys at API payload assembly**

Update [`systems/flux/flux/api/_payloads_signals.py`](/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/systems/flux/flux/api/_payloads_signals.py) so equities MakerV4 signal rows and inventory-skew adjustments preserve:

```python
"local_qty_base"
"global_qty_base"
"global_qty_base_complete"
"global_qty_complete"
```

but do not emit:

```python
"local_qty"
"global_qty"
```

Prefer a narrow conditional keyed by strategy metadata (`strategy_family == "maker_v4"` and equities grouping) instead of changing MakerV3/tokenmm payload behavior globally.

**Step 3: Run targeted backend tests**

Run: `pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k "global_qty or local_qty or makerv4" -q`
Expected: PASS

**Step 4: Run a broader MakerV4 signal slice**

Run: `pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_equities_profile_contract.py -q`
Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv4/strategy.py systems/flux/flux/api/_payloads_signals.py tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "refactor: drop equities makerv4 qty aliases"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Update docs and run verification

**Files:**
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `docs/plans/2026-03-16-equities-signal-family-and-qty-prune.md`

**Step 1: Update equities docs**

Replace direct references to `local_qty` / `global_qty` in the equities deploy docs with canonical `local_qty_base` / `global_qty_base` wording, and call out that the short alias fields were removed from the equities MakerV4 signal contract.

**Step 2: Run frontend verification**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/SignalFamilyFilter.test.tsx tests/signal/MakerV4SignalTable.test.tsx tests/signal/SignalTable.audit.test.tsx`
Expected: PASS

**Step 3: Run backend verification**

Run: `pytest tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_equities_profile_contract.py -q`
Expected: PASS

**Step 4: Run diff hygiene**

Run: `git diff --check`
Expected: PASS

**Step 5: Commit**

```bash
git add deploy/equities/README.md deploy/equities/strategies/README.md docs/plans/2026-03-16-equities-signal-family-and-qty-prune.md
git commit -m "docs: sync equities signal inventory naming"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Notes

- Assumption for this plan: “remove `global_qty` / `local_qty`” means removing the legacy alias fields on the equities MakerV4 path, not removing canonical `global_qty_base` / `local_qty_base`.
- Non-goal: changing MakerV3/tokenmm inventory skew payload semantics or the shared portfolio inventory persistence schema.
- Non-goal: replacing the dedicated equities MakerV4 Signal table layout; this plan only changes control visibility plus qty alias cleanup.

## Execution Handoff

Plan complete and saved to `docs/plans/2026-03-16-equities-signal-family-and-qty-prune.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
