# LP Hedger Production Rollout

This runbook freezes the production rollout contract for the shared Flux host LP deployment.

## Topology

- Public shared host: `http://<host>:5022`
- Canonical LP GUI route: `/lp`
- Canonical LP API family: `/api/v1/hedgers/*`
- Hidden LP backend: `LP_API_BACKEND_URL=http://127.0.0.1:5025`
- Shared service target: `flux-lp.target`

The public Flux host continues to serve Fluxboard and Pulse on `:5022`. The dedicated LP backend remains loopback-only on `127.0.0.1:5025`.

## Active And Staged Instances

Band1 and Band2 remain the live auto-started production pair for this rollout:

- `service-eth-plume-lp-hedger`
- `service-eth-plume-lp-hedger-band2`

The shared `/lp` surface also exposes two staged generic entries:

- `hype_usdt_lp` via `service-hedger3`
- `plume_weth_lp` via `service-hedger4`

These staged services are enrolled in Pulse/systemd and have checked-in `.ini` configs, but they are deliberately kept out of `flux-lp.target` so install/reboot leaves them stopped until operators fix their readiness errors and start them explicitly.

`third_lp` remains hidden and template-only as `third_lp_hedger.ini.disabled`.

## Operator-Managed Files

Operators own these host-local files during rollout:

- `/etc/flux/common.env`
- `/etc/flux/lp-system.ini`
- `/etc/flux/lp-api.env`
- `/etc/flux/service-eth-plume-lp-hedger.env`
- `/etc/flux/service-eth-plume-lp-hedger-band2.env`
- `/etc/flux/service-hedger3.env`
- `/etc/flux/service-hedger4.env`

Required shared-host values:

- `LP_API_BACKEND_URL=http://127.0.0.1:5025`
- `LP_SYSTEM_CONFIG=/etc/flux/lp-system.ini`
- `/etc/flux/lp-system.ini` must be readable by the Flux service user `ubuntu`, for example `root:ubuntu` with mode `0640`

## Operator Surface Contract

The `/lp` surface is the production home for the Chainsaw LP Hedger operator workflow. The shared Fluxboard surface must preserve:

- hedger instance selection for Band1, Band2, `hype_usdt_lp`, and `plume_weth_lp`
- running/stopped, dry-run, and hedging-enabled state visibility
- restart and enable/disable controls
- config editing
- geometry override editing
- threshold override editing
- recent-hedges clear
- staged readiness visibility for generic entries via `config_ready` and `config_readiness_errors`

Intentional monorepo deltas must be documented in `fluxboard/docs/lp_contract.md`; they are not allowed to drift implicitly.

## Preflight Checklist

Before enabling `flux-lp.target`, confirm:

```bash
python3 ops/scripts/lp_hedger_preflight.py --json
```

The preflight must report `ok: true` before operators proceed. It checks:

1. `/etc/flux/common.env` points the public host at `LP_API_BACKEND_URL=http://127.0.0.1:5025`.
2. `/etc/flux/lp-system.ini` contains `[redis]`, `[plume]`, `[bybit]`, `[bybit_hedger]`, and `[bybit_hedger_band2]`.
3. `/etc/flux/lp-system.ini` is readable by the Flux service user `ubuntu`.
4. The Band1, Band2, `hype_usdt_lp`, and `plume_weth_lp` hedger INI files exist and are readable.
5. The shared public host remains on `:5022`, and the LP API remains loopback-bound on `:5025`.

## Install And Restart Order

1. Refresh the shared frontend bundles from the rollout worktree:

   ```bash
   pnpm --dir fluxboard build
   pnpm --dir pulse-ui build
   ```

2. Install or refresh systemd assets with `sudo ops/scripts/deploy/install_lp_systemd.sh`.
3. Update `/etc/flux/common.env` and `/etc/flux/lp-system.ini`, then ensure `/etc/flux/lp-system.ini` is readable by `ubuntu` (for example `sudo chown root:ubuntu /etc/flux/lp-system.ini && sudo chmod 0640 /etc/flux/lp-system.ini`).
4. Update the per-service env files for `lp-api`, Band1, Band2, `service-hedger3`, and `service-hedger4`.
5. Run `sudo systemctl daemon-reload`.
6. Restart the shared public host so it reloads `LP_API_BACKEND_URL` from `/etc/flux/common.env`.
7. Start or restart `flux-lp.target`.

`flux-lp.target` intentionally starts only `lp-api`, Band1, and Band2. `service-hedger3` and `service-hedger4` remain stopped after cutover unless operators start them manually.

The restart order is important because the public host must see the current shared-host env before operators expect `/lp` or `/api/v1/hedgers/*` to proxy correctly.

## Smoke Validation

After cutover, verify:

```bash
curl -fsS http://127.0.0.1:5022/lp >/dev/null
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/instances
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/eth_plume_lp
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/hype_usdt_lp
curl -fsS http://127.0.0.1:5022/api/v1/hedgers/plume_weth_lp
curl -fsS http://127.0.0.1:5022/api/pulse/jobs
```

Then confirm the Pulse-managed service set shows healthy jobs for:

- `lp-api`
- `service-eth-plume-lp-hedger`
- `service-eth-plume-lp-hedger-band2`
- `service-hedger3`
- `service-hedger4`

Run the scripted rollout check and record the result in `docs/reviews/2026-03-09-lp-hedger-prod-rollout.md`:

```bash
bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022
```

## Canary And Go/No-Go

Use Band1 and Band2 on `/lp` as the live canary surface before declaring the rollout complete. The staged generic entries should be visible on the selector but remain blocked from restart/enable until their configs are ready.

Go/no-go gates:

1. `python3 ops/scripts/lp_hedger_preflight.py --json` reports `ok: true`.
2. `bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022` succeeds.
3. `/lp` shows Band1, Band2, `hype_usdt_lp`, and `plume_weth_lp`; Band1/Band2 preserve the live operator controls, and the staged generic entries show readiness errors while restart/enable remain blocked.
4. Pulse reports healthy jobs for `lp-api`, `service-eth-plume-lp-hedger`, and `service-eth-plume-lp-hedger-band2`, and reports `service-hedger3` and `service-hedger4` as managed stopped jobs.
5. Any remaining GUI deltas are only the documented monorepo differences in `fluxboard/docs/lp_contract.md`.

If any gate fails, the decision is no-go and operators should execute rollback immediately instead of widening the rollout.

## Rollback

Rollback keeps the user-facing route contract unchanged:

1. Stop `flux-lp.target`.
2. Restore the prior `/etc/flux/common.env`, `/etc/flux/lp-system.ini`, and per-service env files.
3. Restart the shared public host to restore the previous proxy target.
4. Verify `/lp`, `/api/v1/hedgers/*`, and Pulse behavior against the prior known-good state.

Rollback trigger conditions include any failed scripted rollout check, missing `/lp` operator controls, unhealthy Pulse jobs for Band1/Band2, staged jobs unexpectedly auto-starting, or any unexpected change to chainsaw-compatible hedger IDs, job IDs, Redis key families, or payload field names.

If the monorepo rollout cannot satisfy the contract quickly, operators may fall back to the prior Chainsaw-managed LP deployment while preserving the same hedger IDs, job IDs, Redis key families, and payload field names.
