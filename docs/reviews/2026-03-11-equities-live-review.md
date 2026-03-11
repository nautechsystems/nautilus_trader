# Equities Live Review Baseline

## Outcome

- Decision: `HOLD`
- Date: `2026-03-11`
- Active deploy contract: `MakerV4`
- Active strategy file: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml`
- Rollback file: `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled`

## Fresh Live Probe Results

Required Step 1 probes on `2026-03-11` captured the current live failure state directly:

- `/equities` shell:
  - Command: `curl -fsS http://13.213.194.42:5022/equities | sed -n '1,20p'`
  - HTML head included `<link rel="icon" type="image/svg+xml" href="/static/fluxboard/favicon.svg" />`
  - HTML head included `<script type="module" crossorigin src="/tokenmm/assets/index-DshLjUYS.js"></script>`
  - HTML head included `<link rel="stylesheet" crossorigin href="/tokenmm/assets/index-6uS6GK5c.css">`
  - Result: the public shared-host `/equities` shell is serving the wrong asset-owner path. On the `tokenmm-api` host it should load Fluxboard assets from `/static/fluxboard/assets/*`, but it is loading `/tokenmm/assets/*` instead.
- Signals API:
  - Command: `curl -fsS 'http://13.213.194.42:5022/api/v1/signals?profile=equities' | jq '.data.strategies[0]'`
  - `id = "aapl_tradexyz_makerv4"`
  - `meta.class = "maker_v4"`
  - `tradeable = false`
  - `blocked = true`
  - `params.bot_on = false`
  - `maker_v4.quote_snapshot.hedge_ready = false`
  - `maker_v4.quote_snapshot.ibkr_quote_age_ms = 94930856`
  - `state.state = "bot_off"`
  - `balances_count = 1`
  - `debug.md_health.state_stale = true`
  - `debug.md_health.stale_legs = ["hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID", "ibkr:AAPL.NASDAQ"]`
  - Result: the active MakerV4 row is present, but the live signal surface is blocked, stale, and not tradeable.
- Balances API:
  - Command: `curl -fsS 'http://13.213.194.42:5022/api/v1/balances?profile=equities' | jq '.data'`
  - one row only
  - row is Hyperliquid `USDC`
  - `count = 1`
  - `components[0].strategy_id = "aapl_tradexyz_makerv4"`
  - `components[0].stale = true`
  - `components[0].age_ms = 166713929`
  - `degraded = true`
  - `stale_after_ms = 30000`
  - Result: balances are stale/degraded and the shared equities portfolio view has collapsed to one stale Hyperliquid cash row.
- Service graph:
  - Command: `systemctl --no-pager --type=service --all | rg 'flux@equities|chainsaw@md-ibkr-publisher'`
  - `chainsaw@md-ibkr-publisher.service` = failed
  - `flux@equities-api.service` = active/running
  - `flux@equities-bridge.service` = inactive/dead
  - `flux@equities-node-aapl_tradexyz_makerv4.service` = inactive/dead
  - `flux@equities-portfolio.service` = inactive/dead
  - Result: only the equities API is up; the publisher and the rest of the equities runtime graph are not running.
- IBKR gateway container:
  - Command: `docker logs --tail 120 nautilus-ib-gateway-live`
  - multiple failed or expired 2FA attempts appeared first
  - retry path logged `socat ... connect(5, AF=2 127.0.0.1:4001, 16): Connection refused`
  - final successful login path logged:
    - `2026-03-11 01:04:14:908 IBC: Second Factor Authentication initiated`
    - `2026-03-11 01:04:28:965 IBC: Login has completed`
    - `2026-03-11 01:04:29:556 IBC: Configuration tasks completed`
  - Result: IBKR auth eventually succeeded, but downstream runtime recovery did not follow.

## Supporting Host Drift And Provenance Evidence

- `sed -n '1,120p' /etc/flux/tokenmm-api.env`
  - Result: `WORKDIR=/home/ubuntu/nautilus_trader`
  - Result: `CMD="env FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 /home/ubuntu/nautilus_trader/.venv/bin/python -m nautilus_trader.flux.runners.tokenmm.run_api --config /home/ubuntu/nautilus_trader/deploy/tokenmm/tokenmm.live.toml --mode live --confirm-live --host 0.0.0.0 --port 5022 --serve-fluxboard --serve-pulse"`
