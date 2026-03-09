---
name: writing-plans
description: Use when you have a spec or requirements for a multi-step task, before touching code
---

# Writing Plans

## Overview

Write comprehensive implementation plans assuming the engineer has zero context for our codebase and questionable taste. Document everything they need to know: which files to touch for each task, code, testing, docs they might need to check, how to test it. Give them the whole plan as bite-sized tasks. DRY. YAGNI. TDD. Frequent commits.

Assume they are a skilled developer, but know almost nothing about our toolset or problem domain. Assume they don't know good test design very well.

**Announce at start:** "I'm using the writing-plans skill to create the implementation plan."

**Context:** This should be run in a dedicated worktree (created by brainstorming skill).

**Save plans to:** `docs/plans/YYYY-MM-DD-<feature-name>.md`

## Bite-Sized Task Granularity

**Each step is one action (2-5 minutes):**

- "Write the failing test" - step
- "Run it to make sure it fails" - step
- "Implement the minimal code to make the test pass" - step
- "Run the tests and make sure they pass" - step
- "Commit" - step

## Plan Document Header

**Every plan MUST start with this header:**

```markdown
# [Feature Name] Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** [One sentence describing what this builds]

**Architecture:** [2-3 sentences about approach]

**Tech Stack:** [Key technologies/libraries]

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: [Name] | not_started | unassigned | none | `path/to/file` | `shared` | `shared` | none | not_run | Plan created |
| Task 2: [Name] | not_started | unassigned | Task 1 | `path/to/other_file` | `shared` | `shared` | none | not_run | Plan created |

---
```

## Progress Tracker Rules

Every plan MUST include a `## Progress Tracker` section directly before the first task.

Use this exact status vocabulary:

- `not_started` - task has not begun
- `in_progress` - implementation is underway
- `in_review_spec` - waiting on or undergoing spec compliance review
- `in_review_quality` - waiting on or undergoing code quality review
- `blocked` - cannot proceed without clarification or dependency
- `completed` - task finished and verified

Tracker requirements:

- Include every task in the table, in task order
- `Owner` should name the current responsible party (`main`, `implementer`, `spec-reviewer`, `code-quality-reviewer`, or specific agent handle if available)
- `Depends On` must list prerequisite task names or `none`
- `Write Scope` must name the exact files, modules, or directories the task is allowed to change
- `Lane Branch` must name the dedicated implementer branch for parallel SDD lanes, or `shared` when serial execution stays in one branch/worktree
- `Worktree Path` must name the dedicated workspace path for parallel SDD lanes, or `shared` when serial execution stays in one branch/worktree
- `Commit / Diff` must record the latest relevant short SHA or pinned review range (`abc1234`, `abc1234..def5678`, or `none`)
- `Verification` must record the most recent command/result summary (`not_run`, `pytest ... PASS`, `pnpm test FAIL`, `review only`)
- `Notes / Last Update` should be a short factual update with verification or blocker context
- Treat the plan doc as the canonical execution record; TodoWrite is supplemental only
- Add an `Overall` row when useful for long plans
- If execution will use `executing-plans`, still include review-capable statuses so the tracker stays portable between execution modes

Update expectations:

- When implementation starts, set `Commit / Diff` to `none` until the first task commit exists
- Before dispatching a parallel implementer lane, replace `shared` with the actual lane branch and worktree path
- After each task commit, update `Commit / Diff` immediately
- Before review, pin the exact diff the reviewer should inspect in `Commit / Diff`
- After every verification command, update `Verification` immediately with pass/fail state
- On completion, the row should tell a future reader exactly which workspace produced the reviewed diff, which commit landed the task on the orchestration branch, and what command last verified it

## Task Structure

````markdown
### Task N: [Component Name]

**Files:**
- Create: `exact/path/to/file.py`
- Modify: `exact/path/to/existing.py:123-145`
- Test: `tests/exact/path/to/test.py`

**Dependencies:** `none` or `Task M: [Name]`

**Write Scope:** `exact/path/to/file.py`, `tests/exact/path/to/test.py`

**Verification Commands:**
- `pytest tests/path/test.py::test_name -v`
- `pytest tests/path/test.py -v`

**Step 1: Write the failing test**

```python
def test_specific_behavior():
    result = function(input)
    assert result == expected
```

**Step 2: Run test to verify it fails**

Run: `pytest tests/path/test.py::test_name -v`
Expected: FAIL with "function not defined"

**Step 3: Write minimal implementation**

```python
def function(input):
    return expected
```

**Step 4: Run test to verify it passes**

Run: `pytest tests/path/test.py::test_name -v`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/path/test.py src/path/file.py
git commit -m "feat: add specific feature"
```
````

After each task section, add a short tracker reminder:

```markdown
**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
```

## Remember

- Exact file paths always
- Complete code in plan (not "add validation")
- Exact commands with expected output
- Reference relevant skills with @ syntax
- DRY, YAGNI, TDD, frequent commits
- Build the tracker table before Task 1 so execution starts with a real source of truth
- Make task names in the tracker exactly match task headings
- Design tasks so independent work has disjoint write scope if parallel execution is desired
- If parallel SDD is likely, pre-plan lane branch names and worktree paths so the controller can allocate them without improvising

## Execution Handoff

After saving the plan, offer execution choice:

**"Plan complete and saved to `docs/plans/<filename>.md`. Two execution options:**

**1. Subagent-Driven (this session)** - I orchestrate fresh subagent lanes, use spec-first review, and parallelize only when task ownership is disjoint

**2. Separate Session (checkpointed)** - Open new session with executing-plans, batch execution with human checkpoints

**Which approach?"**

**If Subagent-Driven chosen:**

- **REQUIRED SUB-SKILL:** Use superpowers:subagent-driven-development
- Stay in this session
- Fresh subagent lanes + review-driven execution

**If Separate Session chosen:**

- Guide them to open new session in worktree
- **REQUIRED SUB-SKILL:** New session uses superpowers:executing-plans
