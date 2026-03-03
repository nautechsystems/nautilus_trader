# Data Testing Spec

This section defines a rigorous test matrix for validating adapter data
functionality using the `DataTester` actor. Both Python
(`nautilus_trader.test_kit.strategies.tester_data`) and Rust
(`nautilus_testkit::testers`) provide the `DataTester`. Each test case is
identified by a prefixed ID (e.g. TC-D01) and grouped by functionality.

**Each adapter must pass the subset of tests matching its supported data types.**

Test groups are ordered from least derived to most derived data: instruments
and raw book data first, then quotes, trades, bars, and derivatives data.
An adapter that passes groups 1–4 is considered baseline data compliant.

Document adapter-specific data behavior (custom channels, throttling,
snapshot semantics, etc.) in the adapter's own guide, not here.

## Prerequisites

Before running data tests:

- Target instrument available and loadable via the instrument provider.
- API credentials set via environment variables (`{VENUE}_API_KEY`, `{VENUE}_API_SECRET`) when
  the venue requires authentication for the data being tested.
- If the venue offers a demo/testnet mode (e.g. `is_demo=True`), use credentials created
  for that environment. Demo and production API keys are typically separate and not
  interchangeable; using the wrong credentials produces authentication errors (e.g. HTTP 401).

**Python node setup** (reference: `examples/live/{adapter}/{adapter}_data_tester.py`):

```python
from nautilus_trader.live.node import TradingNode
from nautilus_trader.test_kit.strategies.tester_data import DataTester, DataTesterConfig

node = TradingNode(config=config_node)
tester = DataTester(config=config_tester)
node.trader.add_actor(tester)
# Register adapter factories, build, and run
```

**Rust node setup** (reference: `crates/adapters/{adapter}/examples/node_data_tester.rs`):

```rust
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

let tester_config = DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_quotes(true);
let tester = DataTester::new(tester_config);
node.add_actor(tester)?;
node.run().await?;
```

Each group below begins with a summary table, followed by detailed test cards.
Test IDs use spaced numbering to allow insertion without renumbering.

---

## Group 1: Instruments

Verify instrument loading and subscription before testing market data streams.

| TC      | Name                        | Description                                          | Skip when            |
|---------|-----------------------------|------------------------------------------------------|----------------------|
| TC-D01  | Request instruments         | Load all instruments for a venue.                    | Never.               |
| TC-D02  | Subscribe instrument        | Subscribe to instrument updates.                     | No instrument sub.   |
| TC-D03  | Load specific instrument    | Load a single instrument by ID.                      | Never.               |

### TC-D01: Request instruments

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected.                                                     |
| **Action**         | DataTester requests all instruments for the venue on start.            |
| **Event sequence** | `on_instruments` callback receives instrument list.                    |
| **Pass criteria**  | At least one instrument received; each has valid symbol, price precision, and size increment. |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    request_instruments=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_request_instruments(true)
```

### TC-D02: Subscribe instrument

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to instrument updates.                           |
| **Event sequence** | `on_instrument` callback receives instrument.                          |
| **Pass criteria**  | Instrument received with correct `instrument_id`, valid fields.        |
| **Skip when**      | Adapter does not support instrument subscriptions.                     |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_instrument=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_instrument(true)
```

### TC-D03: Load specific instrument

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected.                                                     |
| **Action**         | Load a specific instrument by `InstrumentId` via the instrument provider. |
| **Event sequence** | Instrument available in cache after load.                              |
| **Pass criteria**  | Instrument loaded with correct ID, price precision, size increment, and trading rules. |
| **Skip when**      | Never.                                                                 |

**Considerations:**

- This tests the instrument provider's `load` / `load_async` method directly.
- Verify the instrument is cached and available via `self.cache.instrument(instrument_id)`.

---

## Group 2: Order book

Test order book subscription modes and snapshot requests.

| TC      | Name                           | Description                                        | Skip when              |
|---------|--------------------------------|----------------------------------------------------|------------------------|
| TC-D10  | Subscribe book deltas          | Stream `OrderBookDeltas` updates.                  | No book support.       |
| TC-D11  | Subscribe book at interval     | Periodic `OrderBook` snapshots.                    | No book support.       |
| TC-D12  | Subscribe book depth           | `OrderBookDepth10` snapshots.                      | No book depth.         |
| TC-D13  | Request book snapshot          | One-time book snapshot request.                    | No book snapshot.      |
| TC-D14  | Managed book from deltas       | Build local book from delta stream.                | No book support.       |
| TC-D15  | Request historical book deltas | Historical book deltas request.                    | No historical deltas.  |

