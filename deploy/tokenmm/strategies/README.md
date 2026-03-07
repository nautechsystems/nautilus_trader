# TokenMM `strategies` contract

This directory holds one TOML file per TokenMM node process enrolled into the Pulse-managed
`flux@tokenmm-node-*` services.

## File naming

- Use the exact Flux strategy ID as the file name: `<flux_strategy_id>.toml`.
- Examples:
  - `plumeusdt_bybit_perp_makerv3.toml`
  - `plumeusdt_bybit_spot_makerv3.toml`
  - `plumeusdt_okx_perp_makerv3.toml`
  - `plumeusdt_binance_spot_makerv3.toml`
- Include the actual execution venue, product type, symbol, and strategy family in the strategy ID.
- Keep the active production set aligned with `deploy/tokenmm/tokenmm.live.toml`.
- Disabled configs should use the `.toml.disabled` suffix until they are re-enrolled.

## Required TOML keys per file

- `[identity].strategy_id` and `[identity].strategy_instance_id` stay aligned to the file name.
- `[strategy].strategy_id` stays descriptive and unique across node processes.
- `[venues].execution_venue` and `[venues].reference_venue` identify the strategy routing roles.
- `[node.venues.<VENUE>].instrument_id` defines the instrument loaded for each venue client.
- Use the `BINANCE_SPOT` venue alias for shared reference pricing.
- Use the `BINANCE_PERP` venue alias when Binance perpetual execution and Binance spot reference data must coexist in one node.
- `exec_reconciliation_lookback_mins` should stay bounded at `15` for shared-account startup safety.
- `[node].filter_unclaimed_external_orders = true` stays enabled for multi-node startup isolation.
- `[node].filter_position_reports = false` keeps venue positions visible in balances and risk views.
- `[node.venues.BYBIT].recv_window_ms` is recommended at `20000` for live/demo startup reconciliation.
- `[node.venues.OKX].api_passphrase_env` is required for OKX live execution/data auth.
- Do not duplicate `[redis]` in per-node deploy files; nodes inherit it from `deploy/tokenmm/tokenmm.live.toml`.
- Do not duplicate `[portfolio]` in per-node deploy files; nodes inherit the shared portfolio inventory feed
  from `deploy/tokenmm/tokenmm.live.toml`.

## Inventory semantics

- `local_qty` is maker-leg inventory only for that strategy.
- `global_qty` is the shared TokenMM portfolio aggregate for the base asset.
- Each node publishes a maker-leg inventory component to Redis.
- `flux.runners.tokenmm.run_portfolio` aggregates those components into the shared
  portfolio inventory feed consumed by all TokenMM strategies.
- Live TokenMM nodes must not flatten positions on service stop or restart.
- `manage_stop = false` is required in each per-node strategy config.
- Flattening inventory must be an explicit operator action, not a service lifecycle side effect.

Each file is a complete node config consumed directly by `python -m flux.runners.tokenmm.run_node`.
Start from `tokenmm.strategy.template.toml`.

## Env conventions

- Production node lifecycle is managed from Pulse via flux@ units.
- Production secrets should be provided through `/etc/flux/common.env` plus `/etc/flux/tokenmm-node-*.env`.
- `deploy/tokenmm/tokenmm_stack.env` is for local paper/testnet smoke only.
- Node files should reference env var names such as `BYBIT_API_KEY`, `BINANCE_API_KEY`, and `OKX_API_KEY`, not inline secrets.
- Use the same `[flux].namespace` and `[flux].schema_version` as the shared API/bridge config.
- Pulse-managed node services pass `--shared-config deploy/tokenmm/tokenmm.live.toml` so node runners inherit
  the shared `[redis]` and `[portfolio]` tables.
