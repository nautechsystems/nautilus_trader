# Positions

This guide explains how NautilusTrader creates and updates positions from order fills and calculates profit
and loss (PnL). It also explains how NautilusTrader preserves closed cycles under `NETTING` order management
system (OMS) configurations.

## Overview

A position records exposure to an instrument during an open-close cycle. It aggregates the fills
assigned to one position ID and tracks its quantity, average prices, realized PnL, commissions, and
related identifiers. Use a market price with the position's methods to calculate unrealized PnL and
notional value.

The execution engine creates positions when orders fill and tracks them from open to close. OMS configuration
determines whether fills share a net position or stay in separate hedged positions.

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

A position closes when the **net quantity becomes zero** (`FLAT`). At closure:

- The closing order ID is recorded. A settlement at contract expiration leaves it empty.
- Duration is calculated from open to close.
- Final realized PnL is computed.
- In either OMS type, when the position later reopens under the same ID, the engine snapshots
  the closed state to preserve historical PnL (see [Position snapshotting](#position-snapshotting)).

### Settlement at contract expiration

In a live node, an `InstrumentClose` of type `CONTRACT_EXPIRED` settles binary option positions only on venues
served by the node's execution clients. Under that condition, it settles each open position in that instrument
at the close price (such as `1` for a winning outcome and `0` for a losing one). For each position, the
execution engine:

- Closes the current cycle at the close price without creating an order or fill.
- Books realized PnL for the settled quantity. Fill quantities and commissions stay as filled, so opening fees
  stay in realized PnL.
- Emits one `PositionClosed` with no closing order ID.

The first close applied to an instrument is authoritative. A repeated or conflicting close, or a restart that
restores the settled position, does not settle it again. After settlement, a fill or fill void for the
instrument still updates its order but no longer changes positions. The engine logs a warning and emits no
position event.

Settlement does not redeem venue assets or change account balances. Venue account reports stay
authoritative for cash, so an unredeemed payout counts toward account balances only once the venue
reports the redeemed funds.

Backtests and sandbox paper trading keep the simulated venue's expiration handling instead. The matching
engine closes positions with expiration fills, and those fills credit the simulated account. The live node
does not apply engine settlement on a venue whose execution client settles expiring contracts itself, as the
sandbox client does.

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

### Reversal accounting

An opposite-side fill larger than the open quantity closes the existing exposure and opens the
residual in the other direction. The execution engine splits this fill into a close and a new
opening, with closed-state retention governed by [Position snapshotting](#position-snapshotting).

An unsplit reversal fill applied directly to one `Position` starts a new accounting episode for the residual
exposure. The episode has these resets, including during fill-void replay:

- `avg_px_open` becomes the reversal fill price.
- `avg_px_close` becomes `None`, and `realized_return` becomes zero until a subsequent closing fill.
- `buy_qty` and `sell_qty` restart with only the opening residual on the new entry side and zero on the other
  side. Later close averages thus exclude volume from the previous direction.

**Only the closing portion** realizes PnL against the previous entry price. The object's `realized_pnl` and
commission totals stay cumulative across this reversal, with the fill's commission counted once. The reset
does not clear fill history, opening timestamps, or peak quantity. If the position instead reaches `FLAT` and
a later fill reopens it, the full cycle resets, including realized PnL and commissions.

These episode resets apply to fill-driven reversals. A quantity adjustment that changes the position's side
does not perform the same reset.

## Position adjustments

Position adjustments record quantity or PnL changes that occur outside normal order fills. The
system represents these changes as `PositionAdjusted` events.

### Base-currency commissions

When trading spot currency pairs (for example, BTC/USDT) or FX spot, commissions paid in the base
currency directly affect the net quantity received or delivered:

- **Opening fills**: Commission is deducted from the traded quantity. A buy of 1.0 BTC with
  0.001 BTC commission results in a net long position of 0.999 BTC.
- **Closing fills**: Commission is applied to `signed_qty` because it affects actual inventory. A sale of a
  0.999 BTC LONG position with 0.000999 BTC commission leaves you SHORT 0.000999 BTC, not FLAT. You gave up
  0.999999 BTC total.
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
- After `purge_events_for_order()`, a position with fills regenerates commission adjustments from the
  surviving fills and reapplies non-commission adjustments. With no fills, the position becomes an empty
  `FLAT` shell and clears its adjustment history.

## OMS types and position management

NautilusTrader supports two position management modes. A strategy configured with
`OmsType.UNSPECIFIED` uses the owning execution client's OMS type. For ownership resolution and position ID rules,
see the [Execution guide](execution/index.md#order-management-system-oms).

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
  position ID, the engine archives the closed cycle before replacing the cached state.
- A virtual position flip creates a new ID and keeps the original closed position in the cache. Thus, that
  path does not need a closed-cycle snapshot.

After a virtual flip, later fills for the same order follow the destination position even if they carry the
original position ID. The engine derives the destination from retained order and position history, including
after cache recovery and fill-void corrections. It rejects fills when that history is incomplete or ambiguous.

Virtual `HEDGING` positions always keep replay history across reopens, even when
`carry_replay_events_on_reopen=False`, to preserve this routing when a position ID is reused.

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

Position snapshotting preserves closed cycles for PnL tracking and reporting when a later fill
reopens a closed position.

### Why snapshotting matters

When a position closes (becomes `FLAT`) and then reopens under the same ID with a new trade, its object
resets. This reset tracks the new exposure. Without snapshotting, the historical realized PnL from the
previous position cycle would be lost.

### How it works

When a fill reopens a closed position under the same ID, the execution engine archives the closed
state before opening the next cycle. This applies to both `NETTING` and `HEDGING` OMS.
A `HEDGING` flip using a non-virtual ID follows a separate path: it reuses the ID without
archiving the closed cycle.

The snapshot preserves:

- Final quantities and prices.
- Realized PnL.
- All fill events.
- Commission totals.

The cache stores snapshots by position ID. The active cache entry then represents the new cycle, while
previous snapshots stay accessible. The Portfolio includes their realized PnL in instrument totals.

A fill void that corrects a fill from an earlier cycle is the one exception. The correction moves the cycle
boundaries the stored snapshots describe. Thus, the engine replaces them with the cycles the corrected history
actually closes, keeping each counted once. See
[Position replay across NETTING cycles](execution/index.md#position-replay-across-netting-cycles).

:::note
This closed-cycle archive differs from optional position state snapshots:

- `snapshot_positions=True` publishes state when a position opens, changes, or closes.
- `snapshot_positions_interval_secs` periodically publishes all open positions.

A cache with a Redis or Postgres backing also persists these snapshots. Without cache backing, both paths
publish snapshots on the in-process message bus without persisting them. See
[`LiveExecutionEngineConfig`](/docs/python-api-latest/live.html#nautilus_trader.live.LiveExecutionEngineConfig)
for these settings.
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

The price component of realized PnL is calculated when fills partially or fully close a position,
or when a [settlement at contract expiration](#settlement-at-contract-expiration) closes it.
Commissions in the position's cost currency affect realized PnL as each fill arrives.

```python
# For standard instruments
# LONG: realized_pnl = (exit_price - entry_price) * closed_quantity * multiplier
# SHORT: realized_pnl = (entry_price - exit_price) * closed_quantity * multiplier

# For inverse instruments (side-aware)
# LONG: realized_pnl = closed_quantity * multiplier * (1/entry_price - 1/exit_price)
# SHORT: realized_pnl = closed_quantity * multiplier * (1/exit_price - 1/entry_price)
```

The position side selects the formula. Premium-based inverse instruments (such as coin-settled options) quote
the premium in the base currency. Thus, they use the standard formula and report PnL and notional value in the
base currency.

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

:::warning
In Python, `notional_value()` raises `ValueError` under these conditions:

- An inverse position lacks a base currency.
- The supplied price is not positive for a non-premium inverse position.
- The result cannot be represented as `Money`.

Rust callers can use `try_notional_value()` to handle these calculation errors. `notional_value()` panics if
the calculation fails.
:::

## Position properties and state

### Identifiers

- `id`: Unique position identifier.
- `instrument_id`: The traded instrument.
- `account_id`: Account where position is held.
- `trader_id`: The trader who owns the position.
- `strategy_id`: The strategy managing the position.
- `opening_order_id`: Client order ID that opened the position.
- `closing_order_id`: Client order ID that closed the position, if a fill closed it.

### Position state

- `side`: Current position side (`LONG`, `SHORT`, or `FLAT`).
- `entry`: Opening side for the current cycle (`BUY` for `LONG`, `SELL` for `SHORT`). Updates when
  the position reverses direction.
- `quantity`: Current absolute position size.
- `signed_qty`: Decimal signed position size (positive for `LONG`, negative for `SHORT`).
- `peak_qty`: Maximum quantity reached during the current open-close cycle.
- `is_open`: Whether position is open.
- `is_closed`: Whether position is closed (`FLAT`).
- `is_long`: Whether position side is `LONG`.
- `is_short`: Whether position side is `SHORT`.

### Pricing and valuation

- `avg_px_open`: Decimal average entry price.
- `avg_px_close`: Decimal average exit price when closing.
- `realized_pnl`: Realized profit/loss.
- `realized_return`: Floating-point return ratio (for example, 0.05 for 5%).
- `quote_currency`: Quote currency of the instrument.
- `base_currency`: Base currency if applicable.
- `settlement_currency`: Currency for PnL settlement.

See [Reversal accounting](#reversal-accounting) for how price averages and returns reset while realized PnL
stays cumulative when an unsplit fill reverses one position.

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

- Fill events stay in application order.
- Client order, venue order, and trade ID accessors return sorted, unique values.
- `event_count` reports the number of retained fill events.
- Closed `NETTING` cycles keep their event history in the cache snapshots described above.

This data supports:

- Detailed position analysis.
- Trade reconciliation.
- Performance attribution.
- Audit trails.

:::tip
Use `position.events()` to access the current cycle's retained fills for reconciliation.
The `position.trade_ids()` result helps match against broker statements.
See the [Execution guide](execution/) for reconciliation best practices.
:::

## Numerical precision

`Position` uses checked domain and raw arithmetic for exact quantity and fee operations, and `Decimal`
for signed quantity, average prices, cost basis, and PnL residuals.
`Price`, `Quantity`, and `Money` retain their fixed-point representations. Python `signed_qty`, `avg_px_open`,
and `avg_px_close` properties return `decimal.Decimal` values when present. `realized_return` remains a
floating-point analytics projection and does not feed accounting.

The arithmetic has a finite bound: a 96-bit coefficient and a maximum decimal scale of 28. Input conversions
must fit exactly. Derived products, cost sums, division, and proportional intermediates round within that
bound. An operation fails if an input loses precision, an intermediate overflows, or a nonzero intermediate
disappears. At Decimal's precision limit, PnL sums may absorb a disappearing residual smaller than one native
settlement `Money` unit. A disappearing operand of at least one native unit still fails; representable residuals
remain in the position.
The fallible fill, adjustment, settlement, purge, and fill-void operations leave the position unchanged on error.

### Cost basis and rounding

Opening and closing cost accumulators retain the amounts behind average prices. A rounded average never
feeds the cost basis. A partial reduction retains unallocated opening cost, and the final reduction consumes
that remainder. A separate Decimal PnL remainder retains fractions below the native Money scale between
updates.

Derived Money amounts round midpoint to even at the native scale: nine decimal places in standard mode and
16 in high-precision mode. A currency with a finer supported precision uses that precision instead. Currency
display precision does not truncate the stored amount. Reversal fee allocation works in native Money units:
the closing share truncates toward zero, and the opening share receives the residual, preserving the fee total.

| Operation                                                      | Accounting result                                         |
| -------------------------------------------------------------- | --------------------------------------------------------- |
| Open one unit with a `0.000001 USD` fee                        | Realized PnL is `-0.000001 USD`.                          |
| Buy `0.01` at `0.1234`, then sell it at `0.1235`, without fees | Realized PnL is `0.000001 USD`.                           |
| Close a `0.9` long with a sell of `1` and a `0.006 USD` fee    | Closing fee is `0.0054 USD`; opening fee is `0.0006 USD`. |

Supported quantity precision can exceed the instrument's display precision, for example after a base-currency
fee. The position retains that quantity rather than rounding it to flat. Strategy close orders round such
quantities down to multiples of the instrument's size increment. Dust remains open; no close order is submitted
when the entire quantity rounds to zero. Non-premium inverse PnL requires
positive prices. With the `defi` feature, 17- and 18-decimal inputs are supported when the input and resulting
accounting fit the Decimal and domain bounds.

### Persistence and replay

Serialized positions include opening cost, closing cost, closed quantity, and the PnL remainder. Preserve
these fields when restoring a position that will process more fills. `Position.to_dict()` includes this state,
and `Position.from_dict()` validates its accounting inputs. Position lifecycle and snapshot financial values
use decimal text in Arrow; SQL stores signed quantity and average prices as `NUMERIC`.

Position payloads and EventStore lifecycle accounting from older builds do not satisfy this contract.
Regenerate those stores from the original inputs before replaying with this version. The EventStore
envelope format marker does not version individual accounting payloads.

EventStore replay derives accounting from fills, adjustments, and settlement. Lifecycle notifications validate
that accounting rather than replacing it with averages that omit cost basis and residuals. This layout has no
reader for the prior floating-point accounting format.

## Integration with other components

Positions interact with several key components:

- **Portfolio**: Aggregates position exposure and PnL across instruments and strategies.
- **ExecutionEngine**: Creates and updates positions from fills.
- **Cache**: Stores current position state and closed-cycle snapshots.
- **RiskEngine**: Reads open positions when it checks whether an order reduces exposure.

:::info
Positions are not created for spread instruments. Contingent orders can still trigger for spreads,
but they operate without position linkage. The engine handles spread instruments separately from
regular positions.
:::

## Related guides

- [Events](events/): How fills produce position events.
- [Orders](orders/): Orders that create and modify positions.
- [Execution](execution/): Fill handling that updates positions.
- [Portfolio](portfolio.md): Portfolio-level position aggregation.
