# Execution Algorithms

An `ExecutionAlgorithm` receives primary orders selected by `exec_algorithm_id` and can split them
into smaller spawned orders. NautilusTrader supports custom algorithms and includes a native Rust
TWAP implementation. Use this page to configure TWAP, write an algorithm, and manage spawned
orders.

For the component and routing model, see [Execution](index.md#execution-flow).

## TWAP (time-weighted average price)

TWAP spreads a primary order across regular intervals to reduce the market impact of submitting
the full quantity at once. To register the native algorithm with an initialized `BacktestEngine`:

```python
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.config import ExecutionAlgorithmConfig

engine.add_native_exec_algorithm(
    "TwapAlgorithm",
    ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP")),
)
```

Orders routed to TWAP require these string-valued `exec_algorithm_params`:

| Key             | Meaning                                                 |
| --------------- | ------------------------------------------------------- |
| `horizon_secs`  | Horizon used with the interval to determine the slices. |
| `interval_secs` | Time between slices.                                    |

Both values must parse as positive numbers, and `horizon_secs` must be at least
`interval_secs`. The algorithm submits the first slice immediately and the remaining slices at
the configured interval. TWAP denies the primary order before submission when the order type,
instrument, or schedule is unsupported or invalid.

## Custom execution algorithms

To define a Python execution algorithm, subclass `ExecutionAlgorithm` and implement
`on_order(...)`:

```python
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.config import ExecutionAlgorithmConfig


class MyExecutionAlgorithm(ExecutionAlgorithm):
    def __init__(self) -> None:
        super().__init__(
            ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("MY-ALGO")),
        )

    def on_order(self, order) -> None: ...
```

Python execution algorithms provide cache and portfolio access, a clock for timers, signals, and
methods for spawning orders.

After registration, the message bus routes an order to the algorithm whose `ExecAlgorithmId`
matches the order's `exec_algorithm_id`. The optional `exec_algorithm_params` field is a
`Mapping[str, str]`. Override `on_order_list(...)` to handle a list as a unit; its default
implementation passes each order to `on_order(...)`.

:::warning
Validate required `exec_algorithm_params` keys and parse their string values before executing an
order. Call `deny_order(...)` with a standardized
[reason code](index.md#order-denied-reasons), such as
`VALIDATION_FAILED: horizon_secs not found in exec_algorithm_params`, when the order cannot be
executed.
:::

An order received by an execution algorithm is the primary order. Use these methods to create
spawned orders:

- `spawn_market(...)`: Creates a `MARKET` order.
- `spawn_market_to_limit(...)`: Creates a `MARKET_TO_LIMIT` order.
- `spawn_limit(...)`: Creates a `LIMIT` order.

Each method takes the primary order as its first argument. By default, the method reduces the
primary order quantity by the spawned `quantity`. Pass `reduce_primary=False` to keep the primary
quantity unchanged.

:::warning
When `reduce_primary=True`, the spawned quantity must not exceed the primary order's `leaves_qty`
(remaining unfilled quantity).
:::

If a spawned order is denied or rejected before acceptance, the deducted quantity is automatically
restored to the primary order. Once accepted by the venue, the reduction is considered committed.

An execution algorithm can keep spawning orders, submit the remaining primary order, or do both.
The built-in TWAP algorithm submits the remaining primary order on the final interval.

## Spawned orders

Every spawned order sets `exec_spawn_id` to the primary order's `client_order_id`. Its own
`client_order_id` follows this pattern:

```text
{exec_spawn_id}-E{spawn_sequence}
```

For example, the first order spawned from `O-20230404-001-000` has the ID
`O-20230404-001-000-E1`.

:::note
The primary and spawned terminology distinguishes execution slicing from parent and child
contingent-order relationships.
:::

## Execution algorithm order queries

The `Cache` provides two primary queries:

- `orders_for_exec_algorithm(...)`: Returns orders for an algorithm, with optional venue,
  instrument, strategy, account, and side filters.
- `orders_for_exec_spawn(...)`: Returns the primary order and its spawned orders for a primary
  `ClientOrderId`.

## Related guides

- [Execution](index.md): Component routing, OMS behavior, risk checks, and command outcomes.
- [Execution policies](policies.md): Order-state and command-delivery boundaries.
- [Orders](../orders/): Order types, instructions, and state transitions.
