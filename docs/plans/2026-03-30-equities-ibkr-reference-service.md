# Equities IBKR Reference Service Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Rehome the equities shared IBKR reference market-data boundary into `nautilus_trader` so grouped equities nodes consume one Flux-owned shared reference feed instead of depending on `~/chainsaw` or per-node IBKR market-data sessions.

**Architecture:** Add a Flux-owned equities IBKR reference publisher runner that derives its universe from `deploy/equities/equities.live.toml`, publishes profile-scoped shared Redis market-data and health keys, and remove direct per-node IBKR market-data ownership by introducing an equities-scoped `interactive_brokers_shared_reference` data adapter that reuses the existing IBKR exec path. Keep grouped-node recovery V1 intact, keep IBKR execution node-local, and update deploy/readiness surfaces to use the new service instead of `chainsaw@md-ibkr-publisher.service`.

**Tech Stack:** Python, Nautilus live data/exec clients, Flux equities runners, Interactive Brokers adapter surfaces, Redis, pytest, systemd release-lane deploys.

**Context Docs:**
- Design: `docs/plans/2026-03-30-equities-ibkr-reference-service-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/plans/2026-03-28-equities-market-data-recovery-v1-design.md`, `docs/plans/2026-03-28-equities-market-data-recovery-v1.md`, `deploy/equities/README.md`, `docs/runbooks/equities-shared-node-cutover.md`, `ops/scripts/deploy/install_equities_systemd.sh`

**Decision Summary:**
- Replace the Chainsaw-owned IBKR publisher with a Flux-owned equities service, not a generic multi-profile platform.
- Do not keep `md_ibkr.ini`; derive universe and session/account settings from `deploy/equities/equities.live.toml`.
- Add profile-scoped shared Redis quote/health keys for the publisher instead of reusing Chainsaw `last:*` keys.
- Add an equities-scoped `interactive_brokers_shared_reference` data adapter so grouped nodes receive normal `QuoteTick` updates without opening IBKR market-data sessions.
- Keep IBKR execution node-local for now by reusing the existing Interactive Brokers exec client/factory.
- Preserve the reviewed V1 node-scoped quote supervisor, pair-level tradeability gating, and fail-closed cancel-only behavior.
- Keep execution serial on the existing implementation branch/worktree to match the user’s consolidation directive; do not create extra worktrees for this wave.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | not_started | main | Task 1-5 | `docs/plans/2026-03-30-equities-ibkr-reference-service.md` | `shared` | `shared` | none | not_run | Plan created; execute serially on `fix/equities-market-data-recovery-v1-impl-20260328` with fresh implementer/spec/quality subagents per task and no extra worktrees |
| Task 1: Add Shared IBKR Reference Config And Redis Key Contract | not_started | unassigned | none | `systems/flux/flux/common/keys.py`, `deploy/equities/equities.live.toml`, `tests/unit_tests/flux/common/test_keys.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 2: Implement The Flux-Owned IBKR Reference Publisher Runner | not_started | unassigned | Task 1 | `systems/flux/flux/runners/shared/ibkr_reference_publisher.py`, `systems/flux/flux/runners/equities/run_ibkr_reference_publisher.py`, `tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 3: Add The Shared-Reference IBKR Market-Data Adapter And Wire Equities Nodes To It | not_started | unassigned | Task 1, Task 2 | `nautilus_trader/adapters/interactive_brokers/shared_reference/config.py`, `nautilus_trader/adapters/interactive_brokers/shared_reference/data.py`, `nautilus_trader/adapters/interactive_brokers/shared_reference/factories.py`, `nautilus_trader/adapters/interactive_brokers/__init__.py`, `systems/flux/flux/runners/live/venues.py`, `systems/flux/flux/strategies/shared/equities_arb/core.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py` | `shared` | `shared` | none | not_run | Plan created |
| Task 4: Update Readiness, Deploy, And Operator Surfaces To Use The Flux-Owned Publisher | not_started | unassigned | Task 2, Task 3 | `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `ops/scripts/deploy/install_equities_systemd.sh`, `ops/scripts/deploy/check_equities_live_readiness.sh`, `deploy/equities/README.md`, `docs/runbooks/equities-shared-node-cutover.md` | `shared` | `shared` | none | not_run | Plan created |
| Task 5: Run End-To-End Verification And Clean Up The Consolidated Equities Integration Branch | not_started | unassigned | Task 1, Task 2, Task 3, Task 4 | `docs/plans/2026-03-30-equities-ibkr-reference-service.md`, git branch/PR metadata, touched code from Tasks 1-4 only as needed for fixes | `shared` | `shared` | none | not_run | Plan created |

---

### Task 1: Add Shared IBKR Reference Config And Redis Key Contract

**Files:**
- Modify: `systems/flux/flux/common/keys.py`
- Modify: `deploy/equities/equities.live.toml`
- Test: `tests/unit_tests/flux/common/test_keys.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`

**Dependencies:** `none`

**Write Scope:** `systems/flux/flux/common/keys.py`, `deploy/equities/equities.live.toml`, `tests/unit_tests/flux/common/test_keys.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/common/test_keys.py -k 'profile_market or market_data_status'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'reference or account_scope or strategy_contract'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py -k 'effective_venue_resolution'`

**Step 1: Write the failing config/key tests**

Add tests that prove:

- `FluxRedisKeys` exposes profile-scoped shared market-data keys for the IBKR reference publisher
- `FluxRedisKeys` exposes a profile-scoped shared publisher status key/channel
- `deploy/equities/equities.live.toml` contains one equities-owned `ibkr_reference_publisher` service table instead of relying on `md_ibkr.ini`
- the shared equities config remains the source of truth for `reference_account_scope_id` and `reference_instrument_id`

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/common/test_keys.py -k 'profile_market or market_data_status'`

