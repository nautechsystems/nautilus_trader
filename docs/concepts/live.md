# Live trading

:::info
We are currently working on this guide.
:::

Live trading in NautilusTrader enables traders to deploy their backtested strategies in a real-time 
trading environment with no code changes. This seamless transition from backtesting to live trading 
is a cornerstone of the platform's design, ensuring consistency and reliability. Even so, there are
still some key differences to be aware of between backtesting and live trading.

This guide provides an overview of the live trading process and its key aspects.

## Configuration

When operating a live trading system, configuring your execution engine and strategies properly is 
essential for ensuring performance, reliability, and accuracy. The following is an overview of the
key concepts and settings involved for live configuration.

### Execution Engine configuration

The `LiveExecEngineConfig` sets up the live execution engine, managing order processing, execution events, and reconciliation with trading venues. 
The following outlines the main configuration options.

:::info
See also the `LiveExecEngineConfig` [API Reference](../api_reference/config#class-liveexecengineconfig) for further details.
:::

#### Reconciliation
  
**Purpose**: Ensures that the system state remains consistent with the trading venue by recovering any missed events, such as order and position status updates.

**Settings**:
 - `reconciliation`: (default True): Activates reconciliation at startup, aligning the system's internal state with the execution venue's state.
 - `reconciliation_lookback_mins`: Specifies how far back (in minutes) the system should request past events to reconcile. This provides recovery for uncached execution state.

:::info
See also [Execution reconciliation](../concepts/execution#execution-reconciliation) for further details.
:::

#### Order filtering

**Purpose**: Manages which order events and reports should be processed by the system to avoid conflicts with other trading nodes and unnecessary data handling.

**Settings**:
 - `filter_unclaimed_external_orders`: (default False) Filters out unclaimed external orders, which can help prevent irrelevant external orders from impacting the strategy.
 - `filter_position_reports`: (default False) Filters out position status reports, which is useful in scenarios where multiple nodes are trading the same instruments on the same account, thus avoiding conflicting position data.

#### In-flight order checks

**Purpose**: Regularly checks the status of in-flight orders (orders that have been submitted, modified or canceled but not yet confirmed) to ensure they are processed correctly and promptly.

**Settings**:
- `inflight_check_interval_ms`: (default 2,000 ms) Determines how frequently the system checks the status of in-flight orders.
- `inflight_check_threshold_ms`: (default 5,000 ms) Sets the time threshold after which an in-flight order is considered for a status check with the venue. Adjusting this setting is particularly important if you are colocated with the venue, to avoid potential race conditions.

#### Queue management

**Purpose**: Handles the internal buffering of orders and events to ensure smooth processing and to prevent system overloads.

**Settings**:
 - `qsize` (default 100,000): Sets the size of the internal queue buffers, which helps in managing the flow of data within the engine.

### Strategy configuration

The `StrategyConfig` class outlines the configuration for trading strategies, ensuring that each strategy operates with the correct parameters and manages orders effectively.
The following outlines the main configuration options.

:::info
See also the `StrategyConfig` [API Reference](../api_reference/config#class-strategyconfig) for further details.
:::

#### Strategy identification

**Purpose**: Provides unique identifiers for each strategy to prevent conflicts and ensure proper tracking of orders.

**Settings**:
 - `strategy_id`: A unique ID for the strategy, ensuring it can be distinctly identified.
 - `order_id_tag`: A unique tag for the strategy's orders, differentiates orders from multiple strategies.

#### Order Management System (OMS) type
        
**Purpose**: Defines how the order management system handles position IDs, influencing how orders are processed and tracked.

**Settings**:
 - `oms_type`: Specifies the type of OMS, which dictates the handling of position IDs and impacts the overall order processing flow.

#### External order claims

**Purpose**: Enables the strategy to claim external orders based on specified instrument IDs, ensuring that relevant external orders are associated with the correct strategy.

**Settings**:
 - `external_order_claims`: Lists instrument IDs for external orders that the strategy should claim, helping to manage and track these orders accurately.

#### Contingent order management

**Purpose**: Automates the management of contingent orders, such as One-Updates-the-Other (OUO) and One-Cancels-the-Other (OCO) orders, ensuring they are handled correctly.

**Settings**:
 - `manage_contingent_orders`: (default False) If enabled, the strategy will automatically manage contingent orders, reducing the need for manual intervention.

#### GTD (Good-Till-Date) expiry management

**Purpose**: Ensures that orders with GTD time-in-force instructions are managed properly, with timers reactivated as necessary.

**Settings**:
 - `manage_gtd_expiry`: (default False) If enabled, the strategy will manage GTD expirations, ensuring that orders remain active as intended.

By configuring these parameters thoughtfully, you can ensure that your trading system operates efficiently,
handles orders correctly, and remains resilient in the face of potential issues, such as lost events or conflicting data.
