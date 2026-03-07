# Equities `strategies` contract

This directory holds one TOML file per equities node process enrolled into the Pulse-managed
`flux@equities-node-*` services.

## File naming

- Use the exact Flux strategy ID as the file name: `<flux_strategy_id>.toml`.
- Recommended naming pattern: `<stock>_tradexyz_makerv3.toml`.
- One stock uses one strategy file and one node process.
- Keep the active enrolled set aligned with `deploy/equities/equities.live.toml`.
- Disabled configs should use the `.toml.disabled` suffix until they are re-enrolled.

## Required TOML keys per file

- `[identity].strategy_id` and `[identity].strategy_instance_id` stay aligned to the file name.
- `[strategy].strategy_id` stays descriptive and unique across node processes.
- `[strategy].strategy_groups` stays `equities`.
- `[venues].execution_venue` stays `HYPERLIQUID` and `[venues].reference_venue` stays `IBKR`.
- `[node.venues.HYPERLIQUID].instrument_id` defines the trade[XYZ] builder-perp instrument.
- `[node.venues.IBKR].instrument_id` defines the IBKR reference instrument, for example `AAPL.NASDAQ`.
- `[node.venues.IBKR.dockerized_gateway]` carries the read-only live gateway runtime, including the nightly `11:45 PM America/New_York` restart window.
- `[node.venues.HYPERLIQUID].dex = "xyz"` stays explicit.
- `[node.venues.HYPERLIQUID].private_key_env` and `account_address_env` must reference env var names, not inline secrets.
- Do not duplicate `[redis]` in per-node deploy files; nodes inherit it from `deploy/equities/equities.live.toml`.
- Do not duplicate `[portfolio]` in per-node deploy files; nodes inherit the shared portfolio inventory feed from `deploy/equities/equities.live.toml`.

## Inventory semantics

- `local_qty` is strategy-local inventory for that stock.
- `global_qty` is the shared `equities` portfolio aggregate for the stock portfolio.
- Each node publishes a strategy inventory component to Redis.
- `flux.runners.equities.run_portfolio` aggregates those components into the shared portfolio inventory feed consumed by all equities strategies.

## Env conventions

- Production node lifecycle is managed from Pulse via flux@ units.
- Production secrets should be provided through `/etc/flux/common.env` plus `/etc/flux/equities-node-*.env`.
- Required trade[XYZ] env vars are `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS`.
- `deploy/equities/equities_stack.env` is for local paper/testnet smoke only.
- Use the same `[flux].namespace` and `[flux].schema_version` as the shared API/bridge config.
- Pulse-managed node services pass `--shared-config deploy/equities/equities.live.toml` so node runners inherit the shared `[redis]` and `[portfolio]` tables.

Each file is a complete node config consumed directly by `python -m flux.runners.equities.run_node`.
Start from `equities.strategy.template.toml`.
