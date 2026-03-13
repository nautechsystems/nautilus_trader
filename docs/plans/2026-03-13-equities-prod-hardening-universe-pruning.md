# Equities Prod Hardening and Universe Pruning Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Move equities from a broad read-only basket with noisy edge cases into a smaller, explicitly admitted US-stock production basket whose market data, balances, and readiness are stable enough for a one-symbol live canary.

**Architecture:** Treat `deploy/equities/equities.live.toml` as the canonical production universe and shrink it first. Disabled names should leave the allowlist, contract catalog, strategy discovery set, systemd target, and Pulse sudoers surface entirely so readiness only reflects names we intentionally support. After the basket is small and explicit, fix remaining stale market-data paths only for the retained core names.

**Tech Stack:** TOML deploy config, Python Flux runners/API, Rust Hyperliquid/network adapters, systemd/Pulse, pytest, cargo test, journald, Redis, Fluxboard.

## Continuation Context

- Current live state on `2026-03-13 05:33 UTC`:
  - all `23/23` equities node units are active
  - `chainsaw@md-ibkr-publisher` is active
  - balances are green: `/api/v1/balances?profile=equities` => `degraded=false`, `count=7`, `mv="$313.98"`
  - strategy market data is not fully green: maker quotes present on `20/23`, ref quotes present on `21/23`
  - current missing legs are:
    - maker: `amd_tradexyz_makerv3`, `mu_tradexyz_makerv3`, `pltr_tradexyz_makerv3`
    - ref: `rivn_tradexyz_makerv3`, `usar_tradexyz_makerv3`
- Current active checked-in universe is the full 23-name set documented in `deploy/equities/README.md`.
- The current broad basket is mixing three concerns:
  - names we absolutely should support in prod
  - names that are plausible second-wave adds
  - names that are non-US, structurally noisy, recent, or low-confidence for first-wave prod

## Recommended Universe Split

**Recommended Tier 1 core basket for prod-readiness and first canary**

- `aapl_tradexyz_makerv3`
- `amd_tradexyz_makerv3`
- `amzn_tradexyz_makerv3`
- `googl_tradexyz_makerv3`
- `meta_tradexyz_makerv3`
- `msft_tradexyz_makerv3`
- `nvda_tradexyz_makerv3`
- `orcl_tradexyz_makerv3`
- `pltr_tradexyz_makerv3`
- `tsla_tradexyz_makerv3`

**Recommended second-wave disabled basket**

- `coin_tradexyz_makerv3`
- `hood_tradexyz_makerv3`
- `intc_tradexyz_makerv3`
- `mu_tradexyz_makerv3`
- `nflx_tradexyz_makerv3`
- `rivn_tradexyz_makerv3`

**Recommended immediate decommission / out-of-scope basket**

- `baba_tradexyz_makerv3`
- `crcl_tradexyz_makerv3`
- `crwv_tradexyz_makerv3`
- `mstr_tradexyz_makerv3`
- `sndk_tradexyz_makerv3`
- `tsm_tradexyz_makerv3`
- `usar_tradexyz_makerv3`

**Admission policy for any future re-add**