### TC-D10: Subscribe book deltas

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to order book deltas.                            |
| **Event sequence** | `OrderBookDeltas` events received in `on_order_book_deltas`.           |
| **Pass criteria**  | Deltas received with valid instrument ID; at least one delta contains bid/ask updates. |
| **Skip when**      | Adapter does not support order book data.                              |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_book_deltas=True,
    book_type=BookType.L2_MBP,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_book_deltas(true)
    .with_book_type(BookType::L2_MBP)
```

### TC-D11: Subscribe book at interval

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to periodic order book snapshots.                |
| **Event sequence** | `OrderBook` events received in `on_order_book` at configured interval. |
| **Pass criteria**  | Book snapshots received with bid/ask levels; updates arrive at approximately the configured interval. |
| **Skip when**      | Adapter does not support order book data.                              |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_book_at_interval=True,
    book_type=BookType.L2_MBP,
    book_depth=10,
    book_interval_ms=1000,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_book_at_interval(true)
    .with_book_type(BookType::L2_MBP)
    .with_book_depth(Some(NonZeroUsize::new(10).unwrap()))
    .with_book_interval_ms(NonZeroUsize::new(1000).unwrap())
```

### TC-D12: Subscribe book depth

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to `OrderBookDepth10` snapshots.                 |
| **Event sequence** | `OrderBookDepth10` events received in `on_order_book_depth`.           |
| **Pass criteria**  | Depth snapshots received with up to 10 bid/ask levels; prices are correctly ordered. |
| **Skip when**      | Adapter does not support book depth subscriptions.                     |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_book_depth=True,
    book_type=BookType.L2_MBP,
    book_depth=10,
)
```

**Rust config:** Not yet supported. Book depth subscription is TODO in the Rust `DataTester`.

### TC-D13: Request book snapshot

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester requests a one-time order book snapshot.                    |
| **Event sequence** | Book snapshot received via historical data callback.                   |
| **Pass criteria**  | Snapshot contains bid/ask levels with valid prices and sizes.          |
| **Skip when**      | Adapter does not support book snapshot requests.                       |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    request_book_snapshot=True,
    book_depth=10,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_request_book_snapshot(true)
    .with_book_depth(Some(NonZeroUsize::new(10).unwrap()))
```

### TC-D14: Managed book from deltas

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, book deltas streaming.           |
| **Action**         | DataTester subscribes to deltas with `manage_book=True`; builds local order book from the delta stream. |
| **Event sequence** | `OrderBookDeltas` applied to local `OrderBook`; book logged with configured depth. |
| **Pass criteria**  | Local book builds correctly from deltas; bid levels descend, ask levels ascend; book is not empty after initial snapshot. |
| **Skip when**      | Adapter does not support order book data.                              |

**Considerations:**

- The managed book applies each delta to an `OrderBook` instance maintained by the actor.
- Use `book_levels_to_print` to control logging verbosity.

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_book_deltas=True,
    manage_book=True,
    book_type=BookType.L2_MBP,
    book_levels_to_print=10,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_book_deltas(true)
    .with_manage_book(true)
    .with_book_type(BookType::L2_MBP)
```

### TC-D15: Request historical book deltas

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester requests historical order book deltas.                      |
| **Event sequence** | Historical deltas received via callback.                               |
| **Pass criteria**  | Deltas received with valid timestamps and book actions.                |
| **Skip when**      | Adapter does not support historical book delta requests.               |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    request_book_deltas=True,
)
```

**Rust config:** Not yet supported. Historical book delta requests are TODO in the Rust `DataTester`.

---

## Group 3: Quotes

Test quote tick subscriptions and historical requests.

| TC      | Name                      | Description                                     | Skip when              |
|---------|---------------------------|-------------------------------------------------|------------------------|
| TC-D20  | Subscribe quotes          | Verify `QuoteTick` events flow after start.     | Never.                 |
| TC-D21  | Request historical quotes | Request historical quote ticks.                 | No historical quotes.  |

