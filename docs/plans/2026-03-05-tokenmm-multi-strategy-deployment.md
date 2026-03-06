# TokenMM Five-Strategy Fluxboard Params/Balances/Trades Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **Execution mode:** Do **not** use `superpowers:subagent-driven-development`. Implement directly in the current worktree, in small batches, with normal local verification between batches.

**Goal:** Make Fluxboard `params`, `balances`, and `trades` show the 5 allowlisted PLUME MakerV3 strategies on the existing `makerv3` deployment, without widening scope to new strategy families or unrelated profile surfaces.

**Architecture:** `deploy/tokenmm/tokenmm.live.toml` `[api].tokenmm_strategy_ids` remains the single source of truth for the 5-node TokenMM set. `params` and `trades` remain multi-strategy views that preserve per-strategy identity, while `balances` remains the shared-portfolio view tagged to `tokenmm`. `strategy=` must continue to override `profile=tokenmm` for per-strategy debug/ops workflows.

**Tech Stack:** Flask Flux API, Redis-backed Flux stores, existing Fluxboard frontend, MakerV3 live runners/scripts, `pytest`, shell smoke checks.

---

## Scope

**In scope**

1. `GET /api/v1/params?profile=tokenmm` returns the 5 allowlisted strategies in registry order.
2. `GET /api/v1/balances?profile=tokenmm` returns the shared TokenMM portfolio view plus component health.
3. `GET /api/v1/trades?profile=tokenmm` returns the merged 5-strategy blotter, preserving source `strategy_id` per row.
4. `GET /api/v1/trades/delta?profile=tokenmm` may remain as the existing safe/no-global-cursor contract if the current Fluxboard trades surface depends on it.
5. Stack/runbook validation must prove those three surfaces work for the 5 configured PLUME MakerV3 nodes.

**Out of scope**

1. Adding more strategies or new strategy families.
2. Equities, hedgers, rebalancers, arbitrage, or any Phase 2 topology work.
3. New `flux:v2:*` stream routing.
4. New Signal/Alert product work.
5. Any “shared risk engine across nodes” design changes.

## Scope decision for the current branch

This plan **supersedes** the earlier “balances only” Phase 1 decision:

1. `params`, `balances`, and `trades` are now the required TokenMM profile surfaces.
2. `signals` and `alerts` are **not** part of this plan and should not be expanded further.
3. If the current tree already contains wider profile fanout for `signals`/`alerts`, do not add more work there in this execution unless it is required to keep the requested `params`/`balances`/`trades` behavior correct.

## Acceptance criteria

1. `GET /api/v1/params?profile=tokenmm` returns exactly the 5 allowlisted strategy payloads from `[api].tokenmm_strategy_ids`, in that order, with each payload preserving its own `strategy_id`.
2. `GET /api/v1/balances?profile=tokenmm` returns a shared portfolio tagged to `strategy_id="tokenmm"`, includes `components`, `missing_required`, `degraded`, and uses the configured required subset semantics.
3. `GET /api/v1/trades?profile=tokenmm` returns merged rows across the 5 allowlisted strategies, ordered deterministically by timestamp/seq/strategy/row_id, with each row retaining the originating `strategy_id`.
4. `strategy=` continues to force a single-strategy view for `params`, `balances`, and `trades`, even when `profile=tokenmm` is supplied.
5. Stack health/runbook checks validate `params` and `balances`, and document how to validate `trades` safely in a non-trading smoke run.
6. No new API or Socket.IO surface is added beyond what is needed for the three requested pages.

## Review notes to carry into implementation

1. **Keep the scope tight.** The current tree has already drifted beyond the older Phase 1 note by widening non-requested `signals`/`alerts` profile behavior. Do not widen that further in this execution.
2. **Known socket alert bug exists outside scope.** `nautilus_trader/flux/api/socketio.py` currently computes alerts from the first strategy only while REST alerts aggregate multiple strategies. Since alerts are out of scope here, do not pull that bug into this task unless your changes touch that code path.
3. **Live-default safety is a concern.** `scripts/deploy/tokenmm_stack.sh` and `deploy/tokenmm/tokenmm_stack.env.example` currently default to `live` + execution enabled. Do not make this more permissive, and use explicit paper/test overrides for smoke verification.
4. **Shared-account uniqueness still matters.** If you touch the stack/config validation path, validate both `[identity].strategy_id` and `[strategy].strategy_id` uniqueness across `deploy/tokenmm/strategies/`.
5. **Position merge assumption must remain explicit.** `balances` currently net positions by `(exchange, instrument)` across strategies. That is acceptable only if the published rows are strategy-scoped rather than full-account duplicates.

## Files that are likely in play

