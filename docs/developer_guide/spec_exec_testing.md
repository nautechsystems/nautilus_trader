# Execution Testing Spec

This section defines a rigorous test matrix for validating adapter execution
functionality using the `ExecTester` strategy. Both Python
(`nautilus_trader.test_kit.strategies.tester_exec`) and Rust
(`nautilus_testkit::testers`) provide the `ExecTester`. Each test case is
identified by a prefixed ID (e.g. TC-E01) and grouped by functionality.

**Each adapter must pass the subset of tests matching its supported capabilities.**

Tests progress from simple (single market order) to complex (brackets,
modification chains, rejection handling). An adapter that passes groups 1–5 is
considered baseline compliant. Data connectivity should be verified first using
the [Data Testing Spec](spec_data_testing.md).

Document adapter-specific behavior (how a venue simulates market orders,
handles TIF options, etc.) in the adapter's own guide, not here. Each adapter
guide should include a capability matrix showing which order types, time-in-force
options, actions, and flags it supports.

## Prerequisites

Before running execution tests:

- Demo/testnet account with valid API credentials (preferred, not required).
- Account funded with sufficient margin for the test instrument and quantities.
- Target instrument available and loadable via the instrument provider.
- Environment variables set: `{VENUE}_API_KEY`, `{VENUE}_API_SECRET` (or sandbox variants).
- If the venue offers a demo/testnet mode (e.g. `is_demo=True`), use credentials created
  for that environment. Demo and production API keys are typically separate and not
  interchangeable; using the wrong credentials produces authentication errors (e.g. HTTP 401).
- Risk engine bypassed (`LiveRiskEngineConfig(bypass=True)`) to avoid interference.
- Reconciliation enabled to verify state consistency.

**Python node setup** (reference: `examples/live/{adapter}/{adapter}_exec_tester.py`):

```python
from nautilus_trader.live.node import TradingNode
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester, ExecTesterConfig

node = TradingNode(config=config_node)
strategy = ExecTester(config=config_tester)
node.trader.add_strategy(strategy)
# Register adapter factories, build, and run
```

**Rust node setup** (reference: `crates/adapters/{adapter}/examples/node_exec_tester.rs`):

```rust
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};

let tester_config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, order_qty);
let tester = ExecTester::new(tester_config);
node.add_strategy(tester)?;
node.run().await?;
```

## Basic smoke test

A quick sanity check that can run at any time, for example after adapter changes or between
development iterations. The tester opens a position with a market order on start, places a
buy and sell post-only limit order, waits 30 seconds, then stops (cancelling open orders and
closing the position).

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.001"),
    open_position_on_start_qty=Decimal("0.001"),
    enable_limit_buys=True,
    enable_limit_sells=True,
    use_post_only=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, dec!(0.001))
    .with_open_position_on_start_qty(Some(dec!(0.001)))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(true)
    .with_use_post_only(true)
```

**Expected behavior:**

1. On start: market order fills, opening a position.
2. Two limit orders placed at `tob_offset_ticks` away from best bid/ask (default 500 ticks).
3. Strategy idles for 30 seconds. Check logs for errors, rejected orders, or disconnections.
4. On stop: open limit orders cancelled, position closed with a market order.

**Pass criteria:** No errors in logs, position opened and closed cleanly, limit orders
acknowledged by the venue.

---

Each group below begins with a summary table, followed by detailed test cards.
Test IDs use spaced numbering to allow insertion without renumbering.

---

## Group 1: Market orders

Test market order submission and fills. Market orders should execute immediately.

| TC     | Name                          | Description                                         | Skip when           |
|--------|-------------------------------|-----------------------------------------------------|---------------------|
| TC-E01 | Market BUY - submit and fill  | Open long position via market buy.                  | No market orders.   |
| TC-E02 | Market SELL - submit and fill | Open short position via market sell.                | No market orders.   |
| TC-E03 | Market order with IOC TIF     | Market order explicitly using IOC time in force.    | No IOC.             |
| TC-E04 | Market order with FOK TIF     | Market order explicitly using FOK time in force.    | No FOK.             |
| TC-E05 | Market order with quote qty   | Market order using quote currency quantity.         | No quote quantity.  |
| TC-E06 | Close position via market     | Close an open position with a market order on stop. | No market orders.   |

### TC-E01: Market BUY - submit and fill

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, market data flowing, no open position. |
| **Action**         | ExecTester opens a long position via `open_position_on_start_qty`.     |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Position opened with side=LONG, quantity matches config, fill price within market range, `AccountState` updated. |
| **Skip when**      | Adapter does not support market orders.                                |

**Considerations:**

- Some adapters simulate market orders as aggressive limit IOC orders (check adapter guide).
- The event sequence from the strategy's perspective should be identical regardless of the venue mechanism.
- Fill price should be within the recent bid/ask spread.
- Partial fills are valid; verify the cumulative filled quantity matches the order quantity.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
```

### TC-E02: Market SELL - submit and fill

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, market data flowing, no open position. |
| **Action**         | ExecTester opens a short position via negative `open_position_on_start_qty`. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Position opened with side=SHORT, quantity matches config, fill price within market range. |
| **Skip when**      | Adapter does not support market orders or short selling.               |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("-0.01"),
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(-1, 2)))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
```

### TC-E03: Market order with IOC TIF

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, market data flowing.             |
| **Action**         | Open position with `open_position_time_in_force=IOC`.                  |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Same as TC-E01; the IOC TIF is explicitly set on the order.            |
| **Skip when**      | No IOC support.                                                        |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
    open_position_time_in_force=TimeInForce.IOC,
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false);
config.open_position_time_in_force = TimeInForce::Ioc;
```

