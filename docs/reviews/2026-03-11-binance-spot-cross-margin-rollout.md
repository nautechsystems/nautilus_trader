# Binance Spot Cross-Margin Rollout Review

Date: `2026-03-11`

## Evidence Header

- Snapshot time (UTC): `2026-03-11T05:41:55Z`
- Host/base URL used for live checks: `http://127.0.0.1:5022`
- Supporting readiness commits:
  - `69eb2f9ae` - `test(tokenmm): assert binance spot contract semantically`
  - `65174edaf` - `docs(tokenmm): use real terminal denial runtime terms`
- Supporting verification results:
  - `uv run --no-sync python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` -> `44 passed`
  - `uv run --no-sync python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py` -> `4 passed`
  - `uv run --no-sync python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py` -> `48 passed`

## Scope

Blocked rollout review for the Binance spot cross-margin rollout attempt of
`plumeusdt_binance_spot_makerv3`.

## Final Config And Code State

- The code/doc readiness evidence for the supported Binance spot cross-margin
  market-making path is captured in the supporting commits and verification
  results above.
- `deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml` is pinned to:
  - `account_type = "MARGIN"`
  - `allow_cash_borrowing = true`
  - `spot_cash_borrowing_policy = "both_sides"`
  - `force_bot_off_on_start = true`
  - `bot_on = false`
- The operator runbook exists at
  `docs/runbooks/tokenmm-binance-spot-market-making.md`.
- Shared env cleanup completed locally:
  - `/etc/flux/common.env` now points `WORKDIR` and `PYTHONPATH` to
    `/home/ubuntu/nautilus_trader`
  - `/etc/flux/tokenmm-node-plumeusdt_binance_spot_makerv3.env` already points
    `WORKDIR` and `PYTHONPATH` to `/home/ubuntu/nautilus_trader`

## Live Evidence Snapshot

- Balances: `/api/v1/balances?profile=tokenmm`
  - `BINANCE_SPOT-MARGIN-master`: `PLUME -30314.96734613`, `USDT 1285.28070703`
  - `BINANCE_SPOT-SPOT-master`: `PLUME 0`, `USDT 0`
  - Observed fact: the effective inventory and liability are still presented on
    the unsupported PM-style balance surface.
  - Observed fact: the supported spot/cross-margin side is not yet funded for
    rollout.
- Signal: `/api/v1/signals?profile=tokenmm`
  - `plumeusdt_binance_spot_makerv3`
  - `tradeable=false`
  - `blocked=true`
  - `state="bot_off"`
  - `local_qty_base=-30314.96734613`
- Alerts: `/api/v1/alerts?profile=tokenmm&limit=50`
  - `terminal_order_denied` with reason
    `unsupported_account_mode: binance portfolio margin account requires papi spot/margin endpoints; configured adapter supports spot or cross margin only`
  - recent `exchange_order_rejected` / `order_denied` entries with
    `UNSUPPORTED_ACCOUNT_MODE`
- Service and env notes
  - `flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service` remains active
  - recent journal access shows the service is still running; the blocker is
    unsupported account mode and unfixed balances, not a failed process
  - only one Binance key-pair env name is configured on the box:
    `BINANCE_API_KEY` / `BINANCE_API_SECRET`
  - there is no evidence in the env surface of an alternate supported non-PM
    Binance account/key to rotate to

## Outcome

- Rollout blocked before canary enable.
- No quote enable was performed.
- No promotion was performed.
- Observed blockers:
  - the current live balance layout is still the unsupported PM presentation,
    with effective inventory/liability on `BINANCE_SPOT-MARGIN-master` and
    `BINANCE_SPOT-SPOT-master` still zeroed
  - historical alerts include `UNSUPPORTED_ACCOUNT_MODE` and
    `terminal_order_denied`
  - the supported account side is not yet funded for rollout
  - there is no evidence in the env surface of an alternate supported non-PM
    Binance credential
- Operational inference: the current credentialed rollout path has not yet been
  rotated to a supported, funded regular cross-margin account.

## Actions Taken

- Code, tests, and operator docs for the supported Binance spot cross-margin
  path were completed.
- Shared env root-path cleanup was completed locally.
- The rollout review was recorded with blocked pre-canary evidence.
- No canary, quote enable, or promotion step was attempted.

## Next Required Operator Action

1. Provide or rotate to a supported regular cross-margin Binance account/key.
2. Flatten the current PM liability and fund the supported account with the
   intended inventory.
3. Rerun the bot-off restart and canary before enabling quoting or attempting
   promotion.
