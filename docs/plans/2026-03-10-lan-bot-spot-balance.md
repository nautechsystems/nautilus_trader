# Lan Bot Spot Balance Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Extend the Lan rogue trader Telegram bot so it monitors the combined Binance PM + spot balance for the configured asset using the same Binance API key/secret env vars and otherwise preserves the current alerting behavior.

**Architecture:** Keep one alert service and one state machine. Add a small spot-account balance client for `GET /api/v3/account`, then compose it with the existing PM client behind the existing `fetch_balance() -> Decimal` contract so `LanRogueTraderAlertService` continues to reason about one effective balance number. Preserve current PM semantics, treat a missing spot asset row as zero spot balance, and fail the poll on HTTP/parsing/signing errors from either source so the bot never alerts on partial data.

**Tech Stack:** Python 3.13, `requests`, pytest, INI config, Markdown deploy docs

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Branch pushed and PR #39 opened against main |
| Task 1: Lock In Combined PM+Spot Semantics In Tests | completed | main | Spec and quality review passed; targeted tg-bot slice verified with `19 passed` |
| Task 2: Implement Spot And Combined Balance Clients | completed | main | Combined PM+spot client wiring verified; runner slice still blocked locally by missing compiled core extension |
| Task 3: Wire Runtime Defaults And Operator Docs | completed | main | Config template and docs updated; tg-bot contract tests cover README and runbook text |
| Task 4: Verify End-To-End Behavior And Prepare Branch For Review | completed | main | Fresh verification run, branch pushed, and PR #39 opened: https://github.com/clickconfirm/nautilus-trader/pull/39 |

---

### Task 1: Lock In Combined PM+Spot Semantics In Tests

**Files:**
- Modify: `tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py`
- Test: `tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py`

**Step 1: Write the failing tests**

Add focused tests that define the new balance contract before implementation:

```python
def test_binance_spot_fetch_balance_sums_free_and_locked_for_asset() -> None:
    ...

def test_combined_binance_balance_sums_pm_and_spot_usdt() -> None:
    ...

def test_combined_binance_balance_treats_missing_spot_asset_as_zero() -> None:
    ...

def test_load_config_defaults_spot_base_url(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ...
```

Use a spot payload shaped like Binance `GET /api/v3/account`:

```python
{
    "accountType": "SPOT",
    "balances": [
        {"asset": "USDT", "free": "100.25", "locked": "4.75"},
    ],
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
PYTHONPATH=systems/flux uv run --active --no-sync pytest -q \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux/tg_bots
```

Expected: FAIL because the spot client, combined client, and spot config default do not exist yet.

**Step 3: Write minimal implementation hooks**

Add only the names and minimal plumbing needed to make the tests import the new types:

```python
class BinanceSpotClient:
    def fetch_balance(self) -> Decimal: ...


class CombinedBalanceClient:
    def fetch_balance(self) -> Decimal: ...
```

Extend `WatchConfig` with:

```python
binance_spot_base_url: str
```

**Step 4: Run test to verify the target failures are now implementation failures, not import/setup failures**

Run the same pytest command.

Expected: FAIL on assertion mismatches about spot parsing or combined balance behavior, not on missing symbols.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  systems/flux/flux/tg_bots/lan_rogue_trader_alert.py
git commit -m "test: define lan bot combined pm and spot balance semantics"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Implement Spot And Combined Balance Clients

**Files:**
- Modify: `systems/flux/flux/tg_bots/lan_rogue_trader_alert.py`
- Modify: `systems/flux/flux/runners/tg_bots/run_lan_rogue_trader_alert.py`
- Modify: `systems/flux/flux/tg_bots/__init__.py`
- Test: `tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py`

**Step 1: Write one more failing regression for the error boundary**

Add a regression that proves partial data is not silently accepted:

```python
def test_combined_binance_balance_propagates_spot_http_failures() -> None:
    ...
```

The expectation should be that a spot HTTP/parsing/signing failure raises and causes the poll to fail, matching the current “don’t alert on incomplete data” behavior.

**Step 2: Run test to verify it fails**

Run:

```bash
PYTHONPATH=systems/flux uv run --active --no-sync pytest -q \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux/tg_bots
```

Expected: FAIL because the composed client still does not distinguish “spot asset absent” from “spot request failed”.

**Step 3: Write minimal implementation**

Implement the smallest compositional change:

```python
class BinanceSpotClient:
    def fetch_balance(self) -> Decimal:
        response = self.session.get(
            f"{self.base_url}/api/v3/account",
            params=signed_params,
            headers=headers,
            timeout=self.timeout_sec,
        )
        ...
        for row in balances:
            if row_asset == self.asset:
                return _as_decimal(row["free"], "free") + _as_decimal(row["locked"], "locked")
        return Decimal("0")


class CombinedBalanceClient:
    def __init__(self, pm_client: BinancePmClient, spot_client: BinanceSpotClient) -> None:
        ...

    def fetch_balance(self) -> Decimal:
        return self.pm_client.fetch_balance() + self.spot_client.fetch_balance()
```

In the runner, construct the composed client with the same key/secret from `WatchConfig`:

```python
pm_client = BinancePmClient(...)
spot_client = BinanceSpotClient(...)
client = CombinedBalanceClient(pm_client=pm_client, spot_client=spot_client)
```

