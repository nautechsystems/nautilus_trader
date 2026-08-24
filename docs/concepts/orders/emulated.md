# Emulated orders

Emulation lets you use order types even when your trading venue does not natively support them.

The `OrderEmulator` monitors the market data selected by `emulation_trigger`. When the local order
matches its release condition, the emulator transforms it into a `MARKET` or `LIMIT` order and sends
that order through the normal risk and execution path. For example, an emulated `STOP_LIMIT` becomes
a `LIMIT` order after its stop price triggers.

## Submitting an order for emulation

Set `emulation_trigger` on an order constructor or `OrderFactory` method. The local emulator accepts
these values:

| Trigger type | Market data used                                       |
| ------------ | ------------------------------------------------------ |
| `DEFAULT`    | Quotes, with the same local behavior as `BID_ASK`.     |
| `BID_ASK`    | Best bid and ask quotes.                               |
| `LAST_PRICE` | Trades.                                                |
| `NO_TRIGGER` | No local emulation; submit through the normal pathway. |

Other `TriggerType` values describe trigger methods that some venues support, but the local
`OrderEmulator` does not accept them as `emulation_trigger` values.

The choice of trigger type determines how the order emulation will behave:

- For stop orders, the emulator compares the trigger price with the selected market data.
- For trailing-stop orders, it updates the trailing trigger from that market data.
- For emulated `LIMIT` orders, it compares the limit price with that market data and releases a
  `MARKET` order when matched.

## Technical details

The same `OrderEmulator` component manages supported emulated order types in all
[environment contexts](../architecture.md#environment-contexts).

:::note
NautilusTrader does not configure a fixed count limit for emulated orders. Available memory and the
cost of market data processing provide practical limits.
:::

## Lifecycle

An emulated order progresses through these stages:

1. A `Strategy` submits it through `submit_order`.
1. The `RiskEngine` applies pre-trade checks and may deny it.
1. The `OrderEmulator` holds and monitors it locally.
1. A matching market update transforms it into a `MARKET` or `LIMIT` order and releases it.
1. The `RiskEngine` checks the released order again before venue submission.

:::note
Emulated orders pass through the normal risk controls. A strategy can modify or cancel them, and a
cancel-all request includes them.
:::

:::info
An emulated order retains its client order ID when transformed, so cache queries continue to use the
same ID.
:::

### Held emulated orders

While the `OrderEmulator` holds an order:

- It caches the original `SubmitOrder` command.
- It processes the order in a local matching core.
- It subscribes to the required quotes or trades if no matching subscription exists.
- It accepts strategy modifications and market-driven updates until release or cancellation.

### Released emulated orders

When market data matches an emulated order, release performs these actions:

- It transforms the order into a `MARKET` or `LIMIT` order through another `OrderInitialized`
  event.
- It sets the order's `emulation_trigger` to `NO_TRIGGER` so components no longer treat it as
  emulated.
- It sends the transformed order and original `SubmitOrder` command back through the `RiskEngine`.
- If the risk engine does not deny it, the `ExecutionEngine` routes it to an `ExecutionClient`.

## Order types that can be emulated

The released type depends on the original emulated order type:

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

## Querying

Use the cache or the order object to query emulation status.

### Through the cache

The `Cache` provides these methods:

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

When working with emulated orders:

1. Query the `Cache` instead of storing local order references.
1. Account for the order type changing on release.
1. Handle a denial at either the initial or release-time risk check.

## Related guides

- [Orders](index.md) - Order concepts, execution instructions, and the order factory.
- [Advanced orders](advanced.md) - Order lists, contingency types, and bracket orders.
- [Strategies](../strategies.md) - Order management from strategies.
