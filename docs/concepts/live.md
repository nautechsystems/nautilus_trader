# Live Trading

Live trading in NautilusTrader enables traders to deploy their backtested strategies in a real-time
trading environment with no code changes. This seamless transition from backtesting to live trading
is a core feature of the platform, ensuring consistency and reliability. However, there are
key differences to be aware of between backtesting and live trading.

This guide provides an overview of the key aspects of live trading.

:::info Platform differences
Windows signal handling differs from Unix-like systems. If you are running on Windows, please read
the note on [Windows signal handling](#windows-signal-handling) for guidance on graceful shutdown
behavior and Ctrl+C (SIGINT) support.
:::

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
            account_type=BinanceAccountType.USDT_FUTURES,
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
            account_type=BinanceAccountType.USDT_FUTURES,
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

#### Continuous reconciliation

**Purpose**: Maintains accurate execution state through a continuous reconciliation loop that runs *after* startup reconciliation completes, this loop:

- (1) Monitors in-flight orders for delays exceeding a configured threshold.
- (2) Reconciles open orders with the venue at configurable intervals.
- (3) Audits internal *own* order books against the venue's public books.

**Startup sequence**: The continuous reconciliation loop waits for startup reconciliation to complete before beginning periodic checks. This prevents race conditions where continuous checks might interfere with the initial state reconciliation. The `reconciliation_startup_delay_secs` parameter applies an additional delay *after* startup reconciliation completes.

If an order's status cannot be reconciled after exhausting all retries, the engine resolves the order as follows:

**In-flight order timeout resolution** (when venue doesn't respond after max retries):

| Current status   | Resolved to | Rationale                                  |
|------------------|-------------|--------------------------------------------|
| `SUBMITTED`      | `REJECTED`  | No confirmation received from venue.       |
| `PENDING_UPDATE` | `CANCELED`  | Modification remains unacknowledged.       |
| `PENDING_CANCEL` | `CANCELED`  | Venue never confirmed the cancellation.    |

**Order consistency checks** (when cache state differs from venue state):

| Cache status       | Venue status | Resolution  | Rationale                                                           |
|--------------------|--------------|-------------|---------------------------------------------------------------------|
| `ACCEPTED`         | Not found    | `REJECTED`  | Order doesn't exist at venue, likely was never successfully placed. |
| `ACCEPTED`         | `CANCELED`   | `CANCELED`  | Venue canceled the order (user action or venue-initiated).          |
| `ACCEPTED`         | `EXPIRED`    | `EXPIRED`   | Order reached GTD expiration at venue.                              |
| `ACCEPTED`         | `REJECTED`   | `REJECTED`  | Venue rejected after initial acceptance (rare but possible).        |
| `PARTIALLY_FILLED` | `CANCELED`   | `CANCELED`  | Order canceled at venue with fills preserved.                       |
| `PARTIALLY_FILLED` | Not found    | `CANCELED`  | Order doesn't exist but had fills (reconciles fill history).        |

:::note
**Important reconciliation caveats:**

- **"Not found" resolutions**: These are only performed in full-history mode (`open_check_open_only=False`). In open-only mode (`open_check_open_only=True`, the default), these checks are intentionally skipped. This is because open-only mode uses venue-specific "open orders" endpoints which exclude closed orders by design, making it impossible to distinguish between genuinely missing orders and recently closed ones.
- **Recent order protection**: The engine skips reconciliation actions for orders with last event timestamp within the `open_check_threshold_ms` window (default 5 seconds). This prevents false positives from race conditions where orders may still be processing at the venue.
- **Targeted query safeguard**: Before marking orders as `REJECTED` or `CANCELED` when "not found", the engine attempts a targeted single-order query to the venue. This helps prevent false negatives due to bulk query limitations or timing delays.
- **`FILLED` orders**: When a `FILLED` order is "not found" at the venue, this is considered normal behavior (venues often don't track completed orders) and is ignored without generating warnings.

:::

#### Retry coordination and lookback behavior

The execution engine reuses a single retry counter (`_recon_check_retries`) for both the inflight loop (bounded by `inflight_check_retries`) and the open-order loop (bounded by `open_check_missing_retries`). This shared budget ensures the stricter limit wins and prevents duplicate venue queries for the same order state.

When the open-order loop exhausts its retries, the engine issues one targeted `GenerateOrderStatusReport` probe before applying a terminal state. If the venue returns the order, reconciliation proceeds and the retry counter resets automatically.

**Single-order query protection**: To prevent rate limit exhaustion when many orders need individual queries, the engine limits single-order queries per reconciliation cycle via `max_single_order_queries_per_cycle` (default: 10). When this limit is reached, remaining orders are deferred to the next cycle. Additionally, the engine adds a configurable delay (`single_order_query_delay_ms`, default: 100ms) between single-order queries to further prevent rate limiting. This ensures the system can handle scenarios where bulk queries fail for hundreds of orders without overwhelming the venue API.

Orders that age beyond `open_check_lookback_mins` rely on this targeted probe. Keep the lookback generous for venues with short history windows, and consider increasing `open_check_threshold_ms` if venue timestamps lag the local clock so recently updated orders are not marked missing prematurely.

This ensures the trading node maintains a consistent execution state even under unreliable conditions.

| Setting                              | Default        | Description                                                                                                                         |
|--------------------------------------|----------------|-------------------------------------------------------------------------------------------------------------------------------------|
| `inflight_check_interval_ms`         | 2,000&nbsp;ms  | Determines how frequently the system checks in-flight order status. Set to 0 to disable.                                            |
| `inflight_check_threshold_ms`        | 5,000&nbsp;ms  | Sets the time threshold after which an in-flight order triggers a venue status check. Adjust if colocated to avoid race conditions. |
| `inflight_check_retries`             | 5&nbsp;retries | Specifies the number of retry attempts the engine will make to verify the status of an in-flight order with the venue, should the initial attempt fail. |
| `open_check_interval_secs`           | None           | Determines how frequently (in seconds) open orders are checked at the venue. Set to None or 0.0 to disable. Recommended: 5-10 seconds, considering API rate limits. |
| `open_check_open_only`               | True           | When enabled, only open orders are requested during checks; if disabled, full order history is fetched (resource-intensive).         |
| `open_check_lookback_mins`           | 60&nbsp;min    | Lookback window (minutes) for order status polling during continuous reconciliation. Only orders modified within this window are considered. |
| `open_check_threshold_ms`            | 5,000&nbsp;ms  | Minimum time since the order's last cached event before open-order checks act on venue discrepancies (missing, mismatched status, etc.). |
| `open_check_missing_retries`         | 5&nbsp;retries | Maximum retries before resolving an order that is open in cache but not found at venue. Prevents false positives from race conditions. |
| `max_single_order_queries_per_cycle` | 10             | Maximum number of single-order queries per reconciliation cycle. Prevents rate limit exhaustion when many orders fail bulk query checks. |
| `single_order_query_delay_ms`        | 100&nbsp;ms    | Delay (milliseconds) between single-order queries to prevent rate limit exhaustion. |
| `reconciliation_startup_delay_secs`  | 10.0&nbsp;s    | Additional delay (seconds) applied *after* startup reconciliation completes before starting continuous reconciliation loop. Provides time for additional system stabilization. |
| `own_books_audit_interval_secs`      | None           | Sets the interval (in seconds) between audits of own order books against public ones. Verifies synchronization and logs errors for inconsistencies. |

:::warning
**Important configuration guidelines:**

- **`open_check_lookback_mins`**: Do not reduce below 60 minutes. This lookback window must be sufficiently generous for your venue's order history retention. Setting it too short can trigger false "missing order" resolutions even with built-in safeguards, as orders may appear missing when they're simply outside the query window.
- **`reconciliation_startup_delay_secs`**: Do not reduce below 10 seconds for production systems. This delay is applied *after* startup reconciliation completes, allowing additional time for system stabilization before continuous reconciliation checks begin. This prevents continuous checks from starting immediately after startup reconciliation finishes.

:::

#### Additional options

The following additional options provide further control over execution behavior:

| Setting                            | Default | Description                                                                                                |
|------------------------------------|---------|------------------------------------------------------------------------------------------------------------|
| `generate_missing_orders`          | True    | If `LIMIT` order events will be generated during reconciliation to align position discrepancies. These orders use the strategy ID `INTERNAL-DIFF` and calculate precise prices to achieve target average positions.  |
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
Set an interval to enable the relevant purge loop; leaving it unset disables both scheduling and deletion.
Each loop delegates to the cache APIs described in [Purging cached state](cache.md#purging-cached-state).

#### Queue management

**Purpose**: Handles the internal buffering of order events to ensure smooth processing and to prevent system resource overloads.

| Setting                          | Default  | Description                                                                                          |
|----------------------------------|----------|------------------------------------------------------------------------------------------------------|
| `qsize`                          | 100,000  | Sets the size of internal queue buffers, managing the flow of data within the engine.                |
| `graceful_shutdown_on_exception` | False    | If the system should perform a graceful shutdown when an unexpected exception occurs during message queue processing (does not include user actor/strategy exceptions). |

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

### Windows signal handling

:::warning
Windows: asyncio event loops do not implement `loop.add_signal_handler`. As a result, the legacy
`TradingNode` does not receive OS signals via asyncio on Windows. Use Ctrl+C (SIGINT) handling or
programmatic shutdown; SIGTERM parity is not expected on Windows.
:::

On Windows, asyncio event loops do not implement `loop.add_signal_handler`, so Unix-style signal
integration is unavailable. As a result, `TradingNode` does not receive OS signals via asyncio on
Windows and will not gracefully stop unless you intervene.

Recommended approaches on Windows:

- Wrap `run` with a `try/except KeyboardInterrupt` and call `node.stop()` then `node.dispose()`.
  Ctrl+C on Windows raises `KeyboardInterrupt` in the main thread, providing a clean teardown path.
- Alternatively, publish a `ShutdownSystem` command programmatically (or call `shutdown_system(...)`
  from an actor/component) to trigger the same shutdown path.

The “inflight check loop task still pending” message is consistent with the lack of asyncio signal
handling on Windows, i.e., the normal graceful shutdown path isn’t being triggered.

This is tracked as an enhancement request to support Ctrl+C (SIGINT) for Windows in the legacy path.
<https://github.com/nautechsystems/nautilus_trader/issues/2785>.

For the new v2 system, `LiveNode` already supports Ctrl+C cleanly via `tokio::signal::ctrl_c()` and a
Python SIGINT bridge, so the runner stops and tasks are shut down cleanly.

Example pattern for Windows:

```python
try:
    node.run()
except KeyboardInterrupt:
    pass
finally:
    try:
        node.stop()
    finally:
        node.dispose()
```

## Execution reconciliation

Execution reconciliation is the process of aligning the external state of reality for orders and positions
(both closed and open) with the system's internal state built from events.
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
  - When `generate_missing_orders` is enabled (default: True), orders are generated with strategy ID `INTERNAL-DIFF` to align position discrepancies discovered during reconciliation.
  - A hierarchical price determination strategy ensures reconciliation can proceed even with limited data:
    1. **Calculated reconciliation price** (preferred): Uses the reconciliation price function to achieve target average positions
    2. **Market mid-price**: Falls back to current bid-ask midpoint if reconciliation price cannot be calculated
    3. **Current position average**: Uses existing position average price if no market data is available
    4. **MARKET order** (last resort): When no price information exists (no positions, no market data), a MARKET order is generated
  - LIMIT orders are used when a price can be determined (cases 1-3), ensuring accurate PnL calculations
  - MARKET orders are only used as a last resort when starting fresh with no available pricing data
  - Zero quantity differences after precision rounding are handled gracefully.
- **Exception Handling**:
  - Individual adapter failures do not abort the entire reconciliation process.
  - Missing order status reports are handled gracefully when fill reports arrive first.

If reconciliation fails, the system will not continue to start, and an error will be logged.

### Common reconciliation scenarios

The scenarios below are split between startup reconciliation (mass status) and runtime/continuous checks (in-flight order checks, open-order polls, and own-books audits).

#### Startup reconciliation

| Scenario                               | Description                                                                                       | System behavior                                                                  |
|----------------------------------------|---------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------|
| **Order state discrepancy**            | Local order state differs from venue (e.g., local shows `SUBMITTED`, venue shows `REJECTED`).     | Updates local order to match venue state and emits missing events.               |
| **Missed fills**                       | Venue fills an order but the engine misses the fill event.                                        | Generates missing `OrderFilled` events.                                          |
| **Multiple fills**                     | Order has multiple partial fills, some missed by the engine.                                      | Reconstructs complete fill history from venue reports.                           |
| **External orders**                    | Orders exist on venue but not in local cache (placed externally or from another system).          | Creates orders from venue reports; tags them `EXTERNAL`.                         |
| **Partially filled then canceled**     | Order partially filled then canceled by venue.                                                    | Updates order state to `CANCELED` while preserving fill history.                 |
| **Different fill data**                | Venue reports different fill price/commission than cached.                                        | Preserves cached fill data; logs discrepancies from reports.                     |
| **Filtered orders**                    | Orders marked for filtering via configuration.                                                    | Skips reconciliation based on `filtered_client_order_ids` or instrument filters. |
| **Duplicate client order IDs**         | Multiple orders with same client order ID in venue reports.                                       | Reconciliation fails to prevent state corruption.                                |
| **Position quantity mismatch (long)**  | Internal long position differs from external (e.g., internal: 100, external: 150).                | Generates BUY LIMIT order with calculated price when `generate_missing_orders=True`.          |
| **Position quantity mismatch (short)** | Internal short position differs from external (e.g., internal: -100, external: -150).             | Generates SELL LIMIT order with calculated price when `generate_missing_orders=True`.         |
| **Position reduction**                 | External position smaller than internal (e.g., internal: 150 long, external: 100 long).           | Generates opposite side LIMIT order with calculated price to reduce position.                                |
| **Position side flip**                 | Internal position opposite of external (e.g., internal: 100 long, external: 50 short).            | Generates LIMIT order with calculated price to close internal and open external position.                    |
| **INTERNAL-DIFF orders**               | Position reconciliation orders with strategy ID "INTERNAL-DIFF".                                  | Never filtered, regardless of `filter_unclaimed_external_orders`.                |

#### Runtime/continuous checks

| Scenario                               | Description                                                                 | System behavior                                                                                                        |
|----------------------------------------|-----------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------|
| **In-flight order timeout**            | In-flight order remains unconfirmed beyond threshold.                       | After `inflight_check_retries`, resolves to `REJECTED` to maintain consistent state.                                   |
| **Open orders check discrepancy**      | Periodic open-orders poll detects a state change at the venue.              | At `open_check_interval_secs`, confirms status (respecting `open_check_open_only`) and applies transitions if changed. |
| **Own books audit mismatch**           | Own order books diverge from venue public books.                            | At `own_books_audit_interval_secs`, audits and logs inconsistencies for investigation.                                 |

### Common reconciliation issues

- **Missing trade reports**: Some venues filter out older trades, causing incomplete reconciliation. Increase `reconciliation_lookback_mins` or ensure all events are cached locally.
- **Position mismatches**: If external orders predate the lookback window, positions may not align. Flatten the account before restarting the system to reset state.
- **Duplicate order IDs**: Duplicate client order IDs in mass status reports will cause reconciliation failure. Ensure venue data integrity or contact support.
- **Precision differences**: Small decimal differences in position quantities are handled automatically using instrument precision, but large discrepancies may indicate missing orders.
- **Out-of-order reports**: Fill reports arriving before order status reports are deferred until order state is available.

:::tip
For persistent reconciliation issues, consider dropping cached state or flattening accounts before system restart.
:::
