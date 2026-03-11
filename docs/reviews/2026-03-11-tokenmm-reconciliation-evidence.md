# TokenMM Reconciliation Failure Evidence

## Scope

This note captures the live evidence for the `2026-03-11` startup reconciliation
failures on:

- `plumeusdt_bybit_perp_makerv3`
- `plumeusdt_okx_perp_makerv3`

The goal is to pin down the exact mismatch shape before any code changes are
attempted.

## Service lifecycle outcome

Both nodes failed closed during startup reconciliation and exited with the same
terminal lifecycle:

- `Execution state could not be reconciled`
- `Startup failed: execution state reconciliation did not complete`
- `Main process exited, code=exited, status=78/CONFIG`
- `Failed with result 'exit-code'`

Observed service timestamps:

- Bybit perp failed at `2026-03-11 06:30:19 UTC`
- OKX perp failed at `2026-03-11 06:29:41 UTC`

## Bybit perp evidence

### Exact startup mismatch

From the restart journal window around `2026-03-11 06:30:18 UTC`:

- `Received 0 FillReports`
- `Received 1 PositionStatusReport`
- `Received 0 OrderStatusReports`
- warning: missing cached order for `PLUMEUSDT-LINEAR.BYBIT-EXTERNAL`
- `report.signed_decimal_qty=Decimal('71875')`
- `position_signed_decimal_qty=Decimal('84875')`
- fatal reconciliation error because `generate_missing_orders` is disabled

### Mismatch classification

This is not a pure position-only mismatch.

Observed supporting facts:

- the engine registered one `EXTERNAL` position id and external order claims
- the cache warned that order `4c4fe5a4-543c-4a23-8c85-c9b8fd81831f` was missing
  for position `PLUMEUSDT-LINEAR.BYBIT-EXTERNAL`
- there were no fresh fill reports during this restart window
- from the captured evidence alone, whether the failing set includes an owned
  non-`EXTERNAL` strategy position is undetermined

### Raw failure lines

Relevant journal facts:

- `ExecEngine: Set PositionId count for StrategyId('EXTERNAL') to 1`
- `ExecEngine: Registered external order claims for plumeusdt_bybit_perp_makerv3-000`
- `Cache: Order ... missing in cache for position PLUMEUSDT-LINEAR.BYBIT-EXTERNAL`
- `Cannot reconcile PLUMEUSDT-LINEAR.BYBIT: position net qty 84875 != reported net qty 71875 and generate_missing_orders is disabled`

## OKX perp evidence

### Exact startup mismatch

From the restart journal window around `2026-03-11 06:29:40 UTC`:

- `Received 1 PositionReport`
- `Received 4 FillReports`
- `Received 110 OrderStatusReports`
- external order `696c34a4-b114-4c52-afc7-1f05e9cd8018` was claimed by the
  strategy during reconciliation
- that order emitted a fresh `OrderFilled` event for `last_qty=2356`
- the strategy emitted a fresh `PositionChanged` event during the same startup
  window
- warnings: three missing cached orders for `PLUME-USDT-SWAP.OKX-EXTERNAL`
- `report.signed_decimal_qty=Decimal('2756')`
- `position_signed_decimal_qty=Decimal('5112')`
- fatal reconciliation error because `generate_missing_orders` is disabled

### Mismatch classification

This mismatch is order/fill-linked.

Observed supporting facts:

- `EXTERNAL`-linked reconciliation state
- fresh fill processing during startup
- missing cached order lineage for `EXTERNAL` positions
- a strategy-owned `PositionChanged` event during the same startup window

### Raw failure lines

Relevant journal facts:

- `ExecEngine: Set PositionId count for StrategyId('EXTERNAL') to 1`
- `ExecEngine: Registered external order claims for plumeusdt_okx_perp_makerv3-000`
- `ExecEngine: External order 696c34a4-b114-4c52-afc7-1f05e9cd8018 ... claimed by strategy`
- `OrderFilled(... last_qty=2_356 ...)`
- `PositionChanged(... signed_qty=23237.0 ...)`
- three `Cache: Order ... missing in cache for position PLUME-USDT-SWAP.OKX-EXTERNAL`
- `Cannot reconcile PLUME-USDT-SWAP.OKX: position net qty 5112 != reported net qty 2756 and generate_missing_orders is disabled`

## Cross-case summary

Shared facts across both failures:

- both happen inside startup reconciliation, not steady-state quoting
- both fail in the same netting mismatch branch while
  `generate_missing_orders = false`
- both include `EXTERNAL`-linked reconciliation state
- both include missing cached order lineage warnings for `...-EXTERNAL`
- both shut the node down before the strategy can publish a fresh healthy state

Important difference:

- Bybit perp restart shows no fresh fills arriving during startup
- OKX perp restart does show fresh fills and order reports arriving during
  startup

## Evidence commands used

Primary commands used to collect this note:

- `journalctl -u flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service --since '2026-03-11 06:29:58 UTC' --until '2026-03-11 06:30:19 UTC' --no-pager`
- `journalctl -u flux@tokenmm-node-plumeusdt_okx_perp_makerv3.service --since '2026-03-11 06:29:35 UTC' --until '2026-03-11 06:29:41 UTC' --no-pager`
- `systemctl status --no-pager flux@tokenmm-node-plumeusdt_bybit_perp_makerv3.service`
- `systemctl status --no-pager flux@tokenmm-node-plumeusdt_okx_perp_makerv3.service`