### TC-E04: Market order with FOK TIF

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, market data flowing.             |
| **Action**         | Open position with `open_position_time_in_force=FOK`.                  |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Same as TC-E01; the FOK TIF is explicitly set on the order.            |
| **Skip when**      | No FOK support.                                                        |

**Considerations:**

- FOK requires the entire quantity to be fillable immediately or the order is canceled.
- Use small test quantities so book depth is sufficient for a complete fill.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
    open_position_time_in_force=TimeInForce.FOK,
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false);
config.open_position_time_in_force = TimeInForce::Fok;
```

### TC-E05: Market order with quote quantity

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, adapter supports quote quantity. |
| **Action**         | Open position with `use_quote_quantity=True`, quantity in quote currency. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Order submitted with quote currency quantity; fill quantity is in base currency. |
| **Skip when**      | Adapter does not support quote quantity orders.                        |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("100.0"),  # Quote currency amount
    open_position_on_start_qty=Decimal("100.0"),
    use_quote_quantity=True,
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("100"))
    .with_open_position_on_start(Some(Decimal::from(100)))
    .with_use_quote_quantity(true)
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
```

### TC-E06: Close position via market order on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open position from TC-E01 or TC-E02.                                   |
| **Action**         | Stop the strategy; ExecTester closes position via market order.        |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled` (closing order). |
| **Pass criteria**  | Position closed (net quantity = 0), no open orders remaining.          |
| **Skip when**      | Adapter does not support market orders.                                |

**Considerations:**

- This test naturally follows TC-E01 or TC-E02 as part of the same session.
- `close_positions_on_stop=True` is the default.
- The closing order should be on the opposite side of the position.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
    close_positions_on_stop=True,
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
    .with_close_positions_on_stop(true)
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
```

---

## Group 2: Limit orders

Test limit order submission, acceptance, and behavior across time-in-force options.

| TC     | Name                       | Description                                      | Skip when          |
|--------|----------------------------|--------------------------------------------------|--------------------|
| TC-E10 | Limit BUY GTC              | Place GTC limit buy below TOB, verify accepted.  | Never.             |
| TC-E11 | Limit SELL GTC             | Place GTC limit sell above TOB, verify accepted. | Never.             |
| TC-E12 | Limit BUY and SELL pair    | Both sides simultaneously, verify both accepted. | Never.             |
| TC-E13 | Limit IOC aggressive fill  | Limit IOC at aggressive price, expect fill.      | No IOC.            |
| TC-E14 | Limit IOC passive no fill  | Limit IOC away from market, expect cancel.       | No IOC.            |
| TC-E15 | Limit FOK fill             | Limit FOK at aggressive price, expect fill.      | No FOK.            |
| TC-E16 | Limit FOK no fill          | Limit FOK away from market, expect cancel.       | No FOK.            |
| TC-E17 | Limit GTD                  | Limit with expiry time, verify accepted.         | No GTD.            |
| TC-E18 | Limit GTD expiry           | Wait for GTD expiry, verify `OrderExpired`.      | No GTD.            |
| TC-E19 | Limit DAY                  | Limit with DAY TIF, verify accepted.             | No DAY.            |

### TC-E10: Limit BUY GTC - submit and accept

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places a limit buy at `best_bid - tob_offset_ticks`.        |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order is open on the venue with correct price, quantity, side=BUY, TIF=GTC. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- The `tob_offset_ticks` (default 500) places the order well away from the market to avoid accidental fills.
- Verify the order appears in the cache with `OrderStatus.ACCEPTED`.
- The order should remain open until explicitly canceled.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false)
```

### TC-E11: Limit SELL GTC - submit and accept

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places a limit sell at `best_ask + tob_offset_ticks`.       |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order is open on the venue with correct price, quantity, side=SELL, TIF=GTC. |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(true)
```

### TC-E12: Limit BUY and SELL pair

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places both a limit buy and limit sell.                     |
| **Event sequence** | Two independent sequences: each `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`. |
| **Pass criteria**  | Both orders open on venue, buy below bid, sell above ask.              |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(true)
```

### TC-E13: Limit IOC aggressive fill

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Submit a limit buy IOC at or above the best ask (aggressive price).    |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Order fills immediately; position opened.                              |
| **Skip when**      | Adapter does not support IOC TIF.                                      |

**Considerations:**

- This test requires manual order creation or adapter-specific configuration, as the ExecTester's
  default limit order placement uses GTC TIF.
- IOC orders that don't fill immediately are canceled by the venue.

### TC-E14: Limit IOC passive - no fill

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Submit a limit buy IOC well below the market (passive price).          |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderCanceled`. |
| **Pass criteria**  | Order is immediately canceled by venue with no fill.                   |
| **Skip when**      | Adapter does not support IOC TIF.                                      |

**Considerations:**

- The venue should cancel the unfilled IOC order; verify `OrderCanceled` event (not `OrderExpired`).

### TC-E15: Limit FOK fill

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing, sufficient book depth. |
| **Action**         | Submit a limit buy FOK at aggressive price with quantity within top-of-book depth. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Order fills completely in a single fill event.                         |
| **Skip when**      | Adapter does not support FOK TIF.                                      |

**Considerations:**

- FOK requires the entire quantity to be fillable; use small quantities so book depth is sufficient.

