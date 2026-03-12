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
- The active checked-in equities target is the MakerV3 trade[XYZ] stock universe: `aapl`, `amd`, `amzn`, `baba`, `coin`, `crcl`, `crwv`, `googl`, `hood`, `hyundai`, `intc`, `meta`, `mstr`, `msft`, `mu`, `nflx`, `nvda`, `orcl`, `pltr`, `rivn`, `sndk`, `tsm`, `tsla`, and `usar`.
- `aapl_tradexyz_makerv4.toml.disabled` is rollback/canary material only.
- Shared portfolio aggregation is scoped to `portfolio_id = "equities"`.
- `deploy/equities/equities.live.toml` now carries a shared `[[strategy_contracts]]` manifest as the canonical source of truth for `strategy_id`, `portfolio_asset_id`, venue instrument mapping, and shared account scope ids.
- `deploy/equities/equities.live.toml` also carries shared `[[account_scopes]]` rows as the canonical profile-owned venue account provider contract for `hyperliquid.xyz.main`, `ibkr.reference.main`, and `ibkr.hedge.main`.
- On the shared TokenMM host, Pulse is served by `tokenmm-api` at `/pulse` and manages the enrolled equities services from the same `/etc/flux` registry.
- The shared host also runs an internal-only `equities-api` backend on loopback so `/equities` can read the dedicated equities Redis store without exposing a second public API port.
- `ops/scripts/deploy/equities_stack.sh` is local smoke only and refuses live deploys.
- Live trading is opt-in only when `EQUITIES_MODE=live`, `EQUITIES_CONFIRM_LIVE=1`, and `EQUITIES_ENABLE_EXECUTION=1` are set together through systemd/Pulse-managed services.

## March 11, 2026 MakerV3 contract

