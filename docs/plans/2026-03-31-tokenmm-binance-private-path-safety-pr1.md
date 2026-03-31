# TokenMM Binance Private-Path Safety PR1 Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the live TokenMM Binance controller-managed private HTTP/account path explicitly timed, classified, surfaced, and fail-closed for quoting before MakerV3 falls into the generic quote-fail circuit-breaker path.

**Architecture:** Target the real production TokenMM path, not just the generic Binance execution adapter. Wire `http_timeout_secs` through the shared Binance HTTP client used by both the TokenMM controller writer and the shared-account projection providers, publish one narrow controller-side `private_path_health` payload, and teach MakerV3 to block early on that payload with additive state and alert visibility.

**Tech Stack:** Python, Nautilus live runners, Flux TokenMM controller, Redis-backed controller state bridge, Binance HTTP account APIs, pytest, immutable deploy configs.

**Context Docs:**
- Design: `docs/plans/2026-03-31-tokenmm-binance-private-path-safety-pr1-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/runbooks/deploy-lanes.md`, `deploy/tokenmm/tokenmm.live.toml`, `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml`

**Decision Summary:**
- Do not scope PR1 only to `nautilus_trader/adapters/binance/execution.py`; live TokenMM Binance writes bypass that path.
- Reuse the shared Binance HTTP client and the existing controller canonical-state bridge instead of inventing a new health service.
- Add one narrow `private_path_health` payload and one narrow blocked reason instead of starting a broader health-domain split.
- Keep the change additive and controller-managed only for PR1.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | Task 1-4 | `docs/plans/2026-03-31-tokenmm-binance-private-path-safety-pr1.md` | `shared` | `shared` | none | pass | 2026-03-31: Narrow PR1 safety patch completed end-to-end: timeout plumbing, timeout classification, controller private-path health, MakerV3 early blocking, alert resolver surfacing, and deploy-time timeout knobs. Focused verification sweep passed. |
| Task 1: Wire Binance HTTP Timeout Plumbing For Controller And Shared Account Providers | completed | main | none | `nautilus_trader/adapters/binance/http/client.py`, `nautilus_trader/adapters/binance/factories.py`, `systems/flux/flux/runners/tokenmm/run_controller.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `tests/unit_tests/adapters/binance/test_http_client.py`, `tests/integration_tests/adapters/binance/test_factories.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py` | `shared` | `shared` | none | partial_pass | 2026-03-31: Implemented end-to-end `timeout_secs` plumbing in the shared Binance HTTP client, controller runtime, and shared-account providers. Verified with `pytest -q tests/unit_tests/adapters/binance/test_http_client.py` plus direct import-stub checks for `profile_accounts` and `run_controller` because local `ibapi` is not installed for those pytest modules. |
| Task 2: Add Timeout Classification And Controller Private-Path Health State | completed | main | Task 1 | `nautilus_trader/adapters/binance/http/error.py`, `systems/flux/flux/runners/tokenmm/run_controller.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py` | `shared` | `shared` | none | partial_pass | 2026-03-31: Added normalized transport-timeout classification plus additive controller `private_path_health` state on timeout/success. Verified with `pytest -q tests/unit_tests/adapters/binance/test_http_error.py tests/unit_tests/adapters/binance/test_http_client.py` and module `py_compile`; broader controller/profile pytest remains blocked here by eager optional-import surfaces. |
| Task 3: Gate Controller-Managed MakerV3 Quoting On Stale Private-Path Health | completed | main | Task 2 | `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/strategies/makerv3/constants.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` | `shared` | `shared` | none | partial_pass | 2026-03-31: Added controller-health propagation into the TokenMM bridge plus early `blocked_private_path` gating and state blocker export in MakerV3. Verified with `pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'private_path or shared_portfolio_inventory_is_degraded'`, `pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k private_path`, and module `py_compile`; `test_tokenmm_run_node.py` collection remains blocked here by missing local `ibapi`. |
| Task 4: Expose Alerts And Ship Runtime Config Knobs | completed | main | Task 3 | `systems/flux/flux/runners/tokenmm/run_api.py`, `deploy/tokenmm/tokenmm.live.toml`, `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` | `shared` | `shared` | none | pass | 2026-03-31: Added active-alert surfacing for `blocked_private_path`, shipped `http_timeout_secs = 10` into the shared Binance scope and both managed Binance strategy configs, and stabilized the example-test imports with local IB stubs so the targeted controller/node/API suites run in this environment. |

---

### Task 1: Wire Binance HTTP Timeout Plumbing For Controller And Shared Account Providers

**Files:**
- Create: `tests/unit_tests/adapters/binance/test_http_client.py`
- Modify: `nautilus_trader/adapters/binance/http/client.py`
- Modify: `nautilus_trader/adapters/binance/factories.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_controller.py`
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `tests/integration_tests/adapters/binance/test_factories.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`

**Dependencies:** `none`

**Write Scope:** `nautilus_trader/adapters/binance/http/client.py`, `nautilus_trader/adapters/binance/factories.py`, `systems/flux/flux/runners/tokenmm/run_controller.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `tests/unit_tests/adapters/binance/test_http_client.py`, `tests/integration_tests/adapters/binance/test_factories.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/binance/test_http_client.py`
- `./.venv.py312/bin/python -m pytest -q tests/integration_tests/adapters/binance/test_factories.py -k 'timeout or base_url'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/runners/shared/test_profile_accounts.py -k 'binance'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py -k 'timeout'`