Expected: FAIL because the new shared IBKR reference key builders do not exist yet.

**Step 3: Add the minimal shared config/key contract**

Update the shared Flux key builder and equities live config so:

- there is one canonical profile-scoped market-data key for shared IBKR reference quotes
- there is one canonical profile-scoped health/status key for the shared IBKR publisher
- equities live config holds publisher-specific runtime knobs in a top-level shared table
- the shared config remains deterministic from `strategy_contracts` plus `account_scopes`

Keep the new config equities-specific; do not generalize it for every profile.

**Step 4: Re-run the tests**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/common/keys.py \
  deploy/equities/equities.live.toml \
  tests/unit_tests/flux/common/test_keys.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py
git commit -m "feat: add shared equities ibkr reference contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Implement The Flux-Owned IBKR Reference Publisher Runner

**Files:**
- Create: `systems/flux/flux/runners/shared/ibkr_reference_publisher.py`
- Create: `systems/flux/flux/runners/equities/run_ibkr_reference_publisher.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Dependencies:** `Task 1: Add Shared IBKR Reference Config And Redis Key Contract`

**Write Scope:** `systems/flux/flux/runners/shared/ibkr_reference_publisher.py`, `systems/flux/flux/runners/equities/run_ibkr_reference_publisher.py`, `tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py`
- `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k 'account_scope or strategy_contracts or equities'`

**Step 1: Write the failing publisher tests**

Add tests that prove:

- the publisher derives a deduped universe from `[[strategy_contracts]]`
- it resolves the IBKR reference account scope from `[[account_scopes]]`
- it preserves session-aware SMART vs overnight feed selection
- it writes the new profile-scoped shared market-data keys and health/status keys
- reconnect/backoff and health transitions are explicit and bounded
- it does not require `md_ibkr.ini` or any Chainsaw helpers

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py`

Expected: FAIL because the new runner and shared implementation do not exist yet.

**Step 3: Implement the shared publisher**

Create the Flux-owned publisher runner so:

- shared config is loaded through the existing Flux bootstrap path
- universe derivation comes from `strategy_contracts`
- only the unique IBKR reference instruments are subscribed
- the service connects once to the IBKR reference session
- SMART vs overnight selection logic is preserved from the legacy publisher where still correct
- shared quote payloads and health/status are written through the new Flux Redis contract
- reconnect/backoff is explicit and bounded

