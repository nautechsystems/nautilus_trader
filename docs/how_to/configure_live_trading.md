# Configure a Live Trading Node

Set up a `TradingNode` for live market connectivity. For background on live
trading architecture and reconciliation, see the
[Live trading](../concepts/live.md) concept guide.

:::danger[Jupyter notebooks not recommended for live trading]
Do not run live trading nodes in Jupyter notebooks. Event loop conflicts and
operational risks make them unsuitable:

- Jupyter runs its own asyncio event loop, which conflicts with `TradingNode`'s event loop.
- Workarounds like `nest_asyncio` are not production-grade.
- Cells can run out of order, kernels can crash, and state can disappear.
- Notebooks lack the logging, monitoring, and graceful shutdown needed for production trading.

Use Jupyter for backtesting, analysis, and experimentation. For live trading, run nodes
as standalone Python scripts or services.
:::

:::warning[One TradingNode per process]
Running multiple `TradingNode` instances concurrently in the same process is not supported due to global singleton state.
Add multiple strategies to a single node, or run additional nodes in separate processes for parallel execution.

See [Processes and threads](../concepts/architecture.md#processes-and-threads) for details.
:::

:::warning[Do not block the event loop]
User code on the event loop thread (strategy callbacks, actor handlers, `on_event` methods)
must return quickly. This applies to both Python and Rust. Blocking operations like model
inference, heavy calculations, or synchronous I/O cause missed fills, stale data, and
delayed order submissions. Offload long-running work to an executor or a separate thread/process.
:::

:::info[Platform differences]
Windows signal handling differs from Unix-like systems. If you are running on Windows, please read
the note on [Windows signal handling](#windows-signal-handling) for guidance on graceful shutdown
behavior and Ctrl+C (SIGINT) support.
:::

## TradingNodeConfig

`TradingNodeConfig` inherits from `NautilusKernelConfig` and adds live-specific options.
For background on how config structs handle defaults and `Option<T>` semantics, see
the [Configuration](../concepts/configuration.md) concept guide.

```python
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    trader_id="MyTrader-001",

    # Component configurations
    cache=CacheConfig(),
    message_bus=MessageBusConfig(),
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

### Core configuration parameters

| Setting                  | Default      | Description                                 |
|--------------------------|--------------|---------------------------------------------|
| `trader_id`              | "TRADER-001" | Unique trader identifier (name‑tag format). |
| `instance_id`            | `None`       | Optional unique instance identifier.        |
| `timeout_connection`     | 30.0         | Connection timeout in seconds.              |
| `timeout_reconciliation` | 10.0         | Reconciliation timeout in seconds.          |
| `timeout_portfolio`      | 10.0         | Portfolio initialization timeout.           |
| `timeout_disconnection`  | 10.0         | Disconnection timeout.                      |
| `timeout_post_stop`      | 5.0          | Post‑stop cleanup timeout.                  |

### Cache database configuration

```python
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig

cache_config = CacheConfig(
    database=DatabaseConfig(
        host="localhost",
        port=6379,
        username="nautilus",
        password="pass",
        connection_timeout=2,
        response_timeout=2,
    ),
    encoding="msgpack",  # or "json"
    timestamps_as_iso8601=True,
    buffer_interval_ms=100,
    flush_on_start=False,
)
```

### MessageBus configuration

```python
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import DatabaseConfig

message_bus_config = MessageBusConfig(
    database=DatabaseConfig(
        connection_timeout=2,
        response_timeout=2,
    ),
    timestamps_as_iso8601=True,
    use_instance_id=False,
    types_filter=[QuoteTick, TradeTick],  # Filter specific message types
    stream_per_topic=False,
    autotrim_mins=30,  # Automatic message trimming
    heartbeat_interval_secs=1,
)
```

## Multi-venue configuration

A node can connect to multiple venues. This example configures both
spot and futures markets for Binance:

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

## ExecutionEngine configuration

`LiveExecEngineConfig` controls order processing, execution events, and
venue reconciliation. For full details see the
[API Reference](/docs/python-api-latest/config.html#nautilus_trader.live.config.LiveExecEngineConfig).

### Reconciliation

Recovers missed order and position events to keep system state consistent with the venue.

| Setting                         | Default | Description                                                                     |
|---------------------------------|---------|---------------------------------------------------------------------------------|
| `reconciliation`                | True    | Activate reconciliation at startup to align internal state with the venue.      |
| `reconciliation_lookback_mins`  | None    | How far back (minutes) to request past events for reconciling uncached state.   |
| `reconciliation_instrument_ids` | None    | Include list of instrument IDs to reconcile.                                    |
| `filtered_client_order_ids`     | None    | Client order IDs to skip during reconciliation (for venue‑side duplicates).     |

See [Execution reconciliation](../concepts/live.md#execution-reconciliation) for details.

### Order filtering

Controls which order events and reports the system processes, preventing conflicts
across trading nodes.

| Setting                            | Default | Description                                                                   |
|------------------------------------|---------|-------------------------------------------------------------------------------|
| `filter_unclaimed_external_orders` | False   | Drop unclaimed external orders so they do not affect the strategy.            |
| `filter_position_reports`          | False   | Drop position status reports. Useful when multiple nodes trade one account.   |

:::note[Order tagging behavior]
Reconciliation tags orders by origin:

- **`VENUE` tag**: external orders discovered at the venue (placed outside this system).
- **`RECONCILIATION` tag**: synthetic orders generated to align position discrepancies.

When `filter_unclaimed_external_orders` is enabled, only `VENUE`-tagged orders are filtered.
`RECONCILIATION`-tagged orders are never filtered, so position alignment always succeeds.
:::

### Continuous reconciliation

A background loop starts after startup reconciliation completes. It:

- Monitors in-flight orders for delays exceeding a configured threshold.
- Reconciles open orders with the venue at configurable intervals.
- Audits internal *own* order books against the venue's public books.

The loop waits for startup reconciliation to finish before starting periodic checks.
The `reconciliation_startup_delay_secs` parameter adds a further delay *after* startup
reconciliation completes, giving the system time to stabilize.

When retries are exhausted, the engine resolves the order as follows:

**In-flight order timeout resolution** (venue does not respond after max retries):

| Current status   | Resolved to | Rationale                                  |
|------------------|-------------|--------------------------------------------|
| `SUBMITTED`      | `REJECTED`  | No confirmation received from venue.       |
| `PENDING_UPDATE` | `CANCELED`  | Modification remains unacknowledged.       |
| `PENDING_CANCEL` | `CANCELED`  | Venue never confirmed the cancellation.    |

**Order consistency checks** (when cache state differs from venue state):

| Cache status       | Venue status | Resolution  | Rationale                                                           |
|--------------------|--------------|-------------|---------------------------------------------------------------------|
| `SUBMITTED`        | Not found    | `REJECTED`  | Order never confirmed by venue (e.g., lost during network error).   |
| `ACCEPTED`         | Not found    | `REJECTED`  | Order doesn't exist at venue, likely was never successfully placed. |
| `ACCEPTED`         | `CANCELED`   | `CANCELED`  | Venue canceled the order (user action or venue‑initiated).          |
| `ACCEPTED`         | `EXPIRED`    | `EXPIRED`   | Order reached GTD expiration at venue.                              |
| `ACCEPTED`         | `REJECTED`   | `REJECTED`  | Venue rejected after initial acceptance (rare but possible).        |
| `PARTIALLY_FILLED` | `CANCELED`   | `CANCELED`  | Order canceled at venue with fills preserved.                       |
| `PARTIALLY_FILLED` | Not found    | `CANCELED`  | Order doesn't exist but had fills (reconciles fill history).        |

:::note
**Reconciliation caveats:**

- **"Not found" resolutions** only apply in full-history mode (`open_check_open_only=False`).
  Open-only mode (the default) skips these checks because venue "open orders" endpoints
  exclude closed orders by design, making it impossible to distinguish missing orders from
  recently closed ones.
- **Recent order protection**: the engine skips reconciliation for orders whose last event
  falls within the `open_check_threshold_ms` window (default 5s). This prevents false
  positives from race conditions where the venue is still processing.
- **Targeted query safeguard**: before marking an order `REJECTED` or `CANCELED` when
  "not found", the engine issues a single-order query to the venue.
  This catches false negatives from bulk query limitations or timing delays.
- **`FILLED` orders** that are "not found" at the venue are silently ignored. Venues
  commonly drop completed orders from their query results.

:::

### Retry coordination and lookback behavior

The inflight loop and open-order loop share a single retry counter
(`_recon_check_retries`), bounded by `inflight_check_retries` and
`open_check_missing_retries` respectively. The stricter limit wins,
and avoids duplicate venue queries for the same order state.

When the open-order loop exhausts retries, the engine issues one targeted
`GenerateOrderStatusReport` probe before applying a terminal state. If the
venue returns the order, reconciliation proceeds and the retry counter resets.

**Single-order query protection**: the engine caps single-order queries per
cycle via `max_single_order_queries_per_cycle` (default: 10). Remaining
orders are deferred to the next cycle. A configurable delay
(`single_order_query_delay_ms`, default: 100ms) spaces out consecutive
queries to avoid rate limits. This handles bulk query failures across hundreds of orders
without overwhelming the venue API.

Orders older than `open_check_lookback_mins` rely on this targeted probe.
Keep the lookback generous for venues with short history windows. Increase
`open_check_threshold_ms` if venue timestamps lag the local clock, so
recently updated orders are not marked missing prematurely.

| Setting                              | Default        | Description                                                                                      |
|--------------------------------------|----------------|--------------------------------------------------------------------------------------------------|
| `inflight_check_interval_ms`         | 2,000&nbsp;ms  | How often to check in‑flight order status. Set to 0 to disable.                                  |
| `inflight_check_threshold_ms`        | 5,000&nbsp;ms  | Time before an in‑flight order triggers a venue status check. Lower if colocated.                |
| `inflight_check_retries`             | 5&nbsp;retries | Retry attempts to verify an in‑flight order with the venue.                                      |
| `open_check_interval_secs`           | None           | How often (seconds) to check open orders at the venue. None or 0.0 disables. Recommended: 5-10s.|
| `open_check_open_only`               | True           | When true, query only open orders; when false, fetch full history (resource‑intensive).          |
| `open_check_lookback_mins`           | 60&nbsp;min    | Lookback window (minutes) for order status polling. Only orders modified within this window.     |
| `open_check_threshold_ms`            | 5,000&nbsp;ms  | Minimum time since last cached event before acting on venue discrepancies.                       |
| `open_check_missing_retries`         | 5&nbsp;retries | Max retries before resolving an order open in cache but not found at venue.                      |
| `max_single_order_queries_per_cycle` | 10             | Cap on single‑order queries per cycle. Prevents rate‑limit exhaustion.                           |
| `single_order_query_delay_ms`        | 100&nbsp;ms    | Delay (ms) between single‑order queries to avoid rate limits.                                    |
| `reconciliation_startup_delay_secs`  | 10.0&nbsp;s    | Delay (seconds) *after* startup reconciliation before continuous checks begin.                   |
| `own_books_audit_interval_secs`      | None           | Interval (seconds) between auditing own order books against public books.                        |
| `position_check_interval_secs`       | None           | Interval (seconds) between position consistency checks. On discrepancy, queries for missing fills. None disables. Recommended: 30-60s. |
| `position_check_lookback_mins`       | 60&nbsp;min    | Lookback window (minutes) for querying fill reports on position discrepancy.                     |
| `position_check_threshold_ms`        | 5,000&nbsp;ms  | Minimum time since last local activity before acting on position discrepancies.                  |
| `position_check_retries`             | 3&nbsp;retries | Max attempts per instrument before the engine stops retrying that discrepancy. Once exceeded, an error is logged and the discrepancy is no longer actively reconciled until it clears. |

:::warning

- **`open_check_lookback_mins`**: do not reduce below 60 minutes. A short window
  triggers false "missing order" resolutions because orders fall outside the query range.
- **`reconciliation_startup_delay_secs`**: do not reduce below 10 seconds in production.
  The delay lets the system stabilize after startup reconciliation before continuous
  checks begin.

:::

### Additional options

| Setting                            | Default | Description                                                                                     |
|------------------------------------|---------|-------------------------------------------------------------------------------------------------|
| `allow_overfills`                  | False   | Allow fills exceeding order quantity (logs warning). Useful when reconciliation races fills.     |
| `generate_missing_orders`          | True    | Generate LIMIT orders during reconciliation to align position discrepancies (strategy `EXTERNAL`, tag `RECONCILIATION`). |
| `snapshot_orders`                  | False   | Take order snapshots on order events.                                                           |
| `snapshot_positions`               | False   | Take position snapshots on position events.                                                     |
| `snapshot_positions_interval_secs` | None    | Interval (seconds) between position snapshots.                                                  |
| `debug`                            | False   | Enable debug logging for execution.                                                             |

### Memory management

Periodically purges closed orders, closed positions, and account events from the
in-memory cache, keeping memory bounded during long-running or HFT sessions.

| Setting                                | Default | Description                                                                        |
|----------------------------------------|---------|------------------------------------------------------------------------------------|
| `purge_closed_orders_interval_mins`    | None    | How often (minutes) to purge closed orders from memory. Recommended: 10-15 min.    |
| `purge_closed_orders_buffer_mins`      | None    | How long (minutes) an order must be closed before purging. Recommended: 60 min.    |
| `purge_closed_positions_interval_mins` | None    | How often (minutes) to purge closed positions from memory. Recommended: 10-15 min. |
| `purge_closed_positions_buffer_mins`   | None    | How long (minutes) a position must be closed before purging. Recommended: 60 min.  |
| `purge_account_events_interval_mins`   | None    | How often (minutes) to purge account events from memory. Recommended: 10-15 min.   |
| `purge_account_events_lookback_mins`   | None    | How old (minutes) an account event must be before purging. Recommended: 60 min.    |
| `purge_from_database`                  | False   | Also delete from the backing database (Redis/PostgreSQL). **Use with caution**.    |

Setting an interval enables the purge loop; leaving it unset disables scheduling and
deletion. Database records are unaffected unless `purge_from_database` is true. Each
loop delegates to the cache APIs described in
[Cache](../concepts/cache.md).

### Queue management

| Setting                          | Default | Description                                                                     |
|----------------------------------|---------|---------------------------------------------------------------------------------|
| `qsize`                          | 100,000 | Size of internal queue buffers.                                                 |
| `graceful_shutdown_on_exception` | False   | Gracefully shut down on unexpected queue processing exceptions (not user code). |

## Strategy configuration

For a complete parameter list see the `StrategyConfig`
[API Reference](/docs/python-api-latest/config.html#nautilus_trader.trading.config.StrategyConfig).

### Identification

| Setting        | Default | Description                                                   |
|----------------|---------|---------------------------------------------------------------|
| `strategy_id`  | None    | Unique strategy identifier.                                   |
| `order_id_tag` | None    | Unique tag appended to this strategy's order IDs.             |

### Order management

| Setting                     | Default | Description                                                                                |
|-----------------------------|---------|--------------------------------------------------------------------------------------------|
| `oms_type`                  | None    | [OMS type](../concepts/execution#oms-configuration) for position ID and order processing.  |
| `use_uuid_client_order_ids` | False   | Use UUID4 values for client order IDs.                                                     |
| `external_order_claims`     | None    | Instrument IDs whose external orders this strategy claims.                                 |
| `manage_contingent_orders`  | False   | Automatically manage OTO, OCO, and OUO contingent orders.                                  |
| `manage_gtd_expiry`         | False   | Manage GTD expirations for orders.                                                         |

## Windows signal handling

:::warning
Windows: asyncio event loops do not implement `loop.add_signal_handler`. As a result,
`TradingNode` does not receive OS signals via asyncio on Windows. Use Ctrl+C (SIGINT) handling or
programmatic shutdown; SIGTERM parity is not expected on Windows.
:::

On Windows, asyncio event loops do not implement `loop.add_signal_handler`, so Unix-style
signal integration is unavailable. `TradingNode` does not receive OS signals via asyncio
on Windows and will not stop gracefully unless you intervene.

Recommended approaches:

- Wrap `run` with `try/except KeyboardInterrupt` and call `node.stop()` then `node.dispose()`.
  Ctrl+C raises `KeyboardInterrupt` in the main thread, giving you a clean teardown path.
- Publish a `ShutdownSystem` command programmatically (or call `shutdown_system(...)` from
  an actor/component) to trigger the same shutdown path.

The "inflight check loop task still pending" message appears because the normal graceful
shutdown path is not triggered. This is tracked as
[#2785](https://github.com/nautechsystems/nautilus_trader/issues/2785).

The v2 `LiveNode` already handles Ctrl+C via `tokio::signal::ctrl_c()` and a Python SIGINT
bridge, so runner and tasks shut down cleanly.

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