### TC-E16: Limit FOK no fill

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Submit a limit buy FOK at passive price (well below market).           |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderCanceled`. |
| **Pass criteria**  | Order is immediately canceled by venue with no fill.                   |
| **Skip when**      | Adapter does not support FOK TIF.                                      |

### TC-E17: Limit GTD - submit and accept

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Place limit buy with `order_expire_time_delta_mins` set (e.g., 60 minutes). |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order accepted with GTD TIF and correct expiry timestamp.              |
| **Skip when**      | Adapter does not support GTD TIF.                                      |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    order_expire_time_delta_mins=60,
    enable_limit_buys=True,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false);
config.order_expire_time_delta_mins = Some(60);
```

### TC-E18: Limit GTD expiry

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open GTD limit order from TC-E17 (or use a very short expiry).         |
| **Action**         | Wait for the GTD expiry time to elapse.                                |
| **Event sequence** | `OrderExpired`.                                                        |
| **Pass criteria**  | Order transitions to expired status; `OrderExpired` event received.    |
| **Skip when**      | Adapter does not support GTD TIF.                                      |

**Considerations:**

- Use a short `order_expire_time_delta_mins` (e.g., 1–2 minutes) to avoid long waits.
- Some venues may report expiry as a cancel; verify the adapter maps this to `OrderExpired`.

### TC-E19: Limit DAY - submit and accept

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, market is in trading hours.      |
| **Action**         | Submit limit buy with DAY TIF.                                         |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order accepted with DAY TIF; will be automatically canceled at end of trading day. |
| **Skip when**      | Adapter does not support DAY TIF.                                      |

**Considerations:**

- DAY orders may behave differently on 24/7 crypto venues vs traditional markets.
- Verify behavior when submitted outside trading hours (if applicable).

---

## Group 3: Stop and conditional orders

Test stop and conditional order types. These orders rest on the venue until a trigger condition is met.

| TC     | Name                   | Description                                           | Skip when           |
|--------|------------------------|-------------------------------------------------------|---------------------|
| TC-E20 | StopMarket BUY         | Stop buy above ask, verify accepted.                  | No `STOP_MARKET`.   |
| TC-E21 | StopMarket SELL        | Stop sell below bid, verify accepted.                 | No `STOP_MARKET`.   |
| TC-E22 | StopLimit BUY          | Stop-limit buy with trigger + limit price.            | No `STOP_LIMIT`.    |
| TC-E23 | StopLimit SELL         | Stop-limit sell with trigger + limit price.           | No `STOP_LIMIT`.    |
| TC-E24 | MarketIfTouched BUY    | MIT buy below bid.                                    | No `MIT`.           |
| TC-E25 | MarketIfTouched SELL   | MIT sell above ask.                                   | No `MIT`.           |
| TC-E26 | LimitIfTouched BUY     | LIT buy with trigger + limit price.                   | No `LIT`.           |
| TC-E27 | LimitIfTouched SELL    | LIT sell with trigger + limit price.                  | No `LIT`.           |

### TC-E20: StopMarket BUY

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places a stop-market buy above the current ask.             |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Stop order accepted on venue with correct trigger price and side=BUY.  |
| **Skip when**      | Adapter does not support `StopMarket` orders.                          |

**Considerations:**

- The trigger price should be above the current ask by `stop_offset_ticks`.
- The order should NOT trigger immediately (trigger price is above market).
- Verifying trigger and fill requires the market to move, which may not happen during the test.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=False,
    enable_stop_buys=True,
    enable_stop_sells=False,
    stop_order_type=OrderType.STOP_MARKET,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
    .with_enable_stop_buys(true)
    .with_enable_stop_sells(false)
    .with_stop_order_type(OrderType::StopMarket)
```

### TC-E21: StopMarket SELL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places a stop-market sell below the current bid.            |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Stop order accepted on venue with correct trigger price and side=SELL. |
| **Skip when**      | Adapter does not support `StopMarket` orders.                          |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=False,
    enable_stop_buys=False,
    enable_stop_sells=True,
    stop_order_type=OrderType.STOP_MARKET,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
    .with_enable_stop_buys(false)
    .with_enable_stop_sells(true)
    .with_stop_order_type(OrderType::StopMarket)
```

### TC-E22: StopLimit BUY

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places a stop-limit buy with trigger price above ask and limit offset. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Stop-limit order accepted with correct trigger price, limit price, and side=BUY. |
| **Skip when**      | Adapter does not support `StopLimit` orders.                           |

**Considerations:**

- Requires `stop_limit_offset_ticks` to be set for the limit price offset from the trigger price.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=False,
    enable_stop_buys=True,
    enable_stop_sells=False,
    stop_order_type=OrderType.STOP_LIMIT,
    stop_limit_offset_ticks=50,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
    .with_enable_stop_buys(true)
    .with_enable_stop_sells(false)
    .with_stop_order_type(OrderType::StopLimit);
config.stop_limit_offset_ticks = Some(50);
```

### TC-E23: StopLimit SELL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places a stop-limit sell with trigger price below bid.      |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Stop-limit order accepted with correct trigger price, limit price, and side=SELL. |
| **Skip when**      | Adapter does not support `StopLimit` orders.                           |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=False,
    enable_stop_buys=False,
    enable_stop_sells=True,
    stop_order_type=OrderType.STOP_LIMIT,
    stop_limit_offset_ticks=50,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
    .with_enable_stop_buys(false)
    .with_enable_stop_sells(true)
    .with_stop_order_type(OrderType::StopLimit);
config.stop_limit_offset_ticks = Some(50);
```

