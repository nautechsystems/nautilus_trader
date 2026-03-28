# Equities Shared-Node Prod Cutover

Use this runbook to cut prod equities from one node per strategy to one node per symbol plus maker venue.

## Preconditions

- Source release commit includes grouped-node runtime, API, and installer support.
- The source release has a clean `uv sync --all-groups --all-extras --frozen`.
- Do not start until you have a writable baseline snapshot path under `~/archive`.
- Treat this market-data recovery V1 as an intra-node hardening wave on top of the grouped-node rollout, not as a fleet-wide market-data redesign.
- Confirm the shared IBKR reference boundary is healthy before evaluating maker-feed recovery:
  - the live IBKR gateway is authenticated
  - `chainsaw@md-ibkr-publisher.service` is healthy
  - current `/api/v1/readiness?profile=equities` can be captured for comparison
- Do not restart `chainsaw@md-ibkr-publisher.service` as part of this cutover unless the release explicitly changed publisher config.

## 1. Capture The Live Baseline

Record all of the following before repointing prod:

```bash
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BASELINE="$HOME/archive/equities-shared-node-cutover-${STAMP}.txt"
{
  echo "# current release"
  readlink -f "$HOME/releases/prod/equities/current"
  echo
  echo "# /etc/flux/equities envs"
  find /etc/flux -maxdepth 1 -type f -name 'equities*.env' -print | sort
  echo
  echo "# target"
  sed -n '1,200p' /etc/systemd/system/flux-equities.target
  echo
  echo "# sudoers"
  sed -n '1,200p' /etc/sudoers.d/flux-pulse
  echo
  echo "# live units"
  systemctl list-units 'flux@equities-node-*.service' --no-legend --plain
  echo
  echo "# pulse jobs"
  curl -fsS 'http://127.0.0.1:5024/api/pulse/jobs'
  echo
  echo "# readiness json"
  bash "$HOME/releases/prod/equities/current/ops/scripts/deploy/check_equities_live_readiness.sh" --json
  echo
  echo "# ibkr / publisher"
  systemctl is-active chainsaw@md-ibkr-publisher.service || true
  systemctl is-active nautilus-ib-gateway-live.service || true
} >"$BASELINE"
echo "$BASELINE"
```

Rollback must use this snapshot, not memory.

Also capture repeated quote-age probes for the historically bad rows before any restart:

- `aapl_tradexyz`
- `amd_tradexyz`
- `meta_tradexyz`
- `msft_tradexyz`
- `orcl_tradexyz`
- `tsla_tradexyz`
- `ewy_binance_perp`

Store at least two spaced samples so the post-restart result can be compared against a real baseline rather than a single timestamp.

## 2. Create A Fresh Immutable Release Root

```bash
cd ~/nautilus-trader
SOURCE_REF="$(git rev-parse HEAD)"
RELEASE_ID="$(date -u +%Y%m%dT%H%M%SZ)-shared-nodes"
DEPLOY_LANE=prod \
STACK_NAME=equities \
SOURCE_ROOT="$PWD" \
SOURCE_REF="$SOURCE_REF" \
RELEASE_ID="$RELEASE_ID" \
bash ops/scripts/deploy/create_release_root.sh
```

Expected: `~/releases/prod/equities/releases/<timestamp>-shared-nodes`

## 3. Build The Release-Local Environment

```bash
cd "$HOME/releases/prod/equities/releases/$RELEASE_ID"
uv sync --all-groups --all-extras --frozen
```

Expected: `.venv/bin/python` exists inside the release root.

## 4. Re-Render Prod Systemd And Pulse Surfaces

```bash
cd "$HOME/releases/prod/equities/releases/$RELEASE_ID"
sudo env ROOT_DIR="$PWD" \
  EQUITIES_DEPLOY_ROOT="$PWD" \
  EQUITIES_ENABLE_EXECUTION=1 \
  bash "$PWD/ops/scripts/deploy/install_equities_systemd.sh"
```

Expected before restart:

- `/etc/flux/equities-node-*.env` count is `19`
- every grouped node env contains one or two `--config` flags
- `/etc/systemd/system/flux-equities.target` lists only grouped node services
- `/etc/sudoers.d/flux-pulse` lists only grouped node services

## 5. Verify The Rendered Operator Surface

```bash
find /etc/flux -maxdepth 1 -type f -name 'equities-node-*.env' -print | sort
sed -n '1,200p' /etc/systemd/system/flux-equities.target
sed -n '1,200p' /etc/sudoers.d/flux-pulse
```

