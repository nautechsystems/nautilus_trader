<!-- DOCID: apps/fluxboard/docs/tokenmm_runbook@v1 -->

# TokenMM Serving and Control Runbook

This runbook covers FluxAPI, Fluxboard, and the Pulse control surface for TokenMM.

The supported production lifecycle is Pulse-first: install the managed `flux@` services once, then start, stop,
restart, inspect, and read logs from `/pulse` or `/api/pulse/*`. Direct runner invocations and
`ops/scripts/deploy/tokenmm_stack.sh` remain local-only smoke paths.

## Prerequisites

1. Start Redis and the TokenMM stack if you need live data:

```bash
redis-server --port 6380
python -m flux.runners.tokenmm.run_node \
  --config deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml \
  --shared-config deploy/tokenmm/tokenmm.live.toml \
  --mode paper
python -m flux.runners.tokenmm.run_portfolio \
  --config deploy/tokenmm/tokenmm.live.toml \
  --mode paper
python -m flux.runners.tokenmm.run_bridge --config deploy/tokenmm/tokenmm.live.toml --mode paper --all-strategies
```

   Or use the local smoke wrapper:

```bash
cp deploy/tokenmm/tokenmm_stack.env.example \
  deploy/tokenmm/tokenmm_stack.env
# Fill credentials, then:
ops/scripts/deploy/tokenmm_stack.sh start
```

The stack script can load credentials from AWS Secrets Manager by default:
`/nautilus/tokenmm/bybit`, `/nautilus/tokenmm/binance`, and `/nautilus/tokenmm/okx`
(`TOKENMM_*_SECRET_ID` overrides).
2. Install frontend dependencies once:

```bash
pnpm --dir fluxboard install --frozen-lockfile
pnpm --dir pulse-ui install --frozen-lockfile
```

## Environment and ports

- FluxAPI runner default: `127.0.0.1:5022` when host is not explicitly set.
- Explicit host overrides: CLI `--host ...` or config `[api].host`.
- Vite dev server default: `127.0.0.1:5173`.
- Vite preview default: `127.0.0.1:4173`.
- Socket.IO path: `/socket.io`.
- TokenMM app base path in prod-like mode: `/tokenmm/`.
- Pulse app base path in prod-like mode: `/pulse/`.
- Legacy alias routes: `/tokenm` and `/tokenm/*` redirect to `/tokenmm` for backward compatibility.

Frontend variables (set in `fluxboard/.env`, template: `fluxboard/.env.example`):

- `FLUXAPI_SCHEME`, `FLUXAPI_HOST`, `FLUXAPI_PORT`, `FLUXAPI_URL`: Vite proxy target.
- `VITE_BACKEND_URL=/`: forces same-origin socket URL so Vite proxies `/socket.io` in dev.
- `FLUXBOARD_BASE_PATH=/tokenmm/`: build base path for prod-like serving.

Backend runner variables (export in shell before running `flux.runners.tokenmm.run_api`; also documented as comments in `fluxboard/.env.example`):

- `FLUXBOARD_SERVE_DIST=1`: opt-in static serving in the `run_api` module.
- `FLUXBOARD_DIST`: optional override for built asset directory (default `<repo>/fluxboard/dist`).
- `PULSE_SERVE_DIST=1`: opt-in static serving for the Pulse SPA in the `run_api` module.
- `PULSE_DIST`: optional override for built asset directory (default `<repo>/pulse-ui/dist`).
- `PULSE_ENV_DIR=/etc/flux`: optional override for the enrolled Pulse env-file registry.
- `PULSE_SELF_SERVICE_ID=tokenmm-api`: required on the serving API unit if Pulse should safely defer self-stop and self-restart actions.

## Option A (dev): Vite proxy mode

1. Prepare frontend env:

```bash
cp fluxboard/.env.example fluxboard/.env
```

2. Run FluxAPI:

