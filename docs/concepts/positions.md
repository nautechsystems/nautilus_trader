# Positions

This guide explains how NautilusTrader creates and updates positions from order fills, calculates
profit and loss (PnL), and preserves closed cycles under `NETTING` order management system (OMS)
configurations.

## Overview

A position records exposure to an instrument during an open-close cycle. It aggregates the fills
assigned to one position ID and tracks its quantity, average prices, realized PnL, commissions, and
related identifiers. Use a market price with the position's methods to calculate unrealized PnL and
notional value.

The execution engine creates positions when orders fill and tracks them from open to close. OMS
configuration determines whether fills share a net position or remain in separate hedged positions.

## Position lifecycle

### Creation

The system opens a position on the first fill:

- **NETTING OMS**: Opens on the first fill for an instrument and strategy. The position uses the
  deterministic ID `{instrument_id}-{strategy_id}`.
- **HEDGING OMS**: Opens on first fill for a new `position_id` (multiple positions per instrument).

A position tracks:

- Opening order and fill details.
- Entry side (`BUY` or `SELL`).
- Quantity and average entry price after applying the opening fill.
- Timestamps for initialization and opening.

:::tip
You can access positions through the Cache using `self.cache.position(position_id)` or
`self.cache.positions(instrument_id=instrument_id)` from within your actors/strategies.
:::

### Updates

As additional fills occur, the position:

- Aggregates quantities from buy and sell fills.
- Recalculates average entry and exit prices.
- Updates peak quantity for the current cycle.
- Tracks the current cycle's order IDs and trade IDs.
- Accumulates commissions by currency.

### Closure

A position closes when the net quantity becomes zero (`FLAT`). At closure:

- The closing order ID is recorded.
- Duration is calculated from open to close.
- Final realized PnL is computed.
- In `NETTING` OMS, when the position later reopens, the engine snapshots the closed state to
  preserve historical PnL (see [Position snapshotting](#position-snapshotting)).

## Order fill aggregation

Positions aggregate order fills to maintain an accurate view of market exposure. The aggregation
process handles both sides of trading activity:

### Buy fills

When a BUY order fills:

- Increases long exposure or reduces short exposure.
- Updates average entry price for opening trades.
- Updates average exit price for closing trades.
- Calculates realized PnL for any closed portion.

### Sell fills

When a SELL order fills:

- Increases short exposure or reduces long exposure.
- Updates average entry price for opening trades.
- Updates average exit price for closing trades.
- Calculates realized PnL for any closed portion.

### Net position calculation

The position maintains a `signed_qty` field representing the net exposure:

- Positive values indicate `LONG` positions.
- Negative values indicate `SHORT` positions.
- Zero indicates a `FLAT` (closed) position.

```python
# Example: Position aggregation
# Initial BUY 100 units at $50
signed_qty = +100  # LONG position

# Subsequent SELL 150 units at $55
signed_qty = -50  # Closes the LONG cycle and opens a SHORT cycle

# Final BUY 50 units at $52
signed_qty = 0  # Position FLAT (closed)
```

## Position adjustments

Position adjustments record quantity or PnL changes that occur outside normal order fills. The
system represents these changes as `PositionAdjusted` events.

### Base-currency commissions

When trading spot currency pairs (for example, BTC/USDT) or FX spot, commissions paid in the base
currency directly affect the net quantity received or delivered:

- **Opening fills**: Commission is deducted from the traded quantity. A buy of 1.0 BTC with
  0.001 BTC commission results in a net long position of 0.999 BTC.
- **Closing fills**: Commission is applied to `signed_qty` because it affects actual inventory.
  Selling a 0.999 BTC LONG position with 0.000999 BTC commission leaves you SHORT 0.000999 BTC,
  not FLAT, because you gave up 0.999999 BTC total.
- **Flips**: Commission affects the final position size on both sides of the flip.

:::note
Base-currency commissions only apply to spot currency pairs and FX spot instruments where the
commission currency matches `instrument.base_currency`. For other instruments, commissions are
tracked separately and do not affect position quantity.
:::

### Funding payments

Funding adjustments track periodic payments for perpetual futures without affecting position
quantity. They use `quantity_change = None` and can include a PnL change.

### Adjustment tracking

The position exposes its retained adjustments:

- `position.adjustments()` returns the list of all `PositionAdjusted` events.
- Each adjustment includes its type (`COMMISSION` or `FUNDING`), quantity or PnL change, reason,
  event ID, and timestamps.
- The current adjustment history is cleared when a closed position reopens.
- If fills remain after `purge_events_for_order()`, the position regenerates commission adjustments
  from the surviving fills and reapplies non-commission adjustments. If no fills remain, the
  position becomes an empty `FLAT` shell and clears its adjustment history.

## OMS types and position management

NautilusTrader supports two position management modes. A strategy configured with
`OmsType.UNSPECIFIED` uses the venue's OMS type. For configuration details and position ID rules,
see the [Execution guide](execution.md#order-management-system-oms).

### `NETTING`

In `NETTING` mode, fills for each instrument and strategy are aggregated into a single position:

- One position per instrument and strategy.
- All fills contribute to the same position.
- A fill that crosses zero closes the current cycle and opens a new cycle on the opposite side.
- Historical snapshots preserve closed position states.

### `HEDGING`

In `HEDGING` mode, multiple positions can exist for the same instrument:

- Multiple simultaneous `LONG` and `SHORT` positions.
- Each position has a unique position ID.
- Positions are tracked independently.
- No automatic netting across positions.
- A fill with a new position ID creates a separate position. If a later fill reuses a closed
  position ID, it replaces the cached state without creating a closed-cycle snapshot.

:::warning
`HEDGING` can increase margin requirements when a venue maintains long and short positions
independently. A venue with a `NETTING` OMS exposes only its net position, even when NautilusTrader
tracks multiple virtual positions. Check the venue's position mode and margin rules.
:::

### Strategy vs venue OMS

Strategy and venue OMS types can differ:

| Strategy OMS | Venue OMS | Result                                                              |
| ------------ | --------- | ------------------------------------------------------------------- |
| `NETTING`    | `NETTING` | One position per instrument and strategy.                           |
| `HEDGING`    | `HEDGING` | Multiple positions per instrument and strategy.                     |
| `NETTING`    | `HEDGING` | One virtual position across the venue positions.                    |
| `HEDGING`    | `NETTING` | Multiple virtual positions against the venue's single net position. |

:::tip
Align the strategy and venue OMS types unless the strategy requires virtual positions. See the
integration guide for the venue's position-mode configuration.
:::

## Position snapshotting

Position snapshotting preserves closed `NETTING` cycles for PnL tracking and reporting.

### Why snapshotting matters

In a `NETTING` system, when a position closes (becomes `FLAT`) and then reopens with a new trade,
the position object is reset to track the new exposure. Without snapshotting, the historical
realized PnL from the previous position cycle would be lost.

### How it works

When a closed `NETTING` position receives another fill for the same instrument and strategy, the
execution engine archives the closed state before opening the next cycle. The snapshot preserves:

- Final quantities and prices.
- Realized PnL.
- All fill events.
- Commission totals.

The cache stores snapshots by position ID. The active cache entry then represents the new cycle,
while previous snapshots remain accessible. The Portfolio includes their realized PnL in instrument
totals.

A fill void that corrects a fill from an earlier cycle is the one exception. The correction moves the
cycle boundaries the stored snapshots describe, so the engine replaces them with the cycles the
corrected history actually closes, keeping each counted once. See
[Position replay across NETTING cycles](execution.md#position-replay-across-netting-cycles).

:::note
This closed-cycle archive differs from optional position state snapshots. Setting
`snapshot_positions=True` publishes state when a position opens, changes, or closes, while
`snapshot_positions_interval_secs` periodically publishes all open positions. A cache with a
Redis or Postgres backing also persists these snapshots. Without cache backing, both paths publish
snapshots on the in-process message bus without persisting them. See
[`LiveExecutionEngineConfig`](/docs/python-api-latest/live.html#nautilus_trader.live.LiveExecutionEngineConfig) for
these settings.
:::

### Example scenario

```text
# NETTING OMS Example
# Cycle 1: Open LONG position
BUY 100 units at $50   # Position opens
SELL 100 units at $55  # Position closes, PnL = $500
# Snapshot taken preserving $500 realized PnL

# Cycle 2: Open SHORT position
SELL 50 units at $54   # Position reopens (SHORT)
BUY 50 units at $52    # Position closes, PnL = $100
# Snapshot taken preserving $100 realized PnL

# Total realized PnL = $500 + $100 = $600 (from snapshots)
```

## PnL calculations

Position PnL calculations account for instrument specifications and market conventions.

### Realized PnL

The price component of realized PnL is calculated when fills partially or fully close a position.
Commissions in the position's cost currency affect realized PnL as each fill arrives.

```python
# For standard instruments
# LONG: realized_pnl = (exit_price - entry_price) * closed_quantity * multiplier
# SHORT: realized_pnl = (entry_price - exit_price) * closed_quantity * multiplier

# For inverse instruments (side-aware)
# LONG: realized_pnl = closed_quantity * multiplier * (1/entry_price - 1/exit_price)
# SHORT: realized_pnl = closed_quantity * multiplier * (1/exit_price - 1/entry_price)
```

The position side selects the formula.

### Unrealized PnL

`unrealized_pnl()` calculates PnL for an open position from the supplied `price`. You can pass a
bid, ask, mid, last, or mark price:

```python
position.unrealized_pnl(last_price)  # Using last traded price
position.unrealized_pnl(bid_price)  # Conservative for LONG positions
position.unrealized_pnl(ask_price)  # Conservative for SHORT positions
```

For a `FLAT` position, it returns `Money(0, cost_currency)` regardless of the supplied price.

### Total PnL

`total_pnl()` combines the realized and unrealized components:

```python
total_pnl = position.total_pnl(current_price)
# Returns realized_pnl + unrealized_pnl
```

### Currency considerations

- PnL is calculated in the instrument's cost currency: quote for linear contracts, base for
  inverse contracts, and settlement for quanto contracts.
- For Forex, the cost currency is typically the quote currency.
- Portfolio aggregates realized PnL per instrument in cost currency.
- Multi-currency totals require conversion outside the Position class.

## Commissions and costs

Positions track fill commissions:

- Commissions are accumulated by currency.
- Each fill's commission is added to the running total.
- Multiple commission currencies are supported.
- Realized PnL includes commissions only when denominated in the position's cost currency.
- Other commissions are tracked separately and may require conversion.

```python
commissions = position.commissions()
# Returns list[Money] with aggregated commission totals per currency

notional = position.notional_value(current_price)
# Returns Money in quote (linear), base (inverse), or settlement currency (quanto)
```

In Python, `notional_value()` raises `ValueError` if an inverse position lacks a base currency, the
supplied inverse price is not positive, or the result cannot be represented as `Money`.
Rust callers can use `try_notional_value()` to handle these calculation errors; `notional_value()`
panics if the calculation fails.

## Position properties and state

### Identifiers

- `id`: Unique position identifier.
- `instrument_id`: The traded instrument.
- `account_id`: Account where position is held.
- `trader_id`: The trader who owns the position.
- `strategy_id`: The strategy managing the position.
- `opening_order_id`: Client order ID that opened the position.
- `closing_order_id`: Client order ID that closed the position, if closed.

### Position state

- `side`: Current position side (`LONG`, `SHORT`, or `FLAT`).
- `entry`: Opening side for the current cycle (`Buy` for `LONG`, `Sell` for `SHORT`). Updates when
  the position reverses direction.
- `quantity`: Current absolute position size.
- `signed_qty`: Signed position size (positive for `LONG`, negative for `SHORT`).
- `peak_qty`: Maximum quantity reached during the current open-close cycle.
- `is_open`: Whether position is currently open.
- `is_closed`: Whether position is closed (`FLAT`).
- `is_long`: Whether position side is `LONG`.
- `is_short`: Whether position side is `SHORT`.

### Pricing and valuation

- `avg_px_open`: Average entry price.
- `avg_px_close`: Average exit price when closing.
- `realized_pnl`: Realized profit/loss.
- `realized_return`: Realized return as decimal (e.g., 0.05 for 5%).
- `quote_currency`: Quote currency of the instrument.
- `base_currency`: Base currency if applicable.
- `settlement_currency`: Currency for PnL settlement.

### Instrument specifications

- `multiplier`: Contract multiplier.
- `price_precision`: Decimal precision for prices.
- `size_precision`: Decimal precision for quantities.
- `is_inverse`: Whether instrument is inverse.

### Timestamps

- `ts_init`: When position was initialized.
- `ts_opened`: When position was opened.
- `ts_last`: Last update timestamp.
- `ts_closed`: When the position was closed, if closed.
- `duration_ns`: Duration from open to close in nanoseconds, or zero while open.

### Associated data

- `symbol`: The instrument's ticker symbol.
- `venue`: The trading venue.
- `client_order_ids`: Unique client order IDs for retained fills in the current cycle.
- `venue_order_ids`: Unique venue order IDs for retained fills in the current cycle.
- `trade_ids`: Unique trade IDs for retained fills in the current cycle.
- `events`: Retained order fill events in the current cycle.
- `adjustments`: Retained position adjustments in the current cycle.
- `event_count`: Number of retained fill events in the current cycle.
- `last_event`: Most recently retained fill event.
- `last_trade_id`: Trade ID of the most recently retained fill.

:::info
For complete type information and detailed property documentation, see the Position
[API Reference](/docs/python-api-latest/model/position.html#nautilus_trader.model.Position).
:::

## Events and tracking

Each `Position` object records the fills and adjustments for its current open-close cycle:

- Fill events remain in application order.
- Client order, venue order, and trade ID accessors return sorted, unique values.
- `event_count` reports the number of retained fill events.
- Closed `NETTING` cycles retain their event history in the cache snapshots described above.

This data supports:

- Detailed position analysis.
- Trade reconciliation.
- Performance attribution.
- Audit trails.

:::tip
Use `position.events()` to access the current cycle's retained fills for reconciliation.
The `position.trade_ids()` result helps match against broker statements.
See the [Execution guide](execution.md) for reconciliation best practices.
:::

## Numerical precision

`Position` uses `f64` for signed quantity, average prices, realized returns, and PnL intermediates.
`Price`, `Quantity`, and `Money` retain their fixed-point representations at the API boundary, but
conversions between these types and `f64` can introduce rounding. `f64` represents every integer
exactly only through `2^53`; above that boundary, conversion can lose low-order bits. It provides
roughly 15 to 16 significant decimal digits rather than a fixed number of exact decimal places.

The design avoids the higher computational cost of arbitrary-precision arithmetic. The
average-price calculation also avoids multiplying raw fixed-point values because those products can
overflow their integer representation. Average prices reuse the prior `f64` average, and realized
PnL is converted between `Money` and `f64` as fills accumulate. The resulting precision depends on
the values, settlement-currency precision, and sequence of fills.

`quantity` is derived from `signed_qty` at the instrument's `size_precision`. If that conversion
rounds a residual quantity to zero, the position becomes `FLAT` and normalizes `signed_qty` to zero.
Inverse PnL calculations reject nonpositive open or close prices and positive prices below `1e-15`.
With the `defi` feature, converting a `Price` or `Quantity` with more than 16 decimal places to
`f64` panics, so `Position` does not support 17- or 18-decimal fill values.

Tests in `crates/model/src/position.rs` cover a `0.01` USD commission, nine-decimal price inputs, 100
sequential fills, prices from `0.00001` to `99999.99999`, and same-price round trips. These cases do
not establish a universal precision bound.

:::warning
If a workflow requires exact decimal arithmetic for regulatory reporting or audit records, perform
and retain a separate decimal calculation from the original fills and adjustments. Converting
`Position` float outputs back to decimal, including through `signed_decimal_qty()`, cannot restore
discarded precision. `Position` does not provide an exact-decimal guarantee. Validate the
instruments, currencies, amount ranges, and fill sequences used by the application.
:::

## Integration with other components

Positions interact with several key components:

- **Portfolio**: Aggregates position exposure and PnL across instruments and strategies.
- **ExecutionEngine**: Creates and updates positions from fills.
- **Cache**: Stores current position state and closed-cycle snapshots.
- **RiskEngine**: Reads open positions when it checks whether an order reduces exposure.

:::note
Positions are not created for spread instruments. Contingent orders can still trigger for spreads,
but they operate without position linkage. The engine handles spread instruments separately from
regular positions.
:::

## Related guides

- [Events](events/): How fills produce position events.
- [Orders](orders/): Orders that create and modify positions.
- [Execution](execution.md): Fill handling that updates positions.
- [Portfolio](portfolio.md): Portfolio-level position aggregation.