### TC-D20: Subscribe quotes

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to quotes on start.                              |
| **Event sequence** | `QuoteTick` events received in `on_quote_tick`.                        |
| **Pass criteria**  | At least one `QuoteTick` received with valid bid/ask prices and sizes; bid < ask. |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_quotes=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_quotes(true)
```

### TC-D21: Request historical quotes

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester requests historical quote ticks.                            |
| **Event sequence** | Historical quotes received via `on_historical_data` callback.          |
| **Pass criteria**  | Quotes received with valid timestamps, bid/ask prices and sizes.       |
| **Skip when**      | Adapter does not support historical quote requests.                    |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    request_quotes=True,
    requests_start_delta=pd.Timedelta(hours=1),
)
```

---

## Group 4: Trades

Test trade tick subscriptions and historical requests.

| TC     | Name                      | Description                                     | Skip when              |
|--------|---------------------------|-------------------------------------------------|------------------------|
| TC-D30 | Subscribe trades          | Verify `TradeTick` events flow after start.     | Never.                 |
| TC-D31 | Request historical trades | Request historical trade ticks.                 | No historical trades.  |

### TC-D30: Subscribe trades

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to trades on start.                              |
| **Event sequence** | `TradeTick` events received in `on_trade_tick`.                        |
| **Pass criteria**  | At least one `TradeTick` received with valid price, size, and aggressor side. |
| **Skip when**      | Never.                                                                 |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_trades=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_trades(true)
```

### TC-D31: Request historical trades

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester requests historical trade ticks.                            |
| **Event sequence** | Historical trades received via `on_historical_data` callback.          |
| **Pass criteria**  | Trades received with valid timestamps, prices, sizes, and trade IDs.   |
| **Skip when**      | Adapter does not support historical trade requests.                    |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    request_trades=True,
    requests_start_delta=pd.Timedelta(hours=1),
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_request_trades(true)
```

---

## Group 5: Bars

Test bar subscriptions and historical requests.

| TC      | Name                    | Description                                       | Skip when           |
|---------|-------------------------|---------------------------------------------------|---------------------|
| TC-D40  | Subscribe bars          | Verify `Bar` events flow after start.             | No bar support.     |
| TC-D41  | Request historical bars | Request historical OHLCV bars.                    | No historical bars. |

### TC-D40: Subscribe bars

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, bar type configured.             |
| **Action**         | DataTester subscribes to bars for a configured `BarType`.              |
| **Event sequence** | `Bar` events received in `on_bar`.                                     |
| **Pass criteria**  | At least one `Bar` received with valid OHLCV values; high >= low, high >= open, high >= close. |
| **Skip when**      | Adapter does not support bar subscriptions.                            |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    bar_types=[BarType.from_str("BTCUSDT-PERP.VENUE-1-MINUTE-LAST-EXTERNAL")],
    subscribe_bars=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_bar_types(vec![bar_type])
    .with_subscribe_bars(true)
```

### TC-D41: Request historical bars

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded, bar type configured.             |
| **Action**         | DataTester requests historical bars for a configured `BarType`.        |
| **Event sequence** | Historical bars received via callback.                                 |
| **Pass criteria**  | Bars received with valid OHLCV values and ascending timestamps.        |
| **Skip when**      | Adapter does not support historical bar requests.                      |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    bar_types=[BarType.from_str("BTCUSDT-PERP.VENUE-1-MINUTE-LAST-EXTERNAL")],
    request_bars=True,
    requests_start_delta=pd.Timedelta(hours=1),
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_bar_types(vec![bar_type])
    .with_request_bars(true)
```

---

## Group 6: Derivatives data

Test derivatives-specific data streams: mark prices, index prices, and funding rates.

| TC     | Name                             | Description                                 | Skip when             |
|--------|----------------------------------|---------------------------------------------|-----------------------|
| TC-D50 | Subscribe mark prices            | `MarkPriceUpdate` events.                   | Not a derivative.     |
| TC-D51 | Subscribe index prices           | `IndexPriceUpdate` events.                  | Not a derivative.     |
| TC-D52 | Subscribe funding rates          | `FundingRateUpdate` events.                 | Not a perpetual.      |
| TC-D53 | Request historical funding rates | Historical funding rate data.               | Not a perpetual.      |

