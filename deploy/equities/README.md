# Equities production deploy config

This directory is the deploy root for the dedicated `equities` stack.

## Layout

- `equities.live.toml`: shared node, portfolio, bridge, and API config plus the canonical equities allowlist.
- `equities_stack.env.example`: local paper/testnet smoke environment template for `ops/scripts/deploy/equities_stack.sh`.
- `strategies/`: one complete node TOML per enrolled stock strategy, named by exact strategy ID.
- Runtime services:
  - `flux.runners.equities.run_node`
  - `flux.runners.equities.run_portfolio`
  - `flux.runners.equities.run_bridge`
  - `flux.runners.equities.run_api`
- Systemd assets:
  - `systemd/flux-equities.target`
  - `systemd/common.env.example`

## Intent

- trade[XYZ] is represented as `HYPERLIQUID` plus `dex = "xyz"`.
- One stock uses one strategy file and one node process.
- preserve the outer equities surface: keep `/equities`, `profile=equities`, and `portfolio=equities` stable even if the inner strategy implementation changes later.
- Shared portfolio aggregation is scoped to `portfolio_id = "equities"`.
- Fluxboard and Pulse are served from the same API process at `/equities` and `/pulse`.
- `ops/scripts/deploy/equities_stack.sh` is local smoke only and refuses live deploys.
- Live trading is opt-in only when `EQUITIES_MODE=live`, `EQUITIES_CONFIRM_LIVE=1`, and `EQUITIES_ENABLE_EXECUTION=1` are set together through systemd/Pulse-managed services.

## Inventory and balances model

- `Signal` remains per-strategy MakerV3 state.
- `local_qty` remains per-stock strategy inventory.
- `global_qty` is the shared `equities` portfolio aggregate owned by `flux.runners.equities.run_portfolio`.
- `GET /api/v1/balances?profile=equities` is the portfolio projection across the allowlisted stock strategies.
- `GET /api/v1/balances?strategy=<id>` remains the per-strategy debug view.

## Production control plane

Install the systemd units and seeded env files:

```bash
sudo ops/scripts/deploy/install_equities_systemd.sh
sudoedit /etc/flux/common.env
sudo systemctl daemon-reload
sudo systemctl start flux-equities.target
```

Installer behavior:

- installs `flux@.service`
- installs `/etc/flux/common.env` from `deploy/equities/systemd/common.env.example` if it does not already exist
- installs `/etc/sudoers.d/flux-pulse` for the equities Pulse-managed service set
- writes `/etc/flux/equities-api.env`, `/etc/flux/equities-portfolio.env`, `/etc/flux/equities-bridge.env`
- writes one `/etc/flux/equities-node-<strategy_id>.env` per `deploy/equities/strategies/*.toml`
- rewrites `/etc/systemd/system/flux-equities.target` so the target enrolls every discovered equities node service

Runtime registration is explicit:

- `flux@.service` reads `/etc/flux/common.env` plus `/etc/flux/<service>.env`
- Pulse lists only services whose env files set `PULSE_ENABLED=1`
- The equities target enrolls `equities-api`, `equities-portfolio`, `equities-bridge`, and every discovered `equities-node-*` service
- The equities bridge consumes only the configured `api.equities_strategy_ids` scope by default.
- `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS` stay in `/etc/flux/common.env`; do not inline them into strategy TOMLs.

Primary operator surfaces:

- `http://<host>:5022/equities`
- `http://<host>:5022/pulse`
- `GET /api/pulse/jobs`
- `POST /api/pulse/jobs/group/equities/restart`
- `GET /api/v1/signals?profile=equities`
- `GET /api/v1/params?profile=equities`
- `GET /api/v1/balances?profile=equities`
- `GET /api/v1/trades?profile=equities`

## Local smoke only

For a local paper/testnet bring-up outside systemd:

```bash
cp deploy/equities/equities_stack.env.example deploy/equities/equities_stack.env
EQUITIES_MODE=paper \
EQUITIES_CONFIRM_LIVE=0 \
EQUITIES_ENABLE_EXECUTION=0 \
TRADE_XYZ_AGENT_PK=... \
TRADE_XYZ_ACCOUNT_ADDRESS=... \
ops/scripts/deploy/equities_stack.sh start
```

Optional local secret loading:

- Set `EQUITIES_LOAD_AWS_SECRETS=1` and `EQUITIES_TRADE_XYZ_SECRET_ID=...` to load `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS` from AWS Secrets Manager before credential validation.

Smoke-check the profile surfaces directly:

```bash
curl -fsS http://127.0.0.1:5022/api/v1/params?profile=equities
curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=equities
curl -fsS http://127.0.0.1:5022/api/v1/trades?profile=equities
curl -fsS http://127.0.0.1:5022/api/v1/alerts?profile=equities
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
ops/scripts/deploy/equities_stack.sh stop
```

Expected smoke result:

- `params` returns the equities allowlist in config order.
- `signal` returns one row per enrolled equities strategy.
- `balances` returns the shared `equities` portfolio view plus component readiness metadata.
- `trades` may be empty in paper smoke; if rows are present they retain per-row `strategy_id` values for the enrolled stock strategies.
- `alerts` returns profile-scoped alerts only for the enrolled equities strategies.

Fluxboard contract reference:

- See `fluxboard/docs/equities_contract.md` for the frozen HTTP route and payload expectations behind the `/equities` surface.
