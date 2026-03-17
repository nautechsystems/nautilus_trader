# Equities trade[XYZ] Stock Universe Expansion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Expand the equities MakerV3 deploy contract from the single AAPL canary to the full active trade[XYZ] stock universe, while keeping `/equities` stable and avoiding non-stock trade[XYZ] markets.

**Architecture:** Keep the existing dedicated `equities` stack and MakerV3 contract, but replace its single-strategy AAPL assumptions with an explicit checked-in stock universe manifest. Generate or validate one strategy TOML per active trade[XYZ] stock, update the shared deploy manifest and generated systemd assets to enroll all active stock strategies, and preserve IBKR as the reference venue with exact instrument IDs instead of guessed exchange names.

**Tech Stack:** TOML deploy configs, Python/Flux runners, systemd installer assets, IBKR symbology, Hyperliquid `dex="xyz"` metadata, pytest, curl/jq.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Multi-strategy stock-universe branch verified and ready for PR update |
| Task 1: Lock Active Stock Universe And IBKR Reference Map | completed | main | Enrolled 24 exact-qualified stocks including HYUNDAI/USAR; SMSN and SKHX remain blocked pending exact IBKR qualification |
| Task 2: Add Red Tests For Multi-Strategy Equities Enrollment | completed | main | Multi-strategy contract test now covers 24-stock set and passes |
| Task 3: Implement Multi-Strategy Strategy Config And Shared Manifest Support | completed | main | Shared config, checked-in systemd assets, and per-stock MakerV3 TOMLs added for the enrolled stock set |
| Task 4: Update Generated systemd/Pulse Assets And Docs | completed | main | Checked-in target/sudoers/docs updated for the enrolled 24-stock MakerV3 set |
| Task 5: Verify Clean-Worktree Deploy Contract And Live Rollout Readiness | completed | main | `70 passed` targeted suite, `git diff --check` clean |

---

### Task 1: Lock Active Stock Universe And IBKR Reference Map

**Files:**
- Create: `docs/reviews/2026-03-11-equities-tradexyz-stock-universe.md`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`

**Step 1: Capture the active trade[XYZ] stock universe in a review artifact**

Use the live `dex="xyz"` metadata and the user-provided universe snapshot to lock the active stock-only set. Exclude ETFs, FX, commodities, indices, and delisted products. The expected initial stock candidate set is:

```text
AAPL AMD AMZN BABA COIN CRCL CRWV GOOGL HOOD HYUNDAI INTC META
MSTR MSFT MU NFLX NVDA ORCL PLTR RIVN SKHX SMSN SNDK TSM TSLA
```

Document which symbols are active, which were excluded, and why.

**Step 2: Verify exact IBKR instrument IDs instead of inferring them**

For each active stock symbol, record the exact IBKR reference `instrument_id` to be used in the strategy TOML (for example `AAPL.NASDAQ` for AAPL). If a symbol needs an explicit non-NASDAQ exchange or cannot be qualified cleanly, record that as a blocker instead of guessing.

**Step 3: Write the review note**

Document:
1. the active stock-only trade[XYZ] universe
2. excluded non-stock symbols and delisted symbols
3. the exact IBKR instrument ID map
4. any symbols that require follow-up before live enrollment

**Step 4: Verify the artifact**

Run:

```bash
rg -n 'AAPL|TSLA|NVDA|GOOGL|SKHX|HYUNDAI|excluded|IBKR' \
  docs/reviews/2026-03-11-equities-tradexyz-stock-universe.md \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md
```

Expected: the docs distinguish active stock strategies from excluded non-stock markets and do not leave exchange mappings implicit.

**Step 5: Commit**

```bash
git add \
  docs/reviews/2026-03-11-equities-tradexyz-stock-universe.md \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md
git commit -m "docs(equities): lock tradexyz stock universe"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Add Red Tests For Multi-Strategy Equities Enrollment

**Files:**
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. `deploy/equities/equities.live.toml` enrolls all active stock strategy IDs, not just AAPL
2. the shared `[[contracts]]` catalog includes one Hyperliquid contract and one IBKR contract per enrolled stock strategy
3. the installer/systemd contract is multi-node, not AAPL-only
4. the equities API contract still serves `/equities` with multiple active stock rows without falling back to a single-strategy identity

**Step 2: Run tests to verify they fail**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 ./.venv/bin/python -m pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
```

Expected: failures caused by AAPL-only assumptions in the deploy manifest, template, and generated assets.

**Step 3: Tighten the exact assertions**

Use the final active stock ID set and exact IBKR mappings from Task 1 in the tests. Do not write loose “len > 1” assertions when the contract should be exact.

**Step 4: Re-run the red tests**

Run the same pytest command.
Expected: still FAIL, but now on the intended single-strategy assumptions.

**Step 5: Commit**

```bash
git add \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "test(equities): define multi-strategy stock universe contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Implement Multi-Strategy Strategy Config And Shared Manifest Support

