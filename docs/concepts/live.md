# Live trading

Live trading in NautilusTrader enables traders to deploy their backtested strategies in a real-time
trading environment with no code changes. This seamless transition from backtesting to live trading
is a core feature of the platform, ensuring consistency and reliability. However, there are
key differences to be aware of between backtesting and live trading.

This guide provides an overview of the key aspects of live trading.

## Configuration

When operating a live trading system, configuring your execution engine and strategies properly is
essential for ensuring reliability, accuracy, and performance. The following is an overview of the
key concepts and settings involved for live configuration.

### Execution Engine configuration

The `LiveExecEngineConfig` sets up the live execution engine, managing order processing, execution events, and reconciliation with trading venues.
The following outlines the main configuration options.

By configuring these parameters thoughtfully, you can ensure that your trading system operates efficiently,
handles orders correctly, and remains resilient in the face of potential issues, such as lost events or conflicting data/information.

:::info
See also the `LiveExecEngineConfig` [API Reference](../api_reference/config#class-liveexecengineconfig) for further details.
:::

#### Reconciliation

**Purpose**: Ensures that the system state remains consistent with the trading venue by recovering any missed events, such as order and position status updates.

| Setting                        | Default | Description                                                                                       |
|--------------------------------|---------|---------------------------------------------------------------------------------------------------|
| `reconciliation`               | True    | Activates reconciliation at startup, aligning the system's internal state with the venue's state. |
| `reconciliation_lookback_mins` | None    | Specifies how far back (in minutes) the system requests past events to reconcile uncached state.  |

:::info
See also [Execution reconciliation](../concepts/execution#execution-reconciliation) for further details.
:::

#### Order filtering

**Purpose**: Manages which order events and reports should be processed by the system to avoid conflicts with other trading nodes and unnecessary data handling.

| Setting                            | Default | Description                                                                                                |
|------------------------------------|---------|------------------------------------------------------------------------------------------------------------|
| `filter_unclaimed_external_orders` | False   | Filters out unclaimed external orders to prevent irrelevant orders from impacting the strategy.            |
| `filter_position_reports`          | False   | Filters out position status reports, useful when multiple nodes trade the same account to avoid conflicts. |

#### In-flight order checks

**Purpose**: Regularly checks the status of in-flight orders (orders that have been submitted, modified or canceled but not yet confirmed) to ensure they are processed correctly and promptly.

| Setting                       | Default   | Description                                                                                                                         |
|-------------------------------|-----------|-------------------------------------------------------------------------------------------------------------------------------------|
| `inflight_check_interval_ms`  | 2,000 ms  | Determines how frequently the system checks in-flight order status.                                                                 |
| `inflight_check_threshold_ms` | 5,000 ms  | Sets the time threshold after which an in-flight order triggers a venue status check. Adjust if colocated to avoid race conditions. |
| `inflight_check_retries`      | 5 retries | Specifies the number of retry attempts the engine will make to verify the status of an in-flight order with the venue, should the initial attempt fail. |

If an in-flight order’s status cannot be reconciled after exhausting all retries, the system resolves it by generating one of these events based on its status:

- `SUBMITTED` -> `REJECTED`: Assumes the submission failed if no confirmation is received.
- `PENDING_UPDATE` -> `CANCELED`: Treats a pending modification as canceled if unresolved.
- `PENDING_CANCEL` -> `CANCELED`: Assumes cancellation if the venue doesn’t respond.

This ensures the trading node maintains a consistent execution state even under unreliable conditions.

#### Open order checks

**Purpose**: Regularly verifies the status of open orders matches the venue, triggering reconciliation if discrepancies are found.

| Setting                    | Default | Description                                                                                                                          |
|----------------------------|---------|--------------------------------------------------------------------------------------------------------------------------------------|
| `open_check_interval_secs` | None    | Determines how frequently (in seconds) open orders are checked at the venue. Recommended: 5-10 seconds, considering API rate limits. |
| `open_check_open_only`     | True    | When enabled, only open orders are requested during checks; if disabled, full order history is fetched (resource-intensive).         |

#### Order book audit

**Purpose**: Ensures that the internal representation of *own order* books matches the venues public order books.

| Setting                         | Default | Description                                                                                                                                         |
|---------------------------------|---------|-----------------------------------------------------------------------------------------------------------------------------------------------------|
| `own_books_audit_interval_secs` | None    | Sets the interval (in seconds) between audits of own order books against public ones. Verifies synchronization and logs errors for inconsistencies. |

#### Memory management

**Purpose**: Periodically purges closed orders, closed positions, and account events from the in-memory cache to optimize resource usage and performance during extended / HFT operations.

| Setting                                | Default | Description                                                                                                                             |
|----------------------------------------|---------|-----------------------------------------------------------------------------------------------------------------------------------------|
| `purge_closed_orders_interval_mins`    | None    | Sets how frequently (in minutes) closed orders are purged from memory. Recommended: 10-15 minutes. Does not affect database records. |
| `purge_closed_orders_buffer_mins`      | None    | Specifies how long (in minutes) an order must have been closed before purging. Recommended: 60 minutes to ensure processes complete. |
| `purge_closed_positions_interval_mins` | None    | Sets how frequently (in minutes) closed positions are purged from memory. Recommended: 10-15 minutes. Does not affect database records. |
| `purge_closed_positions_buffer_mins`   | None    | Specifies how long (in minutes) a position must have been closed before purging. Recommended: 60 minutes to ensure processes complete. |
| `purge_account_events_interval_mins`   | None    | Sets how frequently (in minutes) account events are purged from memory. Recommended: 10-15 minutes. Does not affect database records. |
| `purge_account_events_lookback_mins`   | None    | Specifies how long (in minutes) an account event must have occurred before purging. Recommended: 60 minutes. |

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
The following outlines the main configuration options.

:::info
See also the `StrategyConfig` [API Reference](../api_reference/config#class-strategyconfig) for further details.
:::

#### Strategy identification

**Purpose**: Provides unique identifiers for each strategy to prevent conflicts and ensure proper tracking of orders.

| Setting         | Default | Description                                                                                          |
|-----------------|---------|------------------------------------------------------------------------------------------------------|
| `strategy_id`   | None    | A unique ID for the strategy, ensuring it can be distinctly identified.                              |
| `order_id_tag`  | None    | A unique tag for the strategy's orders, differentiating them from multiple strategies.               |

#### Order Management System (OMS) type

**Purpose**: Defines how the order management system handles position IDs, influencing how orders are processed and tracked.

| Setting    | Default | Description                                                                                                                  |
|------------|---------|------------------------------------------------------------------------------------------------------------------------------|
| `oms_type` | None    | Specifies the [OMS type](/docs/concepts/execution.md#oms-configuration), for position ID handling and order processing flow. |

#### External order claims

**Purpose**: Enables the strategy to claim external orders based on specified instrument IDs, ensuring that relevant external orders are associated with the correct strategy.

| Setting                 | Default | Description                                                                                           |
|-------------------------|---------|-------------------------------------------------------------------------------------------------------|
| `external_order_claims` | None    | Lists instrument IDs for external orders the strategy should claim, aiding accurate order management. |

#### Contingent order management

**Purpose**: Automates the management of contingent orders, such as One-Updates-the-Other (OUO) and One-Cancels-the-Other (OCO) orders, ensuring they are handled correctly.

| Setting                    | Default | Description                                                                                          |
|----------------------------|---------|------------------------------------------------------------------------------------------------------|
| `manage_contingent_orders` | False   | If enabled, the strategy automatically manages contingent orders, reducing manual intervention.      |

#### Good Till Date (GTD) expiry management

**Purpose**: Ensures that orders with GTD time in force instructions are managed properly, with timers reactivated as necessary.

| Setting              | Default | Description                                                                                          |
|----------------------|---------|------------------------------------------------------------------------------------------------------|
| `manage_gtd_expiry`  | False   | If enabled, the strategy manages GTD expirations, ensuring orders remain active as intended.         |

## Execution reconciliation

Execution reconciliation is the process of aligning the external state of reality for orders and positions
(both closed and open) with the current system internal state built from events.
This process is primarily applicable to live trading, which is why only the `LiveExecutionEngine` has reconciliation capability.

There are two main scenarios for reconciliation:

- **Previous cached execution state**: Where cached execution state exists, information from reports is used to generate missing events to align the state
- **No previous cached execution state**: Where there is no cached state, all orders and positions that exist externally are generated from scratch

### Common reconciliation issues

- **Missing trade reports**: Some venues filter out older trades, causing incomplete reconciliation. Increase `reconciliation_lookback_mins` or ensure all events are cached locally.
- **Position mismatches**: If external orders predate the lookback window, positions may not align. Flatten the account before restarting the system to reset state.

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
a strategy to resume its operations and continue managing existing open orders at the venue for an instrument.

:::info
See the `LiveExecEngineConfig` [API Reference](../api_reference/config#class-liveexecengineconfig) for further details.
:::

### Reconciliation procedure

The reconciliation procedure is standardized for all adapter execution clients and uses the following
methods to produce an execution mass status:

- `generate_order_status_reports`
- `generate_fill_reports`
- `generate_position_status_reports`

The system state is then reconciled with the reports, which represent external "reality":

- **Duplicate Check**:
    - Check for duplicate order IDs and trade IDs.
- **Order Reconciliation**:
    - Generate and apply events necessary to update orders from any cached state to the current state.
    - If any trade reports are missing, inferred `OrderFilled` events are generated.
    - If any client order ID is not recognized or an order report lacks a client order ID, external order events are generated.
- **Position Reconciliation**:
    - Ensure the net position per instrument matches the position reports returned from the venue.
    - If the position state resulting from order reconciliation does not match the external state, external order events will be generated to resolve discrepancies.

If reconciliation fails, the system will not continue to start, and an error will be logged.

:::tip
The current reconciliation procedure can experience state mismatches if the lookback window is
misconfigured or if the venue omits certain order or trade reports due to filter conditions.

If you encounter reconciliation issues, drop any cached state or ensure the account is flat at
system shutdown and startup.
:::
