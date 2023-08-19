# Execution

NautilusTrader can handle trade execution and order management for multiple strategies and venues
simultaneously (per instance). Several interacting components are involved in execution, making it 
crucial to understand the possible flows of execution messages (commands and events).

The main execution-related components include:
- `Strategy`
- `ExecAlgorithm` (execution algorithms)
- `OrderEmulator`
- `RiskEngine`
- `ExecutionEngine` or `LiveExecutionEngine`
- `ExecutionClient` or `LiveExecutionClient`

## Execution flow

The `Strategy` base class inherits from `Actor` and so contains all of the common data related
methods. It also provides methods for managing orders and trade execution:
- `submit_order(...)`
- `submit_order_list(...)`
- `modify_order(...)`
- `cancel_order(...)`
- `cancel_all_orders(...)`
- `close_position(...)`
- `close_all_positions(...)`
- `query_order(...)`

These methods create the necessary execution commands under the hood and send them on the message 
bus to the relevant components (point-to-point), as well as publishing any events (such as the 
initialization of new orders i.e. `OrderInitialized` events).

The general execution flow looks like the following (each arrow indicates movement across the message bus):

`Strategy` -> `OrderEmulator` -> `ExecAlgorithm` -> `RiskEngine` -> `ExecutionEngine` -> `ExecutionClient`

The `OrderEmulator` and `ExecAlgorithm`(s) components are optional in the flow, depending on
individual order parameters (as explained below).

## Submitting orders

An `OrderFactory` is provided on the base class for every `Strategy` as a convenience, reducing
the amount of boilerplate required to create different `Order` objects (although these objects
can still be initialized directly with the `Order.__init__` constructor if the trader prefers).

The component an order flows to when submitted for execution depends on the following:

- If an `emulation_trigger` is specified, the order will first be sent to the `OrderEmulator`
- If an `exec_algorithm_id` is specified, the order will first be sent to the relevant `ExecAlgorithm` (assuming it exists and has been registered correctly)
- Otherwise, the order is sent to the `RiskEngine`

The following examples show method implementations for a `Strategy`.

This example submits a `LIMIT` BUY order for emulation (see [OrderEmulator](advanced/emulated_orders.md)):
```python
    def buy(self) -> None:
        """
        Users simple buy method (example).
        """
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            price=self.instrument.make_price(5000.00),
            emulation_trigger=TriggerType.LAST_TRADE,
        )

        self.submit_order(order)
```

```{note}
It's possible to specify both order emulation, and an execution algorithm.
```

This example submits a `MARKET` BUY order to a TWAP execution algorithm:
```python
    def buy(self) -> None:
        """
        Users simple buy method (example).
        """
        order: MarketOrder = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            time_in_force=TimeInForce.FOK,
            exec_algorithm_id=ExecAlgorithmId("TWAP"),  
            exec_algorithm_params={"horizon_secs": 20, "interval_secs": 2.5},
        )

        self.submit_order(order)
```

## Execution algorithms

The platform supports customized execution algorithm components and provides some built-in 
algorithms, such as the Time-Weighted Average Price (TWAP) algorithm.

### TWAP (Time-Weighted Average Price)
The TWAP execution algorithm aims to execute orders by evenly spreading them over a specified
time horizon. The algorithm receives a primary order representing the total size and direction
then splits this by spawning smaller child orders, which are then executed at regular intervals
throughout the time horizon.

This helps to reduce the impact of the full size of the primary order on the market, by
minimizing the concentration of trade size at any given time.

The algorithm will immediately submit the first order, with the final order submitted being the
primary order at the end of the horizon period.

Using the TWAP algorithm as an example (found in ``/examples/algorithms/twap.py``), this example 
demonstrates how to initialize and register a TWAP execution algorithm directly with a 
`BacktestEngine` (assuming an engine is already initialized):

```python
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAP
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAPConfig

# Instantiate and add your execution algorithm
exec_algorithm = TWAPExecAlgorithm()
engine.add_exec_algorithm(exec_algorithm)
```

For this particular algorithm, two parameters must be specified: 
- `horizon_secs` 
- `interval_secs` 

The `horizon_secs` parameter determines the time period over which the algorithm will execute, while 
the `interval_secs` parameter sets the time between individual order executions. These parameters 
determine how a primary order is split into a series of spawned orders.

```python
# Configure your strategy
config = EMACrossTWAPConfig(
    instrument_id=str(ETHUSDT_BINANCE.id),
    bar_type="ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL",
    trade_size=Decimal("0.05"),
    fast_ema_period=10,
    slow_ema_period=20,
    twap_horizon_secs=10.0,  # <-- execution algorithm param
    twap_interval_secs=2.5,  # <-- execution algorithm param
)

# Instantiate and add your strategy
strategy = EMACrossTWAP(config=config)
```

