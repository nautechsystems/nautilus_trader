# Equities Live Trading Readiness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Promote the equities stack from "dashboard/live-readonly with partial correctness" to a production-safe live canary where Hyperliquid execution, IBKR reference/hedge state, balances, portfolio, and operational controls are all correct enough to enable real trading on a tightly scoped stock subset.

**Architecture:** Continue the profile-owned control-plane direction from `2026-03-11-equities-hl-vs-ibkr-go-to-prod-plan.md`, but close the runtime gaps exposed by production validation. The clean design is: canonical `strategy_contracts` for per-strategy asset identity, shared `account_scopes` for profile-owned venue account providers, strategy-local inventory publication under canonical `portfolio_asset_id`, and a single equities portfolio snapshot that the API and GUI consume. Treat IBKR gateway lifecycle as a shared operational service, not as an accidental side effect of dozens of node configs.

**Tech Stack:** Python (Flux runners/API/strategy code), Redis/ElastiCache, Nautilus Trader adapters (Hyperliquid, IBKR), TOML deploy config, systemd/Pulse, pytest, journald, AWS ElastiCache, Fluxboard.

## Continuation Context

This plan resumes after `docs/plans/2026-03-11-equities-hl-vs-ibkr-go-to-prod-plan.md`.

Status of the earlier plan:
- Tasks 1-6 are implemented on branch `codex/equities-live-pr`.
- The production equities Redis endpoint is now confirmed and documented as the ElastiCache replication group `equities` with primary `master.equities.wapqos.apse1.cache.amazonaws.com:6379`.
- Live `/equities` and `/api/v1/signals?profile=equities` are up from the isolated worktree.
- Production runtime validation on March 12, 2026 exposed two additional control-plane gaps that block balances correctness and trading readiness.

## Research Findings Driving This Plan

1. The live node fleet is using the production ElastiCache `equities` Redis endpoint from the isolated worktree, not local Redis. Evidence: systemd process environment shows `EQUITIES_REDIS_HOST=master.equities.wapqos.apse1.cache.amazonaws.com` for `equities-api`, `equities-portfolio`, `equities-bridge`, and sample node services.
2. The public equities API is only partially healthy. As of March 12, 2026 UTC, `GET /api/v1/signals?profile=equities` shows `24` strategies, `22` reference legs healthy, `15` maker legs healthy, and `15` strategies with both legs healthy. That is not good enough for live-trading enablement.
3. `GET /api/v1/balances?profile=equities` is still `source = "portfolio_snapshot_v2"`, `degraded = true`, `missing_required_count = 24`, and returns only a single shared Hyperliquid `USDC` row. So the API routing change is live, but the live data feeding it is still wrong.
4. Production equities Redis proves the component-key mismatch is still active. It contains keys like `flux:v1:portfolio:inventory:component:equities:XYZ:AAPL:aapl_tradexyz_makerv3`, while the portfolio snapshot aggregates under canonical asset keys such as `flux:v1:portfolio:inventory:equities:AAPL`. The canonical component key `...:component:equities:AAPL:aapl_tradexyz_makerv3` is empty.
5. The root cause for that mismatch is runtime config merge drift. `run_node._load_runtime_config(...)` only merges shared `redis` and `portfolio`, but `_optional_strategy_config_kwargs(...)` expects `strategy_contracts` to still exist so it can inject `portfolio_asset_id`. Because `strategy_contracts` never reaches the live node runtime, MakerV3 falls back to maker-base identity such as `XYZ:AAPL`.
6. The profile-account projection path is also not actually wired in production. `run_portfolio.py` builds bindings from `strategy_contracts`, but the provider factory currently expects `config["node"]["venues"]["IBKR"]`. Shared `equities.live.toml` has no `node` table, so the bindings exist but every provider is `None`. Production Redis contains no `profile_account_projection` keys.
7. Per-strategy balances snapshots in production currently contain only Hyperliquid account events. Existing IBKR holdings therefore cannot appear in `profile=equities` balances even when IBKR auth is healthy.
8. The current IBKR 2FA behavior is operationally wrong for production. Strategy configs still embed nightly restart plus auto-retry-after-timeout semantics, which causes repeated push prompts after a missed approval window. One shared service should own IBKR auth state and missed 2FA should fail closed, not spam-retry.

