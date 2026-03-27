# Trades Realtime Standard Cutover

Date: 2026-03-26
Branch: `fix/trades-blotter-hardening-20260326`
Worktree: `/home/ubuntu/nautilus_trader/.worktrees/trades-blotter-hardening`

## Status

Trades now uses the backend standard Socket.IO contract in the canonical live view, and the
post-PR72 boundary hardening is complete on the branch above:

- the canonical first-page unfiltered descending snapshot requests `contract_version=2`
- the frontend subscribes with lineage metadata via `subscribe`
- matching `realtime_event` `delta_batch` packets drive steady-state live updates
- global resync completion is now surface-aware, so trades-only TokenMM and Equities surfaces do not
  wait on impossible `order-view` acknowledgements
- non-canonical trades views remain REST-backed and report `LIVE` when the snapshot is fresh instead
  of flashing `RECOVERING` because lineage is intentionally absent
- reconnect, `trade_gap`, and lineage-mismatch recovery are bounded and deduped instead of churning
- degraded TokenMM trade rows with explicit `qty_venue` plus degraded conversion metadata are treated
  as normalized rows, not legacy compatibility rows

Non-canonical trades views remain REST-only and do not advertise `data.realtime`. Removing the last
TokenMM compatibility copy in production is still gated on the runbook cutover that clears retained
legacy Redis trade rows.

## Behavioral Contract

- the canonical standard live view is `page=1`, `page_size=50`, `sort=ts_desc`, with no filters
- the canonical live snapshot requests `contract_version=2` and must return `realtime` lineage before
  the standard subscription is armed
- healthy standard steady state does not run parallel HTTP delta polling
- trade row ordering still uses the inner trade row `seq`; the standard envelope `seq` is tracked separately as the surface cursor
- reconnect and resubscribe use the latest acknowledged standard cursor
- standard envelope seq gaps trigger bounded HTTP delta recovery from the last acknowledged seq
- `recovery_required` with `reason=trade_gap`, `invalidate`, and lineage mismatch trigger bounded
  canonical snapshot recovery; one unchanged condition should not emit repeated recovery churn
- reconnect no longer opens a second snapshot while a standard trade-gap recovery snapshot is already in flight
- `RECOVERING` copy is explicit:
  - reconnect catch-up: `RECOVERING - Reconnecting…`
  - snapshot refresh: `RECOVERING - Refreshing snapshot…`
  - seq-gap replay: `RECOVERING - Replaying missed trades…`
- `manual_refresh_required` is sticky across reconnects; the panel does not silently auto-recover until the user refreshes
- queued recovery snapshots that started before `manual_refresh_required` are discarded and cannot silently clear the fail-closed state
- returning from a non-canonical view waits for a fresh canonical snapshot before the standard subscription is re-armed
- reconnect still forces a fresh canonical snapshot after leave/re-enter churn even if an older canonical request resolves late
- non-canonical views remain honest snapshot mode: a fresh REST view shows `LIVE`, not `RECOVERING`
- legacy events without epoch metadata remain compatible in flag-off mode and do not spuriously trigger snapshot refreshes
- additive legacy `market_update.recovery` hints are edge-triggered for unchanged cursor/legacy conditions

## Rollout Notes

- Steady-state live traffic now runs through standard Socket.IO `subscribe` / `realtime_event` / `unsubscribe`.
- Recovery still uses REST snapshot/delta paths; the current capabilities remain polling-oriented rather
  than websocket replay-oriented.
- Backend `trade_update` removal remains blocked until the remaining rollback and bridge cleanup is complete.
- Run [tokenmm-trades-blotter-cutover.md](/home/ubuntu/nautilus_trader/.worktrees/trades-blotter-hardening/docs/runbooks/tokenmm-trades-blotter-cutover.md)
  before removing the last TokenMM compatibility warning in production.

## Verification

- `VITEST_FULL=1 pnpm --dir fluxboard exec vitest run __tests__/resync-contract.test.tsx __tests__/TradesStore.test.ts __tests__/trades-status.test.tsx Trades.test.tsx __tests__/trades-integration.test.tsx __tests__/realtime/compatibility-matrix.test.tsx`
  - Result: `84` tests passed
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 /home/ubuntu/nautilus_trader/.worktrees/prod-lanes-exec-20260326/.venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/flux/api/test_realtime_contract.py`
  - Result: `84` tests passed

## Known Limits

- The browser harness proves the frontend contract against deterministic sockets, not against a live backend deployment.
- The backend still exposes legacy event traffic for rollback clients and bridge-backed surfaces.
- The last TokenMM compatibility warning must remain until the Redis trade-stream cutover has been run and
  live `compatibility_mode` is observed false.
