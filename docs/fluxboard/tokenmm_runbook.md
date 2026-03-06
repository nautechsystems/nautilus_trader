<!-- DOCID: docs/fluxboard/tokenmm_runbook@v1 -->

# TokenMM Serving Runbook

This runbook covers the two supported serving modes for the TokenMM Fluxboard surface.

## Prerequisites

1. Start Redis and the TokenMM stack if you need live data:

```bash
redis-server --port 6380
python -m nautilus_trader.flux.runners.tokenmm.run_node \
  --config deploy/tokenmm/strategies/bybit_linear_plumeusdt_makerv3_01.toml \
  --shared-config deploy/tokenmm/tokenmm.live.toml \
  --mode paper
python -m nautilus_trader.flux.runners.tokenmm.run_bridge --config deploy/tokenmm/tokenmm.live.toml --mode paper --all-strategies
```

   Or use the managed TokenMM stack:

```bash
cp deploy/tokenmm/tokenmm_stack.env.example \
  deploy/tokenmm/tokenmm_stack.env
# Fill credentials, then:
scripts/deploy/tokenmm_stack.sh start
```

The stack script can load credentials from AWS Secrets Manager by default:
`/nautilus/tokenmm/bybit` and `/nautilus/tokenmm/binance` (`TOKENMM_*_SECRET_ID` overrides).
2. Install frontend dependencies once:

```bash
pnpm --dir fluxboard install --frozen-lockfile
```

## Environment and ports

- FluxAPI runner default: `127.0.0.1:5022` when host is not explicitly set.
- Explicit host overrides: CLI `--host ...` or config `[api].host`.
- Vite dev server default: `127.0.0.1:5173`.
- Vite preview default: `127.0.0.1:4173`.
- Socket.IO path: `/socket.io`.
- TokenMM app base path in prod-like mode: `/tokenmm/`.
- Legacy alias routes: `/tokenm` and `/tokenm/*` redirect to `/tokenmm` for backward compatibility.

Frontend variables (set in `fluxboard/.env`, template: `fluxboard/.env.example`):

- `FLUXAPI_SCHEME`, `FLUXAPI_HOST`, `FLUXAPI_PORT`, `FLUXAPI_URL`: Vite proxy target.
- `VITE_BACKEND_URL=/`: forces same-origin socket URL so Vite proxies `/socket.io` in dev.
- `FLUXBOARD_BASE_PATH=/tokenmm/`: build base path for prod-like serving.

Backend runner variables (export in shell before running `nautilus_trader.flux.runners.tokenmm.run_api`; also documented as comments in `fluxboard/.env.example`):

- `FLUXBOARD_SERVE_DIST=1`: opt-in static serving in the `run_api` module.
- `FLUXBOARD_DIST`: optional override for built asset directory (default `<repo>/fluxboard/dist`).

## Option A (dev): Vite proxy mode

1. Prepare frontend env:

```bash
cp fluxboard/.env.example fluxboard/.env
```

2. Run FluxAPI:

```bash
python -m nautilus_trader.flux.runners.tokenmm.run_api \
  --config deploy/tokenmm/tokenmm.live.toml \
  --host 127.0.0.1 \
  --port 5022
```

3. Run Vite dev server:

```bash
pnpm --dir fluxboard dev
```

4. Open:

- `http://127.0.0.1:5173/tokenmm` (alias: `/tokenm`)

Expected behavior:

- `/api/*` and `/socket.io` requests from Vite are proxied to FluxAPI.
- TokenMM routes render from the dev server.

## Option B (prod-like): FluxAPI serves built SPA at `/tokenmm/*`

1. Build frontend:

```bash
pnpm --dir fluxboard build
```

2. Run FluxAPI with static serving opt-in:

```bash
python -m nautilus_trader.flux.runners.tokenmm.run_api \
  --config deploy/tokenmm/tokenmm.live.toml \
  --serve-fluxboard \
  --host 127.0.0.1 \
  --port 5022
```

Env equivalent:

```bash
FLUXBOARD_SERVE_DIST=1 python -m nautilus_trader.flux.runners.tokenmm.run_api \
  --config deploy/tokenmm/tokenmm.live.toml \
  --host 127.0.0.1 \
  --port 5022
```

3. Open:

- `http://127.0.0.1:5022/tokenmm` (alias: `/tokenm`)
- `http://127.0.0.1:5022/tokenmm/alerts` (deep-link SPA fallback)

Expected behavior:

- Built assets are served from `fluxboard/dist`.
- `/tokenmm/*` deep links return SPA HTML (including `/tokenmm/order-view`).
- `/tokenmm/order-view` remains unavailable at the UI level because TokenMM frontend route/nav excludes order-view.
- `/socket.io` remains served by the API runner.

## Quick smoke checks

Use these after startup:

```bash
curl -fsS http://127.0.0.1:5022/api/v1/healthz
curl -i http://127.0.0.1:5022/tokenmm
curl -i http://127.0.0.1:5022/tokenmm/alerts
# order-view is UI-forbidden. This check validates only SPA deep-link fallback behavior.
curl -i http://127.0.0.1:5022/tokenmm/order-view
curl -i "http://127.0.0.1:5022/socket.io/?EIO=4&transport=polling"
```

Expected highlights:

- `/tokenmm` and `/tokenmm/alerts`: `200`.
- `/tokenmm/order-view`: SPA HTML response (HTTP `200`), and the TokenMM UI must not expose or render order-view.
- `/socket.io/...`: handshake response (`200` with Engine.IO payload).

## Security notes

- Localhost defaults are intentional; only set `--host` or `[api].host` to non-localhost when you intentionally expose the service, and then use auth/TLS/network controls.
- Do not expose `/api/v1/*` or `/socket.io` on non-loopback without authentication, TLS, and a network allowlist. Mutation routes are especially high risk (`PATCH /api/v1/params`, `DELETE /api/v1/alerts`).
- `--serve-fluxboard`/`FLUXBOARD_SERVE_DIST` is explicit opt-in to avoid accidentally serving local build artifacts.
- Avoid setting wildcard `VITE_ALLOWED_HOSTS`; allow only required hosts.
