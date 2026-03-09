# Equities MakerV4 Review Follow-Ups Review

## Scope

This review covers the follow-up plan in `docs/plans/2026-03-09-equities-makerv4-review-followups.md`.
It verifies the review-driven fixes for:

- truthful Makerv4 runtime controls and telemetry
- fail-fast equities API and bridge behavior
- dual-venue balances publication/merge semantics
- local stack secret and IBKR readiness contracts

## Verification

Backend verification:

- `uv run --group test pytest -q tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_run_bridge.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/strategies/makerv4` -> `163 passed`

Fluxboard verification:

- `cd fluxboard && pnpm vitest run tests/signal/MakerV4SignalTable.test.tsx Balances.test.tsx __tests__/config/paramsProfiles.test.ts` -> `23 passed`

Additional task slices:

- Task 6 dual-venue balances slice -> `59 passed`
- Task 7 deploy/secret contract slice -> `14 passed`
- `bash -n ops/scripts/deploy/equities_stack.sh` -> passed

Note:

- `fluxboard/__tests__/panels/signal.test.tsx` is excluded by the current Vitest config, so it was not part of the runnable verification set.

## Findings Status

Closed:

1. Makerv4 now exposes a supplemental IBKR balance snapshot hook and the shared balance publisher appends that snapshot without changing the existing Hyperliquid account path.
2. Nested balance account events preserve explicit account venue, so real IBKR account IDs such as `U1234567` no longer lose venue identity during flattening.
3. Equities runner wiring now attaches an IBKR reference balance snapshot provider for Makerv4 when the active reference venue is the dockerized/data-only IBKR path.
4. Shared-account IBKR cash semantics remain API-owned, and merged balances continue to mark repeated same-account cash rows as `scope="shared_account"`.
5. Local `equities_stack.sh` now allowlists `TRADE_XYZ_VAULT_ADDRESS` in AWS-secret loading and validates `TWS_USERNAME` / `TWS_PASSWORD` when the active strategy contract uses the dockerized IBKR gateway.
6. Deploy docs and env examples now match the real local-smoke credential contract and include rollback instructions.

## Residuals

One live residual remains on the current host:

- The local IBKR gateway handshake is currently failing on `127.0.0.1:4001`, so live `/api/v1/balances?profile=equities` still shows only the Hyperliquid row and the IBKR signal leg remains stale. The node journal shows repeated `ConnectionError("Interactive Brokers handshake did not complete; server version was not received.")`.

This is a live environment/runtime issue, not a remaining code-level review gap in the follow-up patch set.
