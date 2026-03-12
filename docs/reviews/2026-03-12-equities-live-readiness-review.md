# Equities Live Readiness Review

## Result

- Date: `2026-03-12`
- Decision: `HOLD`
- Task 7 status: `completed`
- Scope completed: restarted the live equities API, portfolio, bridge, and full 24-node systemd graph from `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr`; ran the read-only readiness gate from that checkout; collected live signals, balances, Redis, gateway, and node-journal evidence.
- Scope blocked: Task 8 canary enablement remains blocked by IBKR auth/runtime failure and missing canonical equities portfolio inputs.

## Commands Run

```bash
sudo -n systemctl is-active \
  flux@equities-api.service \
  flux@equities-portfolio.service \
  flux@equities-bridge.service \
  flux@equities-node-aapl_tradexyz_makerv3.service \
  flux@equities-node-hyundai_tradexyz_makerv3.service

sudo -n systemctl show -p MainPID,EnvironmentFiles,FragmentPath \
  flux@equities-api.service \
  flux@equities-portfolio.service \
  flux@equities-bridge.service

sudo -n bash -lc '
  pid=$(systemctl show -p MainPID --value flux@equities-api.service)
  tr "\0" "\n" </proc/${pid}/environ | rg "^(WORKDIR|PYTHONPATH|EQUITIES_REDIS_HOST|EQUITIES_REDIS_PORT|CMD)="
'

sudo -n bash -lc '
  set -euo pipefail
  before_api=$(systemctl show -p MainPID --value flux@equities-api.service)
  before_portfolio=$(systemctl show -p MainPID --value flux@equities-portfolio.service)
  before_bridge=$(systemctl show -p MainPID --value flux@equities-bridge.service)
  mapfile -t node_units < <(systemctl list-units "flux@equities-node-*.service" --all --no-legend --plain | awk "{print \$1}" | sed "/^$/d")
  systemctl restart flux@equities-api.service flux@equities-portfolio.service flux@equities-bridge.service "${node_units[@]}"
'

ops/scripts/deploy/check_equities_live_readiness.sh --json
curl -fsS "http://127.0.0.1:5022/api/v1/signals?profile=equities"
curl -fsS "http://127.0.0.1:5022/api/v1/balances?profile=equities"

./.venv/bin/python - <<'PY'
import json, tomllib
from flux.common.keys import FluxRedisKeys
from flux.runners.shared.bootstrap import build_redis_client
cfg = tomllib.load(open("deploy/equities/equities.live.toml", "rb"))
r = build_redis_client(cfg["redis"])
contracts = cfg.get("strategy_contracts") or []
scopes = [row["scope_id"] for row in cfg.get("account_scopes") or []]
proj = {
    scope: bool(r.exists(FluxRedisKeys.profile_account_projection(profile_id="equities", account_scope_id=scope)))
    for scope in scopes
}
missing = []
for row in contracts:
    key = FluxRedisKeys.portfolio_inventory_component(
        strategy_id=row["strategy_id"],
        portfolio_id="equities",
        base_currency=str(row["portfolio_asset_id"]).upper(),
    )
    if not r.exists(key):
        missing.append(row["strategy_id"])
print(json.dumps({"projection_keys": proj, "missing_component_count": len(missing), "missing_component_sample": missing[:5]}))
PY

docker logs --tail 120 nautilus-ib-gateway-live
sudo -n journalctl -u flux@equities-node-aapl_tradexyz_makerv3.service -n 60 --no-pager
```

## Evidence

- Service provenance:
  - `flux@equities-api.service`, `flux@equities-portfolio.service`, and `flux@equities-bridge.service` are all active under `/etc/systemd/system/flux@.service`.
  - `/proc/<api pid>/environ` confirms the live API is pinned to `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr` with checkout-local `.venv/bin/python`, `PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr`, and `EQUITIES_REDIS_HOST=master.equities.wapqos.apse1.cache.amazonaws.com`.
- Restart summary:
  - API: `1452244 -> 1470495`
  - Portfolio: `1449552 -> 1473191`
  - Bridge: `1450017 -> 1470525`
  - Node units after restart: `24/24` active, including `flux@equities-node-hyundai_tradexyz_makerv3.service`.
- Readiness result:
  - `ops/scripts/deploy/check_equities_live_readiness.sh --json` exited `1`.
  - Readiness summary:
    - `balances`: `source=missing`, `degraded=true`, `missing_required=["hyundai_tradexyz_makerv3"]`
    - `profile_account_projections`: expected `["ibkr.hedge.main","ibkr.reference.main"]`, both missing
    - `component_keys`: all `24/24` canonical component keys missing
    - `signals`: `healthy_strategy_count=0`, `stale_signal_leg_count=48`, `unhealthy_strategy_ids=24`
    - `ibkr_auth`: failed; both shared IBKR projections missing and all 24 reference legs unhealthy
- Signals summary:
  - `GET /api/v1/signals?profile=equities` returned `24` strategies.
  - `24/24` strategies report stale reference legs.
  - `24/24` strategies report stale maker legs.
  - `24/24` strategies expose `48` stale legs in `debug.md_health.stale_legs`.
  - Representative states:
    - `aapl_tradexyz_makerv3`: `bot_off`
    - `amd_tradexyz_makerv3`: `bot_off`
    - `amzn_tradexyz_makerv3`: `running`
    - `baba_tradexyz_makerv3`: `bot_off`
    - `coin_tradexyz_makerv3`: `bot_off`
- Balances summary:
  - `GET /api/v1/balances?profile=equities` returned `source = null`, `degraded = true`, `count = 1`.
  - The only row is shared Hyperliquid cash:
    - `equities:cash:hyperliquid:HYPERLIQUID-master:USDC`
  - `missing_required = ["hyundai_tradexyz_makerv3"]`
  - `24/24` components are stale in the balances response, and `hyundai_tradexyz_makerv3` is still marked missing at the balances layer even though its systemd unit is active.
- Redis evidence:
  - `profile_account_projection` keys:
    - `hyperliquid.xyz.main = false`
    - `ibkr.reference.main = false`
    - `ibkr.hedge.main = false`
  - Canonical component keys:
    - `missing_component_count = 24`
    - sample missing strategies: `aapl`, `amd`, `amzn`, `baba`, `coin`
- IBKR auth state:
  - `nautilus-ib-gateway-live` is up, but container logs are only `socat ... 127.0.0.1:4001 ... Connection refused`.
  - `flux@equities-node-aapl_tradexyz_makerv3.service` is looping:
    - `Connecting to 127.0.0.1:4001 with client id: 7`
    - `Failed to receive server version information`
    - `ConnectionError(Interactive Brokers handshake did not complete; server version was not received.)`
  - This is consistent with an unauthenticated or otherwise non-serving IBKR gateway session.

## Residual Risks / Blockers For Task 8

- IBKR is not serving API traffic on `127.0.0.1:4001`, so all 24 reference legs are stale and the shared IBKR account projections are absent.
- The equities portfolio control plane is still not receiving canonical component keys in Redis (`24/24` missing), so readiness cannot go green even if API/systemd are up.
- `/api/v1/balances?profile=equities` is still falling back to degraded legacy behavior (`source = null`) instead of a healthy `portfolio_snapshot_v2`.
- `hyundai_tradexyz_makerv3` moved from a dead systemd unit to an active one, but its balances component is still missing at the API layer.
- No live canary should be enabled until:
  - IBKR auth is re-established,
  - shared projections repopulate,
  - canonical component keys appear,
  - balances stop degrading,
  - and the readiness gate returns `ok = true`.