Do not import from `~/chainsaw`, do not keep `configparser`, and do not create a second sidecar config file.

**Step 4: Re-run the tests**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/shared/ibkr_reference_publisher.py \
  systems/flux/flux/runners/equities/run_ibkr_reference_publisher.py \
  tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "feat: add flux-owned equities ibkr reference publisher"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add The Shared-Reference IBKR Market-Data Adapter And Wire Equities Nodes To It

**Files:**
- Create: `nautilus_trader/adapters/interactive_brokers/shared_reference/config.py`
- Create: `nautilus_trader/adapters/interactive_brokers/shared_reference/data.py`
- Create: `nautilus_trader/adapters/interactive_brokers/shared_reference/factories.py`
- Modify: `nautilus_trader/adapters/interactive_brokers/__init__.py`
- Modify: `systems/flux/flux/runners/live/venues.py`
- Modify: `systems/flux/flux/strategies/shared/equities_arb/core.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`
- Create: `tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py`

**Dependencies:** `Task 1: Add Shared IBKR Reference Config And Redis Key Contract`, `Task 2: Implement The Flux-Owned IBKR Reference Publisher Runner`

**Write Scope:** `nautilus_trader/adapters/interactive_brokers/shared_reference/config.py`, `nautilus_trader/adapters/interactive_brokers/shared_reference/data.py`, `nautilus_trader/adapters/interactive_brokers/shared_reference/factories.py`, `nautilus_trader/adapters/interactive_brokers/__init__.py`, `systems/flux/flux/runners/live/venues.py`, `systems/flux/flux/strategies/shared/equities_arb/core.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py`, `tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py -k 'effective_venue_resolution'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'ibkr or shared_reference or venue_resolution'`

**Step 1: Write the failing shared-adapter tests**

Add tests that prove:

- the shared-reference data client subscribes to profile-scoped shared Redis updates, not IBKR `reqMktData`
- shared snapshot payloads are translated into normal Nautilus `QuoteTick` updates
- grouped nodes still receive local IBKR reference quote ticks through `on_quote_tick`
- equities venue resolution rewrites the IBKR data side to `interactive_brokers_shared_reference`
- IBKR execution still resolves through the normal Interactive Brokers exec client

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py`

Expected: FAIL because the shared-reference adapter does not exist yet.

**Step 3: Implement the shared-reference adapter and wiring**

Add a new IBKR shared-reference adapter surface so:

- the data client consumes the publisher’s shared Redis quote stream
- it forwards translated `QuoteTick` objects into `DataEngine.process`
- it does not create direct IBKR market-data subscriptions
- the equities-specific venue rewrite layer switches IBKR data to the shared-reference adapter while leaving execution on the normal IBKR exec client
- `MakerV4` and the V1 quote supervisor continue receiving local reference quote ticks without code-path regressions

Keep the change equities-scoped; do not silently replace the default Interactive Brokers adapter for the rest of the repo.

**Step 4: Re-run the tests**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/interactive_brokers/shared_reference/config.py \
  nautilus_trader/adapters/interactive_brokers/shared_reference/data.py \
  nautilus_trader/adapters/interactive_brokers/shared_reference/factories.py \
  nautilus_trader/adapters/interactive_brokers/__init__.py \
  systems/flux/flux/runners/live/venues.py \
  systems/flux/flux/strategies/shared/equities_arb/core.py \
  systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/shared/test_equities_arb_core.py \
  tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py
git commit -m "feat: switch equities ibkr reference data to shared publisher"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Update Readiness, Deploy, And Operator Surfaces To Use The Flux-Owned Publisher

**Files:**
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `ops/scripts/deploy/check_equities_live_readiness.sh`
- Modify: `deploy/equities/README.md`
- Modify: `docs/runbooks/equities-shared-node-cutover.md`

**Dependencies:** `Task 2: Implement The Flux-Owned IBKR Reference Publisher Runner`, `Task 3: Add The Shared-Reference IBKR Market-Data Adapter And Wire Equities Nodes To It`

**Write Scope:** `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `ops/scripts/deploy/install_equities_systemd.sh`, `ops/scripts/deploy/check_equities_live_readiness.sh`, `deploy/equities/README.md`, `docs/runbooks/equities-shared-node-cutover.md`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'ibkr or readiness or reference'`
- `bash -n ops/scripts/deploy/install_equities_systemd.sh`
- `bash -n ops/scripts/deploy/check_equities_live_readiness.sh`

**Step 1: Write the failing readiness/deploy tests**

Add tests that prove:

- readiness treats the shared IBKR reference publisher status as a required precondition
- shared reference publisher failure produces an explicit blocked/degraded readiness state
- the installer renders one new equities service env for the shared publisher
- operator docs and restart order no longer mention `chainsaw@md-ibkr-publisher.service`

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'ibkr or readiness or reference'`

