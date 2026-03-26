# Production-Grade Deploy Lanes And Monorepo Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Establish production-grade deploy lanes on the shared host, clean up canonical repo/worktree ownership, and sequence monorepo naming cleanup after live deploy hardening.

**Architecture:** Treat deploy immutability and lane-specific namespaces as the core architecture. Standardize all live stacks on pinned release roots plus lane-aware env generation, then formalize host governance and only afterward perform repo/product naming cleanup toward `flux` while keeping `nautilus_trader` as the engine namespace.

**Tech Stack:** Bash deploy helpers, `systemd`, Pulse API/UI, Flask, Python packaging, pytest, repo governance docs, host env files under `/etc/flux`.

**Context Docs:**
- Design: `docs/plans/2026-03-26-production-grade-deploy-lanes-and-monorepo-design.md`
- PRD: `none`
- Relevant specs/runbooks: `README.md`, `docs/repo/structure.md`, `deploy/tokenmm/README.md`, `deploy/equities/README.md`, `systems/flux/docs/api.md`, `docs/runbooks/ec2-host-baseline.md`, `docs/runbooks/production-host-disk-recovery.md`

**Decision Summary:**
- Deploy immutability and lane-specific namespaces are the primary architectural boundary; naming cleanup is second-phase work.
- `dev` stays mutable, while `pilot` and `prod` must run only from pinned release roots.
- One Pulse surface will manage multiple groups; `equities-pilot` is the first formal pilot lane.
- `nautilus_trader` remains the engine/runtime namespace for now even if the repo/product later becomes `flux`.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Write and land the deploy-lane governance contract | in_progress | main | none | `docs/runbooks/deploy-lanes.md`, `docs/runbooks/equities-pilot-rollout.md`, `AGENTS.md`, `docs/repo/workflows.md`, `ops/README.md` | `codex/prod-lanes-exec-20260326` | `/home/ubuntu/nautilus_trader/.worktrees/prod-lanes-exec-20260326` | none | not_run | Started Task 1 in isolated worktree; drafting lane governance docs and agent contract |
| Task 2: Add shared release-root helpers and immutable-root tests | not_started | unassigned | Task 1 | `ops/scripts/deploy/shared_strategy_stack.sh`, `ops/scripts/deploy/create_release_root.sh`, `tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`, `tests/unit_tests/ops/deploy/test_create_release_root.py` | shared | shared | none | not_run | Plan created |
| Task 3: Harden equities install flow for immutable roots and lane namespaces | not_started | unassigned | Task 2 | `ops/scripts/deploy/install_equities_systemd.sh`, `deploy/equities/README.md`, `deploy/equities/systemd/common.env.example`, `tests/unit_tests/ops/deploy/test_install_equities_systemd.py` | shared | shared | none | not_run | Plan created |
| Task 4: Canonicalize the shared-host dev repo and worktree layout | not_started | unassigned | Task 1 | `/home/ubuntu/nautilus_trader`, `/home/ubuntu/nautilus-trader-dev`, `/home/ubuntu/nautilus_trader_prod`, `/home/ubuntu/.config/superpowers/worktrees/nautilus_trader`, `/home/ubuntu/.worktrees/nautilus_trader`, `docs/runbooks/deploy-lanes.md` | shared | shared | none | not_run | Plan created |
| Task 5: Repoint current prod lanes to immutable release roots | not_started | unassigned | Task 2, Task 3, Task 4 | `/etc/flux/tokenmm*.env`, `/etc/flux/equities*.env`, `~/releases/prod/tokenmm`, `~/releases/prod/equities`, `deploy/tokenmm/README.md`, `deploy/equities/README.md` | shared | shared | none | not_run | Plan created |
| Task 6: Add the first formal `equities-pilot` lane | not_started | unassigned | Task 3, Task 5 | `ops/scripts/deploy/install_equities_systemd.sh`, `deploy/equities/README.md`, `docs/runbooks/equities-pilot-rollout.md`, `/etc/flux/equities-pilot*.env`, `~/releases/pilot/equities`, `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`, `tests/unit_tests/flux/pulse/test_api.py` | shared | shared | none | not_run | Plan created |
| Task 7: Standardize LP and TG-bot installers on the same root policy | not_started | unassigned | Task 2 | `ops/scripts/deploy/install_lp_systemd.sh`, `ops/scripts/deploy/install_tg_bots_systemd.sh`, `deploy/lp/README.md`, `deploy/lp/systemd/common.env.example`, `deploy/tg_bots/systemd/common.env.example`, `tests/unit_tests/ops/deploy/test_install_lp_systemd.py`, `tests/unit_tests/ops/deploy/test_install_tg_bots_systemd.py` | shared | shared | none | not_run | Plan created |
| Task 8: Finalize monorepo naming and structure follow-ups | not_started | unassigned | Task 1, Task 5, Task 6, Task 7 | `README.md`, `docs/repo/structure.md`, `docs/repo/standards.md`, `docs/repo/workflows.md`, `ops/README.md`, `apps/README.md`, `engine/README.md` | shared | shared | none | not_run | Plan created |

