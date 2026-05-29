# Emulated orders

## Introduction

Emulation lets you use order types even when your trading venue does not natively support them.

Nautilus locally mimics the behavior of these order types (such as `STOP_LIMIT` or `TRAILING_STOP` orders)
while using only `MARKET` and `LIMIT` orders for actual execution on the venue.

When you create an emulated order, Nautilus continuously tracks a specific type of market price (specified by the
`emulation_trigger` parameter) and based on the order type and conditions you've set, will automatically submit
the appropriate fundamental order (`MARKET` / `LIMIT`) when the triggering condition is met.

For example, if you create an emulated `STOP_LIMIT` order, Nautilus will monitor the market price until your `stop`
price is reached, and then automatically submits a `LIMIT` order to the venue.

To perform emulation, Nautilus needs to know which **type of market price** it should monitor.
By default, it uses bid and ask prices (quotes), which is why you'll often see `emulation_trigger=TriggerType.DEFAULT`
in examples (this is equivalent to using `TriggerType.BID_ASK`). However, Nautilus supports various other price types,
that can guide the emulation process.

## Submitting an order for emulation

The only requirement to emulate an order is to pass a `TriggerType` to the `emulation_trigger`
parameter of an `Order` constructor, or `OrderFactory` creation method. The following
emulation trigger types are currently supported:

- `NO_TRIGGER`: disables local emulation completely and order is fully submitted to the venue.
- `DEFAULT`: which is the same as `BID_ASK`.
- `BID_ASK`: emulated using quotes to trigger.
- `LAST_PRICE`: emulated using trades to trigger.

The choice of trigger type determines how the order emulation will behave:

- For `STOP` orders, the trigger price will be compared against the specified trigger type.
- For `TRAILING_STOP` orders, the trailing offset will be updated based on the specified trigger type.
- For `LIMIT` orders being emulated, the limit price will be compared against the specified trigger type to determine when to release the order as a `MARKET` order.

Here are all the available values you can set into `emulation_trigger` parameter and their purposes:

| Trigger Type      | Description                                                                                          | Common use cases                                                                                             |
|:------------------|:-----------------------------------------------------------------------------------------------------|:-------------------------------------------------------------------------------------------------------------|
| `NO_TRIGGER`      | Disables emulation completely. The order is sent directly to the venue without any local processing. | When you want to use the venue's native order handling, or for simple order types that don't need emulation. |
| `DEFAULT`         | Same as `BID_ASK`. This is the standard choice for most emulated orders.                             | General‑purpose emulation when you want to work with the "default" type of market prices.                    |
| `BID_ASK`         | Uses the best bid and ask prices (quotes) to guide emulation.                                        | Stop orders, trailing stops, and other orders that should react to the current market spread.                |
| `LAST_PRICE`      | Uses the price of the most recent trade to guide emulation.                                          | Orders that should trigger based on actual executed trades rather than quotes.                               |
| `DOUBLE_LAST`     | Uses two consecutive last trade prices to confirm the trigger condition.                             | When you want additional confirmation of price movement before triggering.                                   |
| `DOUBLE_BID_ASK`  | Uses two consecutive bid/ask price updates to confirm the trigger condition.                         | When you want extra confirmation of quote movements before triggering.                                       |
| `LAST_OR_BID_ASK` | Triggers on either last trade price or bid/ask prices.                                               | When you want to be more responsive to any type of price movement.                                           |
| `MID_POINT`       | Uses the middle point between the best bid and ask prices.                                           | Orders that should trigger based on the theoretical fair price.                                              |
| `MARK_PRICE`      | Uses the mark price (common in derivatives markets) for triggering.                                  | Particularly useful for futures and perpetual contracts.                                                     |
| `INDEX_PRICE`     | Uses an underlying index price for triggering.                                                       | When trading derivatives that track an index.                                                                |

## Technical details

