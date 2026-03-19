# Nautilus Upstream Upgrade Workflow Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Add a repeatable upstream-sync workflow that prepares a reviewable upgrade branch, documents the process in-repo, and performs the first sync preparation against the current upstream release.

**Architecture:** Keep the workflow simple and local-first. Use a canonical shell entrypoint under `tooling/dev/` to create an upstream mirror branch and an upgrade branch, keep a compatibility wrapper under `scripts/`, and capture each run in a docs evidence file so future agents can re-run the process without chat history.

**Tech Stack:** Bash, Git, repository docs under `docs/`, targeted shell verification.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `tooling/dev/**`, `scripts/sync_upstream.sh`, `.gitignore`, `docs/plans/2026-03-19-nautilus-upstream-upgrade*`, `docs/runbooks/**`, `docs/reviews/**` | `codex/nautilus-upstream-upgrade-20260319` | `/home/ubuntu/nautilus_trader/.worktrees/nautilus-upstream-upgrade-20260319` | `30058a9b20..719a09e01d` | `bash tooling/dev/test-prepare-nautilus-upgrade.sh PASS; bash -n tooling/dev/prepare-nautilus-upgrade.sh PASS; bash -n scripts/sync_upstream.sh PASS; git diff --check PASS` | Tooling, docs, and first branch-prep evidence implemented; final tracker closeout pending |
| Task 1: Add canonical upstream sync script | completed | main | none | `.gitignore`, `tooling/dev/prepare-nautilus-upgrade.sh`, `scripts/sync_upstream.sh`, `tooling/dev/test-prepare-nautilus-upgrade.sh` | `codex/nautilus-upstream-upgrade-20260319` | `/home/ubuntu/nautilus_trader/.worktrees/nautilus-upstream-upgrade-20260319` | `719a09e01d` | `bash tooling/dev/test-prepare-nautilus-upgrade.sh PASS; bash -n tooling/dev/prepare-nautilus-upgrade.sh PASS; bash -n scripts/sync_upstream.sh PASS` | Spec review and quality review approved; canonical upgrade-prep script and ignore exceptions are tracked |
| Task 2: Add runbook and evidence template | completed | main | Task 1: Add canonical upstream sync script | `docs/runbooks/nautilus-upstream-upgrade.md`, `docs/reviews/nautilus-upstream-upgrade-template.md`, `docs/repo/workflows.md` | `lanes/task-2-nautilus-upgrade-docs` | `/home/ubuntu/nautilus_trader/.worktrees/task-2-nautilus-upgrade-docs` | `30058a9b20..7e6cc683b1` | `git diff --check PASS` | Docs lane passed spec review and quality review, then was integrated into controller branch |
| Task 3: Prepare the first upgrade branch and report | completed | main | Task 1: Add canonical upstream sync script | `git branches`, `docs/reviews/20260319-nautilus-upstream-upgrade-v1.224.0.md` | `codex/nautilus-upstream-upgrade-20260319` | `/home/ubuntu/nautilus_trader/.worktrees/nautilus-upstream-upgrade-20260319` | none | `prepare-nautilus-upgrade.sh PASS; branch list PASS` | Created `upstream-sync/v1.224.0` and `upgrade/nautilus-20260319-v1.224.0`; evidence note written; upgrade branch is identical to `origin/main` because fork point is already `v1.224.0` |
| Task 4: Verify and close out documentation | in_progress | main | Task 2: Add runbook and evidence template, Task 3: Prepare the first upgrade branch and report | `docs/plans/2026-03-19-nautilus-upstream-upgrade.md` | `codex/nautilus-upstream-upgrade-20260319` | `/home/ubuntu/nautilus_trader/.worktrees/nautilus-upstream-upgrade-20260319` | none | `bash tooling/dev/test-prepare-nautilus-upgrade.sh PASS; bash -n tooling/dev/prepare-nautilus-upgrade.sh PASS; bash -n scripts/sync_upstream.sh PASS; git diff --check PASS` | Final tracker closeout and commit metadata update pending |

---

### Task 1: Add canonical upstream sync script

