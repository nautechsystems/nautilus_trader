# Equities production deploy config

This directory is the deploy root for the dedicated `equities` stack.

## Layout

- `equities.live.toml`: shared Redis, portfolio, bridge, API, and contract metadata plus the canonical equities allowlist.
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
- The active checked-in canary is `aapl_tradexyz_makerv4`; `aapl_tradexyz_makerv3.toml.disabled` is rollback material only and is deliberately not discovered by the installer.
- Shared portfolio aggregation is scoped to `portfolio_id = "equities"`.
- On the shared TokenMM host, Pulse is served by `tokenmm-api` at `/pulse` and manages the enrolled equities services from the same `/etc/flux` registry.
- The shared host also runs an internal-only `equities-api` backend on loopback so `/equities` can read the dedicated equities Redis store without exposing a second public API port.
- `ops/scripts/deploy/equities_stack.sh` is local smoke only and refuses live deploys.
- Live trading is opt-in only when `EQUITIES_MODE=live`, `EQUITIES_CONFIRM_LIVE=1`, and `EQUITIES_ENABLE_EXECUTION=1` are set together through systemd/Pulse-managed services.

## March 11, 2026 live contract freeze

- Treat MakerV4 as the current equities contract. `deploy/equities/equities.live.toml` keeps `api.strategy_class = "maker_v4"` / `param_set = "makerv4"` and allowlists only `aapl_tradexyz_makerv4`.
- Treat `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled` as rollback-only material. Do not re-enable it unless you are intentionally rolling the host back off MakerV4.
- On the shared `tokenmm-api` host, `/equities` is a proxied SPA entry route, not the asset owner. That public HTML shell must load Fluxboard assets from the neutral shared prefix `/static/fluxboard/assets/*`; any `/tokenmm/assets/*` reference means the host is serving the wrong stale/shared dist bundle.
- The standalone `flux.runners.equities.run_api` route still serves its own built assets from `/equities/assets/*` when hit directly. Do not treat `/static/fluxboard/assets/*` as a universal standalone-runner contract yet.
- The March 11 live host drift to watch for is `/etc/flux/equities-api.env` or `/etc/flux/equities-node-*.env` pointing at `/.worktrees/makerv3-mono-pr` with `--mode paper` instead of the intended live checkout and flags.

## MakerV4 contract

- `deploy/equities/equities.live.toml` keeps `/equities` stable while `api.strategy_class = "maker_v4"`, the equities allowlist points only to `aapl_tradexyz_makerv4`, and the shared contract metadata publishes `AAPL.NASDAQ` for the active IBKR leg.
- The shared config merge only imports `redis` and `portfolio`, so active node settings live in `deploy/equities/strategies/*.toml`, not in `deploy/equities/equities.live.toml`.
- The `/equities` API contract catalog is built from the shared `[[contracts]]` entries, so that shared IBKR contract entry must mirror the active canary route from `deploy/equities/strategies/*.toml`.
- Per-node configs use `[strategy].param_set = "makerv4"` and keep `strategy_groups = "equities"`.
- Hyperliquid effective account precedence remains `vault_address_env`, then funded `account_address_env`, then agent-wallet master resolution. Production hosts should keep `TRADE_XYZ_AGENT_PK`, `TRADE_XYZ_ACCOUNT_ADDRESS`, and optional `TRADE_XYZ_VAULT_ADDRESS` in `/etc/flux/common.env`.
- MakerV4 after-hours rollout is explicit in the active node files: `node.venues.IBKR.instrument_id = "AAPL.NASDAQ"`, `node.venues.IBKR.use_regular_trading_hours = false`, `strategy.outside_rth_hedge_enabled = true`, and `strategy.ibkr_primary_exchange = "NASDAQ"`.
- The AAPL canary default is `ibkr_primary_exchange = "NASDAQ"`. The current runner contract does not expose a separate IBKR route-exchange field, so keep the reference instrument on the qualifiable listing venue and do not set `BLUEOCEAN` as `instrument_id`.
- IBKR hedge fees are not live-discovered. MakerV4 quotes use runtime param `assumed_hedge_fee_bps` as an operator-managed assumption, with the current default seeded from runtime params at `1.0` bps.

## Inventory and balances model

- `Signal` remains per-strategy MakerV4 state for the active equities rollout.
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
- writes `/etc/flux/equities-api.env` for the internal loopback backend (`PULSE_ENABLED=0`, `127.0.0.1:5024`)
- writes `/etc/flux/equities-portfolio.env`, `/etc/flux/equities-bridge.env`
- writes one `/etc/flux/equities-node-<strategy_id>.env` per `deploy/equities/strategies/*.toml`
- rewrites `/etc/systemd/system/flux-equities.target` so the target enrolls every discovered equities node service

Runtime registration is explicit:

