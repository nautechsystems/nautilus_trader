# MakerV3 (Flux Production Wrappers)

This directory contains thin runner wrappers for the production Flux modules under `nautilus_trader/flux`.

## What changed

- New runners:
  - `examples/live/makerv3/run_node.py`
  - `examples/live/makerv3/run_bridge.py`
  - `examples/live/makerv3/run_api.py`
- Removed legacy deprecated runner entrypoints in favor of these production wrappers.

## Safety defaults

1. Default mode is `paper`.
2. Live mode requires explicit `--confirm-live` on each runner.
3. No `eval`-based secret loading is used or required.
4. API runner binds to `127.0.0.1` by default; external exposure requires explicit host override.
5. If you bind to a non-loopback address, treat the API as an internal service and front it with TLS + auth + IP
   allowlist.

## Configuration

Default config file:

- `examples/live/makerv3/config/makerv3.toml`
- Live stack config template: `examples/live/makerv3/config/makerv3.live.toml`
- Live env template: `examples/live/makerv3/config/makerv3.live.env.example`
- Managed stack script: `scripts/deploy/makerv3_stack.sh`

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

Only source trusted env files. `source` executes shell syntax; keep env files as strict `KEY=VALUE` lines.

## Run order

1. Start Redis.

```bash
redis-server --port 6380
```

2. Start the Nautilus node (safe default `paper` mode).

```bash
python examples/live/makerv3/run_node.py
```

3. Start the Flux bridge.

```bash
python examples/live/makerv3/run_bridge.py
```

Bridge strategy scope behavior:

1. Default scope is `identity.strategy_id` from the config file.
2. Override with `--strategy-id <id>` for a single strategy.
3. Use `--all-strategies` to consume all strategy streams for the selected mode.
4. `--strategy-id` and `--all-strategies` are mutually exclusive.

4. Start the Flux API.

```bash
python examples/live/makerv3/run_api.py
```

By default, `run_api.py` binds to `127.0.0.1` unless you explicitly override host via:
- CLI: `--host ...`
- config: `[api].host` in `makerv3.toml`

Expose externally only when intentional, for example:

```bash
python examples/live/makerv3/run_api.py --host 0.0.0.0
```

## TokenMM serving modes

### Option A (dev): Vite proxy for `/api/*` and `/socket.io`

Run FluxAPI and Vite in separate terminals:

```bash
# Terminal 1 (FluxAPI)
python examples/live/makerv3/run_api.py --host 127.0.0.1 --port 5022

# Terminal 2 (Fluxboard dev server)
cp fluxboard/.env.example fluxboard/.env
pnpm --dir fluxboard dev
```

Then open `http://127.0.0.1:5173/tokenmm`.

Notes:
- Vite proxies `/api/*` and `/socket.io` using `FLUXAPI_*` variables from `fluxboard/.env`.
- Keep `VITE_BACKEND_URL=/` so Socket.IO stays same-origin and goes through the Vite proxy.

### Option B (prod-like): FluxAPI serves `fluxboard/dist` at `/tokenmm/*`

Build Fluxboard, then run the API with static serving enabled:

```bash
pnpm --dir fluxboard build
python examples/live/makerv3/run_api.py --serve-fluxboard --host 127.0.0.1 --port 5022
```

Equivalent env opt-in:

```bash
FLUXBOARD_SERVE_DIST=1 python examples/live/makerv3/run_api.py --host 127.0.0.1 --port 5022
```

`FLUXBOARD_SERVE_DIST`/`FLUXBOARD_DIST` are backend runner env vars (set in shell for `run_api.py`), not Vite-only vars.

Then open `http://127.0.0.1:5022/tokenmm` (deep links under `/tokenmm/*` use SPA fallback).

Security and behavior:
- Server returns SPA HTML fallback for `/tokenmm/*` deep links.
- `/tokenmm/order-view` remains unavailable because the frontend TokenMM route/nav excludes it.
- `FLUXBOARD_DIST` can override the built asset path (default: `<repo>/fluxboard/dist`).
- Localhost (`127.0.0.1`) is the default bind when host is not explicitly set; only use `--host` or `[api].host` to expose intentionally.
## Live mode (explicit confirmation required)

```bash
python examples/live/makerv3/run_node.py --mode live --confirm-live
python examples/live/makerv3/run_bridge.py --mode live --confirm-live
python examples/live/makerv3/run_api.py --mode live --confirm-live
```

Without `--confirm-live`, each runner fails fast in `live` mode.

## Production stack script (recommended)

Use the managed deployment script to run Redis + node + bridge + API/Fluxboard with logs and pid tracking:

```bash
cp examples/live/makerv3/config/makerv3.live.env.example \
  examples/live/makerv3/config/makerv3.live.env
# Fill BYBIT/BINANCE keys in makerv3.live.env

scripts/deploy/makerv3_stack.sh start
scripts/deploy/makerv3_stack.sh status
scripts/deploy/makerv3_stack.sh health
```

Useful controls:

```bash
scripts/deploy/makerv3_stack.sh logs api
scripts/deploy/makerv3_stack.sh logs node
scripts/deploy/makerv3_stack.sh stop
```

Safety/behavior:
1. Default mode is `live` for this script, with explicit `MAKERV3_CONFIRM_LIVE=1` gate.
2. Execution stays off by default (`MAKERV3_ENABLE_EXECUTION=0`) until you opt in.
3. Runtime files live under `.run/makerv3-prod/` (logs + pid files).
4. AWS secret loading is supported without `eval` via
   `MAKERV3_LOAD_AWS_SECRETS=1`, `MAKERV3_BYBIT_SECRET_ID`, `MAKERV3_BINANCE_SECRET_ID`.