**Files:**
- Create: `tooling/dev/prepare-nautilus-upgrade.sh`
- Modify: `scripts/sync_upstream.sh`

**Dependencies:** `none`

**Write Scope:** `tooling/dev/prepare-nautilus-upgrade.sh`, `scripts/sync_upstream.sh`

**Verification Commands:**
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
- `bash -n scripts/sync_upstream.sh`

**Step 1: Write the script interface first**

Define the environment variables, branch naming rules, and evidence-file output that the script will support before filling in implementation details.

**Step 2: Implement safe branch preparation**

Create logic for:
- bootstrapping `upstream`
- fetching upstream tags and branches
- choosing the target release tag
- creating `upstream-sync/<tag>`
- creating `upgrade/nautilus-<date>-<tag>`
- merging upstream into the upgrade branch without rewriting `main`

**Step 3: Preserve compatibility**

Update `scripts/sync_upstream.sh` to delegate to the canonical `tooling/dev/` script so existing callers do not break.

**Step 4: Run syntax checks**

Run:
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
- `bash -n scripts/sync_upstream.sh`

Expected: both commands exit `0`.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Add runbook and evidence template

**Files:**
- Create: `docs/runbooks/nautilus-upstream-upgrade.md`
- Create: `docs/reviews/nautilus-upstream-upgrade-template.md`
- Modify: `docs/repo/workflows.md`

**Dependencies:** `Task 1: Add canonical upstream sync script`

**Write Scope:** `docs/runbooks/nautilus-upstream-upgrade.md`, `docs/reviews/nautilus-upstream-upgrade-template.md`, `docs/repo/workflows.md`

**Verification Commands:**
- `git diff --check`

**Step 1: Write the operator/agent runbook**

Document the bi-weekly process, including inputs, branch naming, target release selection, cherry-pick policy, verification expectations, and stop conditions for human review.

**Step 2: Add the evidence template**

Provide a ready-to-copy review document that records:
- upstream release/tag and date
- selected post-release PRs
- conflicts/manual resolutions
- verification results
- recommended next review steps

**Step 3: Register the workflow**

Add a short section to `docs/repo/workflows.md` pointing tooling/ops contributors at the canonical upstream upgrade path.

**Step 4: Run whitespace/conflict checks**

Run: `git diff --check`
Expected: clean output.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Prepare the first upgrade branch and report

**Files:**
- Create: `docs/reviews/2026-03-19-nautilus-upstream-upgrade-v1.224.0.md`

**Dependencies:** `Task 1: Add canonical upstream sync script`

**Write Scope:** `git branches`, `docs/reviews/2026-03-19-nautilus-upstream-upgrade-v1.224.0.md`

**Verification Commands:**
- `tooling/dev/prepare-nautilus-upgrade.sh`
- `git branch --list 'upstream-sync/*' 'upgrade/*'`

**Step 1: Run the workflow against the current upstream target**

Execute the new script with the current verified upstream release tag `v1.224.0` unless the script resolves the same tag automatically.

**Step 2: Review branch creation output**

Confirm the expected `upstream-sync/*` and `upgrade/*` branches were created and record their names.

**Step 3: Write the first evidence report**

Document the target release, branch names, any merge conflicts, relevant recent PRs for future cherry-picks, and the verification/results from the first run.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify and close out documentation

**Files:**
- Modify: `docs/plans/2026-03-19-nautilus-upstream-upgrade.md`

**Dependencies:** `Task 2: Add runbook and evidence template`, `Task 3: Prepare the first upgrade branch and report`

**Write Scope:** `docs/plans/2026-03-19-nautilus-upstream-upgrade.md`

**Verification Commands:**
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
- `bash -n scripts/sync_upstream.sh`
- `git diff --check`

**Step 1: Update the progress tracker with actual results**

Record the worktree path, branch names, verification commands, and report status.

**Step 2: Re-run final targeted verification**

Run:
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
- `bash -n scripts/sync_upstream.sh`
- `git diff --check`

Expected: clean syntax and clean diff formatting.

**Step 3: Prepare handoff summary**

Ensure the plan and evidence docs clearly tell the next agent or reviewer what branch to inspect and what remains human-reviewed.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