### TC-E24: MarketIfTouched BUY

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Place MIT buy with trigger below current bid (buy on dip).             |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | MIT order accepted on venue with correct trigger price.                |
| **Skip when**      | Adapter does not support `MarketIfTouched` orders.                     |

### TC-E25: MarketIfTouched SELL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Place MIT sell with trigger above current ask (sell on rally).         |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | MIT order accepted on venue with correct trigger price.                |
| **Skip when**      | Adapter does not support `MarketIfTouched` orders.                     |

### TC-E26: LimitIfTouched BUY

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Place LIT buy with trigger below bid and limit price offset.           |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | LIT order accepted with correct trigger price and limit price.         |
| **Skip when**      | Adapter does not support `LimitIfTouched` orders.                      |

### TC-E27: LimitIfTouched SELL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | Place LIT sell with trigger above ask and limit price offset.          |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | LIT order accepted with correct trigger price and limit price.         |
| **Skip when**      | Adapter does not support `LimitIfTouched` orders.                      |

---

## Group 4: Order modification

Test order modification (amend) and cancel-replace workflows.

| TC    | Name                         | Description                                         | Skip when                   |
|-------|------------------------------|-----------------------------------------------------|-----------------------------|
| TC-E30 | Modify limit BUY price       | Amend open limit buy to new price.                  | No modify support.          |
| TC-E31 | Modify limit SELL price      | Amend open limit sell to new price.                 | No modify support.          |
| TC-E32 | Cancel-replace limit BUY     | Cancel and resubmit limit buy at new price.         | Never.                      |
| TC-E33 | Cancel-replace limit SELL    | Cancel and resubmit limit sell at new price.        | Never.                      |
| TC-E34 | Modify stop trigger price    | Amend stop order trigger price.                     | No modify or no stop.       |
| TC-E35 | Cancel-replace stop order    | Cancel and resubmit stop at new trigger price.      | No stop orders.             |
| TC-E36 | Modify rejected              | Modify on unsupported adapter.                      | Adapter supports modify.    |

### TC-E30: Modify limit BUY price

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open GTC limit buy from TC-E10.                                        |
| **Action**         | ExecTester modifies limit buy to a new price as market moves (`modify_orders_to_maintain_tob_offset=True`). |
| **Event sequence** | `OrderPendingUpdate` → `OrderUpdated`.                                 |
| **Pass criteria**  | Order price updated on venue; `OrderUpdated` event contains new price. |
| **Skip when**      | Adapter does not support order modification.                           |

**Considerations:**

- Requires market movement to trigger the ExecTester's order maintenance logic.
- The modify is triggered when the order price drifts from the target TOB offset.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=False,
    modify_orders_to_maintain_tob_offset=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false);
config.modify_orders_to_maintain_tob_offset = true;
```

### TC-E31: Modify limit SELL price

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open GTC limit sell from TC-E11.                                       |
| **Action**         | ExecTester modifies limit sell to new price as market moves.           |
| **Event sequence** | `OrderPendingUpdate` → `OrderUpdated`.                                 |
| **Pass criteria**  | Order price updated on venue; `OrderUpdated` event contains new price. |
| **Skip when**      | Adapter does not support order modification.                           |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=True,
    modify_orders_to_maintain_tob_offset=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(true);
config.modify_orders_to_maintain_tob_offset = true;
```

### TC-E32: Cancel-replace limit BUY

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open GTC limit buy.                                                    |
| **Action**         | ExecTester cancels and resubmits limit buy at new price as market moves. |
| **Event sequence** | `OrderPendingCancel` → `OrderCanceled` → `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`. |
| **Pass criteria**  | Original order canceled, new order accepted at updated price.          |
| **Skip when**      | Never (cancel-replace is always available).                            |

**Considerations:**

- This is the universal alternative when the adapter does not support native modify.
- Two distinct orders in the cache: the canceled original and the new replacement.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=False,
    cancel_replace_orders_to_maintain_tob_offset=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false);
config.cancel_replace_orders_to_maintain_tob_offset = true;
```

### TC-E33: Cancel-replace limit SELL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open GTC limit sell.                                                   |
| **Action**         | ExecTester cancels and resubmits limit sell at new price.              |
| **Event sequence** | `OrderPendingCancel` → `OrderCanceled` → `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`. |
| **Pass criteria**  | Original order canceled, new order accepted at updated price.          |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=False,
    enable_limit_sells=True,
    cancel_replace_orders_to_maintain_tob_offset=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(true);
config.cancel_replace_orders_to_maintain_tob_offset = true;
```

### TC-E34: Modify stop trigger price

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open stop order from TC-E20 or TC-E22.                                 |
| **Action**         | ExecTester modifies stop trigger price as market moves (`modify_stop_orders_to_maintain_offset=True`). |
| **Event sequence** | `OrderPendingUpdate` → `OrderUpdated`.                                 |
| **Pass criteria**  | Stop order trigger price updated on venue.                             |
| **Skip when**      | Adapter does not support modify, or no stop order support.             |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_stop_buys=True,
    modify_stop_orders_to_maintain_offset=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_stop_buys(true);
config.modify_stop_orders_to_maintain_offset = true;
```

### TC-E35: Cancel-replace stop order

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open stop order.                                                       |
| **Action**         | ExecTester cancels and resubmits stop at new trigger price.            |
| **Event sequence** | `OrderPendingCancel` → `OrderCanceled` → `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`. |
| **Pass criteria**  | Original stop canceled, new stop accepted at updated trigger price.    |
| **Skip when**      | No stop order support.                                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_stop_buys=True,
    cancel_replace_stop_orders_to_maintain_offset=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_stop_buys(true);
config.cancel_replace_stop_orders_to_maintain_offset = true;
```

