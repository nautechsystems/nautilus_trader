1. Blocking findings

None in the executed Makerv4 verification matrix or the current code slice.

2. Non-blocking findings

- Live Saturday canary is only a partial venue-proof. The current `paper` node shows the correct MakerV4 row shape, Hyperliquid maker identity, IBKR hedge/reference identity, and a non-degraded balances endpoint, but it does not currently prove live IBKR quote flow or dual-venue live balances/positions. The live balances endpoint currently returns one Hyperliquid `USDC` row only.
- IBKR ref connectivity is operationally noisy on the weekend host. The current node journal shows watchdog reconnects and one transient `client id already in use` (`code: 326`) recovery path before the client resumed. That leaves the ref leg present but stale/empty, which is acceptable for the Saturday canary but not final weekday rollout evidence.
- The bridge journal still contains older pre-fix historical errors from the earlier colon-safety crash, so only the current Makerv4-topic tail should be treated as clean evidence.

3. Residual unrelated failures

None in the executed Makerv4 Python or Fluxboard verification slices.

4. Verification evidence

- Focused lifecycle slice:
  - `uv run --group test pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
  - Result: `9 passed`
- Focused runner/payload slices:
  - `uv run --group test pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'real_makerv4_strategy_satisfies_trader_registration_contract or build_node_derives_makerv4_ibkr_reference_instrument_from_primary_exchange or build_node_passes_makerv4_hedge_config_fields'`
  - Result: `3 passed, 13 deselected`
  - `uv run --group test pytest -q tests/unit_tests/flux/api/test_payloads.py -k 'emits_makerv4_quote_snapshot'`
  - Result: `1 passed, 42 deselected`
- Task 11 Python matrix:
  - `uv run --group test pytest -q tests/integration_tests/adapters/hyperliquid/test_trade_xyz_adapter_contract.py tests/unit_tests/examples/strategies/test_live_venue_registry.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_api.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv4/test_identity_map.py tests/unit_tests/flux/strategies/makerv4/test_instruments.py tests/unit_tests/flux/strategies/makerv4`
  - Result: `162 passed in 3.73s`
- Task 11 Fluxboard slice:
  - `cd fluxboard && pnpm vitest run tests/signal/MakerV4SignalTable.test.tsx Balances.test.tsx __tests__/config/paramsProfiles.test.ts __tests__/panels/signal.test.tsx`
  - Result: `21 passed`

5. Live smoke evidence

- Node service:
  - `flux@equities-node-aapl_tradexyz_makerv4.service` is currently `active (running)`.
  - Current journal shows Makerv4 `SubscribeQuoteTicks` for both `xyz:AAPL-USD-PERP.HYPERLIQUID` and `AAPL.NASDAQ`.
- Signals:
  - `curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities'`
  - Current row shows `id=aapl_tradexyz_makerv4`, `strategy_family=maker_v4`, `balances_ok=true`, maker leg venue `HYPERLIQUID`, hedge/ref venue `IBKR`, and no fake negative IBKR prices in the normalized quote snapshot.
  - Earlier in-session smoke also observed live Hyperliquid maker prices (`255.95 / 256.20`) before the later restart/reconnect cycle; the Saturday canary is therefore capable of receiving maker data, but that signal was not stable after the final restart window.
- Balances:
  - `curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'`
  - Current result is non-degraded with `snapshot_present=true`, `missing_required=[]`, and one `hyperliquid / USDC` row for `HYPERLIQUID-master`.
- Bridge:
  - Current bridge tail shows discovery of Makerv4 inbound streams for `.balances`, `.state`, and `.market_bbo`.
  - No new Makerv4 handler traceback appeared in the current tail; older historical errors remain in the same journal window from pre-fix earlier restarts.

6. Rollback note

- To disable MakerV4 cleanly, stop `flux@equities-node-aapl_tradexyz_makerv4.service` and remove or disable `deploy/equities/strategies/aapl_tradexyz_makerv4.toml` from the equities allowlist/install path.
- The prior MakerV3 equities config remains available for emergency re-enable if you need to restore the earlier signal path while preserving `/equities`, `profile=equities`, and `portfolio=equities`.
- During rollback, `/equities` reverts from the MakerV4 dual-leg signal row shape to the prior MakerV3 surface, and the dedicated MakerV4 signal table is no longer expected to populate.