- `deploy/tokenmm/tokenmm.live.toml`
- `nautilus_trader/flux/runners/tokenmm/run_api.py`
- `nautilus_trader/flux/runners/tokenmm/run_node.py`
- `deploy/tokenmm/README.md`
- `deploy/tokenmm/strategies/README.md`
- `deploy/tokenmm/tokenmm_stack.env.example`
- `nautilus_trader/flux/api/app.py`
- `nautilus_trader/flux/api/payloads.py`
- `nautilus_trader/flux/api/socketio.py`
- `scripts/deploy/tokenmm_stack.sh`
- `tests/unit_tests/flux/api/test_app.py`
- `tests/unit_tests/flux/api/test_payloads.py`
- `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

## Task 1: Lock the TokenMM params contract

**Files**

- Modify: `nautilus_trader/flux/api/app.py`
- Modify: `nautilus_trader/flux/runners/tokenmm/run_api.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Modify: `deploy/tokenmm/README.md`

**Step 1: Add/adjust failing tests for the exact params behavior**

Add or tighten tests so they prove:

```python
response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
assert [item["strategy_id"] for item in response.json["data"]] == api_cfg["tokenmm_strategy_ids"]
```

and:

```python
response = client.get("/api/v1/params", query_string={"profile": "tokenmm", "strategy": "strategy_02"})
assert [item["strategy_id"] for item in response.json["data"]] == ["strategy_02"]
```

**Step 2: Run the targeted tests**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py
```

Expected: a failing assertion if current behavior/order/allowlist handling is wrong.

**Step 3: Implement the minimal fix**

- Keep `[api].tokenmm_strategy_ids` as the source of truth.
- Preserve registry order.
- Keep `strategy=` precedence over `profile=tokenmm`.
- Do not expand `signals`/`alerts` while touching this path.

**Step 4: Re-run the same targeted tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  nautilus_trader/flux/api/app.py \
  nautilus_trader/flux/runners/tokenmm/run_api.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  deploy/tokenmm/README.md
git commit -m "feat: lock tokenmm params profile contract"
```

## Task 2: Lock the TokenMM balances portfolio contract

**Files**

- Modify: `nautilus_trader/flux/api/app.py`
- Modify: `nautilus_trader/flux/api/payloads.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `deploy/tokenmm/README.md`

**Step 1: Add/adjust failing tests for shared-portfolio balances**

Add or tighten tests so they prove:

```python
response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
assert all(row["strategy_id"] == "tokenmm" for row in response.json["data"]["rows"])
assert response.json["data"]["missing_required"] == []
assert "components" in response.json["data"]
```

and preserve the existing position-netting contract only if rows are strategy-scoped:

```python
position = rows_by_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
assert position["strategy_id"] == "tokenmm"
```

**Step 2: Run the targeted tests**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_payloads.py
```

Expected: a failing assertion if aggregation, tagging, degraded semantics, or required-set behavior is wrong.

**Step 3: Implement the minimal fix**

- Preserve the portfolio aggregate tagged to `tokenmm`.
- Keep `components`, `missing_required`, and `degraded`.
- Do not redesign shared-risk behavior.
- If you touch position aggregation, keep the assumption explicit in tests and docs.

**Step 4: Re-run the same targeted tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  nautilus_trader/flux/api/app.py \
  nautilus_trader/flux/api/payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_payloads.py \
  deploy/tokenmm/README.md
git commit -m "feat: lock tokenmm balances portfolio contract"
```

## Task 3: Lock the TokenMM trades blotter contract

**Files**

- Modify: `nautilus_trader/flux/api/app.py`
- Modify: `nautilus_trader/flux/api/socketio.py` (only if required for the existing trades page)
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_socketio_tokenmm.py` (only if Task 3 changes Socket.IO behavior)
- Modify: `deploy/tokenmm/README.md`

**Step 1: Add/adjust failing tests for the 5-strategy trades view**

Add or tighten tests so they prove:

```python
response = client.get("/api/v1/trades", query_string={"profile": "tokenmm"})
assert {row["strategy_id"] for row in response.json["data"]["rows"]} <= set(api_cfg["tokenmm_strategy_ids"])
```

and deterministic merged ordering:

```python
assert [(row["ts_ms"], row["strategy_id"], row["row_id"]) for row in rows] == expected_order
```

and debug override precedence:

```python
response = client.get("/api/v1/trades", query_string={"profile": "tokenmm", "strategy": "strategy_02"})
assert {row["strategy_id"] for row in response.json["data"]["rows"]} == {"strategy_02"}
```

If the current Fluxboard trades page depends on `trades/delta`, also keep a test that the TokenMM profile path advertises no synthetic global cursor:

```python
assert response.json["data"]["last_seq"] == 0
```

**Step 2: Run the targeted tests**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py
```

Expected: a failing assertion if merged ordering, source identity, or per-strategy override is wrong.

**Step 3: Implement the minimal fix**

- Keep `trades` as the merged 5-strategy blotter.
- Preserve row-level `strategy_id`.
- Preserve deterministic sort.
- Do not add any new Signal/Alert work while touching this path.
- Touch `socketio.py` only if the existing trades page actually requires it.

**Step 4: Re-run the same targeted tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  nautilus_trader/flux/api/app.py \
  nautilus_trader/flux/api/socketio.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py \
  deploy/tokenmm/README.md
git commit -m "feat: lock tokenmm trades profile contract"
```

## Task 4: Tighten runbook and stack verification for the 5-node TokenMM set

**Files**

