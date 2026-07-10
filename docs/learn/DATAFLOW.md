# System Dataflow

Message, event, and command flow through NautilusTrader, covering all phases from startup to shutdown.

---

## Message type taxonomy

### Trading Commands (`TradingCommand` enum)

| Command | Purpose |
|---------|---------|
| `SubmitOrder` | Submit a single order |
| `SubmitOrderList` | Submit an order list (OTO/OCO/OUO) |
| `ModifyOrder` | Modify a single order |
| `BatchModifyOrders` | Batch modify multiple orders |
| `CancelOrder` | Cancel a single order |
| `BatchCancelOrders` | Batch cancel multiple orders |
| `CancelAllOrders` | Cancel all orders for an instrument |
| `QueryOrder` | Query order status from venue |
| `QueryAccount` | Query account state from venue |

### Data Commands (`DataCommand` enum)

| Command | Variants |
|---------|----------|
| `Subscribe` | Quotes, Trades, Bars, BookDeltas, BookDepth10, BookSnapshots, Instruments, MarkPrices, IndexPrices, FundingRates, InstrumentStatus, InstrumentClose, OptionGreeks, OptionChain, CustomData |
| `Unsubscribe` | Corresponding unsubscribe for each above |
| `Request` | RequestBars, RequestQuotes, RequestTrades, RequestInstrument(s), RequestBookSnapshot, RequestBookDeltas, RequestBookDepth, RequestCustomData, RequestFundingRates, RequestForwardPrices |

### Order Events (`OrderEventAny` enum)

| Event | When |
|-------|------|
| `OrderInitialized` | Strategy creates the order locally |
| `OrderEmulated` | Order accepted by the local emulator |
| `OrderReleased` | Emulated order triggered, released to venue |
| `OrderDenied` | RiskEngine denies the order |
| `OrderSubmitted` | Order sent to venue |
| `OrderAccepted` | Venue acknowledges the order |
| `OrderRejected` | Venue rejects the order |
| `OrderCanceled` | Order canceled (locally or by venue) |
| `OrderExpired` | Order expired (GTD/GTT) |
| `OrderUpdated` | Order modified successfully |
| `OrderTriggered` | Stop/trigger condition met at venue |
| `OrderFilled` | Order filled (partial or full) |
| `OrderPendingUpdate` | Modify request inflight |
| `OrderPendingCancel` | Cancel request inflight |
| `OrderModifyRejected` | Venue rejected modification |
| `OrderCancelRejected` | Venue rejected cancellation |

### Execution Events (`ExecutionEvent` — channel messages in live mode)

| Variant | Content |
|---------|---------|
| `Order(OrderEventAny)` | Single order event from venue |
| `OrderSubmittedBatch(...)` | Batch submitted acknowledgment |
| `OrderAcceptedBatch(...)` | Batch accepted acknowledgment |
| `OrderCanceledBatch(...)` | Batch canceled acknowledgment |
| `Account(AccountState)` | Account balance/margin update |
| `Report(ExecutionReport)` | Reconciliation reports (OrderStatus, Fill, Position, MassStatus) |

### Position Events (`PositionEvent` enum)

| Event | When |
|-------|------|
| `PositionOpened` | First fill on a new instrument/account |
| `PositionChanged` | Subsequent fill changes quantity |
| `PositionClosed` | Position reduced to zero |

### System Messages

| Message | Topic |
|---------|-------|
| `ShutdownSystem` | `commands.system.shutdown` |
| `TradingStateChanged` | `events.risk` |

---

## Message bus routing

### Two Routing Mechanisms

1. **Typed routing** (high-perf, static dispatch) — `TopicRouter<T>` for pub/sub, `EndpointMap<T>` for point-to-point
2. **Any-based routing** (flexible, runtime dispatch) — for custom data, Python interop

### Endpoint Pattern

Each engine has two command entry points:

| Endpoint | Usage |
|----------|-------|
| `{Engine}.queue_execute` | Normal runtime — routes through async channel (live) or sync queue (backtest) |
| `{Engine}.execute` | Direct dispatch — used by the runner after draining channels |

### Key Topic Patterns

```
data.quotes.{venue}.{symbol}          → QuoteTick
data.trades.{venue}.{symbol}          → TradeTick
data.bars.{bar_type}                  → Bar
data.book.deltas.{venue}.{symbol}     → OrderBookDeltas
data.book.depth10.{venue}.{symbol}    → OrderBookDepth10
data.instrument.{venue}.{symbol}      → InstrumentAny
data.mark_prices.{venue}.{symbol}     → MarkPriceUpdate
data.index_prices.{venue}.{symbol}    → IndexPriceUpdate
data.funding_rates.{venue}.{symbol}   → FundingRateUpdate
data.option_greeks.{venue}.{symbol}   → OptionGreeks

events.order.{strategy_id}            → OrderEventAny
events.position.{strategy_id}         → PositionEvent
events.order.*                        → wildcard (RiskEngine)
events.position.*                     → wildcard (RiskEngine)
events.risk                           → TradingStateChanged
```

