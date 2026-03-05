# TokenMM `strategies.d` contract

This directory holds one TOML file per MakerV3 node process used by
`scripts/deploy/tokenmm_stack.sh`.

## File naming

- Recommended pattern: `NN-<flux_strategy_id>.toml` (for deterministic startup order).
- Example: `01-bybit_binance_plumeusdt_makerv3.toml`.
- Keep exactly 5 strategy files for Phase 1 production (`TOKENMM_EXPECTED_NODES=5` default).

## Required TOML keys per file

- `[identity].strategy_id` (Flux strategy ID, must be globally unique across all processes).
- `[identity].strategy_instance_id` (set equal to strategy_id for simplicity).
- `[strategy].strategy_id` (Nautilus StrategyId / order tag seed; recommended `MAKERV3-TMM-NN`).
- `[node].maker_instrument_id` and `[node].reference_instrument_id`.
- `[redis]` connection values aligned with bridge/API config.

Each file is a complete node config consumed directly by `examples/live/makerv3/run_node.py`.
Start from `tokenmm.strategy.template.toml`.
This directory also includes a ready-to-edit 5-node PLUME set matching the Phase 1 TokenMM allowlist.

## Env conventions

- Secrets stay in `examples/live/makerv3/config/makerv3.live.env` (or `TOKENMM_ENV_PATH`).
- Node files should reference env var names (for example `BYBIT_API_KEY`, `BINANCE_API_KEY`) rather than
  inline secrets.
- Use the same `[flux].namespace` and `[flux].schema_version` as API/bridge configs.
