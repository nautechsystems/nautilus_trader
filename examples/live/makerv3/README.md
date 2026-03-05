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
- Multi-node TokenMM stack script: `scripts/deploy/tokenmm_stack.sh`
- Multi-node strategy config contract: `examples/live/makerv3/config/strategies.d/README.md`

Review and update this file before running.

### TokenMM registry contract (Phase 1)

For Phase 1 multi-strategy deployment, TokenMM is explicitly scoped to these 5 PLUME MakerV3 Flux
strategy IDs (configured in `examples/live/makerv3/config/makerv3.live.toml`):

1. `bybit_binance_plumeusdt_makerv3`
2. `bybit_binance_plumeusdt_makerv3_02`
3. `bybit_binance_plumeusdt_makerv3_03`
4. `bybit_binance_plumeusdt_makerv3_04`
5. `bybit_binance_plumeusdt_makerv3_05`

Naming convention and ownership:

- Flux strategy ID = `[identity].strategy_id` (API/profile routing key).
- Base config default keeps `[strategy].strategy_id = "MAKERV3-001"` for single-node compatibility.
- Multi-node TokenMM deployments should use per-node unique order tags (for example `MAKERV3-TMM-NN`).
- Keep Flux IDs and Nautilus strategy IDs distinct and stable across processes.

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

## Multi-node TokenMM stack (5 nodes + bridge + API)

Create/update per-node configs in `examples/live/makerv3/config/strategies.d/`, then run:

```bash
scripts/deploy/tokenmm_stack.sh start
scripts/deploy/tokenmm_stack.sh status
scripts/deploy/tokenmm_stack.sh health
```

Notes:

1. Default mode for `tokenmm_stack.sh` is `paper` for local smoke. Use `TOKENMM_MODE=live` and
   `TOKENMM_CONFIRM_LIVE=1` for production rollout.
2. `health` checks:
   - `GET /api/v1/healthz`
   - `GET /tokenmm`
   - Socket.IO polling handshake (`/socket.io/?EIO=4&transport=polling`)
3. Runtime logs and pid files are under `.run/tokenmm-stack/`.

## Ops guardrails for multi-node shared-account deployment

1. Keep `[identity].strategy_id` (Flux) and `[strategy].strategy_id` (Nautilus StrategyId/order tags) unique
   across all node processes.
2. `run_node.py` wires Redis cache persistence by default using the `[redis]` config.
3. Production defaults are set in `makerv3.live.toml`:
   - `exec_reconciliation_startup_delay_secs = 10.0`
   - `exec_reconciliation_lookback_mins = 0`
4. `makerv3.toml` remains dev-oriented for reconciliation settings.
5. Keep cancellation boundaries strategy-owned; avoid cross-strategy cancel blasts.
6. Review execution filtering settings per node (`filter_unclaimed_external_orders`,
   `filter_position_reports`) and use `external_order_claims` only when intentionally adopting existing
   venue orders at startup.
7. Keep strategy callbacks non-blocking; avoid blocking I/O on the event loop thread.
8. Exclude `PENDING_CANCEL` orders when generating cancel batches to avoid duplicate cancel pressure.