Do not restart anything until the rendered surfaces show exactly `19` grouped node services and no `equities-node-<strategy_id>` leftovers.

## 6. Restart The Stack Atomically

```bash
sudo systemctl stop 'flux@equities-node-*.service'
sudo systemctl reset-failed 'flux@equities-node-*.service'
sudo systemctl restart flux@equities-portfolio.service
sudo systemctl restart flux@equities-bridge.service
sudo systemctl start flux-equities.target
sudo systemctl restart flux@equities-api.service
```

## 7. Verify Pulse And Live API State

```bash
curl -fsS 'http://127.0.0.1:5024/api/pulse/jobs'
systemctl list-units 'flux@equities-node-*.service' --no-legend --plain
python - <<'PY'
import requests
base='http://127.0.0.1:5024/api/v1'
for path in ['signals','balances','trades','alerts','params','param-schema']:
    r=requests.get(f'{base}/{path}', params={'profile':'equities'}, timeout=10)
    print(path, r.status_code)
    print(r.text[:1000])
PY
```

Expected:

- `19` grouped node services
- external payloads still expose `38` strategy ids
- no grouped node ids leak into API rows

## 8. Verify Public `/equities`

```bash
curl -fsS 'http://127.0.0.1:5022/equities' >/dev/null
```

Then verify a fresh page load still uses the existing realtime contract and does not surface grouped node ids.

## 9. Run The Readiness Gate

```bash
cd "$HOME/releases/prod/equities/releases/$RELEASE_ID"
bash ops/scripts/deploy/check_equities_live_readiness.sh --json
curl -fsS 'http://127.0.0.1:5024/api/v1/readiness?profile=equities'
```

Do not call the cutover healthy until both checks pass.

The readiness gate and the human rollout log should prove all of the following:

- `healthy_strategy_count = 38`
- no maker leg remains in a persistent non-tradeable recovery state: public `feed_state` is never `degraded`, `down`, or `unknown`, public `quote_state` is never `old` or `missing`, and structured recovery logs do not show repeated `bootstrapping`, `blocked`, or `recovering` loops after restart
- the shared IBKR gateway and `chainsaw@md-ibkr-publisher.service` are healthy and were treated as preconditions, not local maker-feed results
- historically stale rows now advance quote timestamps after restart
- balances / projections are no worse than the captured baseline

## 10. Verify Fail-Closed Trading Behavior During Recovery

Before declaring the restart safe, inspect structured logs, counters, or venue/order state and prove:

- when required feeds are non-tradeable, including internal `bootstrapping`, `blocked`, or `recovering` supervisor states and public `feed_state in {degraded, down, unknown}` / `quote_state in {old, missing}`, strategies emit no quote-placement, quote-amendment, or hedge-placement side effects
- those same strategies retain zero working maker quotes while the required feeds remain non-tradeable
- any venue/session blocker is explicit in logs and suppresses per-feed reset churn

Do not infer this from signal rows alone.

## 11. Hold A 15-Minute Soak Window

Run repeated readiness JSON and quote-age probes for a `15`-minute soak window during a live US regular trading session.

Do not sign off until:

- the historically stale rows continue to advance over repeated samples
- readiness stays at or above the captured pre-restart baseline
- no new maker feed falls back into a persistent non-tradeable loop on operator surfaces (`feed_state in {degraded, down, unknown}` or `quote_state in {old, missing}`) or repeated internal recovery-loop logging
- no strategy leaks back into working maker quotes while required feeds remain non-tradeable

Rollback immediately if health regresses below baseline or the historically bad rows remain frozen after the soak window.

## Rollback

Rollback is an atomic revert to the previous immutable release:

1. Repoint `~/releases/prod/equities/current` to the previous release from the baseline snapshot.
2. Re-run `install_equities_systemd.sh` from that previous release root.
3. Stop grouped node units and clear failed state:

```bash
sudo systemctl stop 'flux@equities-node-*.service'
sudo systemctl reset-failed 'flux@equities-node-*.service'
```

4. Restart the previous per-strategy registry:

```bash
sudo systemctl restart flux@equities-portfolio.service
sudo systemctl restart flux@equities-bridge.service
sudo systemctl start flux-equities.target
sudo systemctl restart flux@equities-api.service
```

5. Re-run the verification steps above and confirm no grouped node service remains active or restartable.
6. Use the pre-restart baseline snapshot and quote-age probes as the rollback acceptance threshold. The rollback is not complete until readiness and historically bad rows are back to the recorded baseline or better.