1. US-primary listed common stock only for Tier 1; no ADR / non-US-primary exposure in the first-wave prod basket.
2. Liquidity must be measured, not guessed: require a documented 30-day median daily dollar-volume floor before re-admission.
3. The name must have reliable reference data on IBKR and stable maker data on Hyperliquid for at least one full trading session in read-only mode.
4. The name must be free of recent launch / corporate-action / special-situation churn that would distort a first-wave canary.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | in_progress | main | `2026-03-13 10:07 UTC` The reduced 10-name Tier 1 rollout remains live from the worktree, and the remaining false-red surfaces are now narrowed to a specific Hyperliquid account-projection gap. The balances/control-plane slice is live with funded-master Hyperliquid risk totals (`account_equity_display = "$7478.39"`, `withdrawable_display = "$7478.39"`), master spot assets (`USDC`, `USDE`, `USDH`), and IBKR holdings serialized as `spot` + `equity`, but fresh live verification showed the funded master also has active `xyz` Hyperliquid perp positions (`NVDA`, `COIN`, `GOOGL`) that are still missing from the shared-account balances surface. The discrepancy is now isolated and documented in `docs/plans/2026-03-13-hyperliquid-xyz-account-surface.md`: raw `clearinghouseState(dex="xyz")` shows the live positions, while the current provider/adapter path still drops them. The readiness gate remains explicitly session-aware for IBKR references, but balances/risk should not be treated as fully production-correct until the Hyperliquid `xyz` account-surface plan lands. |
| Task 1: Freeze Admission Policy and Core Basket | completed | main | `2026-03-13 05:57 UTC` deploy READMEs and stack contract tests now freeze the Tier 1 core basket, second-wave disabled basket, decommissioned basket, and re-add admission policy without pruning `equities.live.toml` early |
| Task 2: Prune Shared Allowlist and Contract Catalog | completed | main | `2026-03-13 06:02 UTC` pruned `deploy/equities/equities.live.toml` so `equities_strategy_ids`, `equities_required_strategy_ids`, `[[strategy_contracts]]`, and `[[contracts]]` now keep only the Tier 1 core basket; focused pytest for stack/API/portfolio contracts is green |
| Task 3: Disable Non-Core Strategy Files and Regenerate Service Discovery | completed | main | `2026-03-13 06:07 UTC` renamed 13 non-core strategy files to `.toml.disabled` and re-rendered the checked-in `flux-equities.target` and `flux-pulse.sudoers`; focused stack-contract pytest is green |
| Task 4: Fix Must-Have Market Data for Retained Basket | completed | main | `2026-03-13 06:20 UTC` added regressions for stabilization-failure retries in `nautilus-network` and Hyperliquid integration, then fixed reconnect notification/reset handling so buffered restores are re-queued on subsequent reconnect attempts; websocket suites and `git diff --check` are green |
| Task 5: Roll Out Reduced Basket and Re-Verify Readiness | completed | main | `2026-03-13 09:29 UTC` the reduced basket is live and re-verified against the corrected operational policy. After the earlier row-local leg fix, this slice added the final session-aware rule: the host wrapper and readiness evaluator now ignore IBKR reference-age failures outside the regular US session while still enforcing strict ref freshness in-session. Focused TDD landed in `tests/unit_tests/examples/strategies/test_equities_readiness.py` (`12 passed` full suite), `bash -n ops/scripts/deploy/check_equities_live_readiness.sh` is clean, and a fresh host-local `check_equities_live_readiness.sh --json` now returns `ok = true`, `healthy_strategy_count = 10`, `reference_freshness_enforced = false`, and green balances/projections/components on the Tier 1 basket at `05:29 America/New_York`. |
| Task 6: Gate the First Live Canary | not_started | main | Plan created |

---

### Task 1: Freeze Admission Policy and Core Basket

**Files:**
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/strategies/README.md`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Update the stack contract fixture data so the intended active basket is the 10-name Tier 1 set, and add explicit disabled/decommissioned constants:

```python
CORE_PROD_STRATEGIES = (
    "aapl_tradexyz_makerv3",
    "amd_tradexyz_makerv3",
    "amzn_tradexyz_makerv3",
    "googl_tradexyz_makerv3",
    "meta_tradexyz_makerv3",
    "msft_tradexyz_makerv3",
    "nvda_tradexyz_makerv3",
    "orcl_tradexyz_makerv3",
    "pltr_tradexyz_makerv3",
    "tsla_tradexyz_makerv3",
)
DECOMMISSIONED_STRATEGIES = {
    "baba_tradexyz_makerv3",
    "crcl_tradexyz_makerv3",
    "crwv_tradexyz_makerv3",
    "mstr_tradexyz_makerv3",
    "sndk_tradexyz_makerv3",
    "tsm_tradexyz_makerv3",
    "usar_tradexyz_makerv3",
}
```

Add assertions that the deploy README documents the Tier 1 basket and that decommissioned names are absent from the active contract.

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py -p no:rerunfailures
```