### TC-E36: Modify rejected

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open limit order, adapter does NOT support modify.                     |
| **Action**         | Attempt to modify the order (programmatically, not via ExecTester auto-maintain). |
| **Event sequence** | `OrderModifyRejected`.                                                 |
| **Pass criteria**  | Modify attempt results in `OrderModifyRejected` event with reason; original order remains unchanged. |
| **Skip when**      | Adapter supports order modification.                                   |

**Considerations:**

- This tests the adapter's rejection path, not the ExecTester's cancel-replace logic.
- The rejection reason should indicate that modification is not supported.

---

## Group 5: Order cancellation

Test order cancellation workflows.

| TC    | Name                       | Description                                          | Skip when            |
|-------|----------------------------|------------------------------------------------------|----------------------|
| TC-E40 | Cancel single limit order  | Cancel an open limit order.                          | Never.               |
| TC-E41 | Cancel all on stop         | Strategy stop cancels all open orders (default).     | Never.               |
| TC-E42 | Individual cancels on stop | Cancel orders one-by-one on stop.                    | Never.               |
| TC-E43 | Batch cancel on stop       | Cancel orders via batch API on stop.                 | No batch cancel.     |
| TC-E44 | Cancel already-canceled    | Cancel a non-open order.                             | Never.               |

### TC-E40: Cancel single limit order

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open GTC limit order from TC-E10 or TC-E11.                            |
| **Action**         | Stop the strategy; ExecTester cancels the open limit order.            |
| **Event sequence** | `OrderPendingCancel` → `OrderCanceled`.                                |
| **Pass criteria**  | Order status transitions to CANCELED; no open orders remaining.        |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- `cancel_orders_on_stop=True` (default) triggers cancellation when the strategy stops.
- Verify the `OrderCanceled` event contains the correct `venue_order_id`.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=False,
    cancel_orders_on_stop=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false)
    .with_cancel_orders_on_stop(true)
```

### TC-E41: Cancel all on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Multiple open orders (limit buy + limit sell from TC-E12).             |
| **Action**         | Stop the strategy with `cancel_orders_on_stop=True` (default).         |
| **Event sequence** | For each order: `OrderPendingCancel` → `OrderCanceled`.                |
| **Pass criteria**  | All open orders canceled; no open orders remaining.                    |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=True,
    cancel_orders_on_stop=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(true)
    .with_cancel_orders_on_stop(true)
```

### TC-E42: Individual cancels on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Multiple open orders.                                                  |
| **Action**         | Stop with `use_individual_cancels_on_stop=True`.                       |
| **Event sequence** | Individual `OrderPendingCancel` → `OrderCanceled` for each order.      |
| **Pass criteria**  | Each order canceled individually; all orders reach CANCELED status.    |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=True,
    use_individual_cancels_on_stop=True,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(true);
config.use_individual_cancels_on_stop = true;
```

### TC-E43: Batch cancel on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Multiple open orders, adapter supports batch cancel.                   |
| **Action**         | Stop with `use_batch_cancel_on_stop=True`.                             |
| **Event sequence** | Batch `OrderPendingCancel` → `OrderCanceled` for all orders.           |
| **Pass criteria**  | All orders canceled via single batch request; all reach CANCELED status. |
| **Skip when**      | Adapter does not support batch cancel.                                 |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=True,
    use_batch_cancel_on_stop=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(true)
    .with_use_batch_cancel_on_stop(true)
```

### TC-E44: Cancel already-canceled order

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | A previously canceled order (from TC-E40).                             |
| **Action**         | Attempt to cancel the same order again.                                |
| **Event sequence** | `OrderCancelRejected`.                                                 |
| **Pass criteria**  | Cancel attempt is rejected; `OrderCancelRejected` event received with reason. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- This tests the adapter's error handling for invalid cancel requests.
- The rejection reason should indicate the order is not in a cancelable state.

---

## Group 6: Bracket orders

Test bracket order submission (entry + take-profit + stop-loss).

| TC    | Name                          | Description                                       | Skip when            |
|-------|-------------------------------|---------------------------------------------------|----------------------|
| TC-E50 | Bracket BUY                   | Entry limit buy + TP limit sell + SL stop sell.   | No bracket support.  |
| TC-E51 | Bracket SELL                  | Entry limit sell + TP limit buy + SL stop buy.    | No bracket support.  |
| TC-E52 | Bracket entry fill activates  | Verify TP/SL become active after entry fill.      | No bracket support.  |
| TC-E53 | Bracket with post-only entry  | Entry order uses post-only flag.                  | No bracket or PO.    |