## Design Choice

### Recommended approach

Introduce a **shared equities account-scope contract** and complete the profile-owned control plane before enabling any live trading:

1. Add explicit shared `account_scopes` config in `deploy/equities/equities.live.toml` for:
   - `hyperliquid.xyz.main`
   - `ibkr.reference.main`
   - `ibkr.hedge.main`
2. Make node runtime merge both `strategy_contracts` and `account_scopes`, so every strategy receives canonical `portfolio_asset_id` and can publish inventory under the same identity the portfolio aggregator reads.
3. Make the portfolio runner own account providers from `account_scopes`, not by scraping per-node `node.venues.*`.
4. Use the portfolio runner as the only source of shared account projections and shared-account balance rows.
5. Split IBKR gateway ownership from strategy configs so missed 2FA exits cleanly and does not create retry spam.
6. Only after balances and readiness are correct, enable a one- or two-symbol live canary with `bot_on=true`.

### Alternatives considered

1. **Patch only the node merge table list.**
   - Pros: fastest way to make canonical inventory keys appear.
   - Cons: still leaves shared IBKR/HL account projections unowned and undocumented. Balances would remain incomplete.
2. **Let `run_portfolio` load one representative per-node TOML and scrape its `node.venues` config.**
   - Pros: smaller code diff than adding `account_scopes`.
   - Cons: wrong abstraction. Shared account providers would still depend on arbitrary strategy files and become brittle as MakerV3 and MakerV4 coexist.
3. **Recommended: add dedicated shared `account_scopes` in deploy config.**
   - Pros: correct ownership boundary, supports multiple strategy families, documents production credentials and gateway policy once, and cleanly separates shared accounts from per-strategy local inventory.

## Redesigns Required Before Live Trading

1. Shared venue account providers must be configured from profile-owned `account_scopes`, not scraped from one strategy's node config.
2. Node runtime must carry `strategy_contracts` into the live strategy config path.
3. IBKR gateway lifecycle must move from per-strategy config duplication toward one shared control-plane owner.
4. Readiness must be explicit. Do not infer "safe to trade" from a green-looking GUI.
5. Live enablement must be staged:
   - data/auth correctness
   - balances/portfolio correctness
   - execution safety checks
   - one-symbol canary
   - broader rollout

## Acceptance Criteria

1. Production equities Redis contains canonical component keys like `flux:v1:portfolio:inventory:component:equities:AAPL:aapl_tradexyz_makerv3`; legacy `XYZ:AAPL` keys are gone or ignored.
2. `GET /api/v1/balances?profile=equities` returns non-cash rows for existing IBKR holdings when they exist, with `degraded = false` and `missing_required_count = 0`.
3. Production equities Redis contains `profile_account_projection` keys for the configured shared account scopes.
4. `/equities/balances` shows shared-account rows with explicit provenance fields and fresh timestamps.
5. IBKR 2FA no longer auto-spams after a missed approval window.
6. The live readiness check fails closed when:
   - IBKR auth is down
   - canonical component keys are missing
   - profile account projections are missing
   - balances are degraded
   - too many legs are stale
7. A one-symbol live canary can be enabled without changing the shared control plane again.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | not_started | unassigned | Plan created |
| Task 1: Add Shared Account Scope Contract | not_started | unassigned | Plan created |
| Task 2: Fix Node Runtime Shared-Config Merge | not_started | unassigned | Plan created |
| Task 3: Fix Profile Account Provider Wiring | not_started | unassigned | Plan created |
| Task 4: Reconcile Live Balances And Portfolio Snapshot Inputs | not_started | unassigned | Plan created |
| Task 5: Redesign IBKR Gateway Ownership And 2FA Policy | not_started | unassigned | Plan created |
| Task 6: Add Equities Live Readiness Gate | not_started | unassigned | Plan created |
| Task 7: Execute Read-Only Production Verification | not_started | unassigned | Plan created |
| Task 8: Enable Controlled Live Trading Canary | not_started | unassigned | Plan created |

---

### Task 1: Add Shared Account Scope Contract

**Files:**
- Create: `systems/flux/flux/common/account_scopes.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/systemd/common.env.example`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Test: `tests/unit_tests/flux/common/test_account_projection.py`