```bash
python -m flux.runners.tokenmm.run_api \
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

## Option B (prod-like): FluxAPI serves built SPAs at `/tokenmm/*` and `/pulse/*`

1. Build frontend assets:

```bash
pnpm --dir fluxboard build
pnpm --dir pulse-ui build
```

2. Run FluxAPI with static serving opt-in:

```bash
python -m flux.runners.tokenmm.run_api \
  --config deploy/tokenmm/tokenmm.live.toml \
  --serve-fluxboard \
  --serve-pulse \
  --host 127.0.0.1 \
  --port 5022
```

Env equivalent:

```bash
FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 python -m flux.runners.tokenmm.run_api \
  --config deploy/tokenmm/tokenmm.live.toml \
  --host 127.0.0.1 \
  --port 5022
```

3. Open:

- `http://127.0.0.1:5022/tokenmm` (alias: `/tokenm`)
- `http://127.0.0.1:5022/tokenmm/alerts` (deep-link SPA fallback)
- `http://127.0.0.1:5022/pulse`
- `http://127.0.0.1:5022/pulse/jobs/tokenmm-api`

Expected behavior:

- Built assets are served from `fluxboard/dist`.
- Built Pulse assets are served from `pulse-ui/dist`.
- `/tokenmm/*` deep links return SPA HTML (including `/tokenmm/order-view`).
- `/pulse/*` deep links return SPA HTML.
- `/tokenmm/order-view` remains unavailable at the UI level because TokenMM frontend route/nav excludes order-view.
- `/socket.io` remains served by the API runner.

## Option C (prod): systemd bootstrap, Pulse operations

1. Install the host artifacts:

```bash
sudo ops/scripts/deploy/install_tokenmm_systemd.sh
```

2. Fill host secrets and any per-service overrides:

```bash
sudoedit /etc/flux/common.env
sudo systemctl daemon-reload
```

The installer seeds:

- `flux@.service`
- `flux-tokenmm.target`
- `/etc/flux/common.env`
- `/etc/flux/tokenmm-api.env`
- `/etc/flux/tokenmm-portfolio.env`
- `/etc/flux/tokenmm-bridge.env`
- `/etc/flux/tokenmm-node-*.env`
- `/etc/sudoers.d/flux-pulse`

Each TokenMM env file also pins `WORKDIR`, `PYTHONPATH`, and the checkout `.venv/bin/python`, so a TokenMM rollout
does not require repointing the shared `/etc/flux/common.env`.

3. Bootstrap the deployment if this host is coming up cold:

```bash
sudo systemctl start flux-tokenmm.target
```

This `systemctl` path is bootstrap or disaster recovery only. Routine start/stop/restart of services and nodes is
supported through Pulse.

4. Use Pulse in the browser or API:

- `http://127.0.0.1:5022/pulse`
- `GET http://127.0.0.1:5022/api/pulse/jobs`
- `POST http://127.0.0.1:5022/api/pulse/jobs/group/tokenmm/restart`

## Telemetry Cutover

1. Prepare the rollout checkout before touching systemd envs:

```bash
make build
pnpm --dir fluxboard install --frozen-lockfile
pnpm --dir fluxboard build
pnpm --dir pulse-ui install --frozen-lockfile
pnpm --dir pulse-ui build
.venv/bin/python ops/scripts/deploy/tokenmm_rollout_preflight.py
```

2. Create the local telemetry directory on the host:

```bash
sudo install -d -o ubuntu -g ubuntu /var/lib/nautilus/telemetry/tokenmm
```

3. Restart the live services in this order so portfolio aggregation and bridge consumers are ready before the
   seven execution nodes:

```bash
sudo systemctl restart flux@tokenmm-portfolio.service
sudo systemctl restart flux@tokenmm-bridge.service
sudo systemctl restart flux@tokenmm-api.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bybit_spot_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_okx_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_binance_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_binance_spot_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bitget_perp_makerv3.service
sudo systemctl restart flux@tokenmm-node-plumeusdt_bitget_spot_makerv3.service
```

4. Verify that persistence is live:

```bash
sqlite3 /var/lib/nautilus/telemetry/tokenmm/orders.sqlite "SELECT COUNT(*) FROM order_action;"
sqlite3 /var/lib/nautilus/telemetry/tokenmm/fills.sqlite "SELECT COUNT(*) FROM execution_fill;"
sqlite3 /var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite "SELECT COUNT(*) FROM quote_cycle;"
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
```

5. Optional localhost-only JupyterLab for research:

```bash
sudo systemctl start flux@tokenmm-jupyter.service
sudo journalctl -u flux@tokenmm-jupyter.service -n 20 --no-pager
```

Open `http://127.0.0.1:8888/lab` or use the logged tokenized URL, then load `research/tokenmm/notebooks/tokenmm_trade_data.ipynb`.

## Quick smoke checks

Use these after startup:

```bash
curl -fsS http://127.0.0.1:5022/api/v1/healthz
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
curl -i http://127.0.0.1:5022/tokenmm
curl -i http://127.0.0.1:5022/tokenmm/alerts
curl -i http://127.0.0.1:5022/pulse
# order-view is UI-forbidden. This check validates only SPA deep-link fallback behavior.
curl -i http://127.0.0.1:5022/tokenmm/order-view
curl -i "http://127.0.0.1:5022/socket.io/?EIO=4&transport=polling"
```

Expected highlights:

- `/tokenmm` and `/tokenmm/alerts`: `200`.
- `/pulse`: `200`.
- `/api/pulse/jobs`: `200` with enrolled jobs when `/etc/flux/*.env` exists, otherwise an empty list.
- `/tokenmm/order-view`: SPA HTML response (HTTP `200`), and the TokenMM UI must not expose or render order-view.
- `/socket.io/...`: handshake response (`200` with Engine.IO payload).

## Security notes

- Localhost defaults are intentional; only set `--host` or `[api].host` to non-localhost when you intentionally expose the service, and then use auth/TLS/network controls.
- Do not expose `/api/v1/*` or `/socket.io` on non-loopback without authentication, TLS, and a network allowlist. Mutation routes are especially high risk (`PATCH /api/v1/params`, `DELETE /api/v1/alerts`).
- Do not expose `/api/pulse/*` on non-loopback without strong network controls. These routes can start, stop, and restart production services and return recent logs.
- `--serve-fluxboard`/`FLUXBOARD_SERVE_DIST` and `--serve-pulse`/`PULSE_SERVE_DIST` are explicit opt-in to avoid accidentally serving local build artifacts.
- Avoid setting wildcard `VITE_ALLOWED_HOSTS`; allow only required hosts.