- `ps -ef | rg 'flux\.runners\.tokenmm\.run_api'`
  - Result: the running public `tokenmm-api` process comes from `/home/ubuntu/nautilus_trader/.venv/bin/python` with `--config /home/ubuntu/nautilus_trader/deploy/tokenmm/tokenmm.live.toml --mode live`
- `sed -n '1,120p' /etc/flux/equities-api.env`
  - Result: `CMD="env FLUXBOARD_SERVE_DIST=1 ${EQUITIES_PYTHON_BIN:-python3} -m nautilus_trader.flux.runners.equities.run_api --config /home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/deploy/equities/equities.live.toml --mode paper --host 127.0.0.1 --port 5024 --serve-fluxboard"`
- `ps -ef | rg 'flux\.runners\.equities\.run_api'`
  - Result: the running loopback equities backend comes from `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/.venvs/equities/bin/python` with `--config /home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/deploy/equities/equities.live.toml --mode paper --host 127.0.0.1 --port 5024 --serve-fluxboard`
- `readlink -f /proc/2518130/cwd`
  - Result: `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr`
- `tr '\0' '\n' < /proc/2518130/environ | rg '^(WORKDIR|PYTHONPATH|FLUXBOARD_SERVE_DIST|EQUITIES_API_BACKEND_URL)='`
  - Result: includes `WORKDIR=/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr`
  - Result: includes `PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr`
  - Result: includes `FLUXBOARD_SERVE_DIST=1`
  - Result: includes `EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024`
  - Note: only non-secret provenance envs are recorded here
- `sed -n '1,120p' /etc/flux/equities-node-aapl_tradexyz_makerv4.env`
  - Result: points at the same `makerv3-mono-pr` worktree and still uses `--mode paper`
- `curl -fsS http://127.0.0.1:5024/equities | rg '/tokenmm/assets|/static/fluxboard/assets|/equities/assets' -n`
  - Result: returned only line 8 `/tokenmm/assets/index-DshLjUYS.js`
  - Result: returned only line 9 `/tokenmm/assets/index-6uS6GK5c.css`
- `sed -n '1,20p' /home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/fluxboard/dist/index.html`
  - Result: shows the same `/tokenmm/assets/index-DshLjUYS.js` and `/tokenmm/assets/index-6uS6GK5c.css` references
- `sed -n '1,20p' /home/ubuntu/nautilus_trader/fluxboard/dist/index.html`
  - Result: main checkout currently uses `/static/fluxboard/assets/index-Dh7RM63S.js` and `/static/fluxboard/assets/index-BCpW5E6y.css`
  - Result: those main-checkout asset hashes do not match the public `/equities` shell

These provenance checks close the remaining gap: public `tokenmm-api` is running from the main checkout in live mode, but the loopback equities backend running from `/.worktrees/makerv3-mono-pr` in paper mode is the source whose stale `/tokenmm/assets/*` HTML matches the public `/equities` shell.

## Frozen Contract Record

- Current active contract: MakerV4 is the checked-in and intended live equities contract. `deploy/equities/equities.live.toml` sets `strategy_class = "maker_v4"`, `param_set = "makerv4"`, and allowlists only `aapl_tradexyz_makerv4`.
- Current rollback path: emergency rollback is the disabled MakerV3 file `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled`. Re-enabling it requires an explicit strategy-file swap plus allowlist/metadata rollback.
- Shared-host GUI contract: on the public `tokenmm-api` proxy, `/equities` must serve the neutral Fluxboard shell and resolve static assets from `/static/fluxboard/assets/*`; the standalone equities runner in repo still serves `/equities/assets/*` when reached directly. `/tokenmm/assets/*` on public `/equities` is deployment drift, not a supported variation.

## Remaining Runtime Blockers After IBKR Auth

- `chainsaw@md-ibkr-publisher.service` is still failed, so IBKR market data is not flowing into the equities pipeline even after gateway login completed.
- Installed equities env files are pinned to `/.worktrees/makerv3-mono-pr` and `--mode paper`, which prevents the live host from running the intended MakerV4 checkout and runtime mode.
- The shared-host `/equities` HTML is still coupled to a stale shared Fluxboard bundle that resolves `/tokenmm/assets/*`.
- The equities API surface remains stale/degraded until the publisher, bridge, portfolio, and node services are restored behind the active MakerV4 contract.