- Modify: `scripts/deploy/tokenmm_stack.sh`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/strategies/README.md`
- Modify: `deploy/tokenmm/tokenmm_stack.env.example`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Add/adjust failing tests for ops contract**

Add or tighten tests so they prove:

```python
assert "GET /api/v1/params?profile=tokenmm" in readme_or_script
assert "GET /api/v1/balances?profile=tokenmm" in readme_or_script
```

If you touch stack validation, also add a contract test for unique Nautilus strategy IDs across `deploy/tokenmm/strategies/`.

**Step 2: Run the targeted tests**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
bash -n scripts/deploy/tokenmm_stack.sh
```

Expected: a failing assertion or shell check if the runbook/validation contract is incomplete.

**Step 3: Implement the minimal fix**

- Keep health/readiness centered on `params` + `balances`.
- Document a safe non-trading smoke validation for `trades`.
- If you touch validation logic, add the missing `[strategy].strategy_id` uniqueness guard.
- Do not make live defaults more permissive than they already are.

**Step 4: Re-run the same targeted tests**

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  scripts/deploy/tokenmm_stack.sh \
  deploy/tokenmm/README.md \
  deploy/tokenmm/strategies/README.md \
  deploy/tokenmm/tokenmm_stack.env.example \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs: tighten tokenmm 5-node runbook and validation"
```

## Task 5: Full regression and safe smoke validation

**Files**

- No new product files expected
- Update docs/tests only if verification exposes a real gap

**Step 1: Run the focused regression suite**

Run:

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
```

Expected: PASS.

**Step 2: Run the Flux leakage gate**

Run:

```bash
scripts/ci/check-flux-leakage.sh
```

Expected: `[flux-leakage] OK`

**Step 3: Run a safe local stack smoke**

Run:

```bash
TOKENMM_MODE=paper \
TOKENMM_CONFIRM_LIVE=0 \
TOKENMM_ENABLE_EXECUTION=0 \
TOKENMM_ALLOW_MISSING_KEYS=1 \
scripts/deploy/tokenmm_stack.sh start
```

Then verify:

```bash
curl -fsS http://127.0.0.1:5022/api/v1/params?profile=tokenmm
curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=tokenmm
curl -fsS http://127.0.0.1:5022/api/v1/trades?profile=tokenmm
scripts/deploy/tokenmm_stack.sh stop
```

Expected:

1. `params` returns 5 strategy payloads.
2. `balances` returns `degraded=false` only when required components are fresh.
3. `trades` returns rows only from the allowlisted strategy IDs.

**Step 4: Commit any final doc/test-only fixes**

```bash
git add -A
git commit -m "test: verify tokenmm params balances trades rollout"
```

## Production bar

Do not call this done unless all of the following are true:

1. `params`, `balances`, and `trades` all work for the 5 configured PLUME MakerV3 strategies.
2. `strategy=` override still works for per-strategy operations.
3. No new Signal/Alert scope was introduced.
4. No new live-trading defaults were introduced.
5. Verification commands above were actually run and their output was checked.

## Verification checklist

- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/api/test_app.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/api/test_payloads.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- `bash -n scripts/deploy/tokenmm_stack.sh`
- `scripts/ci/check-flux-leakage.sh`
- Paper-mode stack smoke for `params`, `balances`, `trades`

## Prompt for the executing agent

Copy/paste this into the execution session:

```text
You are in `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr`.

Use `superpowers:using-superpowers`, then `superpowers:executing-plans`, then `superpowers:test-driven-development`, and `superpowers:verification-before-completion`.

Implement `docs/plans/2026-03-05-tokenmm-multi-strategy-deployment.md` directly in this worktree. Do not use `superpowers:subagent-driven-development`. Implement in small batches and stop after each batch for review only if you hit a real blocker; otherwise continue through the plan.

Scope is strict:
- Make Fluxboard `params`, `balances`, and `trades` show the 5 allowlisted PLUME MakerV3 strategies.
- Keep `deploy/tokenmm/tokenmm.live.toml` `[api].tokenmm_strategy_ids` as the source of truth.
- `params` and `trades` must preserve per-row/per-payload `strategy_id`.
- `balances` must remain the shared `tokenmm` portfolio view.
- `strategy=` must keep overriding `profile=tokenmm`.

Do not widen scope to:
- signals
- alerts
- new strategies
- equities
- stream schema migrations

Carry these review notes into the implementation:
- The current tree already drifted wider than the original Phase 1 note; do not expand that drift.
- There is a known Socket.IO alerts bug outside this scope; avoid pulling alerts into this task unless you must touch that path.
- Do not make `tokenmm_stack.sh` or env defaults more permissive; use paper-mode overrides for smoke checks.
- If you touch stack validation, add `[strategy].strategy_id` uniqueness validation across `deploy/tokenmm/strategies/`.
- Keep the balances position-netting assumption explicit in tests/docs.

Required verification before claiming success:
- run the targeted pytest files from the plan
- run `bash -n scripts/deploy/tokenmm_stack.sh`
- run `scripts/ci/check-flux-leakage.sh`
- run a paper-mode stack smoke and curl `params`, `balances`, and `trades`

No pushes. No extra features. Keep the diff tight.
```
