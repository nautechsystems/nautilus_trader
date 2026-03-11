# Equities Live Review Baseline

## Outcome

- Decision: `HOLD`
- Date: `2026-03-11`
- Active deploy contract: `MakerV4`
- Active strategy file: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml`
- Rollback file: `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled`

## Direct Evidence Captured In This Session

- Public host probe:
  - `curl -fsS http://13.213.194.42:5022/equities`
  - Result from this sandbox: `curl: (7) Failed to connect to 13.213.194.42 port 5022`
- Host-local HTTP probe from this sandbox namespace:
  - `curl -fsS http://127.0.0.1:5022/equities`
  - `curl -fsS http://127.0.0.1:5024/api/v1/signals?profile=equities`
  - Result from this sandbox: `curl: (7) Failed to connect ...`
- Local socket/process evidence still shows the host services and drifted commands:
  - `ss -ltnp | rg ':5022|:5024'` showed listeners on `0.0.0.0:5022` and `127.0.0.1:5024`
  - `ps -ef | rg 'run_api|run_node|md-ibkr-publisher|IBC'` showed the public `tokenmm` API on `:5022` and an equities API backend on `127.0.0.1:5024`
  - `/etc/flux/equities-api.env` points at `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/deploy/equities/equities.live.toml` and still uses `--mode paper`
  - `/etc/flux/equities-node-aapl_tradexyz_makerv4.env` points at the same `makerv3-mono-pr` worktree and still uses `--mode paper`
  - `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/fluxboard/dist/index.html` still references `/tokenmm/assets/index-*.js` and `/tokenmm/assets/index-*.css`
- Service/docker introspection limits in this sandbox:
  - `systemctl --no-pager --type=service --all | rg 'flux@equities|chainsaw@md-ibkr-publisher'` returned `Failed to connect to bus: Operation not permitted`
  - `docker logs --tail 120 nautilus-ib-gateway-live` is blocked here because the session cannot access the Docker API or escalate with `sudo`

## Controller-Provided March 11 Live Findings

These were already established on the live host before this docs-only task and remain consistent with the drift captured above:

- `/equities` served the wrong GUI bundle path and pointed at `/tokenmm/assets/...` instead of `/static/fluxboard/assets/...`
- `GET /api/v1/signals?profile=equities` was stale and showed a blocked, non-running `aapl_tradexyz_makerv4` row
- `GET /api/v1/balances?profile=equities` was stale/degraded and returned only one stale Hyperliquid `USDC` cash row
- only `flux@equities-api` was active; the equities bridge, portfolio, and node stack were not up
- the IBKR gateway had already authenticated, but `chainsaw@md-ibkr-publisher.service` was still failed

## Frozen Contract Record

- Current active contract: MakerV4 is the checked-in and intended live equities contract. `deploy/equities/equities.live.toml` sets `strategy_class = "maker_v4"`, `param_set = "makerv4"`, and allowlists only `aapl_tradexyz_makerv4`.
- Current rollback path: emergency rollback is the disabled MakerV3 file `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled`. Re-enabling it requires an explicit strategy-file swap plus allowlist/metadata rollback.
- Shared-host GUI contract: `/equities` must serve the neutral Fluxboard shell and resolve static assets from `/static/fluxboard/assets/*`. `/tokenmm/assets/*` on `/equities` is deployment drift, not a supported variation.

## Remaining Runtime Blockers After IBKR Auth

- `chainsaw@md-ibkr-publisher.service` is still failed, so IBKR market data is not flowing into the equities pipeline even after gateway login completed.
- Installed equities env files are pinned to `/.worktrees/makerv3-mono-pr` and `--mode paper`, which prevents the live host from running the intended MakerV4 checkout and runtime mode.
- The shared-host `/equities` HTML is still coupled to a stale shared Fluxboard bundle that resolves `/tokenmm/assets/*`.
- The equities API surface remains stale/degraded until the publisher, bridge, portfolio, and node services are restored behind the active MakerV4 contract.
