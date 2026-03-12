# TokenMM Bitget Spot + Perp Market-Making Runbook

This runbook defines the supported production contract and rollout gates for:

- `plumeusdt_bitget_spot_makerv3`
- `plumeusdt_bitget_perp_makerv3`

Use it together with `deploy/tokenmm/README.md` and
`docs/runbooks/tokenmm-risk-validation.md`.

## Production target

Preferred production contract:

- Bitget unified/shared-margin account for both spot and perp
- spot quoting uses shared collateral and borrowing only where needed
- first spot rollout constrains borrowing to sell-side borrowing only
- perp runs as USDT-margined perp in one-way/netting mode
- both strategies start bot-off and must clear canary gates independently

Temporary fallback contract, only if the preferred path is blocked:

- spot quotes from funded `USDT + PLUME` inventory with no borrow dependence
- perp quotes only after visible funded collateral is confirmed
- the fallback must be called out explicitly as temporary and must not be
  treated as final Bitget production parity

## Historical blockers resolved on 2026-03-11

- Bitget spot initially showed only `USDT` in the shared balances view and no
  visible Bitget `PLUME`
- Bitget perp previously stopped after `NOTIONAL_EXCEEDS_FREE_BALANCE`, then
  `400`, then `429`, then venue protection
- the pre-prod stack also emitted repeated cross-node UTA private-order
  warnings for the opposite Bitget node on every quote cycle

Those blockers were cleared on `2026-03-11` by:

- upgrading the live Bitget account from Classic to UTA/shared-margin via API
- confirming one-way hold mode via UTA account settings
- fixing cached account-type replay so the perp node restarts cleanly
- fixing Bitget private-order handling so shared-account foreign updates do not
  page `WARN`

## Task 6 scope split

This task has two parts:

- operator-side Bitget account confirmation and funding, which cannot be
  completed from this repo alone
- local code/config validation, which must be cleared before any Bitget
  strategy is enabled

Do not mark Bitget ready for live quoting until both parts are cleared and the
evidence is recorded below.

## Operator-side Bitget account checks

Clear these in the Bitget UI or account API before changing `bot_on`:

1. Confirm the account model and threshold:
   - if the preferred parity path is being used, unified/shared-margin is
     actually enabled for the live account
   - the chosen UTA mode is available to the account and the account equity is
     at or above Bitget's threshold for that mode
   - for advanced UTA/shared-margin, record the exact threshold that was
     cleared in the evidence section; Bitget currently documents a
     `>= 1,000 USD` equivalent requirement for advanced unified mode
2. Confirm instrument and mode support:
   - `PLUME` loan/margin support is enabled if borrow-backed spot quoting is
     expected
   - futures trading is enabled for the account
   - futures hold mode is configured for one-way mode, not hedge mode
3. Confirm API key contract:
   - the key / secret / passphrase map to the intended live Bitget account
   - the key has read + trade permissions
   - if API-driven position-mode changes will ever be used, the key also has
     management permission

## Local code and config checks

Clear these from the local stack before any live enablement:

1. Confirm the deployed config still matches the intended production contract:
   - `plumeusdt_bitget_spot_makerv3` stays `bot_on = false` with
     `spot_cash_borrowing_policy = "sell_only"` for the first preferred-path
     rollout
   - `plumeusdt_bitget_perp_makerv3` stays `bot_on = false`
   - both Bitget strategy TOMLs still target `account_mode = "UTA"` and
     one-way/cross-margin settings
2. Confirm `/etc/flux/common.env` is populated with the intended live Bitget
   key triplet and not stale credentials from another account.
3. Confirm both Bitget strategies still load under the same production
   TokenMM allowlist and profile surfaces.

## Funding and readiness checks

The chosen live contract must be visible in the local API before any restart or
canary:

- preferred UTA/shared-margin path:
  - enough `USDT` is funded for spot bid pressure and perp collateral
  - collateral shows up through our API; do not rely on planned transfers or
    balances visible only in the Bitget UI
- temporary fallback path only:
  - spot shows funded `USDT + PLUME`
  - perp shows visible funded collateral

