# Configure a Live Trading Node

Set up a `LiveNode` for live market connectivity. For the node lifecycle, see
[Live trading](../concepts/live.md). For command outcomes, see
[Execution](../concepts/execution.md#command-outcomes). For state recovery, see
[Execution reconciliation](../concepts/reconciliation.md).

:::danger[Jupyter notebooks not recommended for live trading]
Do not run live trading nodes in Jupyter notebooks. The node owns a long-running loop on the
calling thread, and notebook lifecycle controls make production operation unsafe:

- Cells can run out of order, kernels can crash, and state can disappear.
- Notebooks lack the logging, monitoring, and graceful shutdown needed for production trading.

Use Jupyter for backtesting, analysis, and experimentation. For live trading, run nodes
as standalone Python scripts or services.
:::

:::warning[One LiveNode per process]
Running multiple `LiveNode` instances concurrently in the same process is not supported due to global singleton state.
Add multiple strategies to a single node, or run additional nodes in separate processes for parallel execution.

See [Processes and threads](../concepts/architecture.md#processes-and-threads) for details.
:::

:::warning[Do not block the event loop]
User code on the event loop thread (strategy callbacks, actor handlers, and time event callbacks)
must return quickly. This applies to both Python and Rust. Blocking operations like model
inference, heavy calculations, or synchronous I/O cause missed fills, stale data, and
delayed order submissions. Offload long-running work to an executor or a separate thread/process.
:::

:::info[Platform differences]
Windows signal handling differs from Unix-like systems. If you are running on Windows, please read
the note on [Windows signal handling](#windows-signal-handling) for guidance on graceful shutdown
behavior and Ctrl+C (SIGINT) support.
:::

## LiveNodeConfig

`LiveNodeConfig` owns the node's core component settings. Register data and execution clients with
`LiveNode.builder(...)`, not through client dictionaries on this config. For background on config
defaults and `Option<T>` semantics, see
the [Configuration](../concepts/configuration.md) concept guide.

```python
from nautilus_trader.common import Environment
from nautilus_trader.common import LogLevel
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveNodeConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import PortfolioConfig
from nautilus_trader.model import TraderId

config = LiveNodeConfig(
    environment=Environment.LIVE,
    trader_id=TraderId.from_str("MY-TRADER-001"),
    logging=LoggerConfig(stdout_level=LogLevel.INFO),
    cache=CacheConfig(),
    msgbus=MessageBusConfig(),
    data_engine=LiveDataEngineConfig(),
    risk_engine=LiveRiskEngineConfig(),
    exec_engine=LiveExecEngineConfig(),
    portfolio=PortfolioConfig(),
)
```

### Core configuration parameters

| Setting                       | Default      | Description                                 |
| ----------------------------- | ------------ | ------------------------------------------- |
| `trader_id`                   | "TRADER-001" | Unique trader identifier (name‑tag format). |
| `instance_id`                 | `None`       | Optional unique instance identifier.        |
| `timeout_connection_secs`     | 60.0         | Connection timeout in seconds.              |
| `timeout_reconciliation_secs` | 30.0         | Reconciliation timeout in seconds.          |
| `timeout_portfolio_secs`      | 10.0         | Portfolio initialization timeout.           |
| `timeout_disconnection_secs`  | 10.0         | Disconnection timeout.                      |
| `delay_post_stop_secs`        | 10.0         | Delay for residual events after stopping.   |
| `timeout_shutdown_secs`       | 5.0          | Pending‑task shutdown timeout in seconds.   |

### Cache database configuration

Rust-native live systems keep cache behavior in `CacheConfig` and Redis connection settings in
`RedisCacheConfig`.

```rust
use nautilus_common::{
    cache::{CacheConfig, database::CacheDatabaseFactory},
    enums::SerializationEncoding,
};
use nautilus_infrastructure::redis::cache::RedisCacheConfig;

let config = CacheConfig {
    encoding: SerializationEncoding::MsgPack,
    timestamps_as_iso8601: true,
    buffer_interval_ms: Some(100),
    flush_on_start: false,
    ..Default::default()
};

let database = RedisCacheConfig {
    host: Some("localhost".to_string()),
    port: Some(6379),
    username: Some("nautilus".to_string()),
    password: Some("pass".to_string()),
    connection_timeout: 2,
    response_timeout: 2,
    ..Default::default()
};

let cache_database = database
    .create(trader_id, instance_id, config.clone())
    .await?;
```

Attach the adapter after building the Rust-native node and before starting it. The node restores
the database before reconciliation when `exec_engine.load_cache` is enabled, which is the default.

```rust
let node_config = LiveNodeConfig {
    trader_id,
    ..Default::default()
};
let mut node = LiveNode::build("LiveNode".to_string(), Some(node_config))?;
node.set_cache_database(cache_database)?;
node.run().await?;
```

Set `CacheConfig.flush_on_start = true` to clear the attached backing instead of restoring it.

Python injects the same database config through `LiveNodeBuilder`. The node constructs and owns the
adapter when it starts:

```python
from nautilus_trader.common import Environment
from nautilus_trader.infrastructure import RedisCacheConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId

node = (
    LiveNode.builder("LiveNode", TraderId("TRADER-001"), Environment.LIVE)
    .with_cache_database_factory(RedisCacheConfig(host="localhost", port=6379))
    .with_load_state(True)
    .with_save_state(True)
    .build()
)

try:
    node.run()
finally:
    node.dispose()
```

Pass `PostgresCacheConfig` instead to back the cache with Postgres. Any other object raises
`NotImplementedError` from `with_cache_database_factory`, and a failed database connection fails
`run()`.

`with_load_state` and `with_save_state` control actor and strategy state persistence, which requires
a Redis backing. The Postgres adapter backs cache state only: with registered actors or strategies,
`with_load_state(True)` fails when the trader starts, while `with_save_state(True)` fails when the
node stops or is disposed. On startup the kernel passes non‑empty persisted state to `on_load`; when
stopping or disposing the node it persists whatever `on_save` returns.

:::warning
State persistence is not continuous checkpointing. The kernel saves state at most once per run, so a
`SIGKILL` or a crash loses every change since the last save. Dispose the node so `dispose()` closes
the backing and flushes buffered writes; returning straight from `run()` can drop the final save.
:::

### MessageBus configuration

Message bus behavior stays in `MessageBusConfig`. Redis connection settings live in
`RedisMessageBusConfig`, which implements `MessageBusBackingFactory` and constructs the backing
from those settings.

```rust
use nautilus_common::{
    enums::SerializationEncoding,
    msgbus::{MessageBusBackingFactory, MessageBusConfig},
};
use nautilus_infrastructure::redis::msgbus::RedisMessageBusConfig;

let config = MessageBusConfig {
    encoding: SerializationEncoding::Json,
    timestamps_as_iso8601: true,
    use_instance_id: false,
    types_filter: Some(vec!["QuoteTick".to_string(), "TradeTick".to_string()]),
    stream_per_topic: false,
    autotrim_mins: Some(30),
    heartbeat_interval_secs: Some(1),
    ..Default::default()
};

let redis_config = RedisMessageBusConfig {
    connection_timeout: 2,
    response_timeout: 2,
    ..Default::default()
};

let backing = redis_config.create(trader_id, instance_id, config.clone())?;
```

Python injects the Redis config through `LiveNodeBuilder`:

```python
from nautilus_trader.common import Environment
from nautilus_trader.common import MessageBusConfig
from nautilus_trader.infrastructure import RedisMessageBusConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId

trader_id = TraderId("TRADER-001")
message_bus = MessageBusConfig(
    external_streams=["external-stream"],
    stream_per_topic=False,
)
redis_config = RedisMessageBusConfig(
    host="localhost",
    port=6379,
)
node = (
    LiveNode.builder("LiveNode", trader_id, Environment.LIVE)
    .with_msgbus_config(message_bus)
    .with_external_msgbus_factory(redis_config)
    .build()
)
node.run()
```

Existing code can continue passing `RedisMessageBusFactory(redis_config)` to
`with_external_msgbus_factory`.

`MessageBusConfig` alone does not install a backing. Pair it with a factory as shown above. The
factory always installs external egress, and calling `run()` also consumes the configured external
streams. Entries already in a stream before the node starts are not replayed. A host loop based on
`start()` and `poll()` does not service external message‑bus ingress; use `run()` when
`external_streams` is configured. See [message bus backing
configuration](../concepts/message_bus.md#backing-config) for lifecycle and ingress details.
External producers that write directly to Redis must supply the required `type` field. See
[external egress and ingress](../concepts/message_bus.md#external-egress-and-ingress) for the wire
fields and Python custom‑data registration.

## Multi-venue configuration

A node can connect to multiple clients. This example registers Binance spot and USD‑M futures data
clients before building the node:

```python
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceDataClientFactory
from nautilus_trader.adapters.binance import BinanceEnvironment
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId

node = (
    LiveNode.builder(
        "BINANCE-MULTI-CLIENT-001",
        TraderId.from_str("MULTI-VENUE-001"),
        Environment.LIVE,
    )
    .add_data_client(
        "BINANCE_SPOT",
        BinanceDataClientFactory(),
        BinanceDataClientConfig(
            product_type=BinanceProductType.SPOT,
            environment=BinanceEnvironment.LIVE,
        ),
    )
    .add_data_client(
        "BINANCE_FUTURES",
        BinanceDataClientFactory(),
        BinanceDataClientConfig(
            product_type=BinanceProductType.USD_M,
            environment=BinanceEnvironment.LIVE,
        ),
    )
    .build()
)
```

## ExecutionEngine configuration

`LiveExecEngineConfig` controls order processing, execution events, and
venue reconciliation. For full details see the
[API Reference](/docs/python-api-latest/live.html#nautilus_trader.live.LiveExecEngineConfig).

### Reconciliation

Recovers missed order and position events to keep system state consistent with the venue.

| Setting                         | Default | Description                                                                   |
| ------------------------------- | ------- | ----------------------------------------------------------------------------- |
| `reconciliation`                | True    | Activate reconciliation at startup to align internal state with the venue.    |
| `reconciliation_lookback_mins`  | None    | How far back (minutes) to request past events for reconciling uncached state. |
| `reconciliation_instrument_ids` | None    | Include list of instrument IDs to reconcile.                                  |
| `filtered_client_order_ids`     | None    | Client order IDs to skip during reconciliation (for venue‑side duplicates).   |

See [Execution reconciliation](../concepts/reconciliation.md) for details.

### Order filtering

Controls which order events and reports the system processes, preventing conflicts
across trading nodes.

| Setting                            | Default | Description                                                                 |
| ---------------------------------- | ------- | --------------------------------------------------------------------------- |
| `filter_unclaimed_external_orders` | False   | Drop unclaimed external orders so they do not affect the strategy.          |
| `filter_position_reports`          | False   | Drop position status reports. Useful when multiple nodes trade one account. |

:::note[Order tagging behavior]
Reconciliation tags orders by origin:

- **`VENUE` tag**: external orders discovered at the venue (placed outside this system).
- **`RECONCILIATION` tag**: synthetic orders generated to align position discrepancies.

When `filter_unclaimed_external_orders` is enabled, only `VENUE`-tagged orders are filtered.
`RECONCILIATION`-tagged orders are never filtered, so position alignment always succeeds.
:::

### Continuous reconciliation

Continuous reconciliation keeps runtime execution state aligned after startup by checking
in-flight orders, polling open orders, checking position status, and auditing own order books.
Configure the loop with these settings. For runtime state-transition rules, retry coordination,
and caveats, see [Runtime checks](../concepts/reconciliation.md#runtime-checks).

| Setting                              | Default        | Description                                                                                                                                                                                    |
| ------------------------------------ | -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `inflight_check_interval_ms`         | 2,000&nbsp;ms  | How often to check in‑flight order status. Set to 0 to disable.                                                                                                                                |
| `inflight_check_threshold_ms`        | 5,000&nbsp;ms  | Time before an in‑flight order triggers a venue status check. Lower if colocated.                                                                                                              |
| `inflight_check_retries`             | 5&nbsp;retries | Retry attempts to verify an in‑flight order with the venue.                                                                                                                                    |
| `open_check_interval_secs`           | None           | How often (seconds) to check open orders at the venue. None or 0.0 disables. Recommended: 5-10s.                                                                                               |
| `open_check_open_only`               | True           | When true, query only open orders; when false, fetch full history (resource‑intensive).                                                                                                        |
| `open_check_lookback_mins`           | 60&nbsp;min    | Lookback window (minutes) for order status polling. Only orders modified within this window.                                                                                                   |
| `open_check_threshold_ms`            | 5,000&nbsp;ms  | Minimum time since last cached event before acting on venue discrepancies.                                                                                                                     |
| `open_check_missing_retries`         | 5&nbsp;retries | Max retries before targeted not‑found resolution for eligible orders.                                                                                                                          |
| `max_single_order_queries_per_cycle` | 10             | Cap on single‑order queries per cycle. Prevents rate‑limit exhaustion.                                                                                                                         |
| `single_order_query_delay_ms`        | 100&nbsp;ms    | Delay (ms) between single‑order queries to avoid rate limits.                                                                                                                                  |
| `reconciliation_startup_delay_secs`  | 10.0&nbsp;s    | Delay (seconds) *after* startup reconciliation before continuous checks begin.                                                                                                                 |
| `own_books_audit_interval_secs`      | None           | Interval (seconds) between auditing own order books against public books.                                                                                                                      |
| `position_check_interval_secs`       | None           | Interval (seconds) between position consistency checks. On discrepancy, queries for missing fills. None disables. Recommended: 30-60s.                                                         |
| `position_check_lookback_mins`       | 60&nbsp;min    | Lookback window (minutes) for querying fill reports on position discrepancy.                                                                                                                   |
| `position_check_threshold_ms`        | 5,000&nbsp;ms  | Minimum time since last local activity before acting on position discrepancies.                                                                                                                |
| `position_check_retries`             | 3&nbsp;retries | Max attempts per instrument/account before the engine stops retrying that discrepancy. Once exceeded, an error is logged and the discrepancy is no longer actively reconciled until it clears. |

:::warning

- **`open_check_lookback_mins`**: do not reduce below 60 minutes. A short window
  triggers false "missing order" resolutions because orders fall outside the query range.
- **`open_check_threshold_ms`**: increase if venue timestamps lag the local clock, so
  recently updated orders are not marked missing prematurely.
- **`reconciliation_startup_delay_secs`**: do not reduce below 10 seconds in production.
  The delay lets the system stabilize after startup reconciliation before continuous
  checks begin.

:::

### Additional options

| Setting                            | Default | Description                                                                                                              |
| ---------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------ |
| `allow_overfills`                  | False   | Allow fills exceeding order quantity (logs warning). Useful when reconciliation races fills.                             |
| `generate_missing_orders`          | True    | Generate LIMIT orders during reconciliation to align position discrepancies (strategy `EXTERNAL`, tag `RECONCILIATION`). |
| `snapshot_positions_interval_secs` | None    | Interval (seconds) between position snapshots.                                                                           |
| `debug`                            | False   | Enable debug logging for execution.                                                                                      |

### Memory management

Periodically purges closed orders, closed positions, and account events from the
in-memory cache, keeping memory bounded during long-running or HFT sessions.

| Setting                                | Default | Description                                                                        |
| -------------------------------------- | ------- | ---------------------------------------------------------------------------------- |
| `purge_closed_orders_interval_mins`    | None    | How often (minutes) to purge closed orders from memory. Recommended: 10-15 min.    |
| `purge_closed_orders_buffer_mins`      | None    | How long (minutes) an order must be closed before purging. Recommended: 60 min.    |
| `purge_closed_positions_interval_mins` | None    | How often (minutes) to purge closed positions from memory. Recommended: 10-15 min. |
| `purge_closed_positions_buffer_mins`   | None    | How long (minutes) a position must be closed before purging. Recommended: 60 min.  |
| `purge_account_events_interval_mins`   | None    | How often (minutes) to purge account events from memory. Recommended: 10-15 min.   |
| `purge_account_events_lookback_mins`   | None    | How old (minutes) an account event must be before purging. Recommended: 60 min.    |

Setting an interval enables the purge loop; leaving it unset disables scheduling and deletion.
Each loop delegates to the cache APIs described in
[Cache](../concepts/cache.md).

## Strategy configuration

For a complete parameter list see the `StrategyConfig`
[API Reference](/docs/python-api-latest/trading.html#nautilus_trader.trading.StrategyConfig).

### Identification

| Setting        | Default | Description                                       |
| -------------- | ------- | ------------------------------------------------- |
| `strategy_id`  | None    | Unique strategy identifier.                       |
| `order_id_tag` | None    | Unique tag appended to this strategy's order IDs. |

### Order management

| Setting                     | Default | Description                                                                               |
| --------------------------- | ------- | ----------------------------------------------------------------------------------------- |
| `oms_type`                  | None    | [OMS type](../concepts/execution#oms-configuration) for position ID and order processing. |
| `use_uuid_client_order_ids` | False   | Use UUID4 values for client order IDs.                                                    |
| `external_order_claims`     | None    | Instrument IDs whose external orders and reconciliation activity this strategy claims.    |
| `manage_contingent_orders`  | False   | Automatically manage OTO, OCO, and OUO contingent orders.                                 |
| `manage_gtd_expiry`         | False   | Manage GTD expirations for orders.                                                        |

Read these runtime settings through `strategy.config`; the strategy itself does not duplicate
them as direct properties.

## Windows signal handling

`LiveNode` handles Ctrl+C (SIGINT) and, on Unix, SIGTERM in its Rust run loop.
The Python bridge also routes SIGINT into the same shutdown path, so runner and tasks shut down
cleanly.
