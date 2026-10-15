# Emulated orders

Emulation lets you use order types even when your trading venue does not natively support them.

The `OrderEmulator` monitors the market data selected by `emulation_trigger`. When the local order matches its
release condition, the emulator transforms it into a `MARKET` or `LIMIT` order. It routes that order to the
configured execution algorithm or the execution engine. For example, an emulated `STOP_LIMIT` becomes a
`LIMIT` order after its stop price triggers.

## Submitting an order for emulation

**Supported triggers**: set `emulation_trigger` on an order constructor or `OrderFactory` method.
The local emulator accepts these values.

| Trigger type | Market data used                                   |
| ------------ | -------------------------------------------------- |
| `DEFAULT`    | Quotes, with the same local behavior as `BID_ASK`. |
| `BID_ASK`    | Best bid and ask quotes.                           |
| `LAST_PRICE` | Trades.                                            |

Leave `emulation_trigger` as `None` to disable local emulation and submit through the normal
pathway.

Other `TriggerType` values describe trigger methods that some venues support, but the local
`OrderEmulator` does not accept them as `emulation_trigger` values.

:::warning
The emulator cancels an order submitted with any other `emulation_trigger` value, and logs the
unsupported trigger type as an error.
:::

**Trigger behavior**: emulation depends on the selected trigger type.

- For stop orders, the emulator compares the trigger price with the selected market data.
- For trailing-stop orders, it updates the trailing trigger from that market data.
- For emulated `LIMIT` orders, it compares the limit price with that market data and releases a
  `MARKET` order when matched.

**Cached data**: the emulator creates a matching core for a trigger instrument. The first order's
`emulation_trigger` selects the cached market data used to seed that core.

- `DEFAULT` and `BID_ASK` use the latest cached quote's bid and ask.
- `LAST_PRICE` uses the latest cached trade price.

An eligible order that matches this data releases on submission **without an `OrderEmulated` event**. If the
selected source has no cached data, the order stays emulated until matching market data arrives.

## Technical details

The same `OrderEmulator` component manages supported emulated order types in all
[environment contexts](../architecture.md#environment-contexts).

:::note
NautilusTrader does not configure a fixed count limit for emulated orders. Available memory and the
cost of market data processing provide practical limits.
:::

## Lifecycle

**Lifecycle**: an emulated order progresses through these stages.

1. A `Strategy` submits it through `submit_order`, which routes it to the `OrderEmulator`.
1. The `OrderEmulator` checks for immediate release. If the order does not match, it holds and
   monitors it locally.
1. For a held order, a matching market update transforms it into a `MARKET` or `LIMIT` order and
   releases it once the required prices are available.
1. The emulator routes the released order to an `ExecutionAlgorithm` when `exec_algorithm_id` is
   set, or to the `ExecutionEngine` otherwise.

:::warning[Emulation does not guarantee pre-trade risk checks]
Strategy submissions go directly to the `OrderEmulator`. Direct releases go to the `ExecutionEngine`
without passing through `RiskEngine` command checks. The risk engine receives the `OrderReleased`
event for logging, but does not validate it. An execution algorithm's `submit_order` method routes
its submissions through the risk engine. Do not rely on emulation alone to enforce pre-trade risk
limits.
:::

:::note
A strategy can modify or cancel emulated orders, and a cancel-all request includes them.
:::

:::info
An emulated order keeps its client order ID when transformed, so cache queries continue to use the same ID.
:::

### Held emulated orders

**While held**: the `OrderEmulator` takes these actions while it holds an order.

- It caches the original `SubmitOrder` command.
- It processes the order in a local matching core.
- It subscribes to the required quotes or trades if no matching subscription exists.
- It accepts strategy modifications and market-driven updates until release or cancellation.

### Released emulated orders

**Release sequence**: when market data matches an emulated order, release performs these actions
in order.

1. It transforms the order into a `MARKET` or `LIMIT` order through another `OrderInitialized` event. This
   sets `emulation_trigger` to `None`, so components no longer treat the order as emulated.
1. It updates the original `SubmitOrder` command to describe the transformed order.
1. It sends that command to the configured `ExecutionAlgorithm` or, otherwise, the `ExecutionEngine`.
   The execution engine routes an eligible order to an `ExecutionClient`.

## Order types that can be emulated

**Released type**: the released type depends on the original emulated order type.

| Order type for emulation | Can emulate | Released type |
| ------------------------ | ----------- | ------------- |
| `MARKET`                 | -           | N/A           |
| `MARKET_TO_LIMIT`        | -           | N/A           |
| `LIMIT`                  | ✓           | `MARKET`      |
| `STOP_MARKET`            | ✓           | `MARKET`      |
| `STOP_LIMIT`             | ✓           | `LIMIT`       |
| `MARKET_IF_TOUCHED`      | ✓           | `MARKET`      |
| `LIMIT_IF_TOUCHED`       | ✓           | `LIMIT`       |
| `TRAILING_STOP_MARKET`   | ✓           | `MARKET`      |
| `TRAILING_STOP_LIMIT`    | ✓           | `LIMIT`       |

A `MARKET` release keeps the original time in force, except that `GTD` becomes `GTC` because market
orders do not support `GTD`.

:::warning
An emulated `TRAILING_STOP_LIMIT` submitted without a limit price gets one from the trailing
calculation. If the order triggers before that calculation succeeds, the emulator logs a warning
and keeps the order emulated. Once a market update lets the calculation produce a limit price,
the next matching update releases the order.

For example, the `LAST_OR_BID_ASK` trigger type needs a last trade price. A `BID_ASK` emulation trigger
subscribes the emulator to quotes only. Thus, the order stays held until trade data reaches the cache (such as
from a strategy's trade subscription).
:::

## Querying

Use the cache or the order object to query emulation status.

### Through the cache

**Cache methods**: the `Cache` provides these methods.

- `self.cache.orders_emulated(...)` returns all emulated orders that match its filters.
- `self.cache.is_order_emulated(...)` checks one client order ID.
- `self.cache.orders_emulated_count(...)` returns the number of matching emulated orders.

See the full [API reference](/docs/python-api-latest/cache.html) for additional details.

### Direct order queries

Use `order.is_emulated` to query an order object directly. A `False` value means the order was
released or was never emulated.

:::warning
Do not hold a local reference to an emulated order. The order object transforms
when the emulated order is *released*. Use the `Cache` instead.
:::

## Persistence and recovery

On startup, the `OrderEmulator` reactivates emulated orders that the configured cache database
restored into the cache. This preserves their state across restarts.

## Best practices

**Best practices**: when working with emulated orders, follow these rules.

- Query the `Cache` instead of storing local order references.
- Account for the order type changing on release.
- Handle denials and rejections from the released order's execution path.

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Advanced orders](advanced.md) - Order lists, contingency types, and bracket orders.
- [Strategies](../strategies.md) - Order management from strategies.