**Step 1: Write the failing tests**

Add config-contract tests that require shared `account_scopes` in the deploy config:

```python
def test_equities_live_config_declares_shared_account_scopes() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    scopes = {row["scope_id"]: row for row in config["account_scopes"]}
    assert scopes["ibkr.reference.main"]["provider"] == "ibkr"
    assert scopes["hyperliquid.xyz.main"]["provider"] == "hyperliquid"
```

```python
def test_account_scope_decoder_requires_provider_and_scope_id() -> None:
    with pytest.raises(ValueError):
        decode_account_scopes([{"scope_id": "ibkr.reference.main"}])
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/common/test_account_projection.py -p no:rerunfailures
```

Expected: FAIL because no shared `account_scopes` contract exists yet.

**Step 3: Write the minimal implementation**

Create a shared profile-owned account-scope model, for example:

```python
@dataclass(frozen=True, slots=True)
class AccountScopeConfig:
    scope_id: str
    provider: str
    venue: str
    ibg_host: str | None = None
    ibg_port: int | None = None
    ibg_client_id: int | None = None
    dockerized_gateway: dict[str, Any] | None = None
    account_id: str | None = None
```

Add `[[account_scopes]]` rows in `deploy/equities/equities.live.toml` so shared IBKR and Hyperliquid account providers are configured once in the shared profile contract instead of being inferred from per-node TOMLs.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus:

```bash
rg -n "account_scopes|ibkr.reference.main|hyperliquid.xyz.main" \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  deploy/equities/systemd/common.env.example
```

Expected: PASS with one documented shared-account contract.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/account_scopes.py \
  deploy/equities/equities.live.toml \
  deploy/equities/README.md \
  deploy/equities/systemd/common.env.example \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/common/test_account_projection.py
git commit -m "design: add shared equities account scope contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Fix Node Runtime Shared-Config Merge

**Files:**
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/shared/bootstrap.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`

**Step 1: Write the failing tests**

Add regression coverage for runtime merge and canonical asset injection:

```python
def test_load_runtime_config_keeps_strategy_contracts_and_account_scopes() -> None:
    config = _load_runtime_config(strategy_path, shared_config_path=shared_path)
    assert config["strategy_contracts"]
    assert config["account_scopes"]
```

```python
def test_optional_strategy_config_kwargs_injects_portfolio_asset_id_from_shared_contract() -> None:
    kwargs = _optional_strategy_config_kwargs(...)
    assert kwargs["portfolio_asset_id"] == "AAPL"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -k "portfolio_asset_id or shared_config" -p no:rerunfailures
```

Expected: FAIL because live node runtime currently drops `strategy_contracts`.

**Step 3: Write the minimal implementation**

Extend the shared-runtime merge so node runtime preserves:
- `redis`
- `portfolio`
- `strategy_contracts`
- `account_scopes`

Keep the merge explicit and equities-scoped; do not blindly merge every shared table.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus the focused runtime slice used during live debugging.

Expected: PASS and local runtime reproduction shows `portfolio_asset_id="AAPL"` reaches MakerV3.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/shared/bootstrap.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py
git commit -m "fix: merge shared equities contracts into node runtime"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Fix Profile Account Provider Wiring

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/runners/equities/run_portfolio.py`
- Modify: `systems/flux/flux/common/account_projection.py`
- Test: `tests/unit_tests/flux/common/test_account_projection.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Step 1: Write the failing tests**

Add tests proving the provider is built from shared `account_scopes`, not `node.venues`:

```python
def test_build_profile_account_provider_bindings_uses_shared_account_scopes() -> None:
    bindings = build_profile_account_provider_bindings(config=config)
    ibkr = next(b for b in bindings if b.account_scope_id == "ibkr.reference.main")
    assert ibkr.provider is not None
