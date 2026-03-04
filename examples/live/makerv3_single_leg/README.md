# MakerV3 Single-Leg (Flux Production Wrappers)

This directory contains thin runner wrappers for the production Flux modules under `nautilus_trader/flux`.

## What changed

- New runners:
  - `examples/live/makerv3_single_leg/run_node.py`
  - `examples/live/makerv3_single_leg/run_bridge.py`
  - `examples/live/makerv3_single_leg/run_api.py`
- Deprecated old POC runner entrypoints under `examples/live/poc/*`.

## Safety defaults

1. Default mode is `paper`.
2. Live mode requires explicit `--confirm-live` on each runner.
3. No `eval`-based secret loading is used or required.

## Configuration

Default config file:

- `examples/live/makerv3_single_leg/config/makerv3_single_leg.toml`

Review and update this file before running.

## Secrets loading (explicit only)

Use explicit environment loading, for example:

```bash
cat > .env.makerv3 <<'ENV'
BYBIT_API_KEY=...
BYBIT_API_SECRET=...
BINANCE_API_KEY=...
BINANCE_API_SECRET=...
ENV

set -a
source ./.env.makerv3
set +a
```

## Run order

1. Start Redis.

```bash
redis-server
```

2. Start the Nautilus node (safe default `paper` mode).

```bash
python examples/live/makerv3_single_leg/run_node.py
```

3. Start the Flux bridge.

```bash
python examples/live/makerv3_single_leg/run_bridge.py
```

4. Start the Flux API.

```bash
python examples/live/makerv3_single_leg/run_api.py
```

## Live mode (explicit confirmation required)

```bash
python examples/live/makerv3_single_leg/run_node.py --mode live --confirm-live
python examples/live/makerv3_single_leg/run_bridge.py --mode live --confirm-live
python examples/live/makerv3_single_leg/run_api.py --mode live --confirm-live
```

Without `--confirm-live`, each runner fails fast in `live` mode.
