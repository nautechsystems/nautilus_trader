# LP Hedger Production Rollout Review

## Outcome

- Decision: `GO` or `NO-GO`
- Shared host base URL: `http://<host>:5022`
- Restart window:
  - `flux@tokenmm-api.service`: `YYYY-MM-DD HH:MM UTC`
  - `flux-lp.target`: `YYYY-MM-DD HH:MM UTC`

## Verification Evidence

- Preflight:
  - `python3 ops/scripts/lp_hedger_preflight.py --json`
  - Result: `...`
- Rollout check:
  - `bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://<host>:5022`
  - Result: `...`

## Live Smoke Evidence

- `/lp`: `...`
- `/api/v1/hedgers/instances`: `...`
- `/api/v1/hedgers/eth_plume_lp`: `...`
- `/api/pulse/jobs`: `...`

## Residual Risks

- `...`

## Rollback Note

If the rollout is a no-go, stop `flux-lp.target`, restore the prior `/etc/flux/common.env`, `/etc/flux/lp-system.ini`, and per-service env files, restart `flux@tokenmm-api.service`, and confirm `/lp`, `/api/v1/hedgers/instances`, and `/api/pulse/jobs` match the previous known-good state or the Chainsaw fallback.