The platform makes it possible to emulate most order types locally, regardless
of whether the type is supported on a trading venue. The logic and code paths for
order emulation are exactly the same for all [environment contexts](../architecture.md#environment-contexts)
and use a common `OrderEmulator` component.

:::note
There is no limitation on the number of emulated orders you can have per running instance.
:::

## Lifecycle

An emulated order will progress through the following stages:

1. Submitted by a `Strategy` through the `submit_order` method.
2. Sent to the `RiskEngine` for pre-trade risk checks (it may be denied at this point).
3. Sent to the `OrderEmulator` where it is *held* / emulated.
4. Once triggered, emulated order is transformed into a `MARKET` or `LIMIT` order and released (submitted to the venue).
5. Released order undergoes final risk checks before venue submission.

:::note
Emulated orders are subject to the same risk controls as *regular* orders, and can be
modified and canceled by a trading strategy in the normal way. They will also be included
when canceling all orders.
:::

:::info
An emulated order will retain its original client order ID throughout its entire life cycle, making it easy to query
through the cache.
:::

### Held emulated orders

The following will occur for an emulated order now *held* by the `OrderEmulator` component:

- The original `SubmitOrder` command will be cached.
- The emulated order will be processed inside a local `MatchingCore` component.
- The `OrderEmulator` will subscribe to any needed market data (if not already) to update the matching core.
- The emulated order can be modified (by the trader) and updated (by the market) until *released* or canceled.

### Released emulated orders

Once data arrival triggers / matches an emulated order locally, the following
*release* actions will occur:

- The order will be transformed to either a `MARKET` or `LIMIT` order (see below table) through an additional `OrderInitialized` event.
- The orders `emulation_trigger` will be set to `NONE` (it will no longer be treated as an emulated order by any component).
- The order attached to the original `SubmitOrder` command will be sent back to the `RiskEngine` for additional checks since any modification/updates.
- If not denied, then the command will continue to the `ExecutionEngine` and on to the trading venue via an `ExecutionClient` as normal.

## Order types which can be emulated

The following table lists which order types are possible to emulate, and
which order type they transform to when being released for submission to the
trading venue.

| Order type for emulation | Can emulate | Released type |
|:-------------------------|:------------|:--------------|
| `MARKET`                 |             | n/a           |
| `MARKET_TO_LIMIT`        |             | n/a           |
| `LIMIT`                  | ✓           | `MARKET`      |
| `STOP_MARKET`            | ✓           | `MARKET`      |
| `STOP_LIMIT`             | ✓           | `LIMIT`       |
| `MARKET_IF_TOUCHED`      | ✓           | `MARKET`      |
| `LIMIT_IF_TOUCHED`       | ✓           | `LIMIT`       |
| `TRAILING_STOP_MARKET`   | ✓           | `MARKET`      |
| `TRAILING_STOP_LIMIT`    | ✓           | `LIMIT`       |

## Querying

When writing trading strategies, it may be necessary to know the state of emulated orders in the system.
There are several ways to query emulation status:

### Through the Cache

The following `Cache` methods are available:

- `self.cache.orders_emulated(...)`: Returns all currently emulated orders.
- `self.cache.is_order_emulated(...)`: Checks if a specific order is emulated.
- `self.cache.orders_emulated_count(...)`: Returns the count of emulated orders.

See the full [API reference](/docs/python-api-latest/cache.html) for additional details.

### Direct order queries

You can query order objects directly using:

- `order.is_emulated`

If either of these return `False`, then the order has been *released* from the
`OrderEmulator`, and so is no longer considered an emulated order (or was never an emulated order).

:::warning
Do not hold a local reference to an emulated order. The order object transforms
when the emulated order is *released*. Use the `Cache` instead.
:::

## Persistence and recovery

If a running system either crashes or shuts down with active emulated orders, then
they will be reloaded inside the `OrderEmulator` from any configured cache database.
This preserves order state across system restarts and recoveries.

## Best practices

When working with emulated orders, consider the following best practices:

1. Always use the `Cache` for querying or tracking emulated orders rather than storing local references
2. Be aware that emulated orders transform to different types when released
3. Remember that emulated orders undergo risk checks both at submission and release

:::note
Order emulation allows you to use advanced order types even on venues that don't natively support them,
making your trading strategies more portable across different venues.
:::

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Advanced orders](advanced.md) - Order lists, contingency types, and bracket orders.
- [Strategies](../strategies.md) - Order management from strategies.
