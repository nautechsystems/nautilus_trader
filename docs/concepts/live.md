# Live Trading

Live trading in NautilusTrader enables traders to deploy their backtested strategies in a real-time
trading environment with no code changes. This seamless transition from backtesting to live trading
is a core feature of the platform, ensuring consistency and reliability. However, there are
key differences to be aware of between backtesting and live trading.

This guide provides an overview of the key aspects of live trading.

## Configuration

When operating a live trading system, configuring your execution engine and strategies properly is
essential for ensuring reliability, accuracy, and performance. The following is an overview of the
key concepts and settings involved for live configuration.

### `TradingNodeConfig`

The main configuration class for live trading systems is `TradingNodeConfig`,
which inherits from `NautilusKernelConfig` and provides live-specific config options:

```python
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    trader_id="MyTrader-001",

    # Component configurations
    cache: CacheConfig(),
    message_bus: MessageBusConfig(),
    data_engine=LiveDataEngineConfig(),
    risk_engine=LiveRiskEngineConfig(),
    exec_engine=LiveExecEngineConfig(),
    portfolio=PortfolioConfig(),

    # Client configurations
    data_clients={
        "BINANCE": BinanceDataClientConfig(),
    },
    exec_clients={
        "BINANCE": BinanceExecClientConfig(),
    },
)
```

#### Core configuration parameters

| Setting                  | Default      | Description                                 |
|--------------------------|--------------|---------------------------------------------|
| `trader_id`              | "TRADER-001" | Unique trader identifier (name-tag format). |
| `instance_id`            | `None`       | Optional unique instance identifier.        |
| `timeout_connection`     | 30.0         | Connection timeout in seconds.              |
| `timeout_reconciliation` | 10.0         | Reconciliation timeout in seconds.          |
| `timeout_portfolio`      | 10.0         | Portfolio initialization timeout.           |
| `timeout_disconnection`  | 10.0         | Disconnection timeout.                      |
| `timeout_post_stop`      | 5.0          | Post-stop cleanup timeout.                  |

#### Cache database configuration

Configure data persistence with a backing database:

```python
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig

cache_config = CacheConfig(
    database=DatabaseConfig(
        host="localhost",
        port=6379,
        username="nautilus",
        password="pass",
        timeout=2.0,
    ),
    encoding="msgpack",  # or "json"
    timestamps_as_iso8601=True,
    buffer_interval_ms=100,
    flush_on_start=False,
)
```

#### MessageBus configuration

Configure message routing and external streaming:

```python
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import DatabaseConfig

message_bus_config = MessageBusConfig(
    database=DatabaseConfig(timeout=2),
    timestamps_as_iso8601=True,
    use_instance_id=False,
    types_filter=[QuoteTick, TradeTick],  # Filter specific message types
    stream_per_topic=False,
    autotrim_mins=30,  # Automatic message trimming
    heartbeat_interval_secs=1,
)
```

### Multi-venue configuration

Live trading systems often connect to multiple venues. Here's an example of configuring both spot and futures markets for Binance:

```python
config = TradingNodeConfig(
    trader_id="MultiVenue-001",

    # Multiple data clients for different market types
    data_clients={
        "BINANCE_SPOT": BinanceDataClientConfig(
            account_type=BinanceAccountType.SPOT,
            testnet=False,
        ),
        "BINANCE_FUTURES": BinanceDataClientConfig(
            account_type=BinanceAccountType.USDT_FUTURE,
            testnet=False,
        ),
    },

    # Corresponding execution clients
    exec_clients={
        "BINANCE_SPOT": BinanceExecClientConfig(
            account_type=BinanceAccountType.SPOT,
            testnet=False,
        ),
        "BINANCE_FUTURES": BinanceExecClientConfig(
            account_type=BinanceAccountType.USDT_FUTURE,
            testnet=False,
        ),
    },
)
```

### ExecutionEngine configuration

The `LiveExecEngineConfig` sets up the live execution engine, managing order processing, execution events, and reconciliation with trading venues.
The following outlines the main configuration options.