```

```python
def test_profile_account_projection_publishes_rows_for_ibkr_scope() -> None:
    snapshot = build_profile_account_snapshot(...)
    assert any(row["account_scope_id"] == "ibkr.reference.main" for row in snapshot["rows"])
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k "account_scope or projection" -p no:rerunfailures
```

Expected: FAIL because current bindings are created with `provider=None`.

**Step 3: Write the minimal implementation**

Teach `build_profile_account_provider_bindings()` to decode the shared `account_scopes` table and create providers from those rows. Do not depend on `config["node"]` in the portfolio process.

Preserve deduping by `account_scope_id` and keep one provider per shared venue account.

**Step 4: Run tests to verify they pass**

Run the same pytest command plus a focused portfolio runner slice:

```bash
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k "projection" -p no:rerunfailures
```

Expected: PASS and local runner checks show non-null IBKR provider bindings.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/profile_accounts.py \
  systems/flux/flux/runners/equities/run_portfolio.py \
  systems/flux/flux/common/account_projection.py \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "feat: build equities account projections from shared account scopes"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Reconcile Live Balances And Portfolio Snapshot Inputs

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing tests**

Add regressions for canonical component publish plus shared-account row display:

```python
def test_makerv3_publishes_canonical_portfolio_component_key() -> None:
    assert redis_client.get(component_key_for("AAPL", "aapl_tradexyz_makerv3")) is not None
```

```python
def test_equities_balances_profile_renders_ibkr_shared_rows_with_provenance() -> None:
    row = body["data"]["rows"][0]
    assert row["source_scope"] == "shared_account"
    assert row["account_scope_id"] == "ibkr.reference.main"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py -k "shared_account or portfolio_component" -p no:rerunfailures
```

Expected: FAIL against the current live bug pattern.

**Step 3: Write the minimal implementation**

After Task 2 and Task 3 land, remove any remaining legacy assumptions that keep balances empty:
- ensure MakerV3 component writes use the canonical asset key only
- ensure portfolio snapshot rows preserve shared-account provenance
- ensure balances payload prefers shared account projections when available

Do not reintroduce strategy-owned IBKR rows.

**Step 4: Run tests to verify they pass**

Run the same pytest command and a focused balances slice:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py -k "profile=equities and balances" -p no:rerunfailures
```

Expected: PASS and no legacy `XYZ:*` portfolio component dependency remains in tests.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/common/portfolio_snapshot.py \
  systems/flux/flux/api/_payloads_balances.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "fix: reconcile equities balances with canonical live inputs"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Redesign IBKR Gateway Ownership And 2FA Policy

