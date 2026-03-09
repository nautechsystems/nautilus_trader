# LP Generic Extra Instances Rollout Review

## Outcome

- Decision: `GO`
- Shared host base URL: `http://13.213.194.42:5022`
- Backup root: `/root/flux-lp-rollout-backups/20260309T170510Z-generic-extra`
- Final successful cutover window: `2026-03-09 17:05-17:08 UTC`
- Final deployed worktree: `/home/ubuntu/nautilus_trader/.worktrees/lp-hedger-go-prod-finalize` tracking the PR branch head after the docs-only closeout commits

## Verification Evidence

- Local verification:
  - `python3 -m pytest -q --noconftest tests/unit_tests/lp/api/test_app.py tests/unit_tests/lp/test_registry.py tests/unit_tests/examples/lp/test_lp_prod_rollout_contract.py tests/unit_tests/examples/lp/test_lp_stack_contract.py`
  - Result: `36 passed`
- Fluxboard verification:
  - `pnpm --dir fluxboard exec vitest run Hedger.test.tsx main.routes.test.tsx config/uiProfiles.test.ts`
  - Result: `41 passed`
- Build verification:
  - `pnpm --dir fluxboard build`
  - Result: `PASS`
- Installer verification:
  - `bash -n ops/scripts/deploy/install_lp_systemd.sh`
  - Result: `PASS`
- Host preflight:
  - `sudo python3 ops/scripts/lp_hedger_preflight.py --json`
  - Result: `ok=true`, `errors=[]`, `warnings=[]`
- Host rollout check:
  - `bash ops/scripts/deploy/check_lp_rollout.sh --base-url http://127.0.0.1:5022`
  - Result: `rollout checks passed against http://127.0.0.1:5022`
- Final clean-worktree repoint:
  - Rebuilt `fluxboard/dist` in `/home/ubuntu/nautilus_trader/.worktrees/lp-hedger-go-prod-finalize`, regenerated `/etc/flux/tokenmm-api.env` from `ops/scripts/deploy/install_tokenmm_systemd.sh`, materialized the ignored `nautilus_trader/*.so` runtime extensions and `pulse-ui/dist` under the clean worktree, updated `/etc/flux/common.env` to that worktree, restarted the shared services, and reran the localhost/public rollout smokes.
  - Result: `PASS`

## Live Smoke Evidence

- `/lp` now serves the shared Fluxboard SPA with the neutral asset prefix `/static/fluxboard/assets/*`.
- `/api/v1/hedgers/instances` now returns exactly four public selector entries:
  - `eth_plume_lp`
  - `eth_plume_lp_band2`
  - `hype_usdt_lp`
  - `plume_weth_lp`
- `hype_usdt_lp` and `plume_weth_lp` return `staged=true`, `config_ready=false`, and concrete `config_readiness_errors` while remaining operator-visible on `/lp`.
- `/api/pulse/jobs` reports the LP group with:
  - `lp-api=active`
  - `service-eth-plume-lp-hedger=active`
  - `service-eth-plume-lp-hedger-band2=active`
  - `service-hedger3=inactive`
  - `service-hedger4=inactive`
- Final public Pulse snapshot: `total=38`, `active=12`, `failed=3`

## Host Drift Corrected During Rollout

- `/etc/flux/common.env` still pointed `WORKDIR` and `PYTHONPATH` at `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr`. That caused the shared host to import stale code until those pointers were updated first to `/home/ubuntu/nautilus_trader/.worktrees/lp-hedger-go-prod` for the initial cutover and then to the clean final deployment worktree `/home/ubuntu/nautilus_trader/.worktrees/lp-hedger-go-prod-finalize`.
- `/etc/flux/tokenmm-api.env` still pointed `FLUXBOARD_DIST` at `/home/ubuntu/nautilus_trader/fluxboard/dist`, which served stale `/tokenmm/assets/*` HTML on `/lp`. Updating it first to `/home/ubuntu/nautilus_trader/.worktrees/lp-hedger-go-prod/fluxboard/dist` restored the neutral `/static/fluxboard/assets/*` contract, and the final deployment now serves from `/home/ubuntu/nautilus_trader/.worktrees/lp-hedger-go-prod-finalize/fluxboard/dist`.

## Residual Risks

- Band1 and Band2 are active again, but they still log `ccxt.base.errors.AuthenticationError` because the configured Bybit key on the host is expired. Live hedging remains credential-blocked until the key is rotated outside the repo.
- `service-hedger3` and `service-hedger4` are intentionally inactive because their staged configs still fail readiness on zero pool addresses and missing Bybit credentials.
- `/api/pulse/jobs` still reports unrelated non-LP shared-host failures (`failed=3`).

## Rollback Note

If rollback is required, restore the files captured under `/root/flux-lp-rollout-backups/20260309T170510Z-generic-extra`, then restart `flux@tokenmm-api.service`, `flux@lp-api.service`, `flux@service-eth-plume-lp-hedger.service`, and `flux@service-eth-plume-lp-hedger-band2.service`, and confirm `/lp`, `/api/v1/hedgers/instances`, and `/api/pulse/jobs` match the previous known-good state.