- MakerV3 is the user-confirmed equities deploy contract. The checked-in repo keeps `deploy/equities/equities.live.toml` on `api.strategy_class = "maker_v3"` / `param_set = "makerv3"` and allowlists the exact-qualified 24-stock universe.
- `deploy/equities/strategies/aapl_tradexyz_makerv4.toml.disabled` remains available as rollback/canary material, but it is not part of normal installer discovery.
- The live host may still be temporarily drifted toward MakerV4 until the next reinstall/restart sequence. Treat any active `flux@equities-node-aapl_tradexyz_makerv4.service` state as live drift, not as the intended repo contract.
- On the shared `tokenmm-api` host, `/equities` is a proxied SPA entry route, not the asset owner. That public HTML shell must load Fluxboard assets from the neutral shared prefix `/static/fluxboard/assets/*`; any `/tokenmm/assets/*` reference means the host is serving the wrong stale/shared dist bundle.
- The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`.
- The March 11 live host drift to watch for is `/etc/flux/equities-api.env` or `/etc/flux/equities-node-*.env` pointing at `/.worktrees/makerv3-mono-pr` with `--mode paper` instead of the intended live checkout and flags.

## MakerV3 deploy contract

- `deploy/equities/equities.live.toml` keeps `/equities` stable while `api.strategy_class = "maker_v3"`, the equities allowlist points to the enrolled stock strategy set, and the shared contract metadata publishes one Hyperliquid and one IBKR contract row per enrolled stock.
- Each `[[strategy_contracts]]` row binds one strategy-local id to one canonical `portfolio_asset_id`, one Hyperliquid maker leg, one IBKR reference leg, and the shared account scopes (`execution_account_scope_id`, `reference_account_scope_id`, optional `hedge_account_scope_id`) that later profile-owned runners will consume.
- Each `[[account_scopes]]` row defines the shared provider config for one profile-owned account scope so the portfolio runner can build shared Hyperliquid/IBKR account projections without scraping one arbitrary node TOML.
- The shared config merge only imports `redis`, `portfolio`, `[[strategy_contracts]]`, and `[[account_scopes]]`, so active node settings live in `deploy/equities/strategies/*.toml` while canonical asset/account contracts stay centralized in `deploy/equities/equities.live.toml`.
- The `/equities` API contract catalog is built from the shared `[[contracts]]` entries, so each shared IBKR contract entry must mirror an enrolled route from `deploy/equities/strategies/*.toml`.
- The old single-canary wording still applies as a safety invariant: shared IBKR contract entry must mirror the active canary route before that route is added to the enrolled stock set.
- Hyperliquid effective account precedence remains `vault_address_env`, then funded `account_address_env`, then agent-wallet master resolution. Production hosts should keep `TRADE_XYZ_AGENT_PK`, `TRADE_XYZ_ACCOUNT_ADDRESS`, and optional `TRADE_XYZ_VAULT_ADDRESS` in `/etc/flux/common.env`.
- The checked-in MakerV3 nodes keep listing-venue IBKR instrument IDs such as `AAPL.NASDAQ`, `005380.KRX`, and `USAR.NASDAQ`, plus `node.venues.IBKR.use_regular_trading_hours = false`. `ibkr.reference.main` is the only equities IBKR gateway owner; enrolled nodes keep a non-owning `[node.venues.IBKR.dockerized_gateway]` block with `manage_container = false` so they connect to the shared gateway without starting or restarting it.
- Keep the reference instrument on the qualifiable listing venue and do not set `BLUEOCEAN` as `instrument_id`.
- `SMSN` and `SKHX` remain intentionally out of the enrolled set until exact IBKR qualification is verified; do not guess those routes into the live allowlist.

## Inventory and balances model

- `Signal` is currently served as per-strategy MakerV4 state on host, but that is drift relative to the intended MakerV3 target.
- `local_qty` remains per-stock strategy inventory.
- `global_qty` is the shared `equities` portfolio aggregate owned by `flux.runners.equities.run_portfolio`.
- `GET /api/v1/balances?profile=equities` is the portfolio projection across the allowlisted stock strategies.
- `GET /api/v1/balances?strategy=<id>` remains the per-strategy debug view.
- The current live balances payload may still use the legacy shared-row marker `scope = "shared_account"`.
- Later balance-model tasks will add `source_scope`, `account_scope_id`, and `source_strategy_ids` so shared IBKR or Hyperliquid account rows can carry explicit provenance without pretending to belong to one `strategy_id`.
- `strategy_id` stays strategy-local metadata. Once those later tasks land, shared rows should anchor identity on `portfolio_asset_id` plus the account scope fields above.

## Production control plane

Install the systemd units and seeded env files:

```bash
uv sync --all-groups --all-extras
sudo ops/scripts/deploy/install_equities_systemd.sh
sudoedit /etc/flux/common.env
sudo systemctl daemon-reload
sudo systemctl start flux-equities.target
```

Run `uv sync --all-groups --all-extras` in the selected checkout first so the installer can pin the checkout-local `.venv/bin/python` into every generated equities env file.

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
- Production equities Redis is the dedicated ElastiCache replication group `equities` in `ap-southeast-1`.
- The write endpoint is `master.equities.wapqos.apse1.cache.amazonaws.com:6379` and the read endpoint is `replica.equities.wapqos.apse1.cache.amazonaws.com:6379`.
- The production group is `cache.r7g.large`, `cluster mode disabled`, `Multi-AZ enabled`, `transit encryption required`, and `auth token enabled`.
- Production hosts should inject that dedicated equities ElastiCache endpoint through `EQUITIES_REDIS_HOST`, `EQUITIES_REDIS_PORT`, `EQUITIES_REDIS_USERNAME`, `EQUITIES_REDIS_PASSWORD`, and `EQUITIES_REDIS_SSL` in `/etc/flux/common.env`.
- `TRADE_XYZ_AGENT_PK`, `TRADE_XYZ_ACCOUNT_ADDRESS`, and optional `TRADE_XYZ_VAULT_ADDRESS` stay in `/etc/flux/common.env`; do not inline them into strategy TOMLs.
- Shared-host Pulse control lives at `tokenmm-api`; the equities installer does not provision a second public API on `:5022`.
- Set `EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024` in `/etc/flux/common.env` so the public `tokenmm-api` process can proxy `/equities`, equities-profile `/api/v1/*`, and equities-profile `/socket.io` to the hidden backend.

Required host sanity checks after install or repoint:

- `sed -n '1,120p' /etc/flux/equities-api.env`
- `sed -n '1,120p' /etc/flux/equities-portfolio.env`
- `sed -n '1,120p' /etc/flux/equities-bridge.env`
- `sed -n '1,120p' /etc/flux/equities-node-aapl_tradexyz_makerv3.env`
- `find /etc/flux -maxdepth 1 -type f -name 'equities-node-*.env' -print | sort`
- `for env_path in /etc/flux/equities-node-*.env; do sed -n '1,120p' "$env_path"; done`
- `curl -fsS http://127.0.0.1:5022/equities | rg '/static/fluxboard/assets/|/tokenmm/assets/|/equities/assets/'`

Expected results:

- the generated envs point at the intended live checkout, not `/.worktrees/makerv3-mono-pr`
- the generated envs append `WORKDIR=` / `PYTHONPATH=` for the selected checkout so the service-level provenance stays explicit even if `/etc/flux/common.env` still reflects an older host default
- the generated env commands use the checkout-local `.venv/bin/python` instead of a floating system `python3`
- equities API and node commands use `--mode live --confirm-live` for the production path
- every generated `equities-node-*.env` is rewritten from the intended checkout and live-mode flags, not just the active canary example
- print and review every rendered `equities-node-*.env` contents before restart so stale checkout paths or paper-mode commands cannot hide behind a filename-only listing
- on the shared `tokenmm-api` host, public `/equities` should emit `/static/fluxboard/assets/*`
- `/tokenmm/assets/*` on the shared public `/equities` route is a failure
- `/equities/assets/*` on the shared public `/equities` route is also a failure for the current shared-host contract
- Do not restart services until those env files match the intended checkout and live flags.

Shared-host recovery order after a repoint:

1. Run `uv sync --all-groups --all-extras` in the selected checkout.
2. Run `sudo ops/scripts/deploy/install_equities_systemd.sh`.
3. Verify the rewritten `/etc/flux/equities-*.env` files before restart.
4. Restart `chainsaw@md-ibkr-publisher.service`, then `flux@equities-portfolio.service`, `flux@equities-bridge.service`, `flux@equities-node-<strategy_id>.service`, and finally `flux@equities-api.service`.
5. If the node fails on `ModuleNotFoundError: No module named 'ibapi'`, the selected checkout `.venv` is incomplete; rerun `uv sync --all-groups --all-extras` in that checkout and restart the node.

Primary operator surfaces:

- `http://<host>:5022/pulse`
- `GET /api/pulse/jobs`
- `POST /api/pulse/jobs/group/equities/restart`
- `GET /api/v1/signals?profile=equities`
- `GET /api/v1/params?profile=equities`
- `GET /api/v1/balances?profile=equities`
- `GET /api/v1/trades?profile=equities`

Read-only live readiness gate:

- Run `ops/scripts/deploy/check_equities_live_readiness.sh` from the selected checkout before any live canary enablement.
- The gate reuses `deploy/equities/equities.live.toml`, the shared Redis env overrides, the canonical `profile_account_projection` Redis keys, the canonical component inventory keys, `GET /api/v1/signals?profile=equities`, and `GET /api/v1/balances?profile=equities`.
- Safe defaults are fail-closed: `missing_required` must stay empty, balances must not be degraded, every configured strategy contract must have its canonical component key, the required IBKR shared projections must be present and fresh, and stale/unhealthy signal counts must stay at zero.
- Override knobs are env-first for host use: `EQUITIES_READINESS_API_BASE_URL`, `EQUITIES_READY_MAX_STALE_SIGNAL_LEGS`, `EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES`, `EQUITIES_READY_PROJECTION_MAX_AGE_MS`, and `EQUITIES_READY_REQUIRED_BALANCE_SOURCE`.

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
- Confirm `ibkr.reference.main` keeps `twofa_timeout_action = "exit"` so a missed 2FA window fails closed instead of generating repeat pushes.
- Confirm enrolled strategy files keep `node.venues.IBKR.dockerized_gateway.manage_container = false` so node processes never own gateway restarts.
- Confirm each active strategy keeps a qualifiable IBKR reference instrument. The checked-in set includes examples such as `AAPL.NASDAQ`, `005380.KRX`, and `USAR.NASDAQ`; do not switch any of them to `BLUEOCEAN`.
- Confirm the IBKR account has the required after-hours permissions for the configured exchange and instrument.
- Confirm the active Hyperliquid config still carries `vault_address_env` when vault routing is required.

Fluxboard contract reference:

- See `fluxboard/docs/equities_contract.md` for the frozen `/equities` route and payload expectations when an equities API is deployed separately from the shared-host Pulse control plane.

## Rollback

- Disable MakerV3 cleanly by removing the intended strategy IDs from `api.equities_strategy_ids` / `api.equities_required_strategy_ids`, rerunning `ops/scripts/deploy/install_equities_systemd.sh`, and stopping the corresponding `flux@equities-node-<strategy_id>.service` units.
- Re-enable the MakerV4 canary only by restoring `deploy/equities/strategies/aapl_tradexyz_makerv4.toml.disabled` to `.toml`, retiring the MakerV3 file from active discovery, switching the shared allowlist/strategy metadata back to MakerV4, rerunning the installer, then `systemctl daemon-reload` and restarting `flux-equities.target`.
- `/equities`, `profile=equities`, and `portfolio=equities` stay stable during the strategy-family switch. The user-facing surface does not change, but the internal strategy family, params schema, and signal telemetry move with the file swap.
