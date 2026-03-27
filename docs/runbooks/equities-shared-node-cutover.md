# Equities Shared-Node Prod Cutover

Use this runbook to cut prod equities from one node per strategy to one node per symbol plus maker venue.

## Preconditions

- Source release commit includes grouped-node runtime, API, and installer support.
- The source release has a clean `uv sync --all-groups --all-extras --frozen`.
- Do not start until you have a writable baseline snapshot path under `~/archive`.

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
} >"$BASELINE"
echo "$BASELINE"
```

Rollback must use this snapshot, not memory.

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

Do not restart `chainsaw@md-ibkr-publisher.service` as part of this cutover unless the release explicitly changed publisher config.

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
bash ops/scripts/deploy/check_equities_live_readiness.sh
curl -fsS 'http://127.0.0.1:5024/api/v1/readiness?profile=equities'
```

Do not call the cutover healthy until both checks pass.

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