---

### Task 1: Write and land the deploy-lane governance contract

**Files:**
- Create: `docs/runbooks/deploy-lanes.md`
- Create: `docs/runbooks/equities-pilot-rollout.md`
- Modify: `AGENTS.md`
- Modify: `docs/repo/workflows.md`
- Modify: `ops/README.md`

**Dependencies:** `none`

**Write Scope:** `docs/runbooks/deploy-lanes.md`, `docs/runbooks/equities-pilot-rollout.md`, `AGENTS.md`, `docs/repo/workflows.md`, `ops/README.md`

**Verification Commands:**
- `tooling/ci/check-repo-structure.sh`
- `rg -n "deploy to pilot|bounce pilot|promote to prod|worktree" AGENTS.md docs/runbooks/deploy-lanes.md docs/runbooks/equities-pilot-rollout.md docs/repo/workflows.md ops/README.md`

**Step 1: Write the failing governance assertions**
- Add the new docs and edits so they state explicit forbidden behavior:
  - no live services from `~/nautilus_trader`
  - no live services from `.worktrees/*`
  - exact meaning of `deploy to pilot`, `bounce pilot`, and `promote to prod`
- Update `AGENTS.md` so agents cannot improvise the live deploy contract.

**Step 2: Run the repo-structure and content checks**
- Run the commands above.
- Expected before full implementation: repo-structure stays green, but content review may still reveal legacy wording or missing lane terminology.

**Step 3: Implement the minimal governance contract**
- Land the lane model as the source of truth for operators and agents.
- Keep the docs concrete and host-specific enough to be actionable on this box.

**Step 4: Re-run checks**
- Run the commands above.
- Expected after implementation: docs reference the new lane model consistently and no repo-structure regressions are introduced.

**Step 5: Commit**
- `git add docs/runbooks/deploy-lanes.md docs/runbooks/equities-pilot-rollout.md AGENTS.md docs/repo/workflows.md ops/README.md`
- `git commit -m "docs: define deploy lane governance"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Add shared release-root helpers and immutable-root tests

**Files:**
- Modify: `ops/scripts/deploy/shared_strategy_stack.sh`
- Create: `ops/scripts/deploy/create_release_root.sh`
- Modify: `tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`
- Create: `tests/unit_tests/ops/deploy/test_create_release_root.py`

**Dependencies:** `Task 1: Write and land the deploy-lane governance contract`

**Write Scope:** `ops/scripts/deploy/shared_strategy_stack.sh`, `ops/scripts/deploy/create_release_root.sh`, `tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`, `tests/unit_tests/ops/deploy/test_create_release_root.py`

**Verification Commands:**
- `bash -n ops/scripts/deploy/shared_strategy_stack.sh ops/scripts/deploy/create_release_root.sh`
- `pytest -q tests/unit_tests/ops/deploy/test_shared_strategy_stack.py tests/unit_tests/ops/deploy/test_create_release_root.py`

**Step 1: Write the failing tests**
- Extend shared-stack tests to cover:
  - immutable-root validation
  - worktree rejection
  - lane-aware root naming
  - required release metadata or manifest checks
- Add a new helper test for release-root creation behavior.

**Step 2: Run the focused tests to verify the gap**
- Run the commands above.
- Expected before implementation: the new helper/validation assertions fail because the shared helpers do not yet model release roots formally.

**Step 3: Implement the shared release-root layer**
- Add common helper functions for:
  - validating lane and stack identifiers
  - rejecting mutable roots
  - writing release metadata
  - resolving a `current` symlink target safely
- Keep this logic shared so every stack can consume one contract.

**Step 4: Re-run the focused tests**
- Run the commands above.
- Expected after implementation: helper behavior is covered by unit tests and the shell scripts parse cleanly.

**Step 5: Commit**
- `git add ops/scripts/deploy/shared_strategy_stack.sh ops/scripts/deploy/create_release_root.sh tests/unit_tests/ops/deploy/test_shared_strategy_stack.py tests/unit_tests/ops/deploy/test_create_release_root.py`
- `git commit -m "feat(deploy): add immutable release-root helpers"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Harden equities install flow for immutable roots and lane namespaces

