# LP Hedger Port Review Summary

## Outcome

Manual closeout review found no remaining blocking issues in the LP hedger port after the final smoke-driven fixes.

## What Changed In Closeout

- `systems/lp/lp/runners/run_api.py` now injects a real Redis client plus Pulse-backed job status/control callables into `create_lp_api_app(...)`.
- `systems/flux/flux/pulse/api.py` exposes lightweight `get_job_status(...)` and `control_job(...)` helpers for non-route consumers.
- `ops/scripts/deploy/lp_stack.sh` now treats `deploy/lp/lp_stack.env` as defaults only and preserves explicit caller-provided `LP_*` overrides.
- `systems/flux/flux/runners/tokenmm/run_api.py` now keeps `/lp` on the public Fluxboard host, proxies only `/api/v1/hedgers/*` to `lp-api`, and serves Fluxboard SPA routes at `/lp`.

## Verification

- `uv run --group test pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/examples/lp/test_lp_stack_contract.py tests/unit_tests/lp/test_config.py tests/unit_tests/lp/test_registry.py tests/unit_tests/lp/test_core.py tests/unit_tests/lp/api/test_app.py tests/unit_tests/lp/runners/test_run_api.py tests/unit_tests/lp/runners/test_run_hedger.py`
  - Result: `67 passed`
- `pnpm --dir fluxboard exec vitest run config/uiProfiles.test.ts main.routes.test.tsx App.test.tsx Nav.test.tsx Hedger.test.tsx`
  - Result: `55 passed`
- PTY smoke on alternate local ports because `5022` was already occupied in the workspace by an unrelated long-running public TokenMM host
  - `/lp` -> `200`
  - `/api/v1/hedgers/instances` -> `200`
  - `/api/v1/hedgers/eth_plume_lp` -> `200`

## Residual Risks

- Local smoke showed `job_status: "unknown"` for LP hedgers, which is expected in this dev environment because the PTY smoke does not provision real systemd/Pulse env enrollment for hedger jobs.
- The current built Fluxboard HTML references `/tokenmm/assets/...` from the shared dist bundle even when served at `/lp`; this still works because the shared public host serves those assets, but the asset base remains coupled to the shared Fluxboard build output.

## Notes

- Multiple reviewer-agent attempts were interrupted during final closeout, so the final Task 7/8 review judgment above is manual and backed by the recorded verification evidence.
