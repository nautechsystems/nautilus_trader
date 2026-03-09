# LP Hedger Production Rollout Review

## Outcome

- Decision: `GO`
- Shared host base URL: `http://13.213.194.42:5022`
- Restart window:
  - `flux@tokenmm-api.service`: `2026-03-09 16:28 UTC`
  - `flux-lp.target`: `2026-03-09 16:28 UTC`

## Verification Evidence

- Preflight:
  - `python3 ops/scripts/lp_hedger_preflight.py --json`
  - Result: `ok=true`, `errors=[]`, `warnings=[]` during the final prepare/cutover cycle on `2026-03-09 16:27 UTC`.
- Rollout check:
  - `bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022`
  - Result: `rollout checks passed against http://127.0.0.1:5022` during the final cutover on `2026-03-09 16:28 UTC`.

## Live Smoke Evidence

- `/lp`: serves the Fluxboard SPA and references the neutral shared asset prefix `/static/fluxboard/assets/index-Tc8Lx3ok.js`.
- `/api/v1/hedgers/instances`: returns exactly two active production instances, `eth_plume_lp` and `eth_plume_lp_band2`, which is the public selector contract for `/lp`.
- `/api/v1/hedgers/eth_plume_lp`: returns `ok=true` with `job_status=active`, `hedger_enabled=true`, `dry_run=false`, plus geometry/threshold overrides and recent hedges payloads.
- `/api/pulse/jobs`: returns the Pulse jobs payload with the LP group present and active (`lp-api`, `service-eth-plume-lp-hedger`, `service-eth-plume-lp-hedger-band2`); final public snapshot was `total=36`, `active=11`, `failed=3`.

## Residual Risks

- The Band1 and Band2 LP hedger services are active, but both are logging `ccxt.base.errors.AuthenticationError` because the configured Bybit API key on the host is expired (`retCode=33004`). The operator GUI is deployed and healthy, but live hedging remains credential-blocked until the Bybit key is rotated outside the repo.
- During follow-up deployment, the shared `flux@tokenmm-api.service` had drifted to `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-telemetry-go-prod`, which lacked `fluxboard/dist/index.html` and crash-looped `:5022`. The final prepare/cutover restored the service back to the LP rollout worktree.
- `/api/pulse/jobs` still reports unrelated non-LP TokenMM failures on the shared host (`failed=3` in the final public snapshot). Those failures were present outside the LP contract and were recorded separately rather than treated as LP rollout regressions.

## Rollback Note

If rollback is required, use the final backup root from the successful restore/cutover path, `/root/flux-lp-rollout-backups/20260309T162757Z`, then stop `flux-lp.target`, restore the prior `/etc/flux/common.env`, `/etc/flux/lp-system.ini`, and per-service env files, restart `flux@tokenmm-api.service`, and confirm `/lp`, `/api/v1/hedgers/instances`, and `/api/pulse/jobs` match the previous known-good state or the Chainsaw fallback.