Validate the readiness surface explicitly:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm' \
     | jq '.data.rows[] | select(.venue=="BITGET")'
   curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm' \
     | jq '.data.strategies[] | select(.id|test("plumeusdt_bitget_(spot|perp)_makerv3")) | {id, tradeable, blocked, state:.state.state, bot_on:.state.bot_on}'
   ```

Readiness expectations:

- Bitget spot shows the `USDT` needed for the chosen path
- if using the fallback path, Bitget spot also shows `PLUME`
- Bitget perp shows visible collateral, or an explicit zero/absence that blocks
  go-live until funding is fixed
- both strategies remain `bot_on=false` with zero managed orders
- any stale account state remains visible in `Signals` / `Balances` as a
  degraded or blocked pre-cutover condition, not a reason to proceed anyway

## Bot-off restart gate

Restart both Bitget nodes in bot-off mode first:

```bash
sudo systemctl restart flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service
```

Review startup evidence immediately:

```bash
journalctl -u flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service --since "10 min ago" --no-pager
journalctl -u flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service --since "10 min ago" --no-pager
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
```

Pass the restart gate only if all of the following are true:

- no fresh unsupported account-mode failure
- no fresh Bitget `400` / `429` burst while bot-off
- balances remain visible and stable
- both strategies stay bot-off with zero managed orders

If any of those checks fail, Bitget remains blocked regardless of what the
exchange UI shows.

## Evidence capture

Before any live enablement, attach or archive the exact evidence used to clear
the gate:

1. Account contract evidence:
   - Bitget account mode screenshot or API payload
   - the UTA mode and equity threshold that was cleared
   - proof that `PLUME` margin/loan support is enabled if borrow-backed spot
     quoting is expected
   - proof that futures is enabled in one-way mode
2. Credential evidence:
   - the specific API key identity used for the live deploy
   - the permissions granted to that key
3. Local readiness evidence:
   - `GET /api/v1/balances?profile=tokenmm` payload excerpt for Bitget rows
   - `GET /api/v1/signals?profile=tokenmm` payload excerpt for the two Bitget
     strategies
   - bot-off restart journal excerpts for both Bitget services
4. Rollout mode evidence:
   - record explicitly whether the preferred UTA/shared-margin path or the
     temporary funded-inventory fallback is being used

Do not treat Task 6 as complete until that evidence exists. This repo update
only defines the contract; it does not prove the live Bitget account satisfies
it.

## 2026-03-11 live execution evidence

Recorded after the UTA rollout and the live canary restart:

1. Account contract evidence:
   - UTA account settings were confirmed by API on `2026-03-11` with
     `accountMode=unified`, `assetMode=multi_assets`, and
     `holdMode=one_way_mode`
   - the live account is now using the preferred UTA/shared-margin path, not
     the funded-inventory fallback
2. Bot-off restart evidence:
   - both Bitget services restarted cleanly in bot-off mode at
     `2026-03-11 20:09:32 UTC`
   - no fresh `unsupported_account_mode`, `400`, or `429` burst appeared on
     restart
3. Spot canary evidence:
   - live canary params: `qty=250`, `n_orders1=1`, `bot_on=true`
   - fresh accepts after the post-fix restart:
     `O-20260311-200940-SPOT-000-319` and `...320` accepted at
     `2026-03-11 20:09:48 UTC`
   - signal state after re-enable: `running`, `tradeable=true`,
     `blocked=false`, `managed_orders=2`
4. Perp canary evidence:
   - live canary params: `qty=500`, `n_orders1=1`, `bot_on=true`
   - fresh accepts after the post-fix restart:
     `O-20260311-200940-PERP-000-312` and `...313` accepted at
     `2026-03-11 20:09:48 UTC`
   - signal state after re-enable: `running`, `tradeable=true`,
     `blocked=false`, `managed_orders=2`
5. Balance snapshot from `GET /api/v1/balances?profile=tokenmm` at
   approximately `2026-03-11 20:10 UTC`:
   - `BITGET-001 spot PLUME total=499.24669092 free=249.24669092 locked=250`
   - `BITGET-001 spot USDT total=493.98512606 free=490.91762606 locked=3.0675`
   - `BITGET-001 perp USDT total=493.98512606 free=490.91762606 locked=3.0675`
6. Shared-account warning evidence:
   - before the final fix, both nodes logged repeated
     `Bitget private order update ignored` warnings for the opposite node
   - after the UTA-aware logging fix and the `20:09:32 UTC` restart, those
     messages no longer appeared at `WARN` in fresh journal windows

Current live status:

- `plumeusdt_bitget_spot_makerv3` is live at canary size on the preferred UTA
  path
- `plumeusdt_bitget_perp_makerv3` is live at canary size on the preferred UTA
  path
- promotion to larger size/depth is still gated on a longer clean observation
  window; do not jump directly to full depth

## Spot canary

Use a minimal spot canary first:

1. Reduce runtime size:

   ```bash
   curl -fsS -X POST 'http://127.0.0.1:5022/api/v1/params' \
     -H 'Content-Type: application/json' \
     -d '{"profile":"tokenmm","strategy_id":"plumeusdt_bitget_spot_makerv3","params":{"qty":250,"n_orders1":1,"n_orders2":0,"n_orders3":0,"bot_on":false}}'
   ```

2. Enable only the Bitget spot strategy.
3. Watch the signal, journal, and alerts:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
   curl -fsS 'http://127.0.0.1:5022/api/v1/alerts?profile=tokenmm&strategy=plumeusdt_bitget_spot_makerv3&limit=50'
   journalctl -u flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service --since "10 min ago" --no-pager
   ```