Expected: FAIL because the current checked-in universe still contains all 23 names.

**Step 3: Write the minimal implementation**

Update the deploy READMEs so they explicitly state:

- the Tier 1 production basket
- the disabled second-wave basket
- the decommissioned first-wave exclusions
- the admission policy for any re-add

**Step 4: Run tests to verify they pass**

Run the same pytest command and:

```bash
rg -n "Tier 1|second-wave|decommission|aapl_tradexyz_makerv3|tsla_tradexyz_makerv3" \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md
```

Expected: PASS with docs matching the intended basket.

**Step 5: Commit**

```bash
git add \
  deploy/equities/README.md \
  deploy/equities/strategies/README.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "docs(equities): define core prod basket and admission policy"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## 2026-03-13 11:26 UTC Update

- The reduced-basket hardening work now has the missing Hyperliquid shared-account visibility fixed.
- Live equities balances show the shared Hyperliquid master account cash plus the live `xyz` perp positions (`NVDA`, `COIN`, `GOOGL`) alongside the IBKR stock rows.
- This closes the main shared-account observability gap for the retained basket and makes the balances/risk surface materially closer to production correctness before enabling broader tradability.

## 2026-03-13 11:24 UTC Update

- Shared Hyperliquid `xyz` account-state surfacing is locally green in the equities worktree for the retained basket:
  - provider contract passes with `dex="xyz"` and shared `NVDA/COIN/GOOGL` perp rows
  - account projection tests pass for stable shared Hyperliquid row IDs
  - equities balances API contract passes
  - Fluxboard shared Hyperliquid parser contract passes
- The release build completed successfully from the worktree.
- The remaining blocker before canary gating is operational:
  - this post-reboot shell cannot reach systemd (`Operation not permitted`)
  - live HTTP endpoints are unreachable from this environment
- Do not advance to canary gating until `flux@equities-portfolio` and `flux@equities-api` are restarted on the rebuilt artifact and live balances are rechecked against the raw Hyperliquid master account.

### Task 2: Prune Shared Allowlist and Contract Catalog

**Files:**
- Modify: `deploy/equities/equities.live.toml`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing tests**

Add and update tests so the live config contract requires:

- `api.equities_strategy_ids == CORE_PROD_STRATEGIES`
- `api.equities_required_strategy_ids == CORE_PROD_STRATEGIES`
- only Tier 1 `[[strategy_contracts]]` rows
- only Tier 1 `[[contracts]]` Hyperliquid and IBKR entries

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  -p no:rerunfailures
```

Expected: FAIL because `equities.live.toml` still carries the 23-name universe.

**Step 3: Write the minimal implementation**

Edit `deploy/equities/equities.live.toml` so the active and required allowlists, `[[strategy_contracts]]`, and `[[contracts]]` only describe the Tier 1 basket.

**Step 4: Run tests to verify they pass**

Run the same pytest command and:

```bash
rg -n "equities_strategy_ids|equities_required_strategy_ids|baba|tsm|usar|pltr|tsla" \
  deploy/equities/equities.live.toml
```

Expected: PASS, with pruned names absent from the active catalog.

**Step 5: Commit**

