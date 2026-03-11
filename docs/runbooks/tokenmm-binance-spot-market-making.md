# TokenMM Binance Spot Market-Making Runbook

This runbook defines the supported production contract for
`plumeusdt_binance_spot_makerv3`.

Use it together with `deploy/tokenmm/README.md` and
`docs/runbooks/tokenmm-risk-validation.md`.

## Current unsupported state

- Current adapter behavior rejects Portfolio Margin / PAPI mode for Binance spot
  market making with `UNSUPPORTED_ACCOUNT_MODE`.
- Portfolio Margin / PAPI is unsupported for the current production rollout.
- Inspect `GET /api/v1/balances?profile=tokenmm` to confirm whether the current
  live balances still carry the effective inventory in margin / Portfolio
  Margin rather than in the supported cross-margin account.
- Current plain spot account rows are zeroed, so do not treat the existing spot
  wallet as funded inventory for quoting.

## Supported production contract

- Supported production setup: regular Binance cross-margin account only.
- Keep `[node.venues.BINANCE_SPOT] allow_cash_borrowing = true`.
- Keep `[strategy] spot_cash_borrowing_policy = "both_sides"` for the first
  rollout so the strategy uses free balances first and borrows only when needed
  on either side.
- Keep `[strategy] force_bot_off_on_start = true` during the cutover.
- Keep `[strategy] bot_on = false` by default until the bot-off restart and
  canary finish cleanly.
- Do not point live market-making credentials at a Portfolio Margin / PAPI
  account.

## Balance preparation

1. Inspect the shared TokenMM balance surface before changing credentials:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
   ```

2. Confirm the live unsupported state is understood before cutover:
   - `GET /api/v1/balances?profile=tokenmm` shows whether the current PM
     inventory is where the effective balance lives
   - the plain spot rows are zeroed
3. Before cutover, flatten the existing PM liability. Keep that exact gate
   closed until the strategy is allowed to quote from the supported account.
4. Move the intended funded inventory into the supported regular Binance
   cross-margin account so the rollout starts from funded inventory rather than
   relying on an empty spot wallet.

## Cutover steps for the bot-off restart and canary

1. Rotate or point the Binance API credentials to the supported cross-margin
   account.
2. Confirm the Binance spot strategy config still matches the supported
   contract:
   - `allow_cash_borrowing = true`
   - `spot_cash_borrowing_policy = "both_sides"`
   - `force_bot_off_on_start = true`
   - `bot_on = false`
3. Restart the Binance spot node in bot-off mode:

   ```bash
   sudo systemctl restart flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service
   ```

4. Review the node journal and API state immediately after restart:

   ```bash
   journalctl -u flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service --since "10 min ago" --no-pager
   curl -fsS 'http://127.0.0.1:5022/api/v1/signals?strategy=plumeusdt_binance_spot_makerv3'
   curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
   ```

5. Verify no `UNSUPPORTED_ACCOUNT_MODE` appears in the journal. Any such line is
   a failed cutover.
6. Keep the strategy bot-off until the restart is clean and the supported
   account is confirmed funded.
7. Enable quoting only after a clean bot-off start, then watch the canary for
   at least one normal quote replacement window and one borrow-needed edge case.
8. Collect concrete pass/fail evidence immediately after quoting is enabled:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/signals?strategy=plumeusdt_binance_spot_makerv3'
   curl -fsS 'http://127.0.0.1:5022/api/v1/alerts?profile=tokenmm&strategy=plumeusdt_binance_spot_makerv3&limit=50'
   journalctl -u flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service --since "10 min ago" --no-pager
   ```
9. Pass the canary only if all of the following are true:
   - signals show accepted/open orders on at least one side
   - alerts do not show a fresh order-denied/rejected burst tied to account
     mode or an unsupported borrowing path
   - the journal shows no `UNSUPPORTED_ACCOUNT_MODE`
   - the journal shows no terminal auto-shutdown
10. Fail the canary immediately if any of the following appear:
   - `UNSUPPORTED_ACCOUNT_MODE`
   - terminal auto-shutdown
   - fresh order-denied/rejected burst tied to account mode or unsupported
     borrowing path

## Acceptance criteria

- The strategy is pointed at a regular Binance cross-margin account, not
  Portfolio Margin / PAPI.
- The node starts cleanly in bot-off mode with no `UNSUPPORTED_ACCOUNT_MODE`.
- `allow_cash_borrowing = true` and
  `spot_cash_borrowing_policy = "both_sides"` remain in force for the first
  rollout.
- The existing PM `PLUME` liability is flattened before quoting is enabled.
- The intended market-making inventory is funded in the supported account.
- After quoting is enabled, signals show accepted/open orders on at least one
  side and the alerts/journal checks remain clean.
- Quoting is enabled only after the clean bot-off restart and canary checks
  complete.

## Rollback criteria

Rollback to a no-trade state immediately if any of the following occur:

- the journal shows `UNSUPPORTED_ACCOUNT_MODE`
- the journal shows terminal auto-shutdown
- the supported account is not the one actually receiving the Binance spot
  session
- the PM liability is not flattened
- the funded inventory is missing or materially wrong
- the alerts feed shows a fresh order-denied/rejected burst tied to account
  mode or unsupported borrowing path
- the bot-off restart is not clean

Rollback actions:

1. Put the strategy back to bot-off and keep quoting disabled.
2. Stop using the attempted credential rotation until the supported account and
   balances are corrected.
3. Preserve `journalctl` output and the relevant API payloads for the incident
   notes.
4. Do not promote Portfolio Margin / PAPI as a fallback live market-making
   mode.

## Out of scope

Portfolio Margin / PAPI support is a separate project. This runbook covers only
the currently supported Binance spot market-making path on a regular
cross-margin account.