**Files:**
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/systemd/common.env.example`
- Create: `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`

**Dependencies:** `Task 2: Add shared release-root helpers and immutable-root tests`

**Write Scope:** `ops/scripts/deploy/install_equities_systemd.sh`, `deploy/equities/README.md`, `deploy/equities/systemd/common.env.example`, `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`

**Verification Commands:**
- `bash -n ops/scripts/deploy/install_equities_systemd.sh`
- `pytest -q tests/unit_tests/ops/deploy/test_install_equities_systemd.py tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`

**Step 1: Write the failing tests**
- Add installer coverage for:
  - rejecting worktree roots
  - consuming explicit release roots
  - rendering lane-aware service IDs and env paths
  - preserving a stable prod root on reruns

**Step 2: Run tests to document current drift**
- Run the commands above.
- Expected before implementation: new tests fail because the installer still writes the current checkout directly and does not model pilot namespaces.

**Step 3: Implement the minimal installer hardening**
- Make equities use the same stable-root policy TokenMM already enforces.
- Add lane-aware service naming so `prod` and `pilot` can coexist cleanly.
- Update README/common env guidance to stop normalizing `~/nautilus-trader` as a live root.

**Step 4: Re-run tests**
- Run the commands above.
- Expected after implementation: equities installer becomes compatible with immutable prod/pilot release roots.

**Step 5: Commit**
- `git add ops/scripts/deploy/install_equities_systemd.sh deploy/equities/README.md deploy/equities/systemd/common.env.example tests/unit_tests/ops/deploy/test_install_equities_systemd.py`
- `git commit -m "feat(equities): harden systemd install for release roots"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Canonicalize the shared-host dev repo and worktree layout

**Files:**
- Modify as needed: `docs/runbooks/deploy-lanes.md`
- Host paths to inspect/archive/remove:
  - `/home/ubuntu/nautilus_trader`
  - `/home/ubuntu/nautilus-trader-dev`
  - `/home/ubuntu/nautilus_trader_prod`
  - `/home/ubuntu/.config/superpowers/worktrees/nautilus_trader`
  - `/home/ubuntu/.worktrees/nautilus_trader`

**Dependencies:** `Task 1: Write and land the deploy-lane governance contract`

**Write Scope:** `docs/runbooks/deploy-lanes.md`, `/home/ubuntu/nautilus-trader-dev`, `/home/ubuntu/nautilus_trader_prod`, `/home/ubuntu/.config/superpowers/worktrees/nautilus_trader`, `/home/ubuntu/.worktrees/nautilus_trader`

**Verification Commands:**
- `git -C /home/ubuntu/nautilus_trader worktree list`
- `find /home/ubuntu -maxdepth 1 -mindepth 1 \\( -type d -o -type l \\) | rg '/nautilus[-_].*|/nautilus-trader$|/nautilus_trader$' | sort`
- `du -sh /home/ubuntu/nautilus_trader /home/ubuntu/nautilus-trader-dev /home/ubuntu/nautilus_trader_prod 2>/dev/null`

**Step 1: Record the current host state**
- Capture which checkout becomes the canonical dev repo.
- Capture which worktrees or clones still contain uncommitted work before deleting or archiving anything.

**Step 2: Verify the current sprawl**
- Run the commands above.
- Expected before cleanup: multiple top-level clones and multiple worktree roots still exist.

**Step 3: Implement the host cleanup**
- Keep one canonical dev repo.
- Standardize one worktree location.
- Archive or remove extra clones only after preserving any uncommitted work.
- Update the governance doc if the final canonical path differs from the current assumption.

**Step 4: Re-run host inventory checks**
- Run the commands above.
- Expected after cleanup: one canonical repo and one canonical worktree root remain.

**Step 5: Commit**
- `git add docs/runbooks/deploy-lanes.md`
- `git commit -m "docs: record canonical dev repo and worktree layout"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Repoint current prod lanes to immutable release roots

**Files:**
- Host envs: `/etc/flux/tokenmm*.env`, `/etc/flux/equities*.env`
- Host release roots:
  - `~/releases/prod/tokenmm`
  - `~/releases/prod/equities`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/equities/README.md`