### TC-E50: Bracket BUY

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester submits a bracket order: limit buy entry + take-profit sell + stop-loss sell. |
| **Event sequence** | Entry: `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`; TP and SL: `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`. |
| **Pass criteria**  | Three orders created and accepted: entry below bid, TP above ask, SL below entry. |
| **Skip when**      | Adapter does not support bracket orders.                               |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_brackets=True,
    bracket_entry_order_type=OrderType.LIMIT,
    bracket_offset_ticks=500,
    enable_limit_buys=True,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_brackets(true)
    .with_bracket_entry_order_type(OrderType::Limit)
    .with_bracket_offset_ticks(500)
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false)
```

### TC-E51: Bracket SELL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester submits bracket: limit sell entry + TP buy + SL buy.        |
| **Event sequence** | Same pattern as TC-E50 but for sell side.                              |
| **Pass criteria**  | Three orders created and accepted on sell side.                        |
| **Skip when**      | Adapter does not support bracket orders.                               |

### TC-E52: Bracket entry fill activates TP/SL

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Bracket order from TC-E50 where entry order fills.                     |
| **Action**         | Entry order fills; verify contingent TP and SL orders activate.        |
| **Event sequence** | Entry: `OrderFilled`; TP and SL transition from contingent to active.  |
| **Pass criteria**  | After entry fill, TP and SL orders are live on the venue.              |
| **Skip when**      | Adapter does not support bracket orders.                               |

**Considerations:**

- This requires the entry order to actually fill, which may need aggressive pricing.
- The TP/SL activation mechanism varies by venue (some activate immediately, some are OCA groups).

### TC-E53: Bracket with post-only entry

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter supports brackets and post-only.                               |
| **Action**         | Submit bracket with `use_post_only=True` (applied to entry and TP).    |
| **Event sequence** | Same as TC-E50 with post-only flag on entry.                           |
| **Pass criteria**  | Entry and TP orders accepted as post-only (maker); SL is not post-only. |
| **Skip when**      | No bracket support or no post-only support.                            |

---

## Group 7: Order flags

Test order-level flags and special parameters.

| TC    | Name                 | Description                                            | Skip when            |
|-------|----------------------|--------------------------------------------------------|----------------------|
| TC-E60 | PostOnly accepted    | Limit with post-only, placed away from TOB.            | No post-only.        |
| TC-E61 | ReduceOnly on close  | Close position with reduce-only flag.                  | No reduce-only.      |
| TC-E62 | Display quantity     | Iceberg order with visible quantity < total.           | No display quantity.  |
| TC-E63 | Custom order params  | Adapter-specific params via `order_params`.            | N/A.                 |

### TC-E60: PostOnly accepted

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places limit buy with `use_post_only=True` at passive price. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order accepted as a maker order; post-only flag acknowledged by venue. |
| **Skip when**      | Adapter does not support post-only flag.                               |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=False,
    use_post_only=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false)
    .with_use_post_only(true)
```

### TC-E61: ReduceOnly on close

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open position (from TC-E01).                                           |
| **Action**         | Stop strategy with `reduce_only_on_stop=True`; closing order uses reduce-only flag. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled` (with reduce-only). |
| **Pass criteria**  | Closing order has reduce-only flag; position fully closed.             |
| **Skip when**      | Adapter does not support reduce-only flag.                             |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
    reduce_only_on_stop=True,
    close_positions_on_stop=True,
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
    .with_reduce_only_on_stop(true)
    .with_close_positions_on_stop(true)
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
```

### TC-E62: Display quantity (iceberg)

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, adapter supports display quantity.                  |
| **Action**         | Place limit order with `order_display_qty` < `order_qty`.              |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order accepted with display quantity set; only display qty visible on the book. |
| **Skip when**      | Adapter does not support display quantity / iceberg orders.            |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("1.0"),
    order_display_qty=Decimal("0.1"),
    enable_limit_buys=True,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
let mut config = ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("1.0"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false);
config.order_display_qty = Some(Quantity::from("0.1"));
```

### TC-E63: Custom order params

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, adapter accepts additional parameters.              |
| **Action**         | Place order with `order_params` dict containing adapter-specific parameters. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted`.               |
| **Pass criteria**  | Order accepted; adapter-specific parameters passed through to venue.   |
| **Skip when**      | N/A (adapter-specific).                                                |

**Considerations:**

- The `order_params` dict is opaque to the ExecTester and passed through to the adapter.
- Consult the adapter's guide for supported parameters.

---

## Group 8: Rejection handling

Test that the adapter correctly handles and reports order rejections.

| TC    | Name                    | Description                                          | Skip when               |
|-------|-------------------------|------------------------------------------------------|-------------------------|
| TC-E70 | PostOnly rejection      | Post-only order that would cross the spread.         | No post-only.           |
| TC-E71 | ReduceOnly rejection    | Reduce-only order with no position to reduce.        | No reduce-only.         |
| TC-E72 | Unsupported order type  | Submit order type not supported by adapter.           | Never.                  |
| TC-E73 | Unsupported TIF         | Submit order with unsupported time in force.          | Never.                  |

### TC-E70: PostOnly rejection

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, quotes flowing.                  |
| **Action**         | ExecTester places post-only order on the wrong side of the book (`test_reject_post_only=True`), causing it to cross the spread. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderRejected`.               |
| **Pass criteria**  | Order rejected by venue; `OrderRejected` event received with reason indicating post-only violation. |
| **Skip when**      | Adapter does not support post-only flag.                               |

**Considerations:**

- The ExecTester's `test_reject_post_only` mode intentionally prices the order to cross.
- Some venues may partially fill instead of rejecting; behavior is venue-specific.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    enable_limit_buys=True,
    enable_limit_sells=False,
    use_post_only=True,
    test_reject_post_only=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_enable_limit_buys(true)
    .with_enable_limit_sells(false)
    .with_use_post_only(true)
    .with_test_reject_post_only(true)
```

