# Binance Spot Cross-Margin Rollout Review

Date: `2026-03-11`

## Scope

Blocked rollout review for the Binance spot cross-margin rollout attempt of
`plumeusdt_binance_spot_makerv3`.

## Final Config And Code State

- Repo code and docs are ready for the supported Binance spot cross-margin
  market-making path.
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
  - Effective inventory and liability remain on the unsupported PM-side
    balance surface; the supported account is not funded for rollout.
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
- Explicit blocker: the current configured Binance account is still the
  unsupported Portfolio Margin account, and no alternate supported regular
  cross-margin credential is configured.

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