---

## Component dataflow (runtime)

### Strategy

**SENDS:**

| Message | → Destination | Via |
|---------|---------------|-----|
| `SubmitOrder` | `RiskEngine.queue_execute` | `msgbus::send_trading_command` |
| `SubmitOrder` (emulated) | `OrderEmulator.execute` | `msgbus::send_trading_command` |
| `SubmitOrder` (algo) | `{algo_id}.execute` | `msgbus::send_any` |
| `CancelOrder` | `ExecEngine.queue_execute` | `msgbus::send_trading_command` |
| `CancelOrder` (emulated) | `OrderEmulator.execute` | `msgbus::send_trading_command` |
| `ModifyOrder` | `RiskEngine.queue_execute` | `msgbus::send_trading_command` |
| `Subscribe/Unsubscribe` | `DataEngine.queue_execute` | `DataCommandSender` channel |
| `OrderInitialized` | `events.order.{strategy_id}` | `msgbus::publish_order_event` |

**RECEIVES:**

| Message | Topic | Via |
|---------|-------|-----|
| `OrderEventAny` | `events.order.{strategy_id}` | `subscribe_order_events` |
| `PositionEvent` | `events.position.{strategy_id}` | `subscribe_position_events` |
| `QuoteTick` | `data.quotes.{venue}.{symbol}` | typed router subscription |
| `TradeTick` | `data.trades.{venue}.{symbol}` | typed router subscription |
| `Bar` | `data.bars.{bar_type}` | typed router subscription |
| `OrderBookDeltas` | `data.book.deltas.{venue}.{symbol}` | typed router subscription |
| etc. | | |

---

### RiskEngine

**RECEIVES:**

| Message | Endpoint/Topic |
|---------|----------------|
| `TradingCommand` | `RiskEngine.execute` / `RiskEngine.queue_execute` |
| `OrderEventAny` | `events.order.*` (wildcard, priority=10) |
| `PositionEvent` | `events.position.*` (wildcard, priority=10) |

**SENDS:**

| Message | → Destination | When |
|---------|---------------|------|
| `TradingCommand` (approved) | `ExecEngine.queue_execute` | Order passes all risk checks, goes through throttler |
| `OrderDenied` | `ExecEngine.process` | Order fails risk validation |
| `OrderModifyRejected` | `ExecEngine.process` | Modify fails risk validation |
| `TradingStateChanged` | `events.risk` topic | Trading state changes (Active/Reducing/Halted) |

**Checks performed:**
- Instrument exists and is tradeable
- Price/quantity within valid ranges
- Sufficient balance/margin
- Position sizing limits
- Trading state enforcement (Active → pass, Reducing → only reducing, Halted → deny)
- Submit/modify rate throttling

---

### ExecutionEngine

**RECEIVES:**

| Message | Endpoint |
|---------|----------|
| `TradingCommand` | `ExecEngine.execute` / `ExecEngine.queue_execute` |
| `OrderEventAny` | `ExecEngine.process` |
| `ExecutionReport` | `ExecEngine.reconcile_execution_report` |
| `InstrumentAny` | `data.instrument.{venue}.*` (subscription) |

**SENDS:**

| Message | → Destination | When |
|---------|---------------|------|
| `client.submit_order()` | ExecutionClient | Routing command to venue |
| `client.cancel_order()` | ExecutionClient | Routing cancel to venue |
| `client.modify_order()` | ExecutionClient | Routing modify to venue |
| `OrderEventAny` | `events.order.{strategy_id}` | After processing any order event |
| `PositionEvent` | `events.position.{strategy_id}` | After fill creates/changes/closes position |
| `OrderFilled` | `Portfolio.update_order` | Fill triggers portfolio recalc |
| Position snapshots | `snapshots.position.{id}` | Periodic snapshots via timer |

---

### DataEngine

**RECEIVES:**

| Message | Endpoint |
|---------|----------|
| `DataCommand` | `DataEngine.execute` / `DataEngine.queue_execute` |
| Market data | `DataEngine.process_data` |
| Instruments | `DataEngine.process` |
| `DataResponse` | `DataEngine.response` |

**SENDS/PUBLISHES:**