```bash
git add \
  deploy/equities/equities.live.toml \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "config(equities): prune live universe to core prod basket"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Disable Non-Core Strategy Files and Regenerate Service Discovery

**Files:**
- Rename: `deploy/equities/strategies/coin_tradexyz_makerv3.toml` -> `deploy/equities/strategies/coin_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/hood_tradexyz_makerv3.toml` -> `deploy/equities/strategies/hood_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/intc_tradexyz_makerv3.toml` -> `deploy/equities/strategies/intc_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/mu_tradexyz_makerv3.toml` -> `deploy/equities/strategies/mu_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/nflx_tradexyz_makerv3.toml` -> `deploy/equities/strategies/nflx_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/rivn_tradexyz_makerv3.toml` -> `deploy/equities/strategies/rivn_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/baba_tradexyz_makerv3.toml` -> `deploy/equities/strategies/baba_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/crcl_tradexyz_makerv3.toml` -> `deploy/equities/strategies/crcl_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/crwv_tradexyz_makerv3.toml` -> `deploy/equities/strategies/crwv_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/mstr_tradexyz_makerv3.toml` -> `deploy/equities/strategies/mstr_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/sndk_tradexyz_makerv3.toml` -> `deploy/equities/strategies/sndk_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/tsm_tradexyz_makerv3.toml` -> `deploy/equities/strategies/tsm_tradexyz_makerv3.toml.disabled`
- Rename: `deploy/equities/strategies/usar_tradexyz_makerv3.toml` -> `deploy/equities/strategies/usar_tradexyz_makerv3.toml.disabled`
- Modify: `deploy/equities/systemd/flux-equities.target`
- Modify: `deploy/equities/systemd/flux-pulse.sudoers`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Expand the stack contract test so every non-core strategy id is absent from:

- strategy discovery
- `flux-equities.target`
- `flux-pulse.sudoers`

and every Tier 1 strategy id remains present.

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py -p no:rerunfailures
```

Expected: FAIL because strategy discovery and generated artifacts still include all 23 names.

**Step 3: Write the minimal implementation**

- Rename the non-core `.toml` files to `.toml.disabled`
- Re-run the generator:

```bash
sudo ops/scripts/deploy/install_equities_systemd.sh
```

This should rewrite `deploy/equities/systemd/flux-equities.target` and `deploy/equities/systemd/flux-pulse.sudoers` to the smaller basket.

**Step 4: Run tests to verify they pass**

Run the same pytest command and:

```bash
rg -n "baba|coin|crcl|crwv|hood|intc|mstr|mu|nflx|rivn|sndk|tsm|usar" \
  deploy/equities/systemd/flux-equities.target \
  deploy/equities/systemd/flux-pulse.sudoers \
  deploy/equities/strategies
```

Expected: only `.toml.disabled` hits remain for non-core names; active systemd artifacts only include Tier 1.

**Step 5: Commit**

```bash
git add \
  deploy/equities/strategies \
  deploy/equities/systemd/flux-equities.target \
  deploy/equities/systemd/flux-pulse.sudoers \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "ops(equities): disable non-core prod strategies"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Fix Must-Have Market Data for Retained Basket

**Files:**
- Modify: `crates/adapters/hyperliquid/src/websocket/client.rs`
- Modify: `crates/adapters/hyperliquid/src/websocket/handler.rs`
- Modify: `crates/network/src/websocket/client.rs`
- Modify: `crates/adapters/hyperliquid/tests/websocket.rs`
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`

**Step 1: Write the failing tests**

Add a reconnect regression that models the retained-core failure shape:

- the client reconnects
- buffered restore is attempted
- the first replacement writer closes during stabilization
- the client must keep retrying until the subscription is live and a book update arrives

Name the regression after the retained-core symptom, for example:

```rust
#[tokio::test]
async fn test_reconnect_retries_until_subscription_recovers_after_stabilization_failure() {
    // server closes the first replacement socket before the first live book update;
    // client must reconnect again and deliver a fresh subscribed book event.
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p nautilus-hyperliquid --test websocket -- --nocapture
```

Expected: FAIL on the new reconnect regression.

**Step 3: Write the minimal implementation**

Fix the reconnect path in the existing Hyperliquid/network websocket stack so the retained Tier 1 symbols no longer get stuck in the "reconnected but no fresh book" state after a stabilization-window failure.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p nautilus-hyperliquid --test websocket -- --nocapture
cargo test -p nautilus-network websocket -- --nocapture
git diff --check
```

Expected: PASS and clean diff check.

**Step 5: Commit**

```bash
git add \
  crates/adapters/hyperliquid/src/websocket/client.rs \
  crates/adapters/hyperliquid/src/websocket/handler.rs \
  crates/network/src/websocket/client.rs \
  crates/adapters/hyperliquid/tests/websocket.rs \
  docs/plans/2026-03-12-equities-live-trading-readiness.md