**Dependencies:** `Task 2: Add shared release-root helpers and immutable-root tests`, `Task 3: Harden equities install flow for immutable roots and lane namespaces`, `Task 4: Canonicalize the shared-host dev repo and worktree layout`

**Write Scope:** `/etc/flux/tokenmm*.env`, `/etc/flux/equities*.env`, `~/releases/prod/tokenmm`, `~/releases/prod/equities`, `deploy/tokenmm/README.md`, `deploy/equities/README.md`

**Verification Commands:**
- `sudo rg -n '^(WORKDIR|PYTHONPATH|CMD)=' /etc/flux/tokenmm*.env /etc/flux/equities*.env`
- `systemctl status flux@tokenmm-api.service flux@equities-api.service --no-pager`
- `curl -fsS http://127.0.0.1:5022/api/pulse/jobs | jq '.groups // .data // .'`

**Step 1: Prepare the failing host verification**
- Record the current mutable roots and worktree references in `/etc/flux`.
- Confirm the exact prod service names that must be repointed.

**Step 2: Run the host checks**
- Run the commands above.
- Expected before implementation: mutable home-checkout or worktree paths still appear in the rendered envs.

**Step 3: Materialize prod release roots and repoint**
- Create pinned prod release roots for equities and TokenMM.
- Rewrite prod env files so they point only at prod `current` symlinks.
- Update the deploy READMEs so the documented prod contract matches the new host reality.

**Step 4: Re-run host checks**
- Run the commands above.
- Expected after implementation: no prod env points at the canonical dev repo or any worktree.

**Step 5: Commit**
- `git add deploy/tokenmm/README.md deploy/equities/README.md`
- `git commit -m "docs: align prod deploy docs with immutable release roots"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Add the first formal `equities-pilot` lane

**Files:**
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `deploy/equities/README.md`
- Modify: `docs/runbooks/equities-pilot-rollout.md`
- Host envs: `/etc/flux/equities-pilot*.env`
- Host release roots: `~/releases/pilot/equities`
- Modify: `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`
- Modify if needed: `tests/unit_tests/flux/pulse/test_api.py`

**Dependencies:** `Task 3: Harden equities install flow for immutable roots and lane namespaces`, `Task 5: Repoint current prod lanes to immutable release roots`

**Write Scope:** `ops/scripts/deploy/install_equities_systemd.sh`, `deploy/equities/README.md`, `docs/runbooks/equities-pilot-rollout.md`, `/etc/flux/equities-pilot*.env`, `~/releases/pilot/equities`, `tests/unit_tests/ops/deploy/test_install_equities_systemd.py`, `tests/unit_tests/flux/pulse/test_api.py`

**Verification Commands:**
- `bash -n ops/scripts/deploy/install_equities_systemd.sh`
- `pytest -q tests/unit_tests/ops/deploy/test_install_equities_systemd.py tests/unit_tests/flux/pulse/test_api.py`
- `systemctl status 'flux@equities-pilot*' --no-pager`
- `curl -fsS http://127.0.0.1:5022/api/pulse/jobs | jq '.groups // .data // .'`

**Step 1: Write the failing tests**
- Add coverage for pilot service IDs, group labels, and lane-aware env rendering.
- Add or extend Pulse tests only if the pilot grouping exposes assumptions in the API layer.

**Step 2: Run tests and host checks**
- Run the commands above.
- Expected before implementation: no formal pilot namespace exists yet.

**Step 3: Implement the pilot lane**
- Add `equities-pilot-*` service IDs and target wiring.
- Create a pinned pilot release root.
- Ensure pilot uses distinct ports and any distinct state paths needed to avoid collision with prod.
- Finish the pilot rollout doc so operators and agents can use one exact contract.

**Step 4: Re-run tests and host checks**
- Run the commands above.
- Expected after implementation: pilot appears as a first-class Pulse group and can be bounced independently of prod.