**Files:**
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Create or modify: `deploy/equities/strategies/*.toml` for each active stock strategy
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `systems/flux/flux/runners/equities/run_api.py`

**Step 1: Implement the stock universe source of truth**

Choose one checked-in source of truth for the enrolled stock strategy set. Recommendation: keep explicit committed strategy TOMLs plus a manifest-friendly shared allowlist in `equities.live.toml`, and make the installer discover the node list from the committed strategy files.

**Step 2: Replace AAPL-only shared config fields**

Generalize or remove shared fields that no longer make sense as single values (`execution_symbol`, `reference_symbol`, `base_asset`, AAPL-only contract rows). Keep `/equities`, `profile=equities`, and `portfolio=equities` stable.

**Step 3: Materialize one strategy TOML per active stock**

Each strategy file must keep:
1. `strategy_groups = "equities"`
2. Hyperliquid `dex = "xyz"`
3. safe-off execution in the checked-in TOML
4. exact IBKR `instrument_id` from Task 1

**Step 4: Keep the API/runner contract aligned**

Update any runner assumptions that still treat the shared equities stack as single-symbol, while preserving existing AAPL behavior as a subset of the new multi-symbol contract.

**Step 5: Run tests to verify green**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 ./.venv/bin/python -m pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
```

Expected: PASS.

**Step 6: Commit**

```bash
git add \
  deploy/equities/equities.live.toml \
  deploy/equities/strategies \
  ops/scripts/deploy/install_equities_systemd.sh \
  systems/flux/flux/runners/equities/run_api.py
git commit -m "feat(equities): enroll tradexyz stock universe"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Update Generated systemd/Pulse Assets And Docs

**Files:**
- Modify: `deploy/equities/systemd/flux-equities.target`
- Modify: `deploy/equities/systemd/flux-pulse.sudoers`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `fluxboard/docs/equities_contract.md`

**Step 1: Remove AAPL-only generated-service assumptions**

Make the checked-in systemd target and sudoers contract compatible with multiple equities node units, matching what the installer emits for the enrolled strategy set.

**Step 2: Update operator docs**

Document:
1. the enrolled trade[XYZ] stock universe
2. how node env files map to strategy IDs
3. how to disable or re-enable individual stock nodes safely
4. that non-stock trade[XYZ] products remain out of scope for `/equities`

**Step 3: Verify docs and generated assets**

Run:

```bash
rg -n 'aapl_tradexyz_makerv3|equities-node-|equities_strategy_ids|TSLA|NVDA|EWY|GOLD' \
  deploy/equities \
  fluxboard/docs/equities_contract.md
```

Expected: AAPL is no longer the only active equities identity; excluded non-stock products are not accidentally documented as part of `/equities`.

**Step 4: Commit**

```bash
git add \
  deploy/equities/systemd/flux-equities.target \
  deploy/equities/systemd/flux-pulse.sudoers \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  fluxboard/docs/equities_contract.md
git commit -m "docs(equities): document multi-strategy stock rollout"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Verify Clean-Worktree Deploy Contract And Live Rollout Readiness

**Files:**
- Verify only: `deploy/equities/equities.live.toml`
- Verify only: `deploy/equities/strategies/*.toml`
- Verify only: `ops/scripts/deploy/install_equities_systemd.sh`
- Verify only: `/etc/flux/equities-*.env` after any optional live rollout

**Step 1: Verify the clean worktree itself**

Run:

```bash
git status --short
git diff --check
```

Expected: no unstaged repo drift other than any intentional plan/review artifacts.

**Step 2: Run the full targeted verification suite**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 ./.venv/bin/python -m pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/common/test_quantity_units.py \
  tests/unit_tests/flux/strategies/makerv3/test_market_data.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py
cargo test -p nautilus-hyperliquid test_create_perp_instrument_surfaces_identity_base_exposure_mode --lib
```

Expected: PASS.

**Step 3: Push and update the PR**

Run:

```bash
git push
gh pr view --json url,number,title,state
```

Expected: the clean PR reflects the expanded equities stock universe.

**Step 4: If rolling live, verify generated envs and services**

Run:

```bash
sudo ops/scripts/deploy/install_equities_systemd.sh
systemctl --no-pager --type=service --all | rg 'flux@equities-node-|flux@equities-api|flux@equities-bridge|flux@equities-portfolio'
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities' | jq '.data.strategies | length'
```

Expected: one service per enrolled stock strategy and a live API row count that matches the allowlist.

**Step 5: Commit rollout-only doc evidence if needed**

```bash
git add docs/reviews/2026-03-11-equities-tradexyz-stock-universe.md
git commit -m "docs(equities): record stock universe rollout evidence"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