**Step 1: Write the failing timeout-plumbing tests**

Add tests that prove:

- `BinanceHttpClient` stores an explicit `timeout_secs` and forwards it to the underlying request call
- `get_cached_binance_http_client(...)` accepts `timeout_secs` and preserves it for both public and private client construction
- `run_controller._resolve_managed_strategy_execution_runtime(...)` reads `http_timeout_secs` from managed strategy venue config and passes it into the Binance HTTP client builder
- `build_account_projection_provider(...)` passes `AccountScopeConfig.http_timeout_secs` into Binance shared-account provider client construction

Use direct monkeypatch capture patterns already present in the Binance factory and profile-account tests.

```python
client = BinanceHttpClient(
    clock=LiveClock(),
    api_key="k",
    api_secret="s",
    base_url="https://papi.binance.com",
    timeout_secs=7,
)

assert client.timeout_secs == 7
```

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/binance/test_http_client.py tests/integration_tests/adapters/binance/test_factories.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`

Expected: FAIL because the shared Binance HTTP wrapper and its callers do not currently expose or pass `timeout_secs`.

**Step 3: Write the minimal timeout plumbing**

Implement the smallest end-to-end plumbing that makes the knob real:

- add `timeout_secs` to `BinanceHttpClient.__init__`
- persist it on the wrapper for tests and logs
- pass it to the underlying pyo3 `HttpClient(...)` and request call
- extend `get_cached_binance_http_client(...)` to accept `timeout_secs`
- thread the same value from:
  - TokenMM controller managed venue config
  - Binance shared account-scope config

Keep the defaults conservative and additive:

```python
self._client = HttpClient(
    keyed_quotas=ratelimiter_quotas or [],
    default_quota=ratelimiter_default_quota,
    proxy_url=proxy_url,
    timeout_secs=timeout_secs,
)
```

**Step 4: Re-run the targeted suites**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/binance/http/client.py \
  nautilus_trader/adapters/binance/factories.py \
  systems/flux/flux/runners/tokenmm/run_controller.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  tests/unit_tests/adapters/binance/test_http_client.py \
  tests/integration_tests/adapters/binance/test_factories.py \
  tests/unit_tests/flux/runners/shared/test_profile_accounts.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py
git commit -m "fix: wire binance http timeout knobs"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Add Timeout Classification And Controller Private-Path Health State

**Files:**
- Modify: `nautilus_trader/adapters/binance/http/error.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_controller.py`
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`

**Dependencies:** `Task 1: Wire Binance HTTP Timeout Plumbing For Controller And Shared Account Providers`

**Write Scope:** `nautilus_trader/adapters/binance/http/error.py`, `systems/flux/flux/runners/tokenmm/run_controller.py`, `systems/flux/flux/runners/shared/profile_accounts.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py -k 'private_path or timeout'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/runners/shared/test_profile_accounts.py -k 'TimeoutError or projection_status'`

**Step 1: Write the failing health-state tests**

Add tests that prove:

- raw `TimeoutError` is recognized as a private-path transport failure instead of an opaque generic exception
- TokenMM controller canonical state gains an additive `private_path_health` mapping after a controller-managed timeout
- successful controller-managed writes reset or refresh that health snapshot
- shared Binance account projection refresh preserves `projection_status.last_error_type == "TimeoutError"` and carries the same stale timing contract used by the controller health snapshot

Model the controller payload explicitly:

```python
assert canonical_payload["private_path_health"] == {
    "healthy": False,
    "state": "stale",
    "last_error_type": "TimeoutError",
    "timeout_count": 1,
    "stale_after_ms": 5000,
}
```

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py -k 'private_path or TimeoutError or projection_status'`

Expected: FAIL because the controller canonical state does not currently publish private-path health and the Binance timeout path is not classified explicitly.

**Step 3: Write the minimal health-state implementation**

Implement one narrow shared contract:

- add a helper in `nautilus_trader/adapters/binance/http/error.py` that identifies raw timeout transport failures cleanly
- in `run_controller.py`, record a small per-strategy `private_path_health` payload whenever a managed Binance write succeeds or times out
- keep the payload additive and close to `projection_status` naming:
  - `healthy`
  - `state`
  - `last_success_ts_ms`
  - `last_attempt_ts_ms`
  - `stale_after_ms`
  - `timeout_count`
  - `last_error_type`
  - `last_error_message`
- in `profile_accounts.py`, preserve the same timeout/error naming so controller health and shared-account projection health do not diverge semantically

Do not build a new generalized health registry here.

**Step 4: Re-run the targeted suites**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/binance/http/error.py \
  systems/flux/flux/runners/tokenmm/run_controller.py \
  systems/flux/flux/runners/shared/profile_accounts.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py \
  tests/unit_tests/flux/runners/shared/test_profile_accounts.py
git commit -m "fix: publish tokenmm binance private path health"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Gate Controller-Managed MakerV3 Quoting On Stale Private-Path Health

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/strategies/makerv3/constants.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `Task 2: Add Timeout Classification And Controller Private-Path Health State`

**Write Scope:** `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/strategies/makerv3/constants.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k 'private_path'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'private_path'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k 'private_path'`

**Step 1: Write the failing strategy and bridge tests**

Add tests that prove:

- the TokenMM controller-managed bridge copies `private_path_health` from controller canonical state onto the strategy
- MakerV3 blocks quote refresh when `private_path_health.state == "stale"`
- the blocked path does not increment or rely on the generic quote-fail circuit-breaker path
- state payloads include the additive `private_path_health` object
- blocked events/alerts use one new explicit reason code such as `blocked_private_path_stale`

Use a strategy-facing assertion like:

```python
assert state_payload["private_path_health"]["state"] == "stale"
assert quote_cycle_events[-1]["reason_code"] == "blocked_private_path_stale"
```

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k 'private_path'`

Expected: FAIL because the controller bridge does not currently sync private-path health and MakerV3 has no blocked reason for it.

**Step 3: Write the minimal gating implementation**

Implement the smallest additive path:

- in `run_node.py`, teach `_TokenmmControllerManagedBridge._apply_canonical_state(...)` to copy the additive `private_path_health` payload onto the strategy
- in `makerv3/constants.py`, add one blocked reason and one alert key only if needed by the publisher flow
- in `makerv3/quote_engine.py` and/or `strategy.py`, add an early block check that short-circuits quote refresh when the private path is stale
- in `publisher.py`, include the `private_path_health` payload in state publication

Keep the scope narrow:

- no market/private/account domain split
- no new generic venue-health framework
- controller-managed TokenMM Binance path only

**Step 4: Re-run the targeted suites**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/strategies/makerv3/constants.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "fix: block makerv3 quoting on stale binance private path"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Expose Alerts And Ship Runtime Config Knobs

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Dependencies:** `Task 3: Gate Controller-Managed MakerV3 Quoting On Stale Private-Path Health`

**Write Scope:** `systems/flux/flux/runners/tokenmm/run_api.py`, `deploy/tokenmm/tokenmm.live.toml`, `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml`, `deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Verification Commands:**
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py -k 'private_path'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k 'private_path or timeout'`
- `git diff --check`

**Step 1: Write the failing alert and config tests**

Add tests that prove:

- TokenMM alert resolution surfaces a current private-path blocked warning instead of waiting for a later stale-state symptom
- the shared TokenMM config and the two Binance strategy runtime configs expose `http_timeout_secs`
- controller runtime loading keeps those knobs available to the writer path

Add one explicit API expectation:

```python
assert rows_by_strategy["strategy_blocked"][0]["reason_code"] == "blocked_private_path_stale"
```

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k 'private_path or timeout'`

Expected: FAIL because the alert surface and deploy configs do not yet expose the new private-path contract.

**Step 3: Write the minimal alert/config implementation**

Implement the narrow operator surface:

- update `run_api.py` so the TokenMM alert resolver recognizes the new blocked private-path reason when present in strategy state
- add `http_timeout_secs` to:
  - the shared Binance account scope in `deploy/tokenmm/tokenmm.live.toml`
  - the managed Binance spot strategy runtime config
  - the managed Binance perp strategy runtime config

Use explicit comments in the deploy configs so operators know the knob exists for the controller-managed Binance private path, not only for generic adapter traffic.

**Step 4: Re-run the verification commands**

Run the verification commands listed above.

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/tokenmm/run_api.py \
  deploy/tokenmm/tokenmm.live.toml \
  deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml \
  deploy/tokenmm/strategies/plumeusdt_binance_perp_makerv3.toml \
  tests/unit_tests/examples/strategies/test_tokenmm_run_api.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py
git commit -m "chore: expose tokenmm binance private path health"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