Keep `LanRogueTraderAlertService` unchanged so the state machine still reasons about one `Decimal` balance.

**Step 4: Run test to verify it passes**

Run:

```bash
PYTHONPATH=systems/flux uv run --active --no-sync pytest -q \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux/tg_bots
```

Expected: PASS with the spot parsing, combined balance, and error-boundary tests all green.

**Step 5: Commit**

```bash
git add systems/flux/flux/tg_bots/lan_rogue_trader_alert.py \
  systems/flux/flux/runners/tg_bots/run_lan_rogue_trader_alert.py \
  systems/flux/flux/tg_bots/__init__.py \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py
git commit -m "feat: include spot balance in lan bot total"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Wire Runtime Defaults And Operator Docs

**Files:**
- Modify: `deploy/tg_bots/lan_rogue_trader_alert.ini`
- Modify: `deploy/tg_bots/README.md`
- Modify: `docs/runbooks/lan-rogue-trader-alert.md`
- Modify: `systems/flux/flux/tg_bots/lan_rogue_trader_alert.py`
- Test: `tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py`

**Step 1: Write the failing config/docs assertions**

Add one config-loading assertion in the existing tg-bot test file so the new default is pinned:

```python
def test_load_config_defaults_spot_base_url(...):
    assert config.binance_spot_base_url == "https://api.binance.com"
```

Then add/update docs text expectations by checking the files directly during implementation review:

- runbook purpose/scope must say the bot watches PM + spot
- README intent/control-plane sections must say the same keys are reused
- INI template must expose `binance_spot_base_url` with a sane default

**Step 2: Run test to verify it fails**

Run:

```bash
PYTHONPATH=systems/flux uv run --active --no-sync pytest -q \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux/tg_bots
```

Expected: FAIL until the new config field is loaded and defaulted.

**Step 3: Write minimal implementation**

Update the config template and docs to reflect the new combined-balance behavior:

```ini
binance_base_url = https://papi.binance.com
binance_spot_base_url = https://api.binance.com
asset = USDT
```

Explicitly document:

- same `LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY` / `LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET`
- effective watched balance = PM total + spot total for the configured asset
- missing spot asset row means zero spot balance, not an operator error

**Step 4: Run test to verify it passes**

Run:

```bash
PYTHONPATH=systems/flux uv run --active --no-sync pytest -q \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux/tg_bots
```

Expected: PASS with the new config default in place.

**Step 5: Commit**

```bash
git add deploy/tg_bots/lan_rogue_trader_alert.ini \
  deploy/tg_bots/README.md \
  docs/runbooks/lan-rogue-trader-alert.md \
  systems/flux/flux/tg_bots/lan_rogue_trader_alert.py \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py
git commit -m "docs: describe lan bot combined pm and spot balance"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Verify End-To-End Behavior And Prepare Branch For Review

**Files:**
- Modify: `docs/plans/2026-03-10-lan-bot-spot-balance.md` (tracker updates only)
- Test: `tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py`
- Test: `docs/runbooks/lan-rogue-trader-alert.md`
- Test: `deploy/tg_bots/README.md`

**Step 1: Re-run the focused verification suite**

Run:

```bash
bash -n systems/flux/flux/runners/tg_bots/run_lan_rogue_trader_alert.py 2>/dev/null || true
PYTHONPATH=systems/flux uv run --active --no-sync pytest -q \
  tests/unit_tests/flux/tg_bots/test_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux/tg_bots
git diff --check
rg -n "Portfolio Margin|PM \\+ spot|PM \\+ spot|PM and spot|combined Binance" \
  docs/runbooks/lan-rogue-trader-alert.md deploy/tg_bots/README.md deploy/tg_bots/lan_rogue_trader_alert.ini
```

Expected:

- pytest passes
- `git diff --check` returns no whitespace/conflict errors
- docs/config grep shows the PM + spot wording in all operator-facing files

**Step 2: If runner verification is needed, do it in an environment that has the compiled core**

Run, only if the local checkout has the built `nautilus_trader.core.data` extension available:

```bash
PYTHONPATH=.:systems/flux uv run --active --with redis --no-sync pytest -q \
  tests/unit_tests/flux/runners/tg_bots/test_run_lan_rogue_trader_alert.py \
  --confcutdir=tests/unit_tests/flux
```

Expected: PASS. If this still fails on missing compiled `nautilus_trader.core.data`, record that as an environment limitation instead of masking it.

**Step 3: Update the tracker and summarize review-ready deltas**

Make sure the Progress Tracker marks all tasks `completed` and records the exact verification evidence.

**Step 4: Push branch and open PR**

```bash
git push -u origin codex/lan-bot-spot-balance
gh pr create --fill --base main --head codex/lan-bot-spot-balance
```

Expected: branch is published and the PR description clearly states that the watched balance is now PM + spot with the same secret env vars.

**Step 5: Commit if tracker/docs changed during verification**

```bash
git add docs/plans/2026-03-10-lan-bot-spot-balance.md
git commit -m "docs: finalize lan bot spot balance execution tracker"
```

Skip this commit if the tracker state was already captured in the latest commit.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