| Data Type | Topic |
|-----------|-------|
| `QuoteTick` | `data.quotes.{venue}.{symbol}` |
| `TradeTick` | `data.trades.{venue}.{symbol}` |
| `Bar` | `data.bars.{bar_type}` |
| `OrderBookDeltas` | `data.book.deltas.{venue}.{symbol}` |
| `OrderBookDepth10` | `data.book.depth10.{venue}.{symbol}` |
| `InstrumentAny` | `data.instrument.{venue}.{symbol}` |
| `MarkPriceUpdate` | `data.mark_prices.{venue}.{symbol}` |
| `IndexPriceUpdate` | `data.index_prices.{venue}.{symbol}` |
| `FundingRateUpdate` | `data.funding_rates.{venue}.{symbol}` |
| `OptionGreeks` | `data.option_greeks.{venue}.{symbol}` |
| Subscribe/Unsubscribe | Forwarded to `DataClientAdapter` |

---

### OrderEmulator

**RECEIVES:**

| Message | Endpoint/Topic |
|---------|----------------|
| `TradingCommand` | `OrderEmulator.execute` |
| `QuoteTick` | `data.quotes.{venue}.{symbol}` (per instrument) |
| `TradeTick` | `data.trades.{venue}.{symbol}` (per instrument) |
| `OrderEventAny` | `events.order.{strategy_id}` (per strategy) |

**SENDS:**

| Message | → Destination | When |
|---------|---------------|------|
| `SubmitOrder` (released) | `RiskEngine.queue_execute` | Trigger condition met |
| `SubmitOrder` (algo) | `{algo_id}.execute` | Released order has exec_algorithm_id |
| `OrderEmulated` | `ExecEngine.process` | Order accepted by emulator |
| `OrderReleased` | strategy (via topic) | Order released to venue |
| `OrderCanceled` | strategy (via topic) | Emulated order canceled |
| `OrderUpdated` | strategy (via topic) | Emulated order modified |
| `Subscribe` | `DataEngine.queue_execute` | Need market data for triggers |

---

### ExecutionAlgorithm (e.g., TWAP)

**RECEIVES:**

| Message | Endpoint |
|---------|----------|
| `TradingCommand` | `{algo_id}.execute` |
| `OrderEventAny` | `events.order.{strategy_id}` (auto-subscribed) |
| `PositionEvent` | `events.position.{strategy_id}` (auto-subscribed) |
| `TimeEvent` | Timer callbacks for scheduled slices |

**SENDS:**

| Message | → Destination | When |
|---------|---------------|------|
| `SubmitOrder` (child) | `RiskEngine.queue_execute` | Spawning child orders |
| `CancelOrder` (child) | `ExecEngine.queue_execute` | Canceling remaining children |

---

### Portfolio

**RECEIVES:**

| Message | Endpoint |
|---------|----------|
| `AccountState` | `Portfolio.update_account` |
| `OrderFilled` | `Portfolio.update_order` |

---

### Execution Clients / Adapters (event producers)

**SENDS (via unbounded channel → `exec_evt_rx`):**

| Method | Channel Message |
|--------|----------------|
| `emit_order_submitted()` | `ExecutionEvent::Order(Submitted)` |
| `emit_order_accepted()` | `ExecutionEvent::Order(Accepted)` |
| `emit_order_rejected()` | `ExecutionEvent::Order(Rejected)` |
| `emit_order_canceled()` | `ExecutionEvent::Order(Canceled)` |
| `emit_order_expired()` | `ExecutionEvent::Order(Expired)` |
| `emit_order_filled()` | `ExecutionEvent::Order(Filled)` |
| `emit_order_updated()` | `ExecutionEvent::Order(Updated)` |
| `emit_order_triggered()` | `ExecutionEvent::Order(Triggered)` |
| `emit_account_state()` | `ExecutionEvent::Account(AccountState)` |
| `send_order_status_report()` | `ExecutionEvent::Report(Order)` |
| `send_fill_report()` | `ExecutionEvent::Report(Fill)` |
| `send_mass_status()` | `ExecutionEvent::Report(MassStatus)` |

---

## Live event loop

The `LiveNode` drives the system via a biased `select!` loop. Execution branches are polled **before** data branches so order actions are never delayed behind a market-data backlog.

**Priority order:**

