# LP Hedger Production Rollout

This runbook freezes the production rollout contract for the shared Flux host LP deployment.

## Topology

- Public shared host: `http://<host>:5022`
- Canonical LP GUI route: `/lp`
- Canonical LP API family: `/api/v1/hedgers/*`
- Hidden LP backend: `LP_API_BACKEND_URL=http://127.0.0.1:5025`
- Shared service target: `flux-lp.target`

The public Flux host continues to serve Fluxboard and Pulse on `:5022`. The dedicated LP backend remains loopback-only on `127.0.0.1:5025`.

## Active Production Instances

Band1 and Band2 are the only active checked-in production instances for this rollout:

- `service-eth-plume-lp-hedger`
- `service-eth-plume-lp-hedger-band2`

All other checked-in LP configs remain templates or disabled `.ini.disabled` files until their geometry and credentials are explicitly validated.

## Operator-Managed Files

Operators own these host-local files during rollout:

- `/etc/flux/common.env`
- `/etc/flux/lp-system.ini`
- `/etc/flux/lp-api.env`
- `/etc/flux/service-eth-plume-lp-hedger.env`
- `/etc/flux/service-eth-plume-lp-hedger-band2.env`

Required shared-host values:

- `LP_API_BACKEND_URL=http://127.0.0.1:5025`
- `LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini`

## Operator Surface Contract

The `/lp` surface is the production home for the Chainsaw LP Hedger operator workflow. The shared Fluxboard surface must preserve:

- hedger instance selection for Band1 and Band2
- running/stopped, dry-run, and hedging-enabled state visibility
- restart and enable/disable controls
- config editing
- geometry override editing
- threshold override editing
- recent-hedges clear

Intentional monorepo deltas must be documented in `fluxboard/docs/lp_contract.md`; they are not allowed to drift implicitly.

## Preflight Checklist

Before enabling `flux-lp.target`, confirm:

```bash
python3 ops/scripts/lp_hedger_preflight.py --json
```

The preflight must report `ok: true` before operators proceed. It checks:

1. `/etc/flux/common.env` points the public host at `LP_API_BACKEND_URL=http://127.0.0.1:5025`.
2. `/etc/flux/lp-system.ini` contains `[redis]`, `[plume]`, `[bybit]`, `[bybit_hedger]`, and `[bybit_hedger_band2]`.
3. The Band1 and Band2 hedger INI files exist and are readable.
4. The shared public host remains on `:5022`, and the LP API remains loopback-bound on `:5025`.

## Install And Restart Order

1. Install or refresh systemd assets with `sudo ops/scripts/deploy/install_lp_systemd.sh`.
2. Update `/etc/flux/common.env` and `/etc/flux/lp-system.ini`.
3. Update the per-service env files for `lp-api`, Band1, and Band2.
4. Run `sudo systemctl daemon-reload`.
5. Restart the shared public host so it reloads `LP_API_BACKEND_URL` from `/etc/flux/common.env`.
6. Start or restart `flux-lp.target`.

The restart order is important because the public host must see the current shared-host env before operators expect `/lp` or `/api/v1/hedgers/*` to proxy correctly.

## Smoke Validation

After cutover, verify:

```bash
curl -fsS http://127.0.0.1:5022/lp >/dev/null
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/instances
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/eth_plume_lp
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
```

Then confirm the Pulse-managed service set shows healthy jobs for:

- `lp-api`
- `service-eth-plume-lp-hedger`
- `service-eth-plume-lp-hedger-band2`

## Rollback

Rollback keeps the user-facing route contract unchanged:

1. Stop `flux-lp.target`.
2. Restore the prior `/etc/flux/common.env`, `/etc/flux/lp-system.ini`, and per-service env files.
3. Restart the shared public host to restore the previous proxy target.
4. Verify `/lp`, `/api/v1/hedgers/*`, and Pulse behavior against the prior known-good state.

If the monorepo rollout cannot satisfy the contract quickly, operators may fall back to the prior Chainsaw-managed LP deployment while preserving the same hedger IDs, job IDs, Redis key families, and payload field names.
