# TokenMM production deploy config

This directory is the production deployment root for the current 4-node PLUME TokenMM stack.

## Layout

- `tokenmm.live.toml`: shared node, portfolio, bridge, and API config plus the canonical TokenMM allowlist.
- `tokenmm_stack.env.example`: stack environment template for `ops/scripts/deploy/tokenmm_stack.sh`.
- `strategies/`: one complete node TOML per deployed strategy, named by exact strategy ID.
- Runtime services:
  - `flux.runners.tokenmm.run_node`
  - `flux.runners.tokenmm.run_portfolio`
  - `flux.runners.tokenmm.run_bridge`
  - `flux.runners.tokenmm.run_api`
- Active production strategy topology:
  - `plumeusdt_bybit_perp_makerv3`
  - `plumeusdt_bybit_spot_makerv3`
  - `plumeusdt_okx_perp_makerv3`
  - `plumeusdt_binance_spot_makerv3`
- Disabled strategy configs stay in `strategies/*.toml.disabled` until they are re-enrolled.

## Intent

- Supported production lifecycle: install with systemd, then manage jobs from Pulse.
- `ops/scripts/deploy/tokenmm_stack.sh` is local smoke only and refuses live deploys.
- Live trading is opt-in only when `TOKENMM_MODE=live`, `TOKENMM_CONFIRM_LIVE=1`, and `TOKENMM_ENABLE_EXECUTION=1` are all set together.
- Redis stays in `tokenmm.live.toml`; per-strategy node deploy files inherit it through the node runner `--shared-config` overlay.
- Production Redis is the dedicated `tokenmm` ElastiCache endpoint; keep the auth token out of git and inject it with `TOKENMM_REDIS_PASSWORD`.
- All four active strategies price off Binance spot. The shared reference venue alias is `BINANCE_SPOT`.

## Inventory and balances model

- `Signal` mirrors exact per-strategy state from MakerV3.
- `local_qty` in strategy state is the maker leg only for that strategy.
- `global_qty` in strategy state is the TokenMM portfolio aggregate for the base asset.
- `run_portfolio` owns that aggregate. Each strategy publishes a maker-leg inventory component, and the
  sidecar recomputes the shared portfolio quantity in Redis.
- `GET /api/v1/balances?profile=tokenmm` remains a portfolio API projection built from allowlisted strategy
  balance snapshots.
- `GET /api/v1/balances?strategy=<id>` remains the per-strategy debug view.

:::info
The portfolio sidecar currently drives shared inventory semantics for MakerV3 strategy state. The balances
endpoint still performs API-side aggregation for the TokenMM portfolio view.
:::

## Production control plane

```bash
sudo ops/scripts/deploy/install_tokenmm_systemd.sh
sudoedit /etc/flux/common.env
sudo systemctl daemon-reload
sudo systemctl start flux-tokenmm.target
```

Runtime registration is explicit:

- `flux@.service` reads `/etc/flux/common.env` plus `/etc/flux/<service>.env`.
- Pulse lists only services whose env files set `PULSE_ENABLED=1`.
- The seeded TokenMM target enrolls `tokenmm-api`, `tokenmm-portfolio`, `tokenmm-bridge`, and the 4 active
  node services.
- Normal production start/stop/restart of services and nodes is supported through Pulse UI/API, not
  `tokenmm_stack.sh`.

Bootstrap or disaster recovery only:

```bash
sudo systemctl start flux-tokenmm.target
```

Primary operator surfaces:

- `http://<host>:5022/tokenmm`
- `http://<host>:5022/pulse`
- `GET /api/pulse/jobs`
- `POST /api/pulse/jobs/group/tokenmm/restart`

## Local smoke only

For a local paper/testnet bring-up outside systemd:

```bash
cp deploy/tokenmm/tokenmm_stack.env.example deploy/tokenmm/tokenmm_stack.env
TOKENMM_MODE=paper \
TOKENMM_CONFIRM_LIVE=0 \
TOKENMM_ENABLE_EXECUTION=0 \
TOKENMM_REDIS_PASSWORD=... \
TOKENMM_ALLOW_MISSING_KEYS=1 \
ops/scripts/deploy/tokenmm_stack.sh start
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
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
ops/scripts/deploy/tokenmm_stack.sh stop
```

Expected smoke result:

- `params` returns the 4 allowlisted strategy IDs in registry order.
- `signal` returns four per-strategy rows. Each row keeps its own `local_qty` and shares the same
  portfolio-scoped `global_qty`.
- `balances` returns the shared `tokenmm` portfolio view plus component readiness metadata.
- `trades` may be empty in paper smoke; if rows are present they must retain allowlisted per-row `strategy_id` values.
- `api/pulse/jobs` returns the enrolled local jobs and statuses when Pulse assets are served.
