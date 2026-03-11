# TokenMM production deploy config

This directory is the production deployment root for the TokenMM stack.

Operator validation runbook: `docs/runbooks/tokenmm-risk-validation.md`

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
  - `plumeusdt_binance_perp_makerv3`
  - `plumeusdt_binance_spot_makerv3`
  - `plumeusdt_bitget_perp_makerv3`
  - `plumeusdt_bitget_spot_makerv3`
- Disabled strategy configs stay in `strategies/*.toml.disabled` until they are re-enrolled.

## Intent

- Supported production lifecycle: install with systemd, then manage jobs from Pulse.
- `ops/scripts/deploy/tokenmm_stack.sh` is local smoke only and refuses live deploys.
- Production deploys resolve to a stable non-worktree deploy root, never the calling worktree.
- Live trading is opt-in only when `TOKENMM_MODE=live`, `TOKENMM_CONFIRM_LIVE=1`, and `TOKENMM_ENABLE_EXECUTION=1` are all set together.
- Redis stays in `tokenmm.live.toml`; per-strategy node deploy files inherit it through the node runner `--shared-config` overlay.
- Production Redis is the dedicated `tokenmm` ElastiCache endpoint; keep the auth token out of git and inject it with `TOKENMM_REDIS_PASSWORD`.
- All seven active strategies price off Binance spot. The shared reference venue alias is `BINANCE_SPOT`.

Deploy-root resolution for `install_tokenmm_systemd.sh`:

1. `TOKENMM_DEPLOY_ROOT`, when explicitly set for first bootstrap or deliberate cutover.
2. Existing `/etc/flux/common.env` `WORKDIR` or `PYTHONPATH`, so reruns preserve the current host root.
3. The current checkout, only when it is not a git worktree.

If the resolved root is a worktree, the installer exits with an error instead of repointing production.

## Binance Spot Market-Making Contract

Dedicated runbook:
`docs/runbooks/tokenmm-binance-spot-market-making.md`

`plumeusdt_binance_spot_makerv3` is supported only on a regular Binance cross-margin account. Portfolio Margin / PAPI is unsupported for Binance spot market making, and current adapter behavior rejects PM mode with `UNSUPPORTED_ACCOUNT_MODE`.

Inspect `GET /api/v1/balances?profile=tokenmm` before cutover. If the effective
inventory still lives in margin / Portfolio Margin while the plain spot rows
are zeroed, treat that as unsupported pre-cutover state, not a
production-ready balance layout.

Operating contract:

- keep `allow_cash_borrowing = true` under `[node.venues.BINANCE_SPOT]`
- keep `spot_cash_borrowing_policy = "both_sides"` under `[strategy]` for the
  first rollout so the strategy uses free balances first and borrows only when
  needed
- keep `force_bot_off_on_start = true` and `bot_on = false` for the first
  restart
- inspect `GET /api/v1/balances?profile=tokenmm`, flatten the existing PM
  liability, and move the intended funded inventory into the supported
  cross-margin account before enabling quoting
- rotate credentials to the supported cross-margin account, restart only the
  Binance spot node in bot-off mode, and verify the journal stays clear of
  `UNSUPPORTED_ACCOUNT_MODE` before enabling quoting
- do not use Portfolio Margin / PAPI as a live fallback; that support is a
  separate project

## Inventory and balances model

This section defines the stable TokenMM production contract for the current
runtime surface. Historical rollout sequencing lives in
`docs/plans/2026-03-07-tokenmm-risk-and-portfolio-productionization.md`, but the
semantics documented here are the current source of truth.

- `Signal` mirrors exact per-strategy state from MakerV3.
- `local_qty_base` in strategy state is the canonical maker-leg base exposure for that strategy.
- `global_qty_base` in strategy state is the canonical TokenMM portfolio aggregate for the base asset.
- temporary compatibility aliases such as `local_qty` and `global_qty` must mirror the corresponding `*_base` fields exactly.
- `run_portfolio` owns the shared TokenMM portfolio snapshot. Each strategy publishes a maker-leg inventory
  component, and the sidecar recomputes the shared portfolio quantity, contributor diagnostics, merged
  balances rows, and merged balances totals in Redis.