### TC-D50: Subscribe mark prices

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, derivative instrument loaded.                       |
| **Action**         | DataTester subscribes to mark price updates.                           |
| **Event sequence** | `MarkPriceUpdate` events received in `on_mark_price`.                  |
| **Pass criteria**  | At least one `MarkPriceUpdate` received with valid instrument ID and mark price. |
| **Skip when**      | Instrument is not a derivative, or adapter does not provide mark prices. |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_mark_prices=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_mark_prices(true)
```

### TC-D51: Subscribe index prices

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, derivative instrument loaded.                       |
| **Action**         | DataTester subscribes to index price updates.                          |
| **Event sequence** | `IndexPriceUpdate` events received in `on_index_price`.                |
| **Pass criteria**  | At least one `IndexPriceUpdate` received with valid instrument ID and index price. |
| **Skip when**      | Instrument is not a derivative, or adapter does not provide index prices. |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_index_prices=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_index_prices(true)
```

### TC-D52: Subscribe funding rates

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, perpetual instrument loaded.                        |
| **Action**         | DataTester subscribes to funding rate updates.                         |
| **Event sequence** | `FundingRateUpdate` events received in `on_funding_rate`.              |
| **Pass criteria**  | At least one `FundingRateUpdate` received with valid instrument ID and rate. |
| **Skip when**      | Instrument is not a perpetual, or adapter does not provide funding rates. |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_funding_rates=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_funding_rates(true)
```

### TC-D53: Request historical funding rates

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, perpetual instrument loaded.                        |
| **Action**         | DataTester requests historical funding rates (default 7-day lookback). |
| **Event sequence** | Historical funding rates received via callback.                        |
| **Pass criteria**  | Funding rates received with valid timestamps and rate values.          |
| **Skip when**      | Instrument is not a perpetual, or adapter does not support historical funding rate requests. |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    request_funding_rates=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_request_funding_rates(true)
```

---

## Group 7: Instrument status

Test instrument status and close event subscriptions.

| TC     | Name                        | Description                                    | Skip when             |
|--------|-----------------------------|------------------------------------------------|-----------------------|
| TC-D60 | Subscribe instrument status | `InstrumentStatus` events.                     | No status support.    |
| TC-D61 | Subscribe instrument close  | `InstrumentClose` events.                      | No close support.     |

### TC-D60: Subscribe instrument status

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to instrument status updates.                    |
| **Event sequence** | `InstrumentStatus` events received in `on_instrument_status`.          |
| **Pass criteria**  | Status events received with valid `MarketStatusAction` (e.g. `Trading`). |
| **Skip when**      | Adapter does not support instrument status subscriptions.              |

**Considerations:**

- Status events may only fire on state changes (e.g. trading halt → resume).
- During normal trading hours, a `Trading` status may be received on subscribe.

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_instrument_status=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_instrument_status(true)
```

### TC-D61: Subscribe instrument close

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, instrument loaded.                                  |
| **Action**         | DataTester subscribes to instrument close events.                      |
| **Event sequence** | `InstrumentClose` events received in `on_instrument_close`.            |
| **Pass criteria**  | Close event received with valid close price and close type.            |
| **Skip when**      | Adapter does not support instrument close subscriptions.               |

**Considerations:**

- Close events typically fire at end-of-session for traditional markets.
- May not fire for 24/7 crypto venues unless the adapter synthesizes a daily close.

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_instrument_close=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_instrument_close(true)
```

---

## Group 8: Lifecycle

Test actor lifecycle behavior: unsubscribe handling and custom parameters.

| TC     | Name                    | Description                                        | Skip when            |
|--------|-------------------------|----------------------------------------------------|----------------------|
| TC-D70 | Unsubscribe on stop     | Unsubscribe from data feeds on actor stop.         | No unsub support.    |
| TC-D71 | Custom subscribe params | Adapter-specific subscription parameters.          | N/A.                 |
| TC-D72 | Custom request params   | Adapter-specific request parameters.               | N/A.                 |

### TC-D70: Unsubscribe on stop

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Active data subscriptions (quotes, trades, book).                      |
| **Action**         | Stop the actor with `can_unsubscribe=True` (default).                  |
| **Event sequence** | Data subscriptions removed; no further data events received.           |
| **Pass criteria**  | Clean unsubscribe; no errors in logs; no data events after stop.       |
| **Skip when**      | Adapter does not support unsubscribe.                                  |

