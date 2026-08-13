# Advanced orders

Order lists group related orders, while contingency metadata describes how fills, cancellations, or
updates should affect linked orders. The component that handles the list determines the behavior:
the backtest matching engine, local order emulator, live adapter and venue, or strategy code.

:::warning
An `OrderList` or `ContingencyType` does not guarantee that every live adapter or venue implements
the relationship. Check the target integration before relying on native contingency behavior.
:::

## Order lists

An order list groups contingent orders or a larger batch under one `order_list_id`. Orders in the
list do not need a contingency relationship; their own metadata defines any relationship.

Production constructors require every order in a list to use the same venue. Orders may target
different instruments at that venue, such as pairs, calendar spreads, or multi‑leg strategies. The
list takes its representative `instrument_id` from the first order; consumers that need the actual
instrument must resolve each order individually.

Caveats for mixed-instrument lists:

- Pre‑trade checks for price precision, quantity precision, and GTD expiry use each order's own
  instrument.
- The cumulative risk check for free balance, notional bounds, position‑reducing exposure, and
  market data uses the list's representative instrument. For a mixed list, this produces a
  single‑instrument bound rather than per‑instrument accuracy.
- Cache lookups like `cache.order_lists(instrument_id=...)` filter against the representative
  `instrument_id`; lists containing other instruments will not match queries for those other
  instruments.
- The execution engine denies mixed-instrument lists when a `position_id` is supplied
  (a position belongs to a single instrument, regardless of OMS).
- Adapter `submit_order_list` implementations vary. Some iterate orders per leg and resolve
  each order's own `instrument_id` against the venue API; others still build the batch
  request around the list's representative `instrument_id` and will misroute non-first
  orders. Treat mixed-instrument lists as adapter-specific; verify the target adapter's
  behavior before relying on it. Backtesting and strategy‑managed routing avoid relying on an
  adapter's mixed‑instrument batch behavior.

## Contingency types

- **OTO (One‑Triggers‑Other):** A parent order releases one or more child orders after a configured
  fill condition.
- **OCO (One‑Cancels‑Other):** A fill in one linked order requests cancellation of the others.
- **OUO (One‑Updates‑Other):** A fill in one linked order requests a quantity update for the others.

:::info
These types correspond to FIX
[`ContingencyType <1385>`](https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_1385.html).
:::

### One-Triggers-Other (OTO)

An OTO relationship has two parts:

1. The parent order enters its execution path.
1. One or more child orders reference the parent and wait for the configured release condition.

The handler determines where the children wait. The backtest engine can hold them locally, while a
live adapter may send native venue instructions, submit all legs, reject the list, or require the
strategy to manage the relationship.

#### Trigger models

| Trigger model | Backtest release condition                                                |
| ------------- | ------------------------------------------------------------------------- |
| **Partial**   | Release children after the parent's first partial fill.                   |
| **Full**      | Release children after the parent's cumulative fill reaches its quantity. |

:::info
The default `BacktestVenueConfig` mode is `OtoTriggerMode.PARTIAL`. Set `oto_trigger_mode` to
`OtoTriggerMode.FULL` to wait for a complete fill. This setting controls release timing; it does not
promise pro rata child sizing. Verify child quantities when the parent fills partially.
:::

#### Enforcing a full-fill trigger in strategy code

If the execution context does not provide the required full‑fill behavior:

1. Submit the parent order without contingent children.
1. Handle `OrderFilled` events for the parent.
1. Confirm the parent has reached `FILLED` status.
1. Submit the stop‑loss, take‑profit, or other child orders.

:::warning
Full‑fill release leaves a partially filled position without its contingent exits until the parent
finishes. Partial release reduces that delay, but the current backtest mode does not guarantee that
child quantities track each partial fill. Check quantities and adapter behavior before treating a
child as complete protection.
:::

### One-Cancels-Other (OCO)

