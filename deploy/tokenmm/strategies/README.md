# TokenMM `strategies` contract

This directory holds one TOML file per TokenMM node process used by
`scripts/deploy/tokenmm_stack.sh`.

## File naming

- Use the exact Flux strategy ID as the file name: `<flux_strategy_id>.toml`.
- Example: `bybit_linear_plumeusdt_makerv3_03.toml`.
- Include the actual execution venue, product type, symbol, and strategy family in the strategy ID.
- Keep exactly 5 strategy files for Phase 1 production (`TOKENMM_EXPECTED_NODES=5` default).

## Required TOML keys per file

- `[identity].strategy_id` and `[identity].strategy_instance_id` stay aligned to the file name.
- `[strategy].strategy_id` stays descriptive and unique across node processes.
- `[venues].execution_venue` and `[venues].reference_venue` identify the strategy routing roles.
- `[node.venues.<VENUE>].instrument_id` defines the instrument loaded for each venue client.
- `exec_reconciliation_lookback_mins` should stay bounded at `15` for shared-account startup safety.
- `[node].filter_unclaimed_external_orders = true` and `[node].filter_position_reports = true` stay enabled for multi-node startup isolation.
- `[node.venues.BYBIT].recv_window_ms` is recommended at `20000` for live/demo startup reconciliation.
- Do not duplicate `[redis]` in per-node deploy files; nodes inherit it from `deploy/tokenmm/tokenmm.live.toml`.

Each file is a complete node config consumed directly by `python -m nautilus_trader.flux.runners.tokenmm.run_node`.
Start from `tokenmm.strategy.template.toml`.

## Env conventions

- Secrets should be provided via `deploy/tokenmm/tokenmm_stack.env` or an explicit `TOKENMM_ENV_PATH`.
- `scripts/deploy/tokenmm_stack.sh` defaults to paper mode with execution disabled.
- Live trading is opt-in only with `TOKENMM_MODE=live`, `TOKENMM_CONFIRM_LIVE=1`, and `TOKENMM_ENABLE_EXECUTION=1`.
- Node files should reference env var names such as `BYBIT_API_KEY` and `BINANCE_API_KEY`, not inline secrets.
- Use the same `[flux].namespace` and `[flux].schema_version` as the shared API/bridge config.
- The stack passes `--shared-config deploy/tokenmm/tokenmm.live.toml` so node runners inherit the shared `[redis]` table.
