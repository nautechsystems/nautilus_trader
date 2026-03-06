# TokenMM production deploy config

This directory is the production deployment root for the 5-node PLUME TokenMM stack.

## Layout

- `tokenmm.live.toml`: shared bridge/API config plus the canonical TokenMM allowlist.
- `tokenmm_stack.env.example`: stack environment template for `scripts/deploy/tokenmm_stack.sh`.
- `strategies/`: one complete node TOML per deployed strategy, named by exact strategy ID.
- Runner modules:
  - `nautilus_trader.flux.runners.tokenmm.run_node`
  - `nautilus_trader.flux.runners.tokenmm.run_bridge`
  - `nautilus_trader.flux.runners.tokenmm.run_api`

## Intent

- `scripts/deploy/tokenmm_stack.sh` defaults to paper mode with execution disabled.
- Runtime flags win over the TOML [flux]/[node] values.
- Live trading is opt-in only when `TOKENMM_MODE=live`, `TOKENMM_CONFIRM_LIVE=1`, and `TOKENMM_ENABLE_EXECUTION=1` are all set together.
- Redis stays in `tokenmm.live.toml`; per-strategy node deploy files inherit it through the node runner `--shared-config` overlay.

## Startup

```bash
cp deploy/tokenmm/tokenmm_stack.env.example deploy/tokenmm/tokenmm_stack.env
scripts/deploy/tokenmm_stack.sh start
scripts/deploy/tokenmm_stack.sh health
```

## Safe paper smoke

The copied env template already starts the stack in a non-trading profile:

```bash
TOKENMM_MODE=paper \
TOKENMM_CONFIRM_LIVE=0 \
TOKENMM_ENABLE_EXECUTION=0 \
TOKENMM_ALLOW_MISSING_KEYS=1 \
scripts/deploy/tokenmm_stack.sh start
```

Smoke-check the portfolio surfaces directly:

- `GET /api/v1/params?profile=tokenmm`
- `GET /api/v1/balances?profile=tokenmm`
- `GET /api/v1/trades?profile=tokenmm`

Example:

```bash
curl -fsS http://127.0.0.1:5022/api/v1/params?profile=tokenmm
curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=tokenmm
curl -fsS http://127.0.0.1:5022/api/v1/trades?profile=tokenmm
scripts/deploy/tokenmm_stack.sh stop
```

Expected smoke result:

- `params` returns the 5 allowlisted strategy IDs in registry order.
- `balances` returns the shared `tokenmm` portfolio view plus component readiness metadata.
- `trades` may be empty in paper smoke; if rows are present they must retain allowlisted per-row `strategy_id` values.

## Live opt-in

Use an explicit one-shot override or set the same values in `deploy/tokenmm/tokenmm_stack.env`:

```bash
TOKENMM_MODE=live \
TOKENMM_CONFIRM_LIVE=1 \
TOKENMM_ENABLE_EXECUTION=1 \
scripts/deploy/tokenmm_stack.sh start
```