- `flux@.service` reads `/etc/flux/common.env` plus `/etc/flux/<service>.env`
- Production logs are journal-first. Keep `FLUX_LOG_LEVEL` in `/etc/flux/common.env` as the shared default and use
  `FLUX_NODE_LOG_LEVEL`, `FLUX_BRIDGE_LOG_LEVEL`, `FLUX_PORTFOLIO_LOG_LEVEL`, or `FLUX_API_LOG_LEVEL` only for
  role-specific overrides.
- Pulse lists only services whose env files set `PULSE_ENABLED=1`
- The equities target enrolls `equities-api`, `equities-portfolio`, `equities-bridge`, and every discovered `equities-node-*` service
- The equities bridge consumes only the configured `api.equities_strategy_ids` scope by default.
- Production hosts should inject the dedicated equities ElastiCache endpoint through `EQUITIES_REDIS_HOST`, `EQUITIES_REDIS_PORT`, `EQUITIES_REDIS_USERNAME`, `EQUITIES_REDIS_PASSWORD`, and `EQUITIES_REDIS_SSL` in `/etc/flux/common.env`.
- `TRADE_XYZ_AGENT_PK`, `TRADE_XYZ_ACCOUNT_ADDRESS`, and optional `TRADE_XYZ_VAULT_ADDRESS` stay in `/etc/flux/common.env`; do not inline them into strategy TOMLs.
- Shared-host Pulse control lives at `tokenmm-api`; the equities installer does not provision a second public API on `:5022`.
- Set `EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024` in `/etc/flux/common.env` so the public `tokenmm-api` process can proxy `/equities`, equities-profile `/api/v1/*`, and equities-profile `/socket.io` to the hidden backend.

Required host sanity checks after install or repoint:

- `sed -n '1,120p' /etc/flux/equities-api.env`
- `sed -n '1,120p' /etc/flux/equities-node-aapl_tradexyz_makerv4.env`
- `curl -fsS http://127.0.0.1:5022/equities | rg '/static/fluxboard/assets/|/tokenmm/assets/|/equities/assets/'`

Expected results:

- the generated envs point at the intended live checkout, not `/.worktrees/makerv3-mono-pr`
- equities API and node commands use `--mode live --confirm-live` for the production path
- on the shared `tokenmm-api` host, public `/equities` emits `/static/fluxboard/assets/*`, never `/tokenmm/assets/*`

Primary operator surfaces:

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
TWS_USERNAME=... \
TWS_PASSWORD=... \
ops/scripts/deploy/equities_stack.sh start
```

Local smoke logs live under `.run/equities-stack/logs`. The script now rotates a log before append when it exceeds the
configured size budget and keeps only a bounded number of rotated files. Use `EQUITIES_LOCAL_LOG_MAX_MB` and
`EQUITIES_LOCAL_LOG_KEEP` to override the defaults.

Optional local secret loading:

- Set `EQUITIES_LOAD_AWS_SECRETS=1` and `EQUITIES_TRADE_XYZ_SECRET_ID=...` to load `TRADE_XYZ_AGENT_PK`, `TRADE_XYZ_ACCOUNT_ADDRESS`, and optional `TRADE_XYZ_VAULT_ADDRESS` from AWS Secrets Manager before credential validation.
- If the active strategy keeps `node.venues.IBKR.dockerized_gateway`, local smoke also requires `TWS_USERNAME` and `TWS_PASSWORD` in the env file or shell before `equities_stack.sh start`.

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

## After-hours production validation

- Confirm the active strategy file keeps `use_regular_trading_hours = false` so IBKR reference data remains available outside RTH.
- Confirm `outside_rth_hedge_enabled = true` on the active MakerV4 strategy before enabling execution.
- Confirm outside-RTH fills are actually available on the configured route before enabling execution.
- Confirm the active strategy keeps a qualifiable IBKR reference instrument. The checked-in AAPL canary uses `AAPL.NASDAQ`; do not switch the instrument ID to `BLUEOCEAN`.
- Confirm the IBKR account has the required after-hours permissions for the configured exchange and instrument.
- Confirm operators understand that `assumed_hedge_fee_bps` is not live-discovered and should be reviewed explicitly before live rollout.

Fluxboard contract reference:

- See `fluxboard/docs/equities_contract.md` for the frozen `/equities` route and payload expectations when an equities API is deployed separately from the shared-host Pulse control plane.

## Rollback

- Disable MakerV4 cleanly by removing `aapl_tradexyz_makerv4` from `api.equities_strategy_ids` / `api.equities_required_strategy_ids`, rerunning `ops/scripts/deploy/install_equities_systemd.sh`, and stopping `flux@equities-node-aapl_tradexyz_makerv4.service`.
- Emergency MakerV3 re-enable remains available through `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled`. Restore it to `.toml`, retire the MakerV4 file from discovery, switch the shared allowlist/strategy metadata back to MakerV3, rerun the installer, then `systemctl daemon-reload` and restart `flux-equities.target`.
- `/equities`, `profile=equities`, and `portfolio=equities` stay stable during rollback. The user-facing surface does not change, but the internal strategy family, params schema, and signal telemetry revert with the strategy file swap.