In backtest local matching, a full or partial fill in one OCO order causes a best‑effort request to
cancel its open siblings. The local order manager applies this behavior only while a sibling remains
active local. After release, the adapter or venue determines cancellation behavior. Another sibling
can fill before cancellation completes.

### One-Updates-Other (OUO)

In backtest local matching, a fill in one OUO order uses that order's remaining quantity as the
target for each open sibling. The engine cancels a sibling when the target is zero or its filled
quantity already meets the target; otherwise, it updates the sibling when needed. This behavior
suits equal‑sized peers and does not preserve a ratio between unequal starting quantities. Live
behavior depends on adapter and venue support.

## Constructing contingent orders

Use `OrderFactory.bracket` to construct a bracket's contingency metadata. In Rust,
`self.order().create_list(...)` assigns a fresh `order_list_id` to an existing group of orders.
Python code instead passes a plain list to `self.submit_order_list(...)`, which creates an
`OrderList` when needed. These grouping paths do not create parent or linked‑order relationships.
The current model enforces only part of the remaining consistency:

- A contingent order must have at least one `linked_order_id`.
- A child identifies its parent through `parent_order_id`.
- Rust `create_list` requires a non‑empty list whose orders use one venue.
- `OrderList.validate` checks for non‑empty, unique client order IDs when a strategy submits the
  list.
- `OrderList.validate` does not verify shared `order_list_id` values, parent references, or other
  cross‑field relationships.

Modification, cancellation, and rejection behavior depends on the component managing the
contingency. Do not assume a parent update or cancellation cascades in every live integration.

:::warning
Handle `OrderDenied` and `OrderRejected` events for every leg. Adapter or venue failures can affect
legs independently and leave a position without its intended protection.
:::

## Bracket orders

Bracket orders combine an entry with take‑profit and stop‑loss children. By default,
`OrderFactory.bracket` creates a `MARKET` entry, a `LIMIT` take‑profit, and a `STOP_MARKET`
stop‑loss. It marks the entry with an `OTO` contingency, marks both exits `reduce_only`, and links
the exits with an `OUO` contingency. The default `LIMIT` take‑profit is also `post_only`.

The factory creates the orders and their relationship metadata. The execution context determines
whether children wait locally, use a native venue instruction, enter the venue with the parent, or
require manual strategy handling.

Create brackets with
[`OrderFactory`](/docs/python-api-latest/common.html#nautilus_trader.common.OrderFactory), which
also supports different entry and exit types, trigger settings, and execution instructions.

In the following example we bracket a *Market* entry to BUY 10 ETHUSDT-PERP contracts with a
take-profit *Limit* at 3,300 USDT and a stop-loss *Stop-Market* triggering at 2,800 USDT. The entry
defaults to `MARKET`, the take-profit to `LIMIT`, and the stop-loss to `STOP_MARKET`; the take-profit
and stop-loss legs are `reduce_only` and linked with the `OUO` contingency:

```rust tab="Rust"
use nautilus_model::{
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

// `bracket()` returns a `bon` builder; finalize with `.call()`.
// The result is a `Vec<OrderAny>` ordered as [entry, stop-loss, take-profit].
let orders = self
    .order()
    .bracket()
    .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
    .order_side(OrderSide::Buy)
    .quantity(Quantity::from(10))
    .tp_price(Price::from("3300.00"))         // take-profit LIMIT (default)
    .sl_trigger_price(Price::from("2800.00")) // stop-loss STOP_MARKET (default)
    .call();
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity

orders = self.order_factory.bracket(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(10),
    tp_price=Price.from_str("3300.00"),  # <-- take-profit LIMIT (default)
    sl_trigger_price=Price.from_str("2800.00"),  # <-- stop-loss STOP_MARKET (default)
)
```

:::warning
Some venues reserve margin for bracket legs. Check the venue's margin rules and handle a child
rejection after the entry fills.
:::

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Emulated orders](emulated.md) - Emulating order types on venues without native support.
- [Execution](../execution.md) - Order execution and fill handling.