- `GET /api/v1/balances?profile=tokenmm` must consume that shared portfolio snapshot rather than recomputing
  shared balances semantics independently.
- `GET /api/v1/balances?profile=tokenmm` may report `source = "portfolio_snapshot"` only when the shared
  snapshot is fresh enough: `server_ts_ms` and inventory `ts_ms` must both be within `stale_after_ms`.
  If that gate fails, the API falls back to the live per-strategy merge path.
- `GET /api/v1/signals?profile=tokenmm` must render strategy-local and portfolio-global quantities from
  canonical strategy state plus portfolio metadata, not derive them from balances except in explicit
  compatibility fallback mode.
- Fluxboard balances/risk drilldown consumes backend-authored `risk_groups`, `risk_groups[].rows`, and row
  `risk_key` / `risk_label` semantics from the API payload. It must not locally infer buckets from coin text.
- `GET /api/v1/balances?strategy=<id>` remains the per-strategy debug view.
- `risk_delta` remains diagnostic only and must not silently replace `local_qty_base` for spot local inventory.
- startup reconciliation failure means degraded or blocked trading, not best-effort stale-cache trading.
- `global_qty_base` may be complete or partial; consumers must use explicit completeness metadata instead of
  inferring status from nullability alone.
- live execution-enabled TokenMM nodes must keep `exec_reconciliation = true` and
  `filter_position_reports = false`; the node now rejects unsafe live startup
  settings instead of silently permitting stale-cache trading.

Shared inventory metadata:

- `aggregation_mode = "strict"` means all required contributors must be fresh and known.
- `aggregation_mode = "partial"` means `global_qty_base` is the sum of fresh known contributors and
  `global_qty_base_complete = false` marks the shared view as incomplete.
- `stale_after_ms` is the freshness budget for preferring the shared `portfolio_snapshot`.
- compatibility aliases `global_qty` and `global_qty_complete` mirror the canonical base fields.
- missing, stale, and unknown contributors remain visible in diagnostics in both modes.

:::info
The design target is a single shared portfolio source of truth. Strategy risk, Flux API, and Fluxboard all
consume the portfolio snapshot owned by `run_portfolio`.
:::

### Order quantity units and startup guardrails

- Every TokenMM strategy config should set `strategy.qty_unit` explicitly to either `venue` or `base`.
- Missing `qty_unit` still starts today for compatibility, but the node logs a startup warning and defaults to
  `venue`. Treat that as configuration debt and fix the TOML before the next deploy.
- Invalid `qty_unit` values fail node startup immediately.
- `qty_unit = "base"` means the configured `qty` / `order_qty` is base exposure and must round-trip cleanly
  into a venue-native order size before Nautilus order creation.
- If a base-sized order cannot convert to an exact venue quantity, startup/runtime quantity resolution fails
  loudly instead of silently truncating risk.
- Derivative strategy startup now logs a `startup_qty_guardrail` line with:
  - maker instrument id
  - configured `qty_unit`
  - configured order quantity
  - resolved venue order quantity
  - local maker position venue quantity
  - local maker position base quantity
  - quantity conversion status/source
- If a derivative local position cannot be normalized into base exposure, startup also logs
  `startup_qty_guardrail_missing_base`. Treat that as a blocking operational issue for risk reconciliation,
  even if the node is otherwise up.

## Production control plane

```bash
export TOKENMM_DEPLOY_ROOT=/path/to/deploy-root
cd "${TOKENMM_DEPLOY_ROOT}"
make build
pnpm --dir fluxboard install --frozen-lockfile
pnpm --dir fluxboard build
pnpm --dir pulse-ui install --frozen-lockfile
pnpm --dir pulse-ui build
.venv/bin/python ops/scripts/deploy/tokenmm_rollout_preflight.py
sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" ops/scripts/deploy/install_tokenmm_systemd.sh
sudoedit /etc/flux/common.env
sudo systemctl daemon-reload
sudo systemctl start flux-tokenmm.target
```

