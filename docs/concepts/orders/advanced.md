# Advanced orders

The following guide should be read in conjunction with the specific documentation from the broker or venue
involving these order types, lists/groups and execution instructions (such as for Interactive Brokers).

## Order lists

Combinations of contingent orders, or larger order bulks can be grouped together into a list with a common
`order_list_id`. The orders contained in this list may or may not have a contingent relationship with
each other, as this is specific to how the orders themselves are constructed, and the
specific venue they are being routed to.

All orders in a list must share the same venue. Orders may target different instruments at that
venue (e.g. pairs, calendar spreads, multi-leg legs); whether the destination venue accepts
mixed-instrument batches is venue-specific. The list's `instrument_id` is taken from the first
order as a representative value; downstream consumers that need a per-order instrument resolve
each order individually.

Caveats for mixed-instrument lists:

- Pre-trade per-order checks (price/quantity precision, GTD) use each order's own instrument.
- The cumulative risk check (free balance, min/max notional, position-reducing exposure,
  per-order market-data lookups) uses the list's representative instrument. For mixed lists
  this is a single-instrument bound, not per-instrument accuracy.
- Cache lookups like `cache.order_lists(instrument_id=...)` filter against the representative
  `instrument_id`; lists containing other instruments will not match queries for those other
  instruments.
- The execution engine denies mixed-instrument lists when a `position_id` is supplied
  (a position belongs to a single instrument, regardless of OMS).
- Adapter `submit_order_list` implementations vary. Some iterate orders per leg and resolve
  each order's own `instrument_id` against the venue API; others still build the batch
  request around the list's representative `instrument_id` and will misroute non-first
  orders. Treat mixed-instrument lists as adapter-specific; verify the target adapter's
  behaviour before relying on it. Backtesting and custom strategy code that handle
  multi-leg routing in user space remain the safest path today.

## Contingency types

- **OTO (One-Triggers-Other)** – a parent order that, once executed, automatically places one or more child orders.
  - *Full-trigger model*: child order(s) are released **only after the parent is completely filled**. Common at most retail equity/option brokers (e.g. Schwab, Fidelity, TD Ameritrade) and many spot-crypto venues (Binance, Coinbase).
  - *Partial-trigger model*: child order(s) are released **pro-rata to each partial fill**. Used by professional-grade platforms such as Interactive Brokers, most futures/FX OMSs, and Kraken Pro.

- **OCO (One-Cancels-Other)** – two (or more) linked live orders where executing one cancels the remainder.

- **OUO (One-Updates-Other)** – two (or more) linked live orders where executing one reduces the open quantity of the remainder.

:::info
These contingency types relate to ContingencyType FIX tag <1385> <https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_1385.html>.
:::

### One-Triggers-Other (OTO)

An OTO order involves two parts:

1. **Parent order** – submitted to the matching engine immediately.
2. **Child order(s)** – held *off-book* until the trigger condition is met.

#### Trigger models

| Trigger model       | When are child orders released?                                                                                                                  |
|---------------------|--------------------------------------------------------------------------------------------------------------------------------------------------|
| **Full trigger**    | When the parent order’s cumulative quantity equals its original quantity (i.e., it is *fully* filled).                                           |
| **Partial trigger** | Immediately upon each partial execution of the parent; the child’s quantity matches the executed amount and is increased as further fills occur. |

:::info
The default backtest venue for NautilusTrader uses a *partial-trigger model* for OTO orders.
To opt-in to a *full-trigger mode*, set `oto_trigger_mode="FULL"` for the venue (e.g. via `BacktestVenueConfig`).
:::

**Working with partial-trigger in production:**

If your strategy requires full-trigger semantics but the venue or backtest engine uses partial-trigger:

1. Submit the parent order without contingent children.
2. Subscribe to `OrderFilled` events for the parent order.
3. Only submit child orders (stop-loss, take-profit) after confirming the parent is fully filled.
4. Use `order.is_closed` and `order.filled_qty == order.quantity` to verify complete fill.

> **Why the distinction matters**
> *Full trigger* leaves a risk window: any partially filled position is live without its protective exit until the remaining quantity fills.
> *Partial trigger* mitigates that risk by ensuring every executed lot instantly has its linked stop/limit, at the cost of creating more order traffic and updates.

An OTO order can use any supported asset type on the venue (e.g. stock entry with option hedge, futures entry with OCO bracket, crypto spot entry with TP/SL).

