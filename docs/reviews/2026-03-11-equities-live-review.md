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
  - Result: `/equities` is serving the wrong asset-owner path. The shell is loading Fluxboard assets from `/tokenmm/assets/*` instead of `/static/fluxboard/assets/*`.
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

## Supporting Host Drift Evidence

- `/etc/flux/equities-api.env` points at `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/deploy/equities/equities.live.toml` and still uses `--mode paper`
- `/etc/flux/equities-node-aapl_tradexyz_makerv4.env` points at the same `makerv3-mono-pr` worktree and still uses `--mode paper`
- `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/fluxboard/dist/index.html` still references `/tokenmm/assets/index-*.js` and `/tokenmm/assets/index-*.css`

## Frozen Contract Record

- Current active contract: MakerV4 is the checked-in and intended live equities contract. `deploy/equities/equities.live.toml` sets `strategy_class = "maker_v4"`, `param_set = "makerv4"`, and allowlists only `aapl_tradexyz_makerv4`.
- Current rollback path: emergency rollback is the disabled MakerV3 file `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled`. Re-enabling it requires an explicit strategy-file swap plus allowlist/metadata rollback.
- Shared-host GUI contract: `/equities` must serve the neutral Fluxboard shell and resolve static assets from `/static/fluxboard/assets/*`. `/tokenmm/assets/*` on `/equities` is deployment drift, not a supported variation.

## Remaining Runtime Blockers After IBKR Auth

- `chainsaw@md-ibkr-publisher.service` is still failed, so IBKR market data is not flowing into the equities pipeline even after gateway login completed.
- Installed equities env files are pinned to `/.worktrees/makerv3-mono-pr` and `--mode paper`, which prevents the live host from running the intended MakerV4 checkout and runtime mode.
- The shared-host `/equities` HTML is still coupled to a stale shared Fluxboard bundle that resolves `/tokenmm/assets/*`.
- The equities API surface remains stale/degraded until the publisher, bridge, portfolio, and node services are restored behind the active MakerV4 contract.
