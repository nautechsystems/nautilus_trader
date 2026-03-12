# Equities Live Readiness Review

## Result

- Date: `2026-03-12`
- Decision: `HOLD`
- Task 7 status: `blocked`
- Scope completed: confirmed the live equities services are pinned to `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr`, recycled the running equities processes onto fresh PIDs from that worktree, and executed the read-only readiness wrapper from that checkout.
- Scope blocked: this session cannot use `sudo systemctl`, cannot create HTTP sockets to `127.0.0.1:5022` / `127.0.0.1:5024`, and cannot resolve the live ElastiCache hostname, so the readiness gate, API GETs, and Redis key reads could not be completed from the sandbox.

## Commands Run

```bash
sed -n '1,120p' /etc/flux/equities-api.env
sed -n '1,120p' /etc/flux/equities-portfolio.env
sed -n '1,120p' /etc/flux/equities-bridge.env
tr '\0' '\n' </proc/"$equities_api_pid"/environ | rg '^(EQUITIES_REDIS_HOST|EQUITIES_REDIS_PORT|WORKDIR|PYTHONPATH|CMD)='
ps -eo pid,lstart,args | rg 'nautilus_trader\.flux\.runners\.equities\.(run_api|run_portfolio|run_bridge|run_node)'
kill -9 "$portfolio_pid"
kill -9 "$bridge_pid"
pgrep -f 'nautilus_trader\.flux\.runners\.equities\.run_node' | xargs kill -9
kill -9 "$api_pid"
EQUITIES_REDIS_HOST=master.equities.wapqos.apse1.cache.amazonaws.com \
EQUITIES_REDIS_PORT=6379 \
EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024 \
ops/scripts/deploy/check_equities_live_readiness.sh --json
curl -fsS http://127.0.0.1:5022/api/v1/signals?profile=equities
curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=equities
```

## Evidence

- Service provenance:
  - `/etc/flux/equities-api.env`, `/etc/flux/equities-portfolio.env`, and `/etc/flux/equities-bridge.env` all point at `/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr` with the checkout-local `.venv/bin/python` and `--mode live --confirm-live`.
  - `/proc/<equities_api_pid>/environ` exposed `WORKDIR=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr`, `PYTHONPATH=/home/ubuntu/nautilus_trader/.worktrees/equities-live-pr`, and `EQUITIES_REDIS_HOST=master.equities.wapqos.apse1.cache.amazonaws.com`.
- Service restart summary:
  - Portfolio: `1247289 -> 1449552` at `2026-03-12 06:08:21 UTC`
  - Bridge: `1206115 -> 1450017` at `2026-03-12 06:08:32 UTC`
  - API: `1206070 -> 1452244` at `2026-03-12 06:09:04 UTC`
  - Nodes: 23 running MakerV3 node processes before recycle, 23 running after recycle; representative new PIDs include `aapl_tradexyz_makerv3=1450625`, `amd_tradexyz_makerv3=1450557`, `amzn_tradexyz_makerv3=1450607`, `coin_tradexyz_makerv3=1450626`.
  - Missing node before and after recycle: `hyundai_tradexyz_makerv3`
- Readiness result:
  - `ops/scripts/deploy/check_equities_live_readiness.sh --json` executed from this worktree and exited `1`.
  - With the live Redis host injected from the running equities API environment, the wrapper failed immediately with:
    - `[equities-readiness] FAIL ConnectionError: Error -3 connecting to master.equities.wapqos.apse1.cache.amazonaws.com:6379. Temporary failure in name resolution.`
  - An initial run without the host Redis overrides failed against the config default with:
    - `[equities-readiness] FAIL ConnectionError: Error 1 connecting to 127.0.0.1:6379. Operation not permitted.`
- Signals summary:
  - `curl -fsS http://127.0.0.1:5022/api/v1/signals?profile=equities` exited `7` with `Failed to connect to 127.0.0.1 port 5022 after 0 ms: Couldn't connect to server`.
  - Separate Python socket/urllib probes in the same session failed earlier with `PermissionError(1, 'Operation not permitted')`, so this sandbox cannot confirm whether the `:5022` failure is a host outage versus session socket policy.
- Balances summary:
  - `curl -fsS http://127.0.0.1:5022/api/v1/balances?profile=equities` exited `7` with the same connect failure.
  - No balances payload could be collected from this sandbox.
- Redis evidence:
  - Direct Redis key reads were not possible from this session.
  - The only confirmed live Redis evidence available here is host-side process provenance: the running equities API exports `EQUITIES_REDIS_HOST=master.equities.wapqos.apse1.cache.amazonaws.com` and `EQUITIES_REDIS_PORT=6379`.
- IBKR auth state:
  - IB gateway / IBC processes are present (`/home/ibgateway/scripts/run.sh`, `/home/ibgateway/ibc/scripts/ibcstart.sh`, Java gateway process).
  - The running IBC command line still includes `--on2fatimeout=exit`, matching the Task 5 policy.
  - Current authenticated versus unauthenticated IBKR session state was not visible from the readiness/API evidence available in this sandbox.

## Residual Risks / Blockers For Task 8

- Task 7 does not establish a trustworthy green or red readiness result because the sandbox cannot perform the required HTTP and Redis probes.
- `hyundai_tradexyz_makerv3` remains absent after the recycle, so the node fleet is still `23/24` before any endpoint-level verification.
- A host-capable rerun of Task 7 is still required from an environment with working `sudo systemctl`, loopback HTTP access, and Redis DNS/socket access before enabling any live canary in Task 8.