On an already-managed host, leave `TOKENMM_DEPLOY_ROOT` unset if `/etc/flux/common.env` already points at the intended root.

Runtime registration is explicit:

- `flux@.service` reads `/etc/flux/common.env` plus `/etc/flux/<service>.env`.
- `install_tokenmm_systemd.sh` pins each TokenMM env file to the resolved deploy root by writing
  `WORKDIR`, `PYTHONPATH`, and the root `.venv/bin/python` into `/etc/flux/tokenmm*.env`.
- Re-running the installer from a worktree does not change the live deploy root when `/etc/flux/common.env`
  already points at a stable checkout, and the installer refuses worktree roots for fresh bootstrap/cutover.
- Production logs are journal-first. Keep `FLUX_LOG_LEVEL` in `/etc/flux/common.env` as the shared default and use
  `FLUX_NODE_LOG_LEVEL`, `FLUX_BRIDGE_LOG_LEVEL`, `FLUX_PORTFOLIO_LOG_LEVEL`, or `FLUX_API_LOG_LEVEL` only for
  role-specific overrides.
- Pulse lists only services whose env files set `PULSE_ENABLED=1`.
- The seeded TokenMM target enrolls `tokenmm-api`, `tokenmm-portfolio`, `tokenmm-bridge`, and the 7 active
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
- Risk validation and rollout gates live in `docs/runbooks/tokenmm-risk-validation.md`.

## JupyterLab

- Optional localhost-only research service template: `deploy/tokenmm/systemd/tokenmm-jupyter.env.example`.
- `install_tokenmm_systemd.sh` also writes `/etc/flux/tokenmm-jupyter.env` for `flux@tokenmm-jupyter.service`.
- Start it separately from the trading target:
  - `sudo systemctl start flux@tokenmm-jupyter.service`
  - `sudo journalctl -u flux@tokenmm-jupyter.service -n 20 --no-pager`
- Direct localhost address: `http://127.0.0.1:8888/lab`
- The notebook root is `research/tokenmm`, and the example notebook is `research/tokenmm/notebooks/tokenmm_trade_data.ipynb`.
- The notebook reads local SQLite telemetry from `TOKENMM_TELEMETRY_DIR`, defaulting to `/var/lib/nautilus/telemetry/tokenmm`.

## Telemetry Persistence

- Run the rollout preflight before changing systemd envs:
  - `.venv/bin/python ops/scripts/deploy/tokenmm_rollout_preflight.py`
- Create the local telemetry directory before restarting live services:
  - `sudo install -d -o ubuntu -g ubuntu /var/lib/nautilus/telemetry/tokenmm`
- Local SQLite verification:
  - `sqlite3 /var/lib/nautilus/telemetry/tokenmm/orders.sqlite 'SELECT COUNT(*) FROM order_action;'`
  - `sqlite3 /var/lib/nautilus/telemetry/tokenmm/fills.sqlite 'SELECT COUNT(*) FROM execution_fill;'`
  - `sqlite3 /var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite 'SELECT COUNT(*) FROM quote_cycle;'`
- For shipped Postgres telemetry, follow `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`.

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

Local smoke logs live under `.run/tokenmm-stack/logs`. The script now rotates a log before append when it exceeds the
configured size budget and keeps only a bounded number of rotated files. Use `TOKENMM_LOCAL_LOG_MAX_MB` and
`TOKENMM_LOCAL_LOG_KEEP` to override the defaults.

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

- `params` returns the 7 allowlisted strategy IDs in registry order.
- `signal` returns seven per-strategy rows. Each row keeps its own `local_qty` and shares the same
  portfolio-scoped `global_qty` alias from the shared portfolio snapshot, alongside canonical
  `local_qty_base` / `global_qty_base` fields.
- `balances` returns the shared `tokenmm` portfolio view from the same shared portfolio snapshot.
- `trades` may be empty in paper smoke; if rows are present they must retain allowlisted per-row `strategy_id` values.
- `api/pulse/jobs` returns the enrolled local jobs and statuses when Pulse assets are served.