**Step 5: Commit**
- `git add ops/scripts/deploy/install_equities_systemd.sh deploy/equities/README.md docs/runbooks/equities-pilot-rollout.md tests/unit_tests/ops/deploy/test_install_equities_systemd.py tests/unit_tests/flux/pulse/test_api.py`
- `git commit -m "feat(equities): add pilot deploy lane"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Standardize LP and TG-bot installers on the same root policy

**Files:**
- Modify: `ops/scripts/deploy/install_lp_systemd.sh`
- Modify: `ops/scripts/deploy/install_tg_bots_systemd.sh`
- Modify: `deploy/lp/README.md`
- Modify: `deploy/lp/systemd/common.env.example`
- Modify: `deploy/tg_bots/systemd/common.env.example`
- Create: `tests/unit_tests/ops/deploy/test_install_lp_systemd.py`
- Modify: `tests/unit_tests/ops/deploy/test_install_tg_bots_systemd.py`

**Dependencies:** `Task 2: Add shared release-root helpers and immutable-root tests`

**Write Scope:** `ops/scripts/deploy/install_lp_systemd.sh`, `ops/scripts/deploy/install_tg_bots_systemd.sh`, `deploy/lp/README.md`, `deploy/lp/systemd/common.env.example`, `deploy/tg_bots/systemd/common.env.example`, `tests/unit_tests/ops/deploy/test_install_lp_systemd.py`, `tests/unit_tests/ops/deploy/test_install_tg_bots_systemd.py`

**Verification Commands:**
- `bash -n ops/scripts/deploy/install_lp_systemd.sh ops/scripts/deploy/install_tg_bots_systemd.sh`
- `pytest -q tests/unit_tests/ops/deploy/test_install_lp_systemd.py tests/unit_tests/ops/deploy/test_install_tg_bots_systemd.py tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`

**Step 1: Write the failing tests**
- Add LP installer coverage for immutable-root behavior.
- Extend TG-bot tests so the env rendering contract includes stable-root expectations rather than defaulting to the mutable home checkout.

**Step 2: Run tests to capture current inconsistency**
- Run the commands above.
- Expected before implementation: LP/TG-bot installers still normalize mutable checkout paths.

**Step 3: Implement the shared-root policy**
- Make LP and TG bots consume the same helper contract as equities and TokenMM.
- Update common env examples so they no longer normalize `~/nautilus-trader` as the live root.

**Step 4: Re-run tests**
- Run the commands above.
- Expected after implementation: all stack installers share one deploy-root policy.

**Step 5: Commit**
- `git add ops/scripts/deploy/install_lp_systemd.sh ops/scripts/deploy/install_tg_bots_systemd.sh deploy/lp/README.md deploy/lp/systemd/common.env.example deploy/tg_bots/systemd/common.env.example tests/unit_tests/ops/deploy/test_install_lp_systemd.py tests/unit_tests/ops/deploy/test_install_tg_bots_systemd.py`
- `git commit -m "feat(deploy): standardize immutable roots across installers"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 8: Finalize monorepo naming and structure follow-ups

**Files:**
- Modify: `README.md`
- Modify: `docs/repo/structure.md`
- Modify: `docs/repo/standards.md`
- Modify: `docs/repo/workflows.md`
- Modify: `ops/README.md`
- Modify: `apps/README.md`
- Modify: `engine/README.md`

**Dependencies:** `Task 1: Write and land the deploy-lane governance contract`, `Task 5: Repoint current prod lanes to immutable release roots`, `Task 6: Add the first formal 'equities-pilot' lane`, `Task 7: Standardize LP and TG-bot installers on the same root policy`

**Write Scope:** `README.md`, `docs/repo/structure.md`, `docs/repo/standards.md`, `docs/repo/workflows.md`, `ops/README.md`, `apps/README.md`, `engine/README.md`

**Verification Commands:**
- `tooling/ci/check-repo-structure.sh`
- `tooling/ci/check-flux-leakage.sh`
- `rg -n "nautilus_trader\\.flux|systems/flux|product/repo/deploy" README.md docs/repo ops/README.md apps/README.md engine/README.md`

**Step 1: Write the failing documentation assertions**
- Identify docs that still blur product/repo naming with engine identity or leave ownership boundaries ambiguous.
- Keep `nautilus_trader` as the engine namespace while documenting the later `flux` product/repo direction.

**Step 2: Run the repo-level checks**
- Run the commands above.
- Expected before implementation: docs still reflect a transitional state and may not clearly encode the post-hardening naming model.

**Step 3: Implement the naming/structure cleanup**
- Update repo docs so the ownership model is explicit and stable.
- Keep `systems/flux` documented as a transitional boundary until a later decomposition or rename is justified.
- Do not perform a flag-day import rename in this phase.

**Step 4: Re-run the checks**
- Run the commands above.
- Expected after implementation: repo docs describe a coherent monorepo target layered on top of hardened deploy lanes.

**Step 5: Commit**
- `git add README.md docs/repo/structure.md docs/repo/standards.md docs/repo/workflows.md ops/README.md apps/README.md engine/README.md`
- `git commit -m "docs: finalize monorepo ownership and naming guidance"`

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