| Venue / Adapter ID                           | Asset classes             | Trigger rule for child                      | Practical notes                                                   |
|----------------------------------------------|---------------------------|---------------------------------------------|-------------------------------------------------------------------|
| Binance / Binance Futures (`BINANCE`)        | Spot, perpetual futures   | **Partial or full** – fires on first fill.  | OTOCO/TP-SL children appear instantly; monitor margin usage.      |
| Bybit Spot (`BYBIT`)                         | Spot                      | **Full** – child placed after completion.   | TP-SL preset activates only once the limit order is fully filled. |
| Bybit Perps (`BYBIT`)                        | Perpetual futures         | **Partial and full** – configurable.        | “Partial‑position” mode sizes TP-SL as fills arrive.              |
| Kraken Futures (`KRAKEN`)                    | Futures & perps           | **Partial and full** – automatic.           | Child quantity matches every partial execution.                   |
| OKX (`OKX`)                                  | Spot, futures, options    | **Full** – attached stop waits for fill.    | Position‑level TP-SL can be added separately.                     |
| Interactive Brokers (`INTERACTIVE_BROKERS`)  | Stocks, options, FX, fut  | **Configurable** – OCA can pro‑rate.        | `OcaType 2/3` reduces remaining child quantities.                 |
| dYdX v4 (`DYDX`)                             | Perpetual futures (DEX)   | On‑chain condition (size exact).            | TP-SL triggers by oracle price; partial fill not applicable.      |
| Polymarket (`POLYMARKET`)                    | Prediction market (DEX)   | N/A.                                        | Advanced contingency handled entirely at the strategy layer.      |
| Betfair (`BETFAIR`)                          | Sports betting            | N/A.                                        | Advanced contingency handled entirely at the strategy layer.      |

### One-Cancels-Other (OCO)

An OCO order is a set of linked orders where the execution of **any** order (full *or partial*) triggers a best-efforts cancellation of the others.
Both orders are live simultaneously; once one starts filling, the venue attempts to cancel the unexecuted portion of the remainder.

### One-Updates-Other (OUO)

An OUO order is a set of linked orders where execution of one order causes an immediate *reduction* of open quantity in the other order(s).
Both orders are live concurrently, and each partial execution proportionally updates the remaining quantity of its peer order on a best-effort basis.

## Contingent order validation

When working with contingent orders (OTO, OCO, OUO), be aware of the following validation rules and error scenarios:

**Order list requirements:**

- All orders in a contingent group must share the same `order_list_id`.
- Parent orders must be submitted before or simultaneously with their children.
- Child orders reference their parent via `parent_order_id`.

**Modification rules:**

- Parent orders can typically be modified while pending, but modifications may cascade to children.
- Child orders can be modified independently on most venues, but check venue-specific behavior.
- Canceling a parent order will cancel all associated child orders.

**Common error scenarios:**

| Scenario | System behavior |
|----------|-----------------|
| Child references non‑existent parent | Order denied with `INVALID_ORDER` error |
| Parent canceled before children trigger | Children automatically canceled |
| OCO sibling filled before cancel propagates | Partial fill honored, remaining quantity canceled |
| Insufficient margin for bracket | Entry may execute, children rejected separately |

:::warning
Always handle `OrderDenied` and `OrderRejected` events in your strategy, especially for contingent orders where
partial failures can leave positions unprotected.
:::

## Bracket orders

Bracket orders are an advanced order type that allows traders to set both take-profit and stop-loss
levels for a position simultaneously. This involves placing a parent order (entry order) and two child
orders: a take-profit `LIMIT` order and a stop-loss `STOP_MARKET` order. When the parent order executes,
the system places the child orders. The take-profit closes the position if the market moves favorably, and the stop-loss limits losses if it moves unfavorably.

Bracket orders can be easily created using the [OrderFactory](/docs/python-api-latest/common.html#nautilus_trader.common.factories.OrderFactory),
which supports various order types, parameters, and instructions.

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
    .core
    .order_factory()
    .bracket()
    .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
    .order_side(OrderSide::Buy)
    .quantity(Quantity::from(10))
    .tp_price(Price::from("3300.00"))         // take-profit LIMIT (default)
    .sl_trigger_price(Price::from("2800.00")) // stop-loss STOP_MARKET (default)
    .call();
```

```python tab="Python"
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.orders import OrderList

bracket: OrderList = self.order_factory.bracket(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(10),
    tp_price=Price.from_str("3300.00"),  # <-- take-profit LIMIT (default)
    sl_trigger_price=Price.from_str("2800.00"),  # <-- stop-loss STOP_MARKET (default)
)
```

:::warning
You should be aware of the margin requirements of positions, as bracketing a position will consume
more order margin.
:::

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Emulated orders](emulated.md) - Emulating order types on venues without native support.
- [Execution](../execution.md) - Order execution and fill handling.