### TC-E71: ReduceOnly rejection

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, no open position for the instrument.                |
| **Action**         | ExecTester opens a market position with `reduce_only=True` via `test_reject_reduce_only=True` and `open_position_on_start_qty`, when no position exists to reduce. |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderRejected`.               |
| **Pass criteria**  | Order rejected; `OrderRejected` event with reason indicating reduce-only violation. |
| **Skip when**      | Adapter does not support reduce-only flag.                             |

**Considerations:**

- The `test_reject_reduce_only` flag only applies to the opening market order submitted via
  `open_position_on_start_qty`.
- Verify no prior position exists for the instrument before running this test.

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
    test_reject_reduce_only=True,
    enable_limit_buys=False,
    enable_limit_sells=False,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
    .with_test_reject_reduce_only(true)
    .with_enable_limit_buys(false)
    .with_enable_limit_sells(false)
```

### TC-E72: Unsupported order type

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, order type not in adapter's supported set.          |
| **Action**         | Submit an order type the adapter does not support.                     |
| **Event sequence** | `OrderDenied` (pre-submission rejection by adapter).                   |
| **Pass criteria**  | Order denied before reaching venue; `OrderDenied` event with reason.   |
| **Skip when**      | Never (every adapter has unsupported order types to test).             |

**Considerations:**

- `OrderDenied` occurs at the adapter level before the order reaches the venue.
- This differs from `OrderRejected` which comes from the venue.
- Test by configuring a stop order type that the adapter does not support.

### TC-E73: Unsupported TIF

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, TIF not in adapter's supported set.                 |
| **Action**         | Submit an order with a TIF the adapter does not support.               |
| **Event sequence** | `OrderDenied` (pre-submission rejection by adapter).                   |
| **Pass criteria**  | Order denied before reaching venue; `OrderDenied` event with reason.   |
| **Skip when**      | Never (every adapter has unsupported TIF options to test).             |

**Considerations:**

- Similar to TC-E72 but for time-in-force options.
- Test with TIF values from the Nautilus enum that the adapter does not map.

---

## Group 9: Lifecycle (start/stop)

Test strategy lifecycle behavior and state management on start and stop.

| TC     | Name                        | Description                                            | Skip when            |
|--------|-----------------------------|--------------------------------------------------------|----------------------|
| TC-E80 | Open position on start      | Open a position immediately when strategy starts.      | No market orders.    |
| TC-E81 | Cancel orders on stop       | Cancel all open orders when strategy stops.             | Never.               |
| TC-E82 | Close positions on stop     | Close open positions when strategy stops.               | No market orders.    |
| TC-E83 | Unsubscribe on stop         | Unsubscribe from data feeds on strategy stop.           | No unsub support.    |
| TC-E84 | Reconcile open orders       | Reconcile existing open orders from a prior session.    | Never.               |
| TC-E85 | Reconcile filled orders     | Reconcile previously filled orders from a prior session.| Never.               |
| TC-E86 | Reconcile open long         | Reconcile existing open long position.                  | Never.               |
| TC-E87 | Reconcile open short        | Reconcile existing open short position.                 | Never.               |

### TC-E80: Open position on start

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, no existing position.            |
| **Action**         | Strategy starts with `open_position_on_start_qty` set.                 |
| **Event sequence** | `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | Position opened on start; market order submitted and filled before limit order maintenance begins. |
| **Skip when**      | Adapter does not support market orders.                                |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    open_position_on_start_qty=Decimal("0.01"),
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_open_position_on_start(Some(Decimal::new(1, 2)))
```

### TC-E81: Cancel orders on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open limit orders from the strategy session.                           |
| **Action**         | Stop the strategy with `cancel_orders_on_stop=True` (default).         |
| **Event sequence** | For each open order: `OrderPendingCancel` → `OrderCanceled`.           |
| **Pass criteria**  | All strategy-owned open orders canceled on stop.                       |
| **Skip when**      | Never.                                                                 |

### TC-E82: Close positions on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Open position from the strategy session.                               |
| **Action**         | Stop the strategy with `close_positions_on_stop=True` (default).       |
| **Event sequence** | Closing order: `OrderInitialized` → `OrderSubmitted` → `OrderAccepted` → `OrderFilled`. |
| **Pass criteria**  | All strategy-owned positions closed; net position = 0.                 |
| **Skip when**      | Adapter does not support market orders.                                |

### TC-E83: Unsubscribe on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Active data subscriptions (quotes, trades, book).                      |
| **Action**         | Stop the strategy with `can_unsubscribe=True` (default).               |
| **Event sequence** | Data subscriptions removed.                                            |
| **Pass criteria**  | No further data events received after stop; clean disconnection.       |
| **Skip when**      | Adapter does not support unsubscribe.                                  |

**Python config:**

```python
ExecTesterConfig(
    instrument_id=instrument_id,
    order_qty=Decimal("0.01"),
    can_unsubscribe=True,
)
```

**Rust config:**

```rust
ExecTesterConfig::new(strategy_id, instrument_id, client_id, Quantity::from("0.01"))
    .with_can_unsubscribe(true)
```

### TC-E84: Reconcile open orders

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | One or more open limit orders on the venue from a prior session.       |
| **Action**         | Start the node with `reconciliation=True`.                             |
| **Event sequence** | `OrderStatusReport` generated for each open order.                     |
| **Pass criteria**  | Each open order is loaded into the cache with correct `venue_order_id`, status=ACCEPTED, price, quantity, side, and order type. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- Leave limit orders open from a prior test session (do not cancel on stop).
- Use `external_order_claims` to claim the instrument so the adapter reconciles orders for it.
- Verify that the reconciled order count matches the venue-reported count.