By configuring these parameters thoughtfully, you can ensure that your trading system operates efficiently,
handles orders correctly, and remains resilient in the face of potential issues, such as lost events or conflicting data/information.

For full details see the `LiveExecEngineConfig` [API Reference](../api_reference/config#class-liveexecengineconfig).

#### Reconciliation

**Purpose**: Ensures that the system state remains consistent with the trading venue by recovering any missed events, such as order and position status updates.

| Setting                         | Default | Description                                                                                        |
|---------------------------------|---------|----------------------------------------------------------------------------------------------------|
| `reconciliation`                | True    | Activates reconciliation at startup, aligning the system's internal state with the venue's state.  |
| `reconciliation_lookback_mins`  | None    | Specifies how far back (in minutes) the system requests past events to reconcile uncached state.   |
| `reconciliation_instrument_ids` | None    | An include list of specific instrument IDs to consider for reconciliation.                         |
| `filtered_client_order_ids`     | None    | A list of client order IDs to filter from reconciliation (useful when the venue holds duplicates). |

See [Execution reconciliation](../concepts/execution#execution-reconciliation) for additional background.

#### Order filtering

**Purpose**: Manages which order events and reports should be processed by the system to avoid conflicts with other trading nodes and unnecessary data handling.

| Setting                            | Default | Description                                                                                                |
|------------------------------------|---------|------------------------------------------------------------------------------------------------------------|
| `filter_unclaimed_external_orders` | False   | Filters out unclaimed external orders to prevent irrelevant orders from impacting the strategy.            |
| `filter_position_reports`          | False   | Filters out position status reports, useful when multiple nodes trade the same account to avoid conflicts. |

#### Order checks

**Purpose**: Maintains accurate execution state by:

- (1) Monitoring in-flight orders.
- (2) Reconciling open orders with the venue.
- (3) Auditing internal *own* order books against the venue’s public books.

If an order’s status cannot be reconciled after exhausting all retries, the engine resolves the order as follows:

| Current status   | Resolved to | Rationale                                  |
|------------------|-------------|--------------------------------------------|
| `SUBMITTED`      | `REJECTED`  | No confirmation received.                  |
| `PENDING_UPDATE` | `CANCELED`  | Modification remains unacknowledged.       |
| `PENDING_CANCEL` | `CANCELED`  | Venue never confirmed the cancellation.    |

This ensures the trading node maintains a consistent execution state even under unreliable conditions.

| Setting                         | Default   | Description                                                                                                                         |
|---------------------------------|-----------|-------------------------------------------------------------------------------------------------------------------------------------|
| `inflight_check_interval_ms`    | 2,000 ms  | Determines how frequently the system checks in-flight order status.                                                                 |
| `inflight_check_threshold_ms`   | 5,000 ms  | Sets the time threshold after which an in-flight order triggers a venue status check. Adjust if colocated to avoid race conditions. |
| `inflight_check_retries`        | 5 retries | Specifies the number of retry attempts the engine will make to verify the status of an in-flight order with the venue, should the initial attempt fail. |
| `open_check_interval_secs`      | None      | Determines how frequently (in seconds) open orders are checked at the venue. Recommended: 5-10 seconds, considering API rate limits. |
| `open_check_open_only`          | True      | When enabled, only open orders are requested during checks; if disabled, full order history is fetched (resource-intensive).         |
| `own_books_audit_interval_secs` | None      | Sets the interval (in seconds) between audits of own order books against public ones. Verifies synchronization and logs errors for inconsistencies. |

#### Additional options

The following additional options provide further control over execution behavior:

| Setting                            | Default | Description                                                                                                |
|------------------------------------|---------|------------------------------------------------------------------------------------------------------------|
| `generate_missing_orders`          | True    | If `MARKET` order events will be generated during reconciliation to align position discrepancies. These orders use the strategy ID `INTERNAL-DIFF`.  |
| `snapshot_orders`                  | False   | If order snapshots should be taken on order events.                                                        |
| `snapshot_positions`               | False   | If position snapshots should be taken on position events.                                                  |
| `snapshot_positions_interval_secs` | None    | The interval (seconds) between position snapshots when enabled.                                            |
| `debug`                            | False   | Enable debug mode for additional execution logging.                                                        |

#### Memory management

**Purpose**: Periodically purges closed orders, closed positions, and account events from the in-memory cache to optimize resource usage and performance during extended / HFT operations.

| Setting                                | Default | Description                                                                                                                             |
|----------------------------------------|---------|-----------------------------------------------------------------------------------------------------------------------------------------|
| `purge_closed_orders_interval_mins`    | None    | Sets how frequently (in minutes) closed orders are purged from memory. Recommended: 10-15 minutes. Does not affect database records.    |
| `purge_closed_orders_buffer_mins`      | None    | Specifies how long (in minutes) an order must have been closed before purging. Recommended: 60 minutes to ensure processes complete.    |
| `purge_closed_positions_interval_mins` | None    | Sets how frequently (in minutes) closed positions are purged from memory. Recommended: 10-15 minutes. Does not affect database records. |
| `purge_closed_positions_buffer_mins`   | None    | Specifies how long (in minutes) a position must have been closed before purging. Recommended: 60 minutes to ensure processes complete.  |
| `purge_account_events_interval_mins`   | None    | Sets how frequently (in minutes) account events are purged from memory. Recommended: 10-15 minutes. Does not affect database records.   |
| `purge_account_events_lookback_mins`   | None    | Specifies how long (in minutes) an account event must have occurred before purging. Recommended: 60 minutes.                            |
| `purge_from_database`                  | False   | If enabled, purge operations will also delete data from the backing database (Redis/PostgreSQL), not just memory. **Use with caution**. |

By configuring these memory management settings appropriately, you can prevent memory usage from growing
indefinitely during long-running / HFT sessions while ensuring that recently closed orders, closed positions, and account events
remain available in memory for any ongoing operations that might require them.

#### Queue management

**Purpose**: Handles the internal buffering of order events to ensure smooth processing and to prevent system resource overloads.

| Setting | Default  | Description                                                                                          |
|---------|----------|------------------------------------------------------------------------------------------------------|
| `qsize` | 100,000  | Sets the size of internal queue buffers, managing the flow of data within the engine.                |

### Strategy configuration

The `StrategyConfig` class outlines the configuration for trading strategies, ensuring that each strategy operates with the correct parameters and manages orders effectively.
For a complete parameter list see the `StrategyConfig` [API Reference](../api_reference/config#class-strategyconfig).

#### Identification

**Purpose**: Provides unique identifiers for each strategy to prevent conflicts and ensure proper tracking of orders.

| Setting                     | Default | Description                                                                                            |
|-----------------------------|---------|--------------------------------------------------------------------------------------------------------|
| `strategy_id`               | None    | A unique ID for the strategy, ensuring it can be distinctly identified.                                |
| `order_id_tag`              | None    | A unique tag for the strategy's orders, differentiating them from multiple strategies.                 |

#### Order management

**Purpose**: Controls strategy-level order handling including position-ID processing, claiming relevant external orders, automating contingent order logic (OUO/OCO), and tracking GTD expirations.

| Setting                     | Default | Description                                                                                                            |
|-----------------------------|---------|------------------------------------------------------------------------------------------------------------------------|
| `oms_type`                  | None    | Specifies the [OMS type](../concepts/execution#oms-configuration), for position ID handling and order processing flow. |
| `use_uuid_client_order_ids` | False   | If UUID4's should be used for client order ID values (required for some venues such as Coinbase Intx). |
| `external_order_claims`     | None    | Lists instrument IDs for external orders the strategy should claim, aiding accurate order management. |
| `manage_contingent_orders`  | False   | If enabled, the strategy automatically manages contingent orders, reducing manual intervention. |
| `manage_gtd_expiry`         | False   | If enabled, the strategy manages GTD expirations, ensuring orders remain active as intended. |

## Execution reconciliation

Execution reconciliation is the process of aligning the external state of reality for orders and positions
(both closed and open) with the systems internal state built from events.
This process is primarily applicable to live trading, which is why only the `LiveExecutionEngine` has reconciliation capability.

There are two main scenarios for reconciliation:

- **Previous cached execution state**: Where cached execution state exists, information from reports is used to generate missing events to align the state.
- **No previous cached execution state**: Where there is no cached state, all orders and positions that exist externally are generated from scratch.

:::tip
**Best practice**: Persist all execution events to the cache database to minimize reliance on venue history, ensuring full recovery even with short lookback windows.
:::

### Reconciliation configuration

Unless reconciliation is disabled by setting the `reconciliation` configuration parameter to false,
the execution engine will perform the execution reconciliation procedure for each venue.
Additionally, you can specify the lookback window for reconciliation by setting the `reconciliation_lookback_mins` configuration parameter.

:::tip
We recommend not setting a specific `reconciliation_lookback_mins`. This allows the requests made
to the venues to utilize the maximum execution history available for reconciliation.
:::

:::warning
If executions have occurred prior to the lookback window, any necessary events will be generated to align
internal and external states. This may result in some information loss that could have been avoided with a longer lookback window.

Additionally, some venues may filter or drop execution information under certain conditions, resulting
in further information loss. This would not occur if all events were persisted in the cache database.
:::

Each strategy can also be configured to claim any external orders for an instrument ID generated during
reconciliation using the `external_order_claims` configuration parameter.
This is useful in situations where, at system start, there is no cached state or it is desirable for
a strategy to resume its operations and continue managing existing open orders for a specific instrument.

Orders generated with strategy ID `INTERNAL-DIFF` during position reconciliation are internal to the engine and cannot be claimed via `external_order_claims`.
They exist solely to align position discrepancies and should not be managed by user strategies.

For a full list of live trading options see the `LiveExecEngineConfig` [API Reference](../api_reference/config#class-liveexecengineconfig).

### Reconciliation procedure

The reconciliation procedure is standardized for all adapter execution clients and uses the following
methods to produce an execution mass status:

- `generate_order_status_reports`
- `generate_fill_reports`
- `generate_position_status_reports`

The system state is then reconciled with the reports, which represent external "reality":

- **Duplicate Check**:
  - Check for duplicate client order IDs and trade IDs.
  - Duplicate client order IDs cause reconciliation failure to prevent state corruption.
- **Order Reconciliation**:
  - Generate and apply events necessary to update orders from any cached state to the current state.
  - If any trade reports are missing, inferred `OrderFilled` events are generated.
  - If any client order ID is not recognized or an order report lacks a client order ID, external order events are generated.
  - Fill report data consistency is verified using tolerance-based comparisons for price and commission differences.
- **Position Reconciliation**:
  - Ensure the net position per instrument matches the position reports returned from the venue using instrument precision handling.
  - If the position state resulting from order reconciliation does not match the external state, external order events will be generated to resolve discrepancies.
  - When `generate_missing_orders` is enabled (default: True), `MARKET` orders are generated with strategy ID `INTERNAL-DIFF` to align position discrepancies discovered during reconciliation.
  - Zero quantity differences after precision rounding are handled gracefully.
- **Exception Handling**:
  - Individual adapter failures do not abort the entire reconciliation process.
  - Missing order status reports are handled gracefully when fill reports arrive first.

If reconciliation fails, the system will not continue to start, and an error will be logged.

### Common reconciliation scenarios

The scenarios below are split between startup reconciliation (mass status) and runtime/continuous checks (in-flight order checks, open-order polls, and own-books audits).

#### Startup reconciliation

| Scenario                               | Description                                                                                       | System behavior                                                                          | Test coverage                                                               |
|----------------------------------------|---------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| **Order state discrepancy**            | Local order state differs from venue (e.g., local shows `SUBMITTED`, venue shows `REJECTED`).     | Updates local order to match venue state and emits missing events.                       | `test_order_state_discrepancy_reconciliation`                               |
| **Missed fills**                       | Venue fills an order but the engine misses the fill event.                                        | Generates missing `OrderFilled` events.                                                  | `test_missed_fill_reconciliation_scenario`                                  |
| **Multiple fills**                     | Order has multiple partial fills, some missed by the engine.                                      | Reconstructs complete fill history from venue reports.                                   | `test_multiple_fills_reconciliation`                                        |
| **External orders**                    | Orders exist on venue but not in local cache (placed externally or from another system).          | Creates orders from venue reports; tags them `EXTERNAL`.                                 | `test_external_order_reconciliation`                                        |
| **Partially filled then canceled**     | Order partially filled then canceled by venue.                                                    | Updates order state to `CANCELED` while preserving fill history.                         | `test_reconcile_state_no_cached_with_partially_filled_order_and_canceled`   |
| **Different fill data**                | Venue reports different fill price/commission than cached.                                        | Preserves cached fill data; logs discrepancies from reports.                             | `test_reconcile_state_with_cached_order_and_different_fill_data`            |
| **Filtered orders**                    | Orders marked for filtering via configuration.                                                    | Skips reconciliation based on `filtered_client_order_ids` or instrument filters.         | `test_filtered_client_order_ids_filters_reports`                            |
| **Duplicate client order IDs**         | Multiple orders with same client order ID in venue reports.                                       | Reconciliation fails to prevent state corruption.                                        | `test_duplicate_client_order_id_fails_validation`                           |
| **Position quantity mismatch (long)**  | Internal long position differs from external (e.g., internal: 100, external: 150).                | Generates BUY order for difference when `generate_missing_orders=True`.                  | `test_long_position_reconciliation_quantity_mismatch`                       |
| **Position quantity mismatch (short)** | Internal short position differs from external (e.g., internal: -100, external: -150).             | Generates SELL order for difference when `generate_missing_orders=True`.                 | `test_short_position_reconciliation_quantity_mismatch`                      |
| **Position reduction**                 | External position smaller than internal (e.g., internal: 150 long, external: 100 long).           | Generates opposite side order to reduce position.                                        | `test_long_position_reconciliation_external_smaller`                        |
| **Position side flip**                 | Internal position opposite of external (e.g., internal: 100 long, external: 50 short).            | Generates order to close internal and open external position.                            | `test_position_reconciliation_cross_side_long_to_short`                     |
| **INTERNAL-DIFF orders**               | Position reconciliation orders with strategy ID "INTERNAL-DIFF".                                  | Never filtered, regardless of `filter_unclaimed_external_orders`.                        | `test_internal_diff_order_not_filtered_when_filter_unclaimed_external_orders_enabled` |

#### Runtime/continuous checks

| Scenario                               | Description                                                                 | System behavior                                                                                                        | Test coverage                                |
|----------------------------------------|-----------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------|----------------------------------------------|
| **In-flight order timeout**            | In-flight order remains unconfirmed beyond threshold.                       | After `inflight_check_retries`, resolves to `REJECTED` to maintain consistent state.                                   | `test_inflight_order_timeout_reconciliation` |
| **Open orders check discrepancy**      | Periodic open-orders poll detects a state change at the venue.              | At `open_check_interval_secs`, confirms status (respecting `open_check_open_only`) and applies transitions if changed. | —                                            |
| **Own books audit mismatch**           | Own order books diverge from venue public books.                            | At `own_books_audit_interval_secs`, audits and logs inconsistencies for investigation.                                 | —                                            |

### Common reconciliation issues

- **Missing trade reports**: Some venues filter out older trades, causing incomplete reconciliation. Increase `reconciliation_lookback_mins` or ensure all events are cached locally.
- **Position mismatches**: If external orders predate the lookback window, positions may not align. Flatten the account before restarting the system to reset state.
- **Duplicate order IDs**: Duplicate client order IDs in mass status reports will cause reconciliation failure. Ensure venue data integrity or contact support.
- **Precision differences**: Small decimal differences in position quantities are handled automatically using instrument precision, but large discrepancies may indicate missing orders.
- **Out-of-order reports**: Fill reports arriving before order status reports are deferred until order state is available.

:::tip
For persistent reconciliation issues, consider dropping cached state or flattening accounts before system restart.
:::