**Files:**
- Modify: `nautilus_trader/adapters/interactive_brokers/gateway.py`
- Modify: `nautilus_trader/adapters/interactive_brokers/factories.py`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/strategies/*.toml`
- Modify: `deploy/equities/README.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Test: `tests/unit_tests/nautilus_trader/adapters/interactive_brokers/test_gateway.py`

**Step 1: Write the failing tests**

Add coverage for the new policy:

```python
def test_equities_ibkr_gateway_policy_does_not_auto_retry_after_twofa_timeout() -> None:
    cfg = load_equities_ibkr_gateway_policy(...)
    assert cfg.relogin_after_twofa_timeout is False
    assert cfg.twofa_timeout_action == "exit"
```

```python
def test_shared_gateway_owner_is_configured_once_for_equities() -> None:
    assert equities_gateway_owner(config) == "ibkr.reference.main"
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/nautilus_trader/adapters/interactive_brokers/test_gateway.py -p no:rerunfailures
```

Expected: FAIL because current configs still auto-retry on missed 2FA.

**Step 3: Write the minimal implementation**

Redesign the equities IBKR policy so:
- one shared service owns gateway lifecycle
- missed 2FA exits and waits for operator action
- strategy nodes fail closed on auth loss instead of generating push spam

Do not rely on one gateway-start config duplicated across 24 node TOMLs.

**Step 4: Run tests to verify they pass**

Run the same pytest command and inspect the checked-in deploy config:

```bash
rg -n "relogin_after_twofa_timeout|twofa_timeout_action|auto_restart_time" \
  deploy/equities/equities.live.toml \
  deploy/equities/strategies
```

Expected: PASS and the deploy contract documents non-spammy IBKR behavior.

**Step 5: Commit**

```bash
git add \
  nautilus_trader/adapters/interactive_brokers/gateway.py \
  nautilus_trader/adapters/interactive_brokers/factories.py \
  deploy/equities/equities.live.toml \
  deploy/equities/strategies \
  deploy/equities/README.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/nautilus_trader/adapters/interactive_brokers/test_gateway.py
git commit -m "refactor: centralize equities ibkr gateway ownership"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Add Equities Live Readiness Gate

**Files:**
- Create: `systems/flux/flux/runners/equities/readiness.py`
- Create: `ops/scripts/deploy/check_equities_live_readiness.sh`
- Modify: `deploy/equities/README.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Step 1: Write the failing tests**

Add checks that fail closed when balances or projections are missing:

```python
def test_equities_readiness_fails_when_profile_account_projection_missing() -> None:
    result = evaluate_equities_readiness(...)
    assert result.ok is False
    assert "profile_account_projection" in result.failures
```

```python
def test_equities_readiness_fails_when_balances_degraded() -> None:
    result = evaluate_equities_readiness(...)
    assert "balances_degraded" in result.failures
```

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py -p no:rerunfailures
```

Expected: FAIL because no readiness evaluator exists yet.

**Step 3: Write the minimal implementation**

Create a small readiness module and operator script that checks:
- IBKR auth healthy
- canonical component keys present
- profile account projections present
- balances not degraded
- acceptable stale-leg threshold
- zero required strategies missing

Keep it read-only and easy to run from the live host.

**Step 4: Run tests to verify they pass**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_readiness.py -p no:rerunfailures
bash -n ops/scripts/deploy/check_equities_live_readiness.sh
```

Expected: PASS and the script is syntax-clean.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/equities/readiness.py \
  ops/scripts/deploy/check_equities_live_readiness.sh \
  deploy/equities/README.md \
  tests/unit_tests/examples/strategies/test_equities_readiness.py
git commit -m "feat: add equities live readiness gate"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Execute Read-Only Production Verification

**Files:**
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Create: `docs/reviews/2026-03-12-equities-live-readiness-review.md`

**Step 1: Deploy the control-plane fixes from the worktree**

Run:

```bash
sudo systemctl restart flux@equities-portfolio.service
sudo systemctl restart flux@equities-bridge.service
sudo systemctl restart 'flux@equities-node-*.service'
sudo systemctl restart flux@equities-api.service
```

Expected: services restart from `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr`.

**Step 2: Run the readiness gate and collect evidence**

Run:

```bash
ops/scripts/deploy/check_equities_live_readiness.sh
curl -fsS http://127.0.0.1:5022/api/v1/signals?profile=equities
curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=equities
```

Expected: readiness is green in read-only mode, balances are non-degraded, and shared-account rows are present.

**Step 3: Write the readiness review**

Capture:
- signals summary
- balances summary
- Redis key evidence
- IBKR auth state
- known residual risks

**Step 4: Commit**

```bash
git add \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/reviews/2026-03-12-equities-live-readiness-review.md
git commit -m "docs: record equities live readiness review"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 8: Enable Controlled Live Trading Canary

**Files:**
- Modify: `deploy/equities/strategies/aapl_tradexyz_makerv3.toml`
- Modify: `deploy/equities/strategies/nvda_tradexyz_makerv3.toml`
- Modify: `deploy/equities/README.md`
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Create: `docs/reviews/2026-03-12-equities-canary-trading-review.md`

**Step 1: Keep full universe read-only and choose the canary set**

Recommended initial canary:
- `aapl_tradexyz_makerv3`
- optional second-stage `nvda_tradexyz_makerv3`

Do not enable the full 24-stock universe first.

**Step 2: Write the failing contract test**

Add a deploy-contract test that allows only the selected canaries to set `bot_on = true` in the live config revision for the first rollout.

**Step 3: Enable the first canary with minimal risk**

Set:
- tiny venue qty
- explicit `bot_on = true` only for the first canary
- all other strategies remain `bot_on = false`

Restart only the canary node and supporting shared services if needed.

**Step 4: Verify live behavior and record review**

Check:
- order placement/cancel behavior
- fills and positions
- balances update
- no IBKR spam
- no new stale-leg regressions

Record the results in `docs/reviews/2026-03-12-equities-canary-trading-review.md`.

**Step 5: Commit**

```bash
git add \
  deploy/equities/strategies/aapl_tradexyz_makerv3.toml \
  deploy/equities/strategies/nvda_tradexyz_makerv3.toml \
  deploy/equities/README.md \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/reviews/2026-03-12-equities-canary-trading-review.md
git commit -m "feat: enable equities live trading canary"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