Expected: FAIL because readiness and deploy docs still assume the Chainsaw-owned publisher.

**Step 3: Implement readiness and deploy changes**

Update the readiness and deploy surfaces so:

- the shared publisher is part of the equities release-lane service graph
- `install_equities_systemd.sh` renders a Flux-owned publisher env/service
- readiness checks the new shared publisher status and shared reference freshness
- docs and cutover instructions use the Flux-owned service name and restart order

Keep the live deploy contract intact:

- immutable release roots only
- no live services from the dev repo or worktrees
- no hot-editing active release roots

**Step 4: Re-run the tests**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/equities/readiness.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  ops/scripts/deploy/install_equities_systemd.sh \
  ops/scripts/deploy/check_equities_live_readiness.sh \
  deploy/equities/README.md \
  docs/runbooks/equities-shared-node-cutover.md
git commit -m "fix: move equities ibkr publisher into flux deploy lane"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Run End-To-End Verification And Clean Up The Consolidated Equities Integration Branch

**Files:**
- Modify as needed for fixes from verification: touched files from Tasks 1-4 only
- Modify: `docs/plans/2026-03-30-equities-ibkr-reference-service.md`
- Git metadata only: branch/PR base and title/body alignment for the consolidated equities integration PR

**Dependencies:** `Task 1: Add Shared IBKR Reference Config And Redis Key Contract`, `Task 2: Implement The Flux-Owned IBKR Reference Publisher Runner`, `Task 3: Add The Shared-Reference IBKR Market-Data Adapter And Wire Equities Nodes To It`, `Task 4: Update Readiness, Deploy, And Operator Surfaces To Use The Flux-Owned Publisher`

**Write Scope:** `docs/plans/2026-03-30-equities-ibkr-reference-service.md`, touched files from Tasks 1-4 only as needed for fixes, branch/PR metadata

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_ibkr_reference_publisher.py tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'ibkr or shared_reference or readiness'`
- `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py`
- `git diff --check`

**Step 1: Run the full targeted verification set**

Run all verification commands listed above from the project worktree using the project venv.

Expected: PASS

**Step 2: Fix any issues through the required review loop**

For every failure or review finding:

- return the task to `in_progress`
- fix only the verified issue
- re-run the relevant tests
- send the task back through spec review, then quality review

Do not skip the review loop because the branch is already large.

**Step 3: Update the consolidation metadata**

Once code and docs are green:

- keep this work on `fix/equities-market-data-recovery-v1-impl-20260328`
- make the existing implementation PR the single integration PR against `main`
- collapse the narrative so the PR clearly absorbs `#88` and `#90` instead of presenting three separate live lines

Do not create another worktree or another long-lived integration branch.

**Step 4: Record final tracker state**

Update the Progress Tracker with:

- final integrated commit(s)
- final verification commands/results
- any remaining rollout risks or operator prerequisites

**Step 5: Commit**

```bash
git add docs/plans/2026-03-30-equities-ibkr-reference-service.md
git commit -m "docs: record equities ibkr reference rehome verification"
```

If fixes touched code in this task, include those files in the same final commit only when they are tightly coupled to the verification findings; otherwise keep the earlier task-sized commits and make this a docs-only closeout commit.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
