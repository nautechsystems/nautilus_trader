# Equities Binance Perp Market-Making Runbook

This runbook defines the rollout contract for Binance USD-M equity perps inside
the shared `equities` profile.

Use it together with `deploy/equities/README.md`,
`deploy/equities/equities.live.toml`, and
`ops/scripts/deploy/binance_equities_universe.py`.

## Credential and funding contract

- Provision a dedicated Binance USD-M futures subaccount for equities market
  making.
- Configure `EQUITIES_BINANCE_API_KEY` and
  `EQUITIES_BINANCE_API_SECRET` for that subaccount.
- Keep every checked-in Binance node TOML pointed at those env vars through
  `[node.venues.BINANCE_PERP].api_key_env` and `api_secret_env`.
- Fund the Binance futures wallet with USDT before enabling quoting.
- No Binance spot key is required for this rollout.
- Keep hedge / FV market on IBKR. Binance is maker venue only for this path.

## Enrollment and discovery contract

- Discovery and enrollment are separate.
- Use `python ops/scripts/deploy/binance_equities_universe.py --show-diff --config deploy/equities/equities.live.toml`
  to review the current Binance equity-perp universe.
- The discovery helper is read-only. It shows newly discovered Binance-only
  routes but does not enroll them.
- Only explicitly enrolled routes from `api.equities_strategy_ids` may trade;
  staging a `[[strategy_contracts]]` row alone is not enough.

## Canary sequence

1. Run the overlap-name canary first.
   Use `PLTR` or `TSLA` so the stock already exists on another maker venue and
   the rollout proves same-stock multi-venue netting.
2. Confirm the shared portfolio surface keeps one stock bucket while preserving
   route visibility.
   The critical checks are `inventory_by_asset` for the net stock view and
   `source_strategy_ids` for shared-account provenance.
3. Run the Binance-only name canary second.
   Use `MSTR` to prove explicit enrollment of newly discovered Binance-only
   routes without auto-trading the rest of the discovered universe.

## Bot-off restart checks

1. Restart the portfolio/API surfaces and the selected Binance node in bot-off
   mode.
2. Confirm the enrolled route appears in signals and that the shared equities
   profile remains healthy before enabling quoting.
3. Inspect the stock-netted and route-local views directly:

   ```bash
   curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'
   curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities'
   ```

4. For the overlap-name canary, confirm `/api/v1/balances?profile=equities`
   shows:
   - one shared stock bucket under `inventory_by_asset` for the canary stock
   - both route contributors under `components`
   - shared IBKR account rows carrying `source_strategy_ids` for both strategy
     routes
5. Do not enable quoting until the node start is clean and the netted portfolio
   view matches the expected route-local contributors.

## Acceptance criteria

- The overlap-name canary proves same-stock multi-venue netting for `PLTR` or
  `TSLA`.
- `/api/v1/balances?profile=equities` shows one stock-netted bucket in
  `inventory_by_asset` and two contributor rows in `components` for the overlap
  canary.
- Shared account rows preserve `source_strategy_ids` so operators can see which
  strategy routes own the exposure.
- The Binance-only name canary proves `MSTR` only trades after explicit
  enrollment.
- The discovery helper continues to report newly discovered Binance-only routes
  without modifying live config.

## Rollback criteria

Rollback to bot-off immediately if any of the following occur:

- the overlap-name canary does not preserve same-stock multi-venue netting
- `inventory_by_asset` and `components` disagree on the canary stock exposure
- shared account rows drop `source_strategy_ids`
- the Binance route starts without funded futures collateral
- a newly discovered Binance-only route appears live without explicit
  enrollment

## Notes

- Shared equities risk is stock-netted across all maker venues.
- Venue-local balances and orders remain route-local by definition.
- This runbook covers Binance USD-M equity perps only; additional equity-perp
  venues should follow the same shared-portfolio contract.