**Python config:**

```python
DataTesterConfig(
    instrument_ids=[instrument_id],
    subscribe_quotes=True,
    subscribe_trades=True,
    can_unsubscribe=True,
)
```

**Rust config:**

```rust
DataTesterConfig::new(client_id, vec![instrument_id])
    .with_subscribe_quotes(true)
    .with_subscribe_trades(true)
    .with_can_unsubscribe(true)
```

### TC-D71: Custom subscribe params

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, adapter accepts additional subscription parameters. |
| **Action**         | Subscribe with `subscribe_params` dict containing adapter-specific parameters. |
| **Event sequence** | Subscription established with custom parameters applied.               |
| **Pass criteria**  | Data flows with adapter-specific parameters in effect.                 |
| **Skip when**      | N/A (adapter-specific).                                                |

**Considerations:**

- The `subscribe_params` dict is opaque to the DataTester and passed through to the adapter.
- Consult the adapter's guide for supported parameters.

### TC-D72: Custom request params

| Field              | Value                                                                  |
|--------------------|------------------------------------------------------------------------|
| **Prerequisite**   | Adapter connected, adapter accepts additional request parameters.      |
| **Action**         | Request data with `request_params` dict containing adapter-specific parameters. |
| **Event sequence** | Request fulfilled with custom parameters applied.                      |
| **Pass criteria**  | Historical data received with adapter-specific parameters in effect.   |
| **Skip when**      | N/A (adapter-specific).                                                |

**Considerations:**

- The `request_params` dict is opaque to the DataTester and passed through to the adapter.
- Consult the adapter's guide for supported parameters.

---

## DataTester configuration reference

Quick reference for all `DataTesterConfig` parameters. Defaults shown are for the Python config.
Note: Rust `DataTesterConfig::new` sets `manage_book=true`, while Python defaults it to `False`.

| Parameter                    | Type              | Default         | Affects groups |
|------------------------------|-------------------|-----------------|----------------|
| `instrument_ids`             | list[InstrumentId]| *required*      | All            |
| `client_id`                  | ClientId?         | None            | All            |
| `bar_types`                  | list[BarType]?    | None            | 5              |
| `subscribe_book_deltas`      | bool              | False           | 2              |
| `subscribe_book_depth`       | bool              | False           | 2              |
| `subscribe_book_at_interval` | bool              | False           | 2              |
| `subscribe_quotes`           | bool              | False           | 3              |
| `subscribe_trades`           | bool              | False           | 4              |
| `subscribe_mark_prices`      | bool              | False           | 6              |
| `subscribe_index_prices`     | bool              | False           | 6              |
| `subscribe_funding_rates`    | bool              | False           | 6              |
| `subscribe_bars`             | bool              | False           | 5              |
| `subscribe_instrument`       | bool              | False           | 1              |
| `subscribe_instrument_status`| bool              | False           | 7              |
| `subscribe_instrument_close` | bool              | False           | 7              |
| `subscribe_params`           | dict?             | None            | 8              |
| `can_unsubscribe`            | bool              | True            | 8              |
| `request_instruments`        | bool              | False           | 1              |
| `request_book_snapshot`      | bool              | False           | 2              |
| `request_book_deltas`        | bool              | False           | 2              |
| `request_quotes`             | bool              | False           | 3              |
| `request_trades`             | bool              | False           | 4              |
| `request_bars`               | bool              | False           | 5              |
| `request_funding_rates`      | bool              | False           | 6              |
| `request_params`             | dict?             | None            | 8              |
| `requests_start_delta`       | Timedelta?        | 1 hour          | 3, 4, 5        |
| `book_type`                  | BookType          | L2_MBP          | 2              |
| `book_depth`                 | PositiveInt?      | None            | 2              |
| `book_interval_ms`           | PositiveInt       | 1000            | 2              |
| `book_levels_to_print`       | PositiveInt       | 10              | 2              |
| `manage_book`                | bool              | False           | 2              |
| `use_pyo3_book`              | bool              | False           | 2              |
| `log_data`                   | bool              | True            | All            |

---
