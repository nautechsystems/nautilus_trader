# Equities Prod Feed Follow-Up

## Context

PR `#91` merged the reviewed equities market-data recovery V1 and the Flux-owned IBKR reference publisher rehome.
That merge fixed the ownership boundary and removed the live equities dependency on `~/chainsaw`.

The remaining work is operational/runtime follow-up:

- equities prod still does not have full-universe end-to-end live pricing
- a subset of strategies still show missing or stale maker/reference legs
- readiness and dashboard health still need to converge with the actual live feed state

## Scope

Keep this wave narrow:

- restore full equities universe live pricing in prod
- debug remaining IBKR reference publisher coverage gaps
- debug remaining Hyperliquid/Binance maker-feed gaps
- reconcile readiness, signal payloads, and actual quote freshness per pair

Out of scope:

- new market-data platform or daemon architecture
- grouped-node topology changes
- `/equities` or `/api/v1/signals?profile=equities` contract redesign
- canonical product identity overhaul
  - tracked separately in issue `#96`
- longer-term shared venue/session architecture
  - tracked separately in issue `#92`

## Immediate Debug Targets

1. Confirm the live prod publisher status and per-instrument coverage in Redis for every enrolled IBKR reference symbol.
2. Trace missing maker feeds for the remaining bad symbols and separate adapter gaps from strategy-local delivery gaps.
3. Verify pair-level tradeability and signal/readiness rendering against the actual live quote timestamps.
4. Re-run prod validation once the live keys advance end to end.

## Verification

- `GET /api/v1/signals?profile=equities`
- `GET /api/v1/readiness?profile=equities`
- `./.venv/bin/python -m pytest tests/unit_tests/adapters/interactive_brokers/test_shared_reference_data.py -q`
- `./.venv/bin/python -m pytest tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_controller.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_balances_merge_dedupe.py -q`