Spot canary pass criteria:

- at least one accepted bid and one accepted ask
- no fresh `terminal_order_denied`
- no fresh venue-protection stop
- no generic unexplained `HTTP 400`
- if borrowing is in use, the reject surface must be detailed enough to tell
  whether the exchange rejected the account mode, borrow path, or order shape

Spot canary immediate fail criteria:

- ask side still cannot quote under the preferred UTA/shared-margin contract
- alerts or journal show fresh `terminal_order_denied`
- venue protection or `429` opens during the canary
- balances do not reflect the chosen live contract

## Perp canary

Run the Bitget perp canary only after the spot contract is either passing or
intentionally held in temporary fallback mode.

1. Confirm visible collateral first:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm'
   ```

2. Reduce runtime size for the first perp enablement.
3. Enable only `plumeusdt_bitget_perp_makerv3`.
4. Watch the signal, journal, and alerts:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm'
   curl -fsS 'http://127.0.0.1:5022/api/v1/alerts?profile=tokenmm&strategy=plumeusdt_bitget_perp_makerv3&limit=50'
   journalctl -u flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service --since "10 min ago" --no-pager
   ```

Perp canary pass criteria:

- no `NOTIONAL_EXCEEDS_FREE_BALANCE`
- no fresh `400` / `429` burst
- no venue-protection stop
- order flow remains compatible with one-way/netting behavior

Perp canary immediate fail criteria:

- free balance still resolves to zero through the live stack
- venue protection opens
- generic unexplained `HTTP 400` remains
- the account behaves like a different mode than the one documented for rollout

## Promotion rules

Promote to broader live quoting only if:

- the preferred UTA/shared-margin contract works for spot, or the temporary
  fallback was explicitly accepted
- perp canary passes with visible collateral and clean journals/alerts
- both strategies survive a bot-off restart cleanly
- the reject surface is actionable enough for on-call debugging

Increase quote size and depth in stages. Do not jump from the minimal canary
directly to full size.

## Rollback rules

Return to `bot_on=false` immediately if any of the following appear:

- fresh `terminal_order_denied`
- fresh `NOTIONAL_EXCEEDS_FREE_BALANCE`
- venue protection or `429`
- unexplained `HTTP 400`
- missing or stale balances for the chosen live contract
- account-mode mismatch between the runbook and the live account

Rollback actions:

1. Put the affected strategy back to bot-off.
2. Preserve journal output and API payloads for the incident note.
3. Do not silently switch from the preferred contract to the fallback contract.
4. Record whether the failure was exchange/account configuration, adapter gap, or
   unsupported Bitget borrow behavior for `PLUME`.

## Out of scope

- automatic promotion of the funded-inventory fallback to the final production
  definition
- hedge-mode Bitget perp support
- multi-symbol Bitget rollout beyond the PLUME spot + perp pair
