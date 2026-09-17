# Kalshi

Kalshi is a regulated event-contract exchange. Every market is a binary contract whose YES side pays
the market's notional value on a win and nothing on a loss, and whose NO side pays the complement.

NautilusTrader provides a venue integration for data and execution via Kalshi's Trade API.

The adapter is implemented in Rust and exposed to Python at `nautilus_trader.adapters.kalshi`; data
and execution therefore behave the same from Rust and Python.

Kalshi publishes market data and order state over REST, so both clients poll the venue. The adapter
carries no WebSocket client, and the consequences of polling are described in
[Limitations and considerations](#limitations-and-considerations).

## Installation

The Python package includes the Kalshi adapter; no adapter-specific extra is required.

To install the latest pre-release build:

```bash
uv pip install --pre nautilus_trader
```

To build the Python package from source, run from the repository root:

```bash
make build-debug
```

For development wheels and source-build prerequisites, see the [installation guide](../getting_started/installation.md).

## Binary options

A [binary option](https://en.wikipedia.org/wiki/Binary_option) is a type of financial exotic option contract in which traders bet on
the outcome of a yes-or-no proposition. If the prediction is correct, the trader receives a fixed payout; otherwise,
they receive nothing. NautilusTrader represents every Kalshi market as a `BinaryOption` instrument.

A market is the instrument: its YES and NO sides are the same contract, quoted from the YES side, so
the instrument identifier is the market ticker with the venue suffix, for example
`KXHIGHNY-25JAN01-T50.KALSHI`. Instruments are built from these market fields:

| Instrument field  | Kalshi source                  |
| ----------------- | ------------------------------ |
| `instrument_id`   | `{ticker}`.KALSHI              |
| `raw_symbol`      | `ticker`                       |
| `event_id`        | `event_ticker`                 |
| `outcome`         | `yes_sub_title`                |
| `description`     | `rules_primary`                |
| `activation_ns`   | `open_time`                    |
| `expiration_ns`   | `latest_expiration_time`       |
| `asset_class`     | `Alternative`                  |
| `currency`        | USD                            |
| `price_precision` | the finest `price_ranges` step |
| `size_precision`  | 2                              |
| `size_increment`  | 0.01                           |

Instruments carry no maker or taker fee metadata, and no minimum or maximum quantity.

An event carries the markets that hold the mutually exclusive outcomes of one real-world occurrence.
The adapter maps an event onto an `OutcomeGroup` whose legs are the event's markets and whose payout
per leg is the market's notional value. The exchange documents that an event's markets are mutually
exclusive; the adapter has not verified exclusivity beyond that, and exhaustiveness is unknown,
because the exchange does not promise that the listed markets name every possible outcome. An event
that is not mutually exclusive, carries no markets, mixes non-binary markets, or whose markets
disagree on what a winning contract pays produces no outcome group.

## Kalshi documentation

Kalshi offers resources for different audiences:

- [Kalshi](https://kalshi.com): the exchange itself.
- [Kalshi API documentation](https://docs.kalshi.com): technical documentation for the Trade API, including its environments,
  authentication flow, and endpoint references.

## Overview

This guide assumes a trader is setting up for both live market data feeds and trade execution.
The Rust implementation includes multiple components, which can be used together or separately
depending on the use case.

- `KalshiHttpClient`: Low-level REST API connectivity, with RSA-signed request authentication.
- `KalshiInstrumentProvider`: Instrument parsing and loading functionality for `BinaryOption`
  instruments.
- `KalshiDataClient`: A market data feed manager.
- `KalshiExecutionClient`: A trade execution gateway.
- `KalshiDataClientFactory`: Factory for Kalshi data clients (used by the live node builder).
- `KalshiExecutionClientFactory`: Factory for Kalshi execution clients (used by the live node
  builder).

:::note
Python users configure live nodes through the exported configuration and factory classes. The
direct HTTP client, provider, data client, and execution client types are Rust-only implementation
components.
:::

The default client identifiers are `KALSHI-DATA` for the data client and `KALSHI-EXEC` for the
execution client, against the `KALSHI` venue.

## API keys

Both clients authenticate every request with an API key ID and the matching RSA private key:

- `api_key_id`, which falls back to the `KALSHI_API_KEY_ID` environment variable.
- `api_key_pem`, the PEM-encoded RSA private key, which falls back to `KALSHI_API_KEY_PEM`.

An explicit configuration value takes precedence over the environment variable. Credential
resolution fails when a value is absent from both the configuration and the environment, or when
the API key ID is blank. Both the data client and the execution client resolve credentials when
they are constructed, so both require a key even for public market data.

Requests are signed as RSA-PSS over SHA-256 of `timestamp + method + path`, where the path is the
route from the API root including the `/trade-api/v2` prefix and excluding any query parameters. The
signature travels in the `KALSHI-ACCESS-SIGNATURE` header with the key ID in `KALSHI-ACCESS-KEY` and
the millisecond timestamp in `KALSHI-ACCESS-TIMESTAMP`.

Set the environment variables before starting the node:

```bash
export KALSHI_API_KEY_ID="your-key-id"
export KALSHI_API_KEY_PEM="$(cat /path/to/private_key.pem)"
```

:::warning
Store the private key in a secret manager or protected environment configuration. Do not commit it
to a repository or share it in logs.
:::

## Configuration

Both configurations select an API environment, which decides the default REST endpoint:

| Environment | REST base URL                                      | Notes                            |
| ----------- | -------------------------------------------------- | -------------------------------- |
| `Demo`      | `https://external-api.demo.kalshi.co/trade-api/v2` | **Adapter default.** Mock funds. |
| `Prod`      | `https://external-api.kalshi.com/trade-api/v2`     | Trades settle real capital.      |

The demo exchange keeps its own credentials and balances, so an API key created for production does
not authenticate against it. Set `base_url` to override the environment's endpoint, for example to
use one of the exchange's shared hostnames.

Both clients poll the venue, and each configuration exposes its own `poll_interval_millis`: the
data client's interval between market polls, and the execution client's interval between order
polls. Each defaults to 2,000 milliseconds, and a faster poll trades request rate for freshness.

## Data capability

Kalshi publishes market data over REST, so the data client polls. One task refreshes the top of
book and the book snapshot of every subscribed market, and a second task fetches trades after the
newest timestamp it has already seen.

| Data type           | Sub. | Snapshot | Hist. | Nautilus type      | Notes                                                             |
| ------------------- | ---- | -------- | ----- | ------------------ | ----------------------------------------------------------------- |
| Instrument metadata | -    | ✓        | -     | `InstrumentAny`    | Loaded at connect; no `SubscribeInstruments` handling.            |
| Quote ticks         | ✓    | -        | -     | `QuoteTick`        | Polled top of book, quoted from the YES side.                     |
| Trade ticks         | ✓    | -        | ✓     | `TradeTick`        | Polled per market after the newest trade already seen.            |
| Order book deltas   | ✓    | ✓        | -     | `OrderBookDeltas`  | `L2_MBP` only; each poll emits a snapshot that replaces the book. |
| Instrument status   | -    | ✓        | -     | `InstrumentStatus` | Emitted when a settled market is polled.                          |
| Instrument close    | -    | ✓        | -     | `InstrumentClose`  | Emitted once per settled market.                                  |
| Market resolution   | -    | ✓        | -     | `MarketResolution` | Emitted once per event version, from the exchange's record.       |

The client's behavior follows from polling:

- The market poll runs every `poll_interval_millis`, which defaults to 2,000 milliseconds. The
  trade poll runs every 5 seconds and reads at most 100 trades per instrument per pass.
- Polling cannot observe every intermediate book state, so a book update is emitted as a snapshot
  that clears and rebuilds the book rather than as an incremental delta. Between two polls a book
  can print through levels that a consumer never sees.
- Quotes are emitted only while the market's status is tradable, which the exchange reports as
  `Active`. A market that is not active is not quoted.
- `subscribe_instruments` does nothing: instruments are loaded from the configured events and
  series at connect and published on refresh.
- Only `BookType::L2_MBP` is accepted for book-delta subscriptions; the venue publishes bid and ask
  levels only.

### Instrument loading

The data client loads instruments when it connects, from the configured scope:

- With `event_tickers` set, it loads the markets of those events.
- With `event_tickers` empty, it loads every open market, optionally narrowed by `series_ticker`.

A market that cannot be converted is skipped with a warning, and the rest of the batch is stored. A
refresh replaces the stored copy of the markets it reads, so a long-running node picks up newly
listed markets.

## Orders capability

Kalshi accepts limit orders, and the adapter emulates market orders. Because the exchange reports
order state over REST, a submission's immediate answer becomes order events and everything after it
is learned by polling.

### Order types

| Order Type             | Binary Options | Notes                                                                                                                            |
| ---------------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `MARKET`               | ✓              | Emulated at the far side of the book and always sent immediate-or-cancel. Denied when the cached book has no price on that side. |
| `LIMIT`                | ✓              | Rests at its price until it trades, expires, or the market closes.                                                               |
| `STOP_MARKET`          | -              | *Not supported by Kalshi*.                                                                                                       |
| `STOP_LIMIT`           | -              | *Not supported by Kalshi*.                                                                                                       |
| `MARKET_IF_TOUCHED`    | -              | *Not supported by Kalshi*.                                                                                                       |
| `LIMIT_IF_TOUCHED`     | -              | *Not supported by Kalshi*.                                                                                                       |
| `TRAILING_STOP_MARKET` | -              | *Not supported by Kalshi*.                                                                                                       |

A market order carries no venue price of its own, so the adapter sends it at the price on the far
side of the book: the best ask for a BUY and the best bid for a SELL, read from the cached order
book. A market order is always sent immediate-or-cancel, so whatever the far side does not fill is
canceled rather than left working. When the cached book holds no price on that side, the order is
denied before any venue request rather than sent at a price the exchange would reject.

Orders are placed on the YES side of the market, because the instrument is quoted from that side.
Prices and counts are sent as fixed-point strings at the instrument's precision.

### Execution instructions

| Instruction   | Binary Options | Notes                                                       |
| ------------- | -------------- | ----------------------------------------------------------- |
| `post_only`   | ✓              | Sent as `post_only` when the order is marked post-only.     |
| `reduce_only` | ✓              | Sent as `reduce_only` when the order is marked reduce-only. |

Two venue instructions come from the execution client configuration rather than the order:

- `self_trade_prevention` is sent as `self_trade_prevention_type` on every order, and defaults to
  `TakerAtCross`, where the member's own resting order is traded as the taker. Set it to `Maker` for
  the member's new order to rest instead of trading against the member's own order.
- `cancel_order_on_pause` is sent as `cancel_order_on_pause`. Leaving it unset uses the exchange's
  own default.

### Time-in-force options

| Nautilus TIF | Kalshi `time_in_force` | Nautilus order scope | Notes                                                                      |
| ------------ | ---------------------- | -------------------- | -------------------------------------------------------------------------- |
| `GTC`        | `good_till_canceled`   | `LIMIT`              | Rests on the book until canceled or the market closes.                     |
| `GTD`        | `good_till_canceled`   | `LIMIT`              | Also sends the order's expiry as `expiration_time`, in whole Unix seconds. |
| `IOC`        | `immediate_or_cancel`  | `LIMIT` or `MARKET`  | Fills what is available and cancels the remainder.                         |
| `FOK`        | `fill_or_kill`         | `LIMIT` or `MARKET`  | Fills the whole quantity immediately or cancels the whole order.           |

The exchange has no explicit expiry instruction: a resting order expires at the timestamp carried in
the request's `expiration_time`, so `GTC` and `GTD` both map onto the venue's good-till-canceled.
A market order is always sent immediate-or-cancel, whatever time in force it declares, because
whatever the far side of the book does not fill must not be left working. Every other time in
force is refused before a venue request, which is why NautilusTrader `DAY` orders are denied: a
Kalshi order rests until it is canceled or the market closes.

### Order management

- An amendment (`ModifyOrder`) carries the new total count and price. It requires a price, and the
  quantity defaults to the order's current total when the command does not name one.
- A cancellation sends the market ticker alongside the order identifier, because the identifier
  alone does not name the shard the order lives on. The order stays tracked after a successful
  cancellation, so a fill that arrived first is still read back by the next poll.
- A balance query reads the member's balance and reports it as the account state.
- `CancelAllOrders` cancels the tracked orders for the instrument, narrowed by the order side when
  the command names one. An order the client cannot map to a Nautilus order is logged and skipped.
- `BatchCancelOrders` cancels one order per request. The exchange publishes a batched cancel, but it
  cancels every working order rather than a named set, so the adapter does not use it.
- `SubmitOrderList` submits each order in the list independently. The adapter implements no
  contingent orders such as OCO, OTO, or brackets.

### Order querying

- `GenerateOrderStatusReport` reads one order by venue order identifier, or scans the member's
  orders for a client order identifier.
- `QueryOrder` requires a venue order identifier.
- Order status reports always report the time in force as good-till-canceled. The exchange does not
  echo the submitted instruction, and a resting order and a terminal order cannot be told apart by
  an in-force value that cannot change how the report is applied.

### Order polling

One task polls the orders the client is still tracking at the execution client's
`poll_interval_millis`, which defaults to 2,000 milliseconds, and reports their state as
`OrderStatusReport`s and `FillReport`s. Fills are deduplicated by trade identifier, so a fill that
the submission path already reported as an order event is not applied a second time.

The venue's read path lags its write path, so an order the exchange has accepted can be missing from
a read for a short while. Only a run of consecutive misses is evidence that the order is not there,
and the client stops tracking an order after five of them.

## Fees

- A fill's commission is the venue's `fee_cost`. A fill that reports no fee is reported with a zero
  commission in USD.
- Kalshi quotes fees finer than USD's two-decimal scale, and a `Money` value is denominated at the
  currency's scale. A fee that cannot be represented exactly is therefore reported at its rounded
  amount with a warning rather than silently: `0.0440` is reported as `0.04 USD`.
- Instruments carry no maker or taker fee metadata, so the adapter ships no venue fee model.
- Fee amounts finer than the currency are the one place the adapter reports an inexact value. Every
  price and contract count is parsed exactly.

## Reconciliation

### Connect-time reconciliation

The exchange holds the account state, so a session that starts against an account with working
orders has to learn about them before it can act. When `reconciliation` is set, which it is by
default, connecting spawns one pass that:

1. Reads the member's working orders and reports each one.
2. Reads each order's fills back, so its reported state is supported by its fill history.
3. Tracks the orders that carry a client order identifier, so the poll task keeps them current. An
   order the member placed outside the platform carries none, so it is reported but not tracked,
   because no order event can be routed to it.
4. Reads the member's positions and reports them.

### Report generation

- `generate_order_status_reports` reads the member's orders, optionally narrowed by instrument, by
  the window's start, and to orders that are still open.
- `generate_fill_reports` reads the member's fills, optionally narrowed by instrument, venue order
  identifier, and the window's start.
- `generate_position_status_reports` reads the member's positions. A positive position is long the
  YES side; a negative position is short the YES side, which is what holding the NO leg amounts to,
  and its average open price is reported as one minus the price its NO leg was bought at.

### Mass-status reconciliation

`generate_mass_status` reads the orders, fills, and positions for a window, and reports whether the
report set is complete. The set is complete only when the venue's live endpoints cover the whole
window and no order or fill could not be read.

Coverage is decided from the venue's historical cutoff. The live endpoints hold orders and fills
older than the later of the order-updated and trade-created cutoffs only in a separate historical
tier, which this adapter does not read. A window that starts before that floor, and an unbounded
window that asks for the whole history, are therefore reported incomplete, as is a cutoff that
cannot be read, which leaves coverage unproven. The adapter logs a warning when it marks a report
set incomplete.

## Precision and numeric handling

Kalshi prices are fixed-point dollars with up to four decimal places, and contract counts have a
minimum granularity of `0.01`. Every price and count arrives as a fixed-point string (`*_dollars`
and `*_fp`) and is parsed into an exact `Price` or `Quantity`; no value passes through a floating
point representation, because a sub-cent price or a fractional contract would lose its value.

| Constant                 | Value   | Meaning                                               |
| ------------------------ | ------- | ----------------------------------------------------- |
| `KALSHI_PRICE_PRECISION` | `4`     | Decimal places on prices.                             |
| `KALSHI_SIZE_PRECISION`  | `2`     | Decimal places on contract counts.                    |
| `KALSHI_CURRENCY`        | `USD`   | The currency every contract is denominated in.        |
| `MAX_PAGE_LIMIT`         | `1_000` | Maximum page size on paginated market-data endpoints. |

The execution client parses a received price at the finest of the instrument's declared precision
and the scale the value itself carries, so a price is never rounded down to a coarser scale. A
market's declared price precision is the finest step in its `price_ranges`: the exchange publishes
price bands with their own steps, and every price on the grid is a multiple of the finest one. A
tapered grid cannot be expressed as one increment, so prices inside a coarser band that fall between
the finer steps are still rejected by the exchange.

Outbound counts and prices are formatted with exactly the instrument's precision. Paginated reads
request at most 1,000 records per page and follow the cursor; one reconciliation pass reads at most
50 order pages before it reports the read as truncated.

## Limitations and considerations

- **REST only.** The adapter has no WebSocket client, so no market data or order state arrives as a
  stream. Books, quotes, trades, and order state are all learned by polling, and a book update is
  emitted as a snapshot that clears and rebuilds the book. Between two polls a book can print
  through levels that a consumer never sees.
- **The historical tier is not read.** The venue moves orders, fills, and settled markets older than
  its published cutoffs into separate historical endpoints, which this adapter does not call. A mass
  status window that starts before the cutoff is reported incomplete, and those records are not
  returned.
- **Sub-cent fees are inexact.** Kalshi quotes fees finer than USD's two-decimal scale, and the
  domain money type is denominated at the currency's scale, so those amounts are reported rounded,
  with a warning. Every price and contract count is exact.
- **The account total is only as fine as its inputs.** The venue's fixed-point balance is finer than
  its cent-denominated portfolio value, which the adapter adds to the cash balance to derive the
  account total.
- **A market order needs a book.** A market order is sent at the far side of the cached book and is
  denied when that side has no price.
- **A tapered price grid is approximated by one increment.** Prices inside a coarser band that fall
  between the finer steps are rejected by the exchange rather than by the adapter.
- **Exclusivity is claimed, not proven.** The adapter builds an event's outcome group from the
  exchange's documentation that its markets are mutually exclusive. Exhaustiveness is unknown.
- **A disputed market must not be settled automatically.** Its resolution is reported as
  `Disputed`. A scalar market, or an event with an undetermined leg or an unpublished effective
  time, produces no resolution at all.
- **`DAY` orders are refused.** A Kalshi order rests until it is canceled or the market closes, so
  the adapter denies any time in force outside `GTC`, `GTD`, `IOC`, and `FOK`.
- **No contingent orders.** Order lists are submitted as independent orders, and there is no OCO,
  OTO, or bracket support.
- **Batched cancellation is not used.** The venue's batched cancel cancels every working order
  rather than a named set, so `BatchCancelOrders` cancels one order per request.
- **Positions are read-only.** The adapter reports positions but offers no position-management
  commands.

## Client configuration

Rust structs and Python classes expose the same client configuration. Both configurations deny
unknown fields, so a misspelled key fails at construction rather than being ignored.

### Data client options

Class/struct: `KalshiDataClientConfig`.

| Option                 | Default | Description                                                             |
| ---------------------- | ------- | ----------------------------------------------------------------------- |
| `environment`          | `Demo`  | `KalshiEnvironment::Demo` or `Prod`; selects the default REST endpoint. |
| `base_url`             | `None`  | REST base URL, which overrides the environment's endpoint when set.     |
| `api_key_id`           | `None`  | API key ID, which falls back to `KALSHI_API_KEY_ID`.                    |
| `api_key_pem`          | `None`  | PEM-encoded RSA private key, which falls back to `KALSHI_API_KEY_PEM`.  |
| `http_timeout_secs`    | `None`  | HTTP request timeout in seconds.                                        |
| `proxy_url`            | `None`  | Proxy URL for HTTP requests.                                            |
| `event_tickers`        | `[]`    | Event tickers to load instruments for; empty loads every open market.   |
| `series_ticker`        | `None`  | A series ticker to load instruments for.                                |
| `poll_interval_millis` | `None`  | Interval between market polls; unset uses 2,000 milliseconds.           |

### Execution client options

Class/struct: `KalshiExecutionClientConfig`. The Rust struct is named `KalshiExecClientConfig`.

| Option                  | Default        | Description                                                                                  |
| ----------------------- | -------------- | -------------------------------------------------------------------------------------------- |
| `environment`           | `Demo`         | `KalshiEnvironment::Demo` or `Prod`; selects the default REST endpoint.                      |
| `base_url`              | `None`         | REST base URL, which overrides the environment's endpoint when set.                          |
| `api_key_id`            | `None`         | API key ID, which falls back to `KALSHI_API_KEY_ID`.                                         |
| `api_key_pem`           | `None`         | PEM-encoded RSA private key, which falls back to `KALSHI_API_KEY_PEM`.                       |
| `http_timeout_secs`     | `None`         | HTTP request timeout in seconds.                                                             |
| `proxy_url`             | `None`         | Proxy URL for HTTP requests.                                                                 |
| `reconciliation`        | `true`         | Reconcile working orders and positions when the client connects.                             |
| `account_id`            | `KALSHI-001`   | The account the execution client reports under.                                              |
| `self_trade_prevention` | `TakerAtCross` | `TakerAtCross` or `Maker`.                                                                   |
| `cancel_order_on_pause` | `None`         | Whether the exchange cancels the order when trading pauses; unset uses the exchange default. |
| `poll_interval_millis`  | `None`         | Interval between order polls; unset uses 2,000 milliseconds.                                 |

### Configuration example

```rust
use nautilus_kalshi::{
    common::enums::KalshiEnvironment,
    config::{KalshiDataClientConfig, KalshiExecClientConfig},
};

let data_config = KalshiDataClientConfig::builder()
    .environment(KalshiEnvironment::Prod)
    .event_tickers(vec!["KXHIGHNY-25JAN01".to_string()])
    .build();

let exec_config = KalshiExecClientConfig::builder()
    .environment(KalshiEnvironment::Prod)
    .reconciliation(true)
    .build();
```

Rust callers build the clients through the factories. Both factories report their name as `KALSHI`
(`KalshiDataClientFactory::name()` and `KalshiExecutionClientFactory::name()`).

The Python surface is deliberately narrow. The module exports the three constants and the two
configuration and factory pairs:

```python
from nautilus_trader.adapters.kalshi import KALSHI
from nautilus_trader.adapters.kalshi import KALSHI_CLIENT_ID
from nautilus_trader.adapters.kalshi import KALSHI_VENUE
from nautilus_trader.adapters.kalshi import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi import KalshiDataClientFactory
from nautilus_trader.adapters.kalshi import KalshiExecutionClientConfig
from nautilus_trader.adapters.kalshi import KalshiExecutionClientFactory

data_config = KalshiDataClientConfig(event_tickers=["KXHIGHNY-25JAN01"])
exec_config = KalshiExecutionClientConfig()
```

`KALSHI` is the venue identifier as a string, `KALSHI_VENUE` is that venue, and `KALSHI_CLIENT_ID`
is the execution client identity (`KALSHI-EXEC`). Pair each configuration with its factory when
building a live node.

The extension module also registers a `KalshiEnvironment` class and a `KalshiSelfTradePrevention`
class for the `environment` and `self_trade_prevention` fields, but the adapter module does not
re-export them.

## Contributing

:::info
For additional features or to contribute to the Kalshi adapter, please see our [contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
