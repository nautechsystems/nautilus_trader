# TokenMM Strategy Isolation And Shutdown Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Prevent a strategy from submitting orders outside its owned instruments and make TokenMM live nodes stop or crash out through the graceful market-exit path.

**Architecture:** Add a reusable instrument submit-allowlist at the `Strategy` boundary, then wire MakerV3/TokenMM to use it for the maker instrument only. Separately, enable live-node graceful shutdown defaults so process stop and internal engine exceptions route through `stop()` and market exit instead of hard termination behavior.

**Tech Stack:** Cython strategy core, Python TokenMM runner, pytest unit tests.

---

### Task 1: Lock the Strategy submission boundary

**Files:**
- Modify: `nautilus_trader/trading/config.py`
- Modify: `nautilus_trader/trading/strategy.pyx`
- Modify: `nautilus_trader/trading/strategy.pxd`
- Test: `tests/unit_tests/trading/test_strategy.py`

**Step 1: Write the failing tests**

Add tests that show:
- a strategy with an explicit allowed instrument list denies `submit_order()` for a different instrument
- the same strategy denies `submit_order_list()` when any order targets a different instrument
- market-exit reduce-only orders are still allowed during exit so cleanup can complete

**Step 2: Run test to verify it fails**

Run: `pytest tests/unit_tests/trading/test_strategy.py -k "allowed_instrument or market_exit"`
Expected: FAIL because no submit allowlist exists yet.

**Step 3: Write minimal implementation**

Add a config field for allowed submit instruments, parse it on strategy init, and deny non-exit submissions outside that allowlist in `submit_order()` and `submit_order_list()`.

**Step 4: Run test to verify it passes**

Run: `pytest tests/unit_tests/trading/test_strategy.py -k "allowed_instrument or market_exit"`
Expected: PASS.

### Task 2: Enable TokenMM graceful stop/crash defaults

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Write the failing tests**

Add tests that show `build_node()` sets:
- `manage_stop=True` on the MakerV3 strategy unless explicitly disabled
- `graceful_shutdown_on_exception=True` on live data/risk/exec engines unless explicitly disabled
- the MakerV3 allowed instrument list contains only the execution instrument

**Step 2: Run test to verify it fails**

Run: `pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k "manage_stop or graceful_shutdown or allowed_submit"`
Expected: FAIL because the runner does not currently wire these defaults.

**Step 3: Write minimal implementation**

Populate those fields in `MakerV3StrategyConfig` and `TradingNodeConfig` creation inside `build_node()`.

**Step 4: Run test to verify it passes**

Run: `pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k "manage_stop or graceful_shutdown or allowed_submit"`
Expected: PASS.

### Task 3: Regression verification

**Files:**
- Test: `tests/unit_tests/trading/test_strategy.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`

**Step 1: Run focused verification**

Run:
- `pytest tests/unit_tests/trading/test_strategy.py -k "market_exit or allowed_instrument"`
- `pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k "manage_stop or graceful_shutdown or allowed_submit"`
- `pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -k "on_start or cancel_managed_quotes"`

**Step 2: Confirm behavior**

Check that:
- wrong-instrument submits are denied
- market exit still closes residual positions
- TokenMM runner builds strategies/nodes with safe defaults for stop and exception shutdown
