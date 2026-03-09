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

1. Refresh the shared frontend bundles from the rollout worktree:

   ```bash
   pnpm --dir fluxboard build
   pnpm --dir pulse-ui build
   ```

2. Install or refresh systemd assets with `sudo ops/scripts/deploy/install_lp_systemd.sh`.
3. Update `/etc/flux/common.env` and `/etc/flux/lp-system.ini`.
4. Update the per-service env files for `lp-api`, Band1, and Band2.
5. Run `sudo systemctl daemon-reload`.
6. Restart the shared public host so it reloads `LP_API_BACKEND_URL` from `/etc/flux/common.env`.
7. Start or restart `flux-lp.target`.

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

Run the scripted rollout check and record the result in `docs/reviews/2026-03-09-lp-hedger-prod-rollout.md`:

```bash
bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022
```

## Canary And Go/No-Go

Use Band1 and Band2 on `/lp` as the canary surface before declaring the rollout complete.

Go/no-go gates:

1. `python3 ops/scripts/lp_hedger_preflight.py --json` reports `ok: true`.
2. `bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022` succeeds.
3. `/lp` shows Band1 and Band2 with the expected running/stopped, dry-run, and hedging-enabled visibility plus restart, enable/disable, config, geometry, threshold, and clear controls.
4. Pulse reports healthy jobs for `lp-api`, `service-eth-plume-lp-hedger`, and `service-eth-plume-lp-hedger-band2`.
5. Any remaining GUI deltas are only the documented monorepo differences in `fluxboard/docs/lp_contract.md`.

If any gate fails, the decision is no-go and operators should execute rollback immediately instead of widening the rollout.

## Rollback

Rollback keeps the user-facing route contract unchanged:

1. Stop `flux-lp.target`.
2. Restore the prior `/etc/flux/common.env`, `/etc/flux/lp-system.ini`, and per-service env files.
3. Restart the shared public host to restore the previous proxy target.
4. Verify `/lp`, `/api/v1/hedgers/*`, and Pulse behavior against the prior known-good state.

Rollback trigger conditions include any failed scripted rollout check, missing `/lp` operator controls, unhealthy Pulse jobs for Band1/Band2, or any unexpected change to chainsaw-compatible hedger IDs, job IDs, Redis key families, or payload field names.

If the monorepo rollout cannot satisfy the contract quickly, operators may fall back to the prior Chainsaw-managed LP deployment while preserving the same hedger IDs, job IDs, Redis key families, and payload field names.