Alternatively, you can specify these parameters dynamically per order, determining them based on 
actual market conditions. In this case, the strategy configuration parameters could be provided to 
an execution model which determines the horizon and interval.

```{note}
There is no limit to the number of execution algorithm parameters you can create. The parameters 
just need to be a dictionary with string keys and primitive values (values that can be serialized 
over the wire, such as ints, floats, and strings).
```

### Writing execution algorithms

To implement a custom execution algorithm you must define a class which inherits from `ExecAlgorithm`.

An execution algorithm is a type of `Actor`, so it's capable of the following:
- Request and subscribe to data
- Access the `Cache`
- Set time alerts and/or timers using a `Clock`

Additionally it can:
- Access the central `Portfolio`
- Spawn secondary orders from a received primary (original) order

Once an execution algorithm is registered, and the system is running, it will receive orders off the
messages bus which are addressed to its `ExecAlgorithmId` via the `exec_algorithm_id` order parameter. 
The order may also carry the `exec_algorithm_params` being a `dict[str, Any]`.

```{warning}
Because of the flexibility of the `exec_algorithm_params` dictionary. It's important to thoroughly 
validate all of the key value pairs for correct operation of the algorithm (for starters that the
dictionary is not ``None`` and all necessary parameters actually exist).
```

Received orders will arrive via the following `on_order(...)` method. These received orders are
know as "primary" (original) orders when being handled by an execution algorithm.

```python
def on_order(self, order: Order) -> None:  # noqa (too complex)
    """
    Actions to be performed when running and receives an order.

    Parameters
    ----------
    order : Order
        The order to be handled.

    Warnings
    --------
    System method (not intended to be called by user code).

    """
    # Handle the order here
```

When the algorithm is ready to spawn a secondary order, it can use one of the following methods:

- `spawn_market(...)` (spawns a `MARKET` order)
- `spawn_market_to_limit(...)` (spawns a `MARKET_TO_LIMIT` order)
- `spawn_limit(...)` (spawns a `LIMIT` order)

```{note}
Additional order types will be implemented in future versions, as the need arises.
```

Each of these methods takes the primary (original) `Order` as the first argument. The primary order
quantity will be reduced by the `quantity` passed in (becoming the spawned orders quantity).

```{warning}
There must be enough primary order quantity remaining (this is validated).
```

Once the desired number of secondary orders have been spawned, and the execution routine is over,
the intention is that the algorithm will then finally send the primary (original) order.

### Spawned orders

All secondary orders spawned from an execution algorithm will carry a `exec_spawn_id` which is
simply the `ClientOrderId` of the primary (original) order, and whose `client_order_id`
derives from this original identifier with the following convention:

- `exec_spawn_id` (primary order `client_order_id` value)
- `spawn_sequence` (the sequence number for the spawned order)

```
{exec_spawn_id}-E{spawn_sequence}
```

e.g. `O-20230404-001-000-E1` (for the first spawned order)

```{note}
The "primary" and "secondary" / "spawn" terminology was specifically chosen to avoid conflict
or confusion with the "parent" and "child" contingency orders terminology (an execution algorithm may also deal with contingent orders).
```

### Managing execution algorithm orders

The `Cache` provides several methods to aid in managing (keeping track of) the activity of
an execution algorithm:

```python

cpdef list orders_for_exec_algorithm(
    self,
    ExecAlgorithmId exec_algorithm_id,
    Venue venue = None,
    InstrumentId instrument_id = None,
    StrategyId strategy_id = None,
    OrderSide side = OrderSide.NO_ORDER_SIDE,
):
    """
    Return all execution algorithm orders for the given query filters.

    Parameters
    ----------
    exec_algorithm_id : ExecAlgorithmId
        The execution algorithm ID.
    venue : Venue, optional
        The venue ID query filter.
    instrument_id : InstrumentId, optional
        The instrument ID query filter.
    strategy_id : StrategyId, optional
        The strategy ID query filter.
    side : OrderSide, default ``NO_ORDER_SIDE`` (no filter)
        The order side query filter.

    Returns
    -------
    list[Order]

    """
```

As well as more specifically querying the orders for a certain execution series/spawn:

```python
cpdef list orders_for_exec_spawn(self, ClientOrderId exec_spawn_id):
    """
    Return all orders for the given execution spawn ID (if found).

    Will also include the primary (original) order.

    Parameters
    ----------
    exec_spawn_id : ClientOrderId
        The execution algorithm spawning primary (original) client order ID.

    Returns
    -------
    list[Order]

    """
```