### TC-E85: Reconcile filled orders

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | One or more filled orders on the venue from a prior session.           |
| **Action**         | Start the node with `reconciliation=True`.                             |
| **Event sequence** | `FillReport` generated for each historical fill.                       |
| **Pass criteria**  | Each filled order is loaded into the cache with correct `venue_order_id`, status=FILLED, fill price, fill quantity, and commission. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- Requires orders that filled in a prior session.
- Verify fill price, quantity, and commission match the venue's reported values.
- Some adapters may only report fills within a lookback window.

### TC-E86: Reconcile open long position

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | An open long position on the venue from a prior session.               |
| **Action**         | Start the node with `reconciliation=True`.                             |
| **Event sequence** | `PositionStatusReport` generated for the long position.                |
| **Pass criteria**  | Position loaded into cache with correct instrument, side=LONG, quantity, and entry price matching the venue. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- Open a long position in a prior session and stop the strategy without closing it
  (`close_positions_on_stop=False`).
- Verify the reconciled position quantity and average entry price match the venue.
- After reconciliation, the strategy should be able to manage or close this position.

### TC-E87: Reconcile open short position

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | An open short position on the venue from a prior session.              |
| **Action**         | Start the node with `reconciliation=True`.                             |
| **Event sequence** | `PositionStatusReport` generated for the short position.               |
| **Pass criteria**  | Position loaded into cache with correct instrument, side=SHORT, quantity, and entry price matching the venue. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- Open a short position in a prior session and stop the strategy without closing it
  (`close_positions_on_stop=False`).
- Verify the reconciled position quantity and average entry price match the venue.
- After reconciliation, the strategy should be able to manage or close this position.

---

## ExecTester configuration reference

Quick reference for all `ExecTesterConfig` parameters. Defaults shown are for the Python config;
the Rust builder uses equivalent defaults.

| Parameter                                       | Type              | Default         | Affects groups |
|-------------------------------------------------|-------------------|-----------------|----------------|
| `instrument_id`                                 | InstrumentId      | *required*      | All            |
| `order_qty`                                     | Decimal           | *required*      | All            |
| `order_display_qty`                             | Decimal?          | None            | 2, 7           |
| `order_expire_time_delta_mins`                  | PositiveInt?      | None            | 2              |
| `order_params`                                  | dict?             | None            | 7              |
| `client_id`                                     | ClientId?         | None            | All            |
| `subscribe_quotes`                              | bool              | True            | —              |
| `subscribe_trades`                              | bool              | True            | —              |
| `subscribe_book`                                | bool              | False           | —              |
| `book_type`                                     | BookType          | L2_MBP          | —              |
| `book_depth`                                    | PositiveInt?      | None            | —              |
| `book_interval_ms`                              | PositiveInt       | 1000            | —              |
| `book_levels_to_print`                          | PositiveInt       | 10              | —              |
| `open_position_on_start_qty`                    | Decimal?          | None            | 1, 9           |
| `open_position_time_in_force`                   | TimeInForce       | GTC             | 1              |
| `enable_limit_buys`                             | bool              | True            | 2, 4, 5, 6     |
| `enable_limit_sells`                            | bool              | True            | 2, 4, 5, 6     |
| `enable_stop_buys`                              | bool              | False           | 3, 4           |
| `enable_stop_sells`                             | bool              | False           | 3, 4           |
| `limit_time_in_force`                           | TimeInForce?      | None            | 2, 6           |
| `tob_offset_ticks`                              | PositiveInt       | 500             | 2, 4           |
| `stop_order_type`                               | OrderType         | STOP_MARKET     | 3              |
| `stop_offset_ticks`                             | PositiveInt       | 100             | 3              |
| `stop_limit_offset_ticks`                       | PositiveInt?      | None            | 3              |
| `stop_time_in_force`                            | TimeInForce?      | None            | 3              |
| `stop_trigger_type`                             | TriggerType?      | None            | 3              |
| `enable_brackets`                               | bool              | False           | 6              |
| `bracket_entry_order_type`                      | OrderType         | LIMIT           | 6              |
| `bracket_offset_ticks`                          | PositiveInt       | 500             | 6              |
| `modify_orders_to_maintain_tob_offset`          | bool              | False           | 4              |
| `modify_stop_orders_to_maintain_offset`         | bool              | False           | 4              |
| `cancel_replace_orders_to_maintain_tob_offset`  | bool              | False           | 4              |
| `cancel_replace_stop_orders_to_maintain_offset` | bool              | False           | 4              |
| `use_post_only`                                 | bool              | False           | 2, 6, 7, 8     |
| `use_quote_quantity`                            | bool              | False           | 1, 7           |
| `emulation_trigger`                             | TriggerType?      | None            | 2, 3           |
| `cancel_orders_on_stop`                         | bool              | True            | 5, 9           |
| `close_positions_on_stop`                       | bool              | True            | 9              |
| `close_positions_time_in_force`                 | TimeInForce?      | None            | 9              |
| `reduce_only_on_stop`                           | bool              | True            | 7, 9           |
| `use_individual_cancels_on_stop`                | bool              | False           | 5              |
| `use_batch_cancel_on_stop`                      | bool              | False           | 5              |
| `dry_run`                                       | bool              | False           | —              |
| `log_data`                                      | bool              | True            | —              |
| `test_reject_post_only`                         | bool              | False           | 8              |
| `test_reject_reduce_only`                       | bool              | False           | 8              |
| `can_unsubscribe`                               | bool              | True            | 9              |
