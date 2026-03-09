# Equities `strategies` contract

This directory holds one TOML file per equities node process enrolled into the Pulse-managed
`flux@equities-node-*` services.

## File naming

- Use the exact Flux strategy ID as the file name: `<flux_strategy_id>.toml`.
- Recommended naming pattern: `<stock>_tradexyz_makerv4.toml`.
- One stock uses one strategy file and one node process.
- Keep the active enrolled set aligned with `deploy/equities/equities.live.toml`.
- Disabled configs should use the `.toml.disabled` suffix until they are re-enrolled.
- The checked-in rollback file is `aapl_tradexyz_makerv3.toml.disabled`; leave it disabled so the installer discovers only the MakerV4 canary.

## Required TOML keys per file

- `[identity].strategy_id` and `[identity].strategy_instance_id` stay aligned to the file name.
- `[strategy].strategy_id` stays descriptive and unique across node processes.
- `[strategy].strategy_groups` stays `equities`.
- `[strategy].param_set = "makerv4"` stays explicit for the active equities rollout.
- `[venues].execution_venue` stays `HYPERLIQUID` and `[venues].reference_venue` stays `IBKR`.
- `[node.venues.HYPERLIQUID].instrument_id` defines the trade[XYZ] builder-perp instrument.
- `[node.venues.IBKR].instrument_id` defines the IBKR reference instrument, for example `AAPL.NASDAQ` for the checked-in AAPL canary.
- `[node.venues.IBKR].use_regular_trading_hours = false` keeps after-hours reference data available for MakerV4.
- `[node.venues.IBKR.dockerized_gateway]` carries the read-only live gateway runtime, including the nightly `11:45 PM America/New_York` restart window.
- `[node.venues.HYPERLIQUID].dex = "xyz"` stays explicit.
- `[node.venues.HYPERLIQUID].private_key_env` and `account_address_env` must reference env var names, not inline secrets.
- `[strategy].outside_rth_hedge_enabled = true` makes the hedge leg explicit for the after-hours rollout.
- `[strategy].ibkr_primary_exchange` sets the listing venue used to derive the reference instrument. Keep it on a qualifiable stock venue such as `NASDAQ`; there is no separate `ibkr_route_exchange` field in the current runner contract.
- For MakerV4, the runner derives the effective IBKR reference instrument from `[node.venues.HYPERLIQUID].instrument_id` plus `[strategy].ibkr_primary_exchange`, so the checked-in canary keeps `AAPL.NASDAQ` aligned with `ibkr_primary_exchange = "NASDAQ"`.
- Keep the shared `[[contracts]]` IBKR entry aligned with the active canary reference instrument, because the `/equities` API contract catalog is built from `deploy/equities/equities.live.toml`.
- Hyperliquid effective account identity resolves in this order: `vault_address_env`, then funded `account_address_env`, then the agent wallet's `userRole`-resolved master account.
- Do not duplicate `[redis]` in per-node deploy files; nodes inherit it from `deploy/equities/equities.live.toml`.
- Do not duplicate `[portfolio]` in per-node deploy files; nodes inherit the shared portfolio inventory feed from `deploy/equities/equities.live.toml`.
- `assumed_hedge_fee_bps` is a MakerV4 runtime param, not a TOML key here. IBKR hedge fees are not live-discovered in this contract; the default is a configured assumption from the runtime param registry.

## Inventory semantics

- `local_qty` is strategy-local inventory for that stock.
- `global_qty` is the shared `equities` portfolio aggregate for the stock portfolio.
- Each node publishes a strategy inventory component to Redis.
- `flux.runners.equities.run_portfolio` aggregates those components into the shared portfolio inventory feed consumed by all equities strategies.

## Env conventions

- Production node lifecycle is managed from Pulse via flux@ units.
- Production secrets should be provided through `/etc/flux/common.env` plus `/etc/flux/equities-node-*.env`.
- Required trade[XYZ] env vars are `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS`.
- Optional vault routing env var is `TRADE_XYZ_VAULT_ADDRESS`.
- Local smoke with the checked-in `node.venues.IBKR.dockerized_gateway` contract also requires `TWS_USERNAME` and `TWS_PASSWORD`.
- Set `TRADE_XYZ_ACCOUNT_ADDRESS` to the funded master account when the configured private key belongs to an agent wallet.
- If vault trading is enabled, provide `vault_address_env` and it will take precedence for account-state queries, fee lookup, and WS subscriptions.
- `deploy/equities/equities_stack.env` is for local paper/testnet smoke only.
- Use the same `[flux].namespace` and `[flux].schema_version` as the shared API/bridge config.
- Pulse-managed node services pass `--shared-config deploy/equities/equities.live.toml` so node runners inherit the shared `[redis]` and `[portfolio]` tables.

Each file is a complete node config consumed directly by `python -m flux.runners.equities.run_node`.
Start from `equities.strategy.template.toml`.