| # | Channel/Source | Dispatches To |
|---|---------------|---------------|
| 1 | SIGINT/SIGTERM | Initiate shutdown |
| 2 | Stop check timer (100ms) | Check `should_stop()` |
| 3 | Shutdown deadline timer | Break out of loop |
| 4 | Open order/position report tasks | Reconciliation futures |
| 5 | Maintenance timer (100ms) | Reconciliation, purge, audit |
| 6 | `time_evt_rx` | `handler.run()` — fire timer callback |
| 7 | `exec_evt_rx` | `ExecEngine.process` / `Portfolio.update_account` / `ExecEngine.reconcile_execution_report` |
| 8 | `exec_cmd_rx` | `ExecEngine.execute` (with inflight tracking) |
| 9 | External msgbus | `BusMessage` from external ingress |
| 10 | `data_evt_rx` | `DataEngine.process` / `DataEngine.process_data` / `DataEngine.response` |
| 11 | `data_cmd_rx` | `DataEngine.execute` |

---

## Backtest flow

In backtest mode, everything is **synchronous** on a single thread:

```
BacktestEngine::run() loop:
    for each data point (chronological):
        1. Advance TestClock to data timestamp
        2. Flush accumulated time events
        3. DataEngine.process_data(data) → publish to subscribers
        4. drain_command_queues():
           - Drain data command queue → DataEngine.execute
           - Drain trading command queue → RiskEngine.execute / ExecEngine.execute
        5. SimulatedExchange.process() → matching engine fills
        6. Repeat until all data consumed
```

**Key differences from live:**
- No async channels — sync `VecDeque` queues via thread-local storage
- `SimulatedExchange` replaces real venue clients
- `TestClock` provides deterministic time
- Same msgbus endpoints, same event flow, just synchronous dispatch

---

## Startup phase

### Live Startup Sequence

```
1. Connect data clients
   └── Instruments arrive as buffered DataEvents

2. Flush pending data
   └── drain data_evt_rx + data_cmd_rx → populate cache with instruments

3. Connect execution clients
   └── load_instruments_from_cache() — instruments now available

4. Drain remaining events
   └── process any residual data/exec events

5. Run reconciliation
   └── mass status + open order + position checks vs. venue

6. Start strategies
   └── Trader.start() → strategy.on_start()

7. Enter select! loop
   └── Begin processing live events
```

### Backtest Startup Sequence

```
1. Initialize exchanges, accounts, open orders
2. Install sync command senders (thread-local VecDeque)
3. kernel.start() → kernel.start_trader() → start strategies
4. Enter data iteration loop
```

---

## Shutdown phase

### Live Shutdown

```
1. initiate_shutdown()
   └── Set ShuttingDown state, start deadline timer

2. Continue processing residual events from all channels

3. Deadline expires → break out of select! loop

4. finalize_stop()
   └── Disconnect clients, stop engines

5. drain_channels()
   └── Drain any remaining events

6. Kernel stop
```

### Backtest Shutdown

```
1. end() — flush remaining timer events to backtest end boundary
2. kernel.stop_trader() → strategy on_stop() callbacks
3. Drain residual command queues, settle venues one final time
4. Stop engines
```

---

## Complete order lifecycle

```
Strategy                RiskEngine           ExecEngine          ExecClient           Venue
   │                       │                     │                   │                  │
   │ submit_order()        │                     │                   │                  │
   │ cache order           │                     │                   │                  │
   │ publish Initialized   │                     │                   │                  │
   │──SubmitOrder────────►│                     │                   │                  │
   │                       │ validate order      │                   │                  │
   │                       │ check risk limits   │                   │                  │
   │                       │ throttle rate       │                   │                  │
   │                       │──SubmitOrder──────►│                   │                  │
   │                       │                     │ route to client   │                  │
   │                       │                     │──submit_order()─►│                  │
   │                       │                     │                   │──WebSocket/REST─►│
   │                       │                     │                   │                  │
   │                       │                     │                   │◄──Submitted ACK──│
   │                       │                     │◄─exec_evt channel─│                  │
   │                       │                     │ update cache      │                  │
   │◄──events.order.{sid}──│◄─events.order.*────│ publish Submitted │                  │
   │ on_order_submitted()  │                     │                   │                  │
   │                       │                     │                   │◄──Accepted───────│
   │                       │                     │◄─exec_evt channel─│                  │
   │◄──events.order.{sid}──│◄─events.order.*────│ publish Accepted  │                  │
   │ on_order_accepted()   │                     │                   │                  │
   │                       │                     │                   │◄──Filled─────────│
   │                       │                     │◄─exec_evt channel─│                  │
   │                       │                     │ update cache      │                  │
   │                       │                     │ update position   │                  │
   │                       │                     │─►Portfolio.update  │                  │
   │◄──events.order.{sid}──│◄─events.order.*────│ publish Filled    │                  │
   │ on_order_filled()     │                     │                   │                  │
   │◄─events.position.{sid}│                     │ publish PosOpened │                  │
   │ on_position_opened()  │                     │                   │                  │
```