git commit -m "fix(hyperliquid): stabilize retained equities prod basket"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Roll Out Reduced Basket and Re-Verify Readiness

**Files:**
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Modify: `docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md`

**Step 1: Build from the worktree**

Run:

```bash
env VIRTUAL_ENV=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/.venv \
PYO3_ONLY=1 BUILD_MODE=release \
/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr/.venv/bin/python build.py
```

Expected: successful release build from the worktree.

**Step 2: Reinstall and restart the reduced basket**

Run:

```bash
sudo ops/scripts/deploy/install_equities_systemd.sh
sudo systemctl daemon-reload
sudo systemctl restart chainsaw@md-ibkr-publisher
sudo systemctl restart flux@equities-portfolio
sudo systemctl restart flux@equities-bridge
for unit in \
  flux@equities-node-aapl_tradexyz_makerv3 \
  flux@equities-node-amd_tradexyz_makerv3 \
  flux@equities-node-amzn_tradexyz_makerv3 \
  flux@equities-node-googl_tradexyz_makerv3 \
  flux@equities-node-meta_tradexyz_makerv3 \
  flux@equities-node-msft_tradexyz_makerv3 \
  flux@equities-node-nvda_tradexyz_makerv3 \
  flux@equities-node-orcl_tradexyz_makerv3 \
  flux@equities-node-pltr_tradexyz_makerv3 \
  flux@equities-node-tsla_tradexyz_makerv3; do
  sudo systemctl restart "$unit"
  sleep 10
done
sudo systemctl restart flux@equities-api
```

**Step 3: Verify live readiness**

Run:

```bash
systemctl list-units 'flux@equities-node-*' --type=service --state=running --no-legend | wc -l
curl -fsS http://127.0.0.1:5024/api/v1/signals?profile=equities | jq '.data.strategies | length'
curl -fsS http://127.0.0.1:5024/api/v1/balances?profile=equities | jq '.data | {degraded, count, totals}'
journalctl -u flux@equities-node-amd_tradexyz_makerv3 -u flux@equities-node-pltr_tradexyz_makerv3 -n 120 --no-pager
```

Expected:

- only 10 equities node units enrolled and running
- `/api/v1/signals?profile=equities` returns only the 10-name basket
- balances remain green
- no retained-core stale legs

**Step 4: Update both plan trackers**

Record:

- final active basket size
- stale or clean data result
- balances result
- any residual blockers

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md
git commit -m "docs(equities): record reduced prod basket rollout"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Gate the First Live Canary

**Files:**
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Modify: `deploy/equities/README.md`

**Step 1: Enforce the canary gate**

Do not enable trading until all of the following are true for the reduced basket:

- `10/10` maker legs fresh
- `10/10` reference legs fresh
- balances stay `degraded=false`
- no fresh IBKR auth failure
- no retained-core websocket reconnect churn in journald for one full session window

**Step 2: Enable one symbol only**

Recommended first canary: `aapl_tradexyz_makerv3` with `qty = 1`, `bot_on = true`, everything else still `bot_on = false`.

**Step 3: Verify no topology drift**

Run:

```bash
curl -fsS http://127.0.0.1:5024/api/v1/params?profile=equities | jq '[.data[] | {strategy_id, qty: .params.qty, bot_on: .params.bot_on}]'
curl -fsS http://127.0.0.1:5024/api/v1/signals?profile=equities | jq '.data.strategies[] | select(.id == "aapl_tradexyz_makerv3")'
```

Expected: exactly one live canary, with the rest still disabled.

**Step 4: Commit**

```bash
git add \
  deploy/equities/README.md \
  docs/plans/2026-03-12-equities-live-trading-readiness.md
git commit -m "docs(equities): record first canary gate"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