---

## Emulated order flow

```
Strategy                OrderEmulator         RiskEngine          ExecEngine
   │                       │                     │                   │
   │ submit_order(         │                     │                   │
   │   emulation=BidAsk)   │                     │                   │
   │──SubmitOrder────────►│                     │                   │
   │                       │ accept, hold order  │                   │
   │                       │ subscribe to quotes │                   │
   │                       │──►DataEngine.sub()  │                   │
   │                       │                     │                   │
   │◄─OrderEmulated───────│──►ExecEngine.process │                   │
   │                       │                     │                   │
   │     ... time passes, market data arrives ... │                   │
   │                       │                     │                   │
   │                       │ trigger condition   │                   │
   │                       │ met (price crosses) │                   │
   │                       │                     │                   │
   │◄─OrderReleased───────│                     │                   │
   │                       │──SubmitOrder──────►│                   │
   │                       │                     │──SubmitOrder────►│
   │                       │                     │                   │ (normal flow)
```

---

## Execution algorithm flow (TWAP)

```
Strategy                TWAP Algorithm        RiskEngine          ExecEngine
   │                       │                     │                   │
   │ submit_order(         │                     │                   │
   │   exec_algo=TWAP,     │                     │                   │
   │   horizon=60s,        │                     │                   │
   │   interval=10s)       │                     │                   │
   │──SubmitOrder────────►│                     │                   │
   │                       │ on_order()          │                   │
   │                       │ calculate 6 slices  │                   │
   │                       │ set timer(10s)      │                   │
   │                       │                     │                   │
   │                       │ spawn_market(1/6)   │                   │
   │                       │──SubmitOrder──────►│                   │
   │                       │                     │──SubmitOrder────►│
   │                       │                     │                   │
   │     ... 10s timer fires ...                 │                   │
   │                       │                     │                   │
   │                       │ spawn_market(2/6)   │                   │
   │                       │──SubmitOrder──────►│                   │
   │                       │                     │──SubmitOrder────►│
   │                       │                     │                   │
   │     ... repeat 4 more times ...             │                   │
   │                       │                     │                   │
   │◄──OrderFilled (×6)───│◄─events.order.*────│ fills arrive      │
   │ (strategy sees all    │ (algo tracks        │                   │
   │  child fills)         │  remaining qty)     │                   │
```

---

## Data flow summary

```
                          ┌─────────────────────┐
                          │    Data Clients      │
                          │ (Binance, OKX, etc.) │
                          └─────────┬────────────┘
                                    │ DataEvent (channel in live,
                                    │            direct in backtest)
                                    ▼
                          ┌─────────────────────┐
                          │    DataEngine        │
                          │  process → cache     │
                          │  publish to topics   │
                          └─────────┬────────────┘
                                    │ typed pub/sub topics
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
              ┌──────────┐  ┌────────────┐  ┌────────────┐
              │ Strategy │  │  Emulator  │  │  Algorithm │
              │on_quote()│  │check trigger│ │  (unused)  │
              │on_trade()│  │            │  │            │
              └────┬─────┘  └────┬───────┘  └────────────┘
                   │             │
                   │ TradingCommand
                   ▼             ▼
              ┌─────────────────────┐
              │     RiskEngine      │
              │  validate → approve │
              │           → deny   │
              └─────────┬──────────┘
                        │ approved TradingCommand
                        ▼
              ┌─────────────────────┐
              │   ExecutionEngine   │
              │  route to client    │
              └─────────┬──────────┘
                        │
              ┌─────────┼──────────────┐
              ▼                        ▼
    ┌──────────────────┐    ┌─────────────────┐
    │MatchingEngine    │    │ExecutionClient   │
    │(backtest/paper)  │    │(live adapter)    │
    └────────┬─────────┘    └────────┬────────┘
             │                       │
             │ OrderEventAny         │ ExecutionEvent (channel)
             ▼                       ▼
    ┌──────────────────────────────────────────┐
    │            ExecutionEngine               │
    │  process event → update cache            │
    │  manage positions → publish events       │
    └──────────────────┬───────────────────────┘
                       │
            ┌──────────┼──────────┐
            ▼          ▼          ▼
      ┌──────────┐ ┌──────────┐ ┌──────────┐
      │ Strategy │ │RiskEngine│ │Portfolio  │
      │on_fill() │ │(monitor) │ │update PnL │
      └──────────┘ └──────────┘ └──────────┘
```
