# Databento

NautilusTrader includes an adapter for the [Databento](https://databento.com/) API and for data in
[Databento Binary Encoding (DBN)](https://databento.com/docs/conventions/databento-binary-encoding).
Databento is a market data provider only, so the adapter does not include an execution client. For execution
or signal use, you can:

- Pair it with a sandbox for simulated execution.
- Route execution through another adapter, such as Interactive Brokers.
- Use it to calculate traditional asset class signals for crypto trading.

The adapter supports:

- Loading historical data from DBN files and decoding to Nautilus objects for backtesting or catalog storage.
- Requesting historical data decoded to Nautilus objects for live trading and backtesting.
- Subscribing to real-time data feeds decoded to Nautilus objects for live trading and sandbox environments.

:::tip
[Databento](https://databento.com/signup) offers $125 in free data credits for new sign-ups.
Use the credits for historical data or the first month of a subscription plan. The team shares the credits,
which expire six months after signup. See Databento's
[pricing and credits FAQ](https://databento.com/docs/faqs/usage-pricing-and-data-credits).

With careful requests, this covers testing and evaluation. Check the
[metadata.get_cost](https://databento.com/docs/api-historical/metadata/metadata-get-cost)
endpoint before requesting data.
:::

## Overview

The adapter uses the [databento-rs](https://github.com/databento/databento-rs) crate,
Databento's official Rust client library.

:::info
You do not need to install `databento` separately. The adapter compiles as a
static library and links automatically during the build.
:::

The following adapter classes are available:

- `DatabentoDataLoader`: Loads DBN data from files.
- `DatabentoHistoricalClient`: Fetches historical market data and instrument definitions via the Databento HTTP API.
- `DatabentoLiveClient`: Subscribes to real-time data feeds via Databento's raw TCP API.
- `DatabentoDataClient`: Data client for live trading nodes, wrapping the historical and live clients.
- `DatabentoDataClientFactory`: Builds the data client from a `DatabentoDataClientConfig` for `LiveNode`.

:::note
Most users configure a live trading node (covered below) and do not work with
these components directly.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento/)

Rust examples live under
[`crates/adapters/databento/examples/`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/databento/examples/).
The data tester subscribes to live quotes and trades for the configured instrument when run:

```bash
cargo run --example databento-data-tester --package nautilus-databento
```

## Databento documentation

See the [Databento new users guide](https://databento.com/docs/quickstart/new-user-guides).
Refer to it alongside this integration guide.

## Databento Binary Encoding (DBN)

Databento Binary Encoding (DBN) is a fast message encoding and storage format for
normalized market data. The [DBN spec](https://databento.com/docs/conventions/databento-binary-encoding)
includes a self-describing metadata header and a fixed set of struct definitions
that standardize how market data is normalized.

The adapter decodes DBN data to Nautilus objects. The same Rust decoder handles:

- Loading and decoding DBN files from disk.
- Decoding historical and live data in real time.

## Supported schemas

The following Databento schemas are supported by NautilusTrader:

| Databento schema                                                                         | Nautilus data type            | Description                      |
| :--------------------------------------------------------------------------------------- | :---------------------------- | :------------------------------- |
| [`mbo`](https://databento.com/docs/schemas/mbo)                                          | `OrderBookDelta`, `TradeTick` | Market by order (L3).            |
| [`mbp-1`](https://databento.com/docs/schemas/mbp-1)                                      | `QuoteTick`, `TradeTick`      | Market by price (L1).            |
| [`mbp-10`](https://databento.com/docs/schemas/mbp-10)                                    | `OrderBookDepth`              | Market by price (L2), 10 levels. |
| [`bbo-1s`](https://databento.com/docs/schemas/bbo)                                       | `QuoteTick`                   | 1-second best bid/offer.         |
| [`bbo-1m`](https://databento.com/docs/schemas/bbo)                                       | `QuoteTick`                   | 1-minute best bid/offer.         |
| [`cmbp-1`](https://databento.com/docs/schemas/mbp-1#consolidated-market-by-price-cmbp-1) | `QuoteTick`, `TradeTick`      | Consolidated top of book.        |
| [`cbbo-1s`](https://databento.com/docs/schemas/bbo#consolidated-bbo-on-interval-cbbo)    | `QuoteTick`                   | Consolidated 1-second BBO.       |
| [`cbbo-1m`](https://databento.com/docs/schemas/bbo#consolidated-bbo-on-interval-cbbo)    | `QuoteTick`                   | Consolidated 1-minute BBO.       |
| [`tcbbo`](https://databento.com/docs/schemas/tbbo#consolidated-bbo-on-trade-tcbbo)       | `QuoteTick`, `TradeTick`      | Trade-sampled consolidated BBO.  |
| [`tbbo`](https://databento.com/docs/schemas/tbbo)                                        | `QuoteTick`, `TradeTick`      | Trade-sampled best bid/offer.    |
| [`trades`](https://databento.com/docs/schemas/trades)                                    | `TradeTick`                   | Trade ticks.                     |
| [`ohlcv-1s`](https://databento.com/docs/schemas/ohlcv)                                   | `Bar`                         | 1-second bars.                   |
| [`ohlcv-1m`](https://databento.com/docs/schemas/ohlcv)                                   | `Bar`                         | 1-minute bars.                   |
| [`ohlcv-1h`](https://databento.com/docs/schemas/ohlcv)                                   | `Bar`                         | 1-hour bars.                     |
| [`ohlcv-1d`](https://databento.com/docs/schemas/ohlcv)                                   | `Bar`                         | Daily bars.                      |
| [`definition`](https://databento.com/docs/schemas/instrument-definitions)                | `Instrument` (various types)  | Instrument definitions.          |
| [`imbalance`](https://databento.com/docs/schemas/imbalance)                              | `DatabentoImbalance`          | Auction imbalance data.          |
| [`statistics`](https://databento.com/docs/schemas/statistics)                            | `DatabentoStatistics`         | Market statistics.               |
| [`status`](https://databento.com/docs/schemas/status)                                    | `InstrumentStatus`            | Market status updates.           |

The table lists possible outputs. A quote requires both bid and ask prices. For `mbp-1` and `cmbp-1`,
the adapter emits trades only for trade records when trade decoding is enabled.

:::note
Databento also documents reference schemas, including corporate actions,
adjustment factors, and security master data. This adapter maps only the schemas
listed above to Nautilus data types. Daily Databento OHLCV uses `ohlcv-1d`, and
`ohlcv-eod` records also decode to daily bars. Official settlement prices and
open interest come from the `statistics` schema, not OHLCV bars.
:::

:::info
The adapter logs a warning and skips definitions for unsupported instrument classes, including index (`I`),
bond (`B`), and commodity spot (`Y`), and continues decoding the remaining definitions in the batch.
It also skips FX spot definitions whose currencies it cannot map. Index definitions can appear in OPRA and
in [Databento Core Indices](https://databento.com/docs/datasets/dbix-cgif), which includes the Cboe Global
Indices Feed. The bundled `publishers.json` maps publisher IDs to datasets and venues. It includes the legacy
Cboe `MAIN.CGIF` entries and lacks the IDs for `DBIX.CORE`. With the bundled file, the DBN loader and
historical client can fail at publisher lookup before the instrument-class check. Open an issue if you
need Nautilus modeling for these instrument classes.

For `GLBX.MDP3`, the adapter skips outright futures and options with a zero or undefined `unit_of_measure_qty`
(the contract multiplier). The warning includes the raw symbol and Databento instrument ID. A skipped definition
cannot supply the exchange venue, contract year, or price precision. Pass `price_precision` when requesting or
subscribing to market data for these symbols where supported. The historical client and DBN loader also support
`set_price_precision` for per-symbol overrides. When loading a single-instrument DBN file, you can also pass its
`instrument_id`. See [Price precision](#price-precision).

Statistics messages with `stat_type` values outside the modeled range (1-20) are
also skipped with a warning. This includes the venue-specific values
`VenueSpecificVolume1` (10001) and `VenueSpecificPrice1` (10002).
:::

### Schema considerations

- **TBBO and TCBBO**: Trade-sampled feeds that pair every trade with the BBO
  immediately *before* the trade's effect. Use them for trades aligned with
  contemporaneous quotes without managing two streams.
- **MBP-1 and CMBP-1 (L1)**: Event-level updates that emit trades only on trade
  events. Choose them for a complete top-of-book event tape. For quote and trade
  alignment, prefer TBBO or TCBBO.
- **MBP-10 (L2)**: Top 10 levels with trades. Use it for depth-aware strategies
  that do not need full MBO data. Includes orders per level. Databento serves
  this schema at 10 levels only, so depth requests must use `depth=10`.
- **MBO (L3)**: Per-order events for queue position modeling and exact book
  reconstruction. `subscribe_book_deltas()` requests no snapshot, so a node
  subscription streams from the point of subscription with no initial book state.
  A strategy that needs a complete book must start before the trading session,
  seed the book from a historical request, or drive `DatabentoLiveClient.subscribe`
  directly with `snapshot=True`.
- **BBO and CBBO intervals** sample top of book in 1-second or 1-minute intervals. Databento skips intervals
  without a trade or quote update. The adapter emits only `QuoteTick` for these schemas.
  Use them for monitoring, spreads, and low-cost signals. They are not suited
  for microstructure work.
- **TRADES**: Trades only. For quote context alongside trades, subscribe with
  MBP-1, which emits a `TradeTick` on every trade event, or use TBBO or TCBBO.
- **OHLCV**: Aggregated bars from trades. Use them for higher-timeframe analytics. Bars carry close timestamps
  by default. Set `bars_timestamp_on_close=False` to timestamp on the interval open. Daily bars use
  `ohlcv-1d`. Use `statistics` for official settlements and open interest.
- **Imbalance and statistics**: Venue operational data with no built-in Nautilus
  equivalent. Subscribe through `subscribe_data()`, or reach them through the
  historical client, the data loader, or the direct live client (see
  [Imbalance and statistics](#imbalance-and-statistics)).
- **Status**: Venue trading-state updates. Subscribe via
  `subscribe_instrument_status`.

:::tip
For cross-venue analysis, use consolidated top-of-book data such as OPRA's national best bid and offer.
The CME Globex consolidated book combines displayed and implied liquidity.
:::

:::info
See also the Databento [Schemas and data formats](https://databento.com/docs/schemas) guide.
:::

## Dataset availability and selection

Databento dataset IDs are separate from Nautilus venue identifiers. The adapter
supports the schemas listed above, but each Databento dataset exposes its own
subset. Check the metadata endpoints before adding a new dataset or schema to a
live configuration:

```bash
databento_metadata() {
  printf 'user = "%s:"\n' "$DATABENTO_API_KEY" | curl --config - "$@"
}

databento_metadata \
  "https://hist.databento.com/v0/metadata.list_schemas?dataset=EQUS.MINI"

databento_metadata \
  "https://hist.databento.com/v0/metadata.list_unit_prices?dataset=EQUS.MINI"

databento_metadata \
  "https://hist.databento.com/v0/metadata.get_cost" \
  --data-urlencode "dataset=EQUS.MINI" \
  --data-urlencode "symbols=AAPL" \
  --data-urlencode "stype_in=raw_symbol" \
  --data-urlencode "schema=bbo-1s" \
  --data-urlencode "start=2026-06-24T14:30:00Z" \
  --data-urlencode "end=2026-06-24T14:31:00Z"
```

For the two common evaluation datasets:

- [`GLBX.MDP3`](https://databento.com/docs/datasets/glbx-mdp3) is the CME Globex MDP 3.0 dataset for CME, CBOT, NYMEX, and
  COMEX futures, options on futures, and spreads. It supports MBO, MBP-1,
  MBP-10, TBBO, trades, BBO intervals, OHLCV, definitions, statistics, and
  status. CMBP-1 and CBBO intervals combine real and implied top-of-book liquidity. TCBBO is unavailable.
  The older MDP 2 history does not include MBO, CMBP-1, or CBBO.
- [`EQUS.MINI`](https://databento.com/docs/datasets/equs-mini) is Databento US Equities Mini. It is a derived aggregated
  top-of-book dataset with anonymized component venues. It supports MBP-1,
  TBBO, trades, BBO intervals, OHLCV, and definitions. It does not support
  MBO, MBP-10, imbalance, statistics, status, or consolidated schemas.

Use `EQUS` as the Nautilus venue for US Equities Mini instruments:
`AAPL.EQUS`, `MSFT.EQUS`, and so on. The built-in venue-to-dataset map routes
`EQUS` to `EQUS.MINI`. Venue codes such as `XNAS` and `XNYS` refer to
venue-specific datasets unless you override them with `venue_dataset_map`.

:::warning
If you override a venue such as `XNAS` to `EQUS.MINI`, keep downstream
instrument IDs consistent. Mini records carry the consolidated `EQUS` publisher,
and file or historical decoding without an explicit `instrument_id` emits
`*.EQUS` identifiers.
:::

Cost depends on the schema, symbols, and time range. For exploration, start with
tight ranges, `definition`, `bbo-1s`, `bbo-1m`, or `trades`, and call
`metadata.get_cost` before pulling historical time series data. Avoid duplicate
quote and trade subscriptions when a combined schema such as `mbp-1` or `tbbo`
already carries the data needed by the strategy.

## Subscriptions and requests

Nautilus subscription methods map to Databento schemas as follows:

| Nautilus subscription method    | Default schema | Available Databento schemas                                                  | Nautilus data type                          |
| :------------------------------ | :------------- | :--------------------------------------------------------------------------- | :------------------------------------------ |
| `subscribe_instrument()`        | `definition`   | `definition`                                                                 | `Instrument`                                |
| `subscribe_quotes()`            | `mbp-1`        | `mbp-1`, `bbo-1s`, `bbo-1m`, `cmbp-1`, `cbbo-1s`, `cbbo-1m`, `tbbo`, `tcbbo` | `QuoteTick`                                 |
| `subscribe_trades()`            | `trades`       | `trades`, `tbbo`, `tcbbo`, `mbp-1`, `cmbp-1`                                 | `TradeTick`                                 |
| `subscribe_book_deltas()`       | `mbo`          | `mbo`                                                                        | `OrderBookDeltas`                           |
| `subscribe_book_depth()`        | `mbp-10`       | `mbp-10`                                                                     | `OrderBookDepth`                            |
| `subscribe_bars()`              | none           | `ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, `ohlcv-1d`                               | `Bar`                                       |
| `subscribe_data()`              | none           | `statistics`, `imbalance`                                                    | `DatabentoStatistics`, `DatabentoImbalance` |
| `subscribe_instrument_status()` | `status`       | `status`                                                                     | `InstrumentStatus`                          |

Pass a non-default schema through the `schema` subscription parameter, as shown in the examples
below. Only `subscribe_quotes()` and `subscribe_trades()` accept a choice. `subscribe_bars()` takes
its schema from the bar aggregation, and `subscribe_data()` from the data type name
(`DatabentoStatistics` or `DatabentoImbalance`). The other methods always use the single schema
listed. The matching historical requests, `request_quotes()` and `request_trades()`, take the same
`schema` values and defaults.

`request_instrument()`, `request_quotes()`, and `request_trades()` accept identifier lists. The adapter
groups identifiers by dataset and preserves successful groups if another group fails. Check
`on_data_response()` for the completion state and failure reason. Quote and trade requests with
multiple IDs do not accept a record limit.

Book and bar requests require one list member. A parent-symbol `request_book_deltas()` can return
several book payloads under the same correlation ID; completion follows the last payload.

Actor and Strategy historical requests include both bounds on `ts_init`. Bar requests select bars
by close time even when `bars_timestamp_on_close=False`. Direct `DatabentoHistoricalClient` calls
use Databento's start-inclusive, end-exclusive bounds on the schema's index timestamp.

:::warning
The "Available Databento schemas" column lists adapter-supported choices for
that Nautilus subscription method. The selected dataset must also support the
schema. For example, `EQUS.MINI` cannot serve `mbo`, `mbp-10`, `statistics`, or
`status`.
:::

:::note
The examples below assume a `Strategy` or `DataActor` context where `self` has
subscription methods.

```python
from nautilus_trader.model import BarType
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId


DATABENTO_CLIENT_ID = ClientId.from_str("DATABENTO")
instrument_id = InstrumentId.from_str("ESZ26.GLBX")
```

:::

### Instrument definition subscriptions

```python
# Stream definition messages, which also populate the live price precision map
self.subscribe_instrument(
    instrument_id=instrument_id,
    client_id=DATABENTO_CLIENT_ID,
)
```

### Subscription lists

Implemented instrument subscriptions accept one ID or a non-empty list through `instrument_id`.
External bar subscriptions accept full bar types through `bar_type`.

#### Quote and trade batching

Quotes and trades use one subscription per dataset. The adapter validates every ID before dispatch
and applies `price_precision` to each symbol.

- A dataset enqueue failure is logged and does not stop later datasets.
- Accepted datasets are not rolled back. The client reports exactly their accepted IDs.
- The client adapter tracks accepted members and sends only missing IDs when another subscribe
  command reaches it. Retry is not automatic.

:::note
Repeating a subscribe call on the same actor sends no command while that actor retains its local
subscription. To retry, release that local subscription first, or subscribe from another owner.
:::

#### Other subscriptions

Other implemented instrument hooks and external bar hooks iterate members locally:

- Existing dataset and schema rules apply to each member.
- A member failure reports the exact synchronous accepted subset.
- Full bar types on the same instrument remain distinct.

The adapter leaves gateway subscriptions active until the session stops. See
[Connection stability](#connection-stability).

### Quote subscriptions (MBP and L1)

Choose one of these alternatives per instrument. While a quote subscription is active, the same actor
ignores further quote subscription calls for that instrument.

Default MBP-1 quotes also emit trades on trade events.

```python
self.subscribe_quotes(instrument_id, client_id=DATABENTO_CLIENT_ID)
```

Batch two contracts in `GLBX.MDP3` with one subscription and an explicit price precision.

```python
self.subscribe_quotes(
    instrument_id=[InstrumentId.from_str("ESZ26.GLBX"), InstrumentId.from_str("NQZ26.GLBX")],
    params={"price_precision": 2},
    client_id=DATABENTO_CLIENT_ID,
)
```

Select MBP-1 explicitly with `schema`.

```python
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)
```

Subscribe to 1-second BBO snapshots for quote ticks only.

```python
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "bbo-1s"},
    client_id=DATABENTO_CLIENT_ID,
)
```

Subscribe to combined real and implied quotes on CME Globex.

```python
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "cbbo-1s"},  # or "cmbp-1" for event-level updates
    client_id=DATABENTO_CLIENT_ID,
)
```

Subscribe to trade-sampled BBO to receive quote and trade ticks on the message bus.

```python
self.subscribe_quotes(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},
    client_id=DATABENTO_CLIENT_ID,
)
```

### Trade subscriptions

Trade calls accept an instrument list as described in
[Quote and trade batching](#quote-and-trade-batching). Choose one of these alternatives per instrument.

Subscribe to trade ticks only.

```python
self.subscribe_trades(instrument_id, client_id=DATABENTO_CLIENT_ID)
```

Subscribe to MBP-1 for trades on trade events.

```python
self.subscribe_trades(
    instrument_id=instrument_id,
    params={"schema": "mbp-1"},
    client_id=DATABENTO_CLIENT_ID,
)
```

Subscribe to TBBO for trades with quote context.

```python
self.subscribe_trades(
    instrument_id=instrument_id,
    params={"schema": "tbbo"},
    client_id=DATABENTO_CLIENT_ID,
)
```

### Order book deltas subscriptions (MBO and L3)

```python
# Subscribe to full order book updates (market by order)
self.subscribe_book_deltas(
    instrument_id=instrument_id,
    book_type=BookType.L3_MBO,  # Uses MBO schema
    client_id=DATABENTO_CLIENT_ID,
)

# Deltas stream from the point of subscription with no initial book snapshot
```

### Instrument status subscriptions

```python
# Subscribe to venue trading-state updates
self.subscribe_instrument_status(
    instrument_id=instrument_id,
    client_id=DATABENTO_CLIENT_ID,
)
```

### Depth, bar, and custom data subscriptions

`subscribe_book_depth()` streams `mbp-10` depth snapshots for each requested instrument. Databento serves a
depth of 10 only, so an explicit depth other than 10 is rejected. The bar aggregation in the
`BarType` selects the OHLCV schema for `subscribe_bars()` (`ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, or
`ohlcv-1d`). The step must be 1 and the price type must be `LAST`, the only price type the decoder
emits. `subscribe_data()` streams `DatabentoStatistics` or `DatabentoImbalance` records for the
instrument ID given as the data type identifier, and rejects a data type with metadata. Records are
published with that identifier, so a subscriber receives only the records of its own instrument.

These three subscriptions require raw symbols, such as `ESZ26.GLBX` or `AAPL.EQUS`. The adapter rejects
continuous (`ES.c.0`) and parent (`ES.FUT`) symbols because market data would arrive on the resolved
instrument's topic.

```python
instrument_id = InstrumentId.from_str("ESZ26.GLBX")

self.subscribe_book_depth(
    instrument_id=instrument_id,
    book_type=BookType.L2_MBP,
    client_id=DATABENTO_CLIENT_ID,
)

self.subscribe_bars(
    BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),
    client_id=DATABENTO_CLIENT_ID,
)

self.subscribe_data(
    DataType("DatabentoStatistics", identifier=str(instrument_id)),
    client_id=DATABENTO_CLIENT_ID,
)
```

### Historical requests for depth and bars

MBP-10 depth and OHLCV bars are available as historical requests. The bar aggregation in the
`BarType` selects the OHLCV schema (`ohlcv-1s`, `ohlcv-1m`, `ohlcv-1h`, or `ohlcv-1d`), and depth
requests use `mbp-10`:

```python
import pandas as pd


instrument_id = InstrumentId.from_str("ESM24.GLBX")

# Request 1-minute bars (uses the ohlcv-1m schema)
self.request_bars(
    bar_type=BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),
    start=pd.Timestamp("2024-03-06", tz="UTC"),
    end=pd.Timestamp("2024-03-07", tz="UTC"),
    client_id=DATABENTO_CLIENT_ID,
)

# Request top 10 levels of market depth (Databento serves depth=10 only)
self.request_book_depth(
    instrument_id=instrument_id,
    depth=10,
    start=pd.Timestamp("2024-03-06T14:30", tz="UTC"),
    end=pd.Timestamp("2024-03-06T14:31", tz="UTC"),
    client_id=DATABENTO_CLIENT_ID,
)
```

## Instrument IDs and symbology

Databento market data includes a numeric `instrument_id` field. The publisher usually assigns this ID.
Databento synthesizes one if the publisher does not provide it. Databento only guarantees this ID is unique
within a given day. This differs from the Nautilus `InstrumentId`, a string of symbol + venue separated by a
period: `"{symbol}.{venue}"`.

The decoder derives the Nautilus `symbol` from the Databento `raw_symbol`, normalizing supported futures
year suffixes as described below. Publisher IDs map to the default Nautilus venue through `publishers.json`.
Subscription `InstrumentId` metadata can also seed the symbol-to-venue map before market data arrives.

Dataset IDs follow Databento's
[dataset naming conventions](https://databento.com/docs/api-historical/basics/datasets),
which are distinct from the venue code in a Nautilus `InstrumentId`.

The adapter converts supported futures codes to native year digits for historical requests and live
subscriptions. For historical requests and subscriptions made through `DatabentoLiveClient` directly,
it infers `stype_in` from the symbol as follows.

- Symbols ending in `.FUT` or `.OPT` use Databento parent symbology, for example
  `ES.FUT.XCME`.
- Three-part symbols whose last part is numeric use continuous symbology, for
  example `ES.c.0.GLBX`.
- All-numeric symbols use Databento `instrument_id` symbology.
- All other symbols use raw symbol symbology, for example `ESZ26.XCME` or
  `AAPL.EQUS`.

Live node subscriptions through `DatabentoDataClient` send raw symbols. Use the direct live client to
stream parent, continuous, or numeric Databento instrument IDs.

Supported futures and futures-option IDs use two-digit years by default, such as `ESZ26.XCME` and
`ESZ26 P5000.XCME`. This applies to `GLBX.MDP3`, `IFEU.IMPACT`, `NDEX.IMPACT`, `IFUS.IMPACT`,
`IFLL.IMPACT`, `XEUR.EOBI`, `XEEE.EOBI`, `XCBF.PITCH`, and `XOSE.ITCH`. The instrument's `raw_symbol`
keeps the native symbol; futures-option `underlying` symbols use the same year digits as instrument IDs.

Rust APIs use `nautilus_model::identifiers::futures::FuturesYearDigits::{One, Two}`, with `Two` as the
default. Python configuration keeps the integer values `1` and `2`.

When using Databento data with Interactive Brokers execution, set `futures_year_digits` to the same
value in both adapters so their instrument IDs match.

Requests send native symbols: `ESU26-ESM27` is sent as `ESU6-ESM7`. Set `futures_year_digits=1` on the data
client config, data loader, historical client, or live client to retain existing IDs.

The factory convenience method `DatabentoDataClientFactory.create_live_data_client()` uses the default.
To retain old IDs, construct `DatabentoDataClient` directly with `futures_year_digits=1`.

To use `XOSE.ITCH`, supply a publisher metadata file with its publisher entry; the bundled file does not include it.

Load definitions for the data's dates through the same loader or historical client instance before
loading market data to use their contract-year context. Without a definition, canonical IDs in the current historical request also
identify contracts that trade into the following year. Earlier historical requests do not supply year hints. Otherwise, one-digit years resolve to the next matching
year at or after the record's `ts_recv` year (`ts_event` for OHLCV). Live records received before a
definition retain their resolved IDs; later records use the definition's year and venue. Repeated mappings
preserve definition-derived years and exchange venues. Inferred IDs refresh from subscription hints;
changed native symbols clear cached identity.

After upgrading, either set `futures_year_digits=1` or re-import existing data into a separate catalog,
rebuild caches, and update configured instrument IDs. Renaming catalog files does not change IDs stored
in records.

Share classes use a space in instrument IDs by default: Databento's `BRK.B` becomes `BRK B.EQUS`, since
a dot separates symbol and venue. This matches Interactive Brokers, which writes the class with a space.
Parent and continuous symbols such as `ES.FUT` and `ES.c.0` keep their dots. Requests send the native
dotted symbol. The instrument's `raw_symbol` keeps `BRK.B`. Set
`share_class_space=False` on the data client config, data loader, historical client, or live client to
retain existing IDs.

Use one symbology type per Databento request or subscription. Batch `AAPL.EQUS` with `MSFT.EQUS`, or
`ES.FUT.XCME` with `NQ.FUT.XCME`. For historical node requests that span datasets, the adapter applies
this rule to each dataset group.

For CME Globex MDP 3.0 (`GLBX.MDP3`), publisher defaults map to the `GLBX`
venue. When `use_exchange_as_venue=True`, definition messages can override
`GLBX` with the instrument's exchange MIC:

- `CBCM`: XCME-XCBT inter-exchange spread
- `NYUM`: XNYM-DUMX inter-exchange spread
- `XCBT`: Chicago Board of Trade (CBOT)
- `XCEC`: Commodities Exchange Center (COMEX)
- `XCME`: Chicago Mercantile Exchange (CME)
- `XFXS`: CME FX Link spread
- `XNYM`: New York Mercantile Exchange (NYMEX)

:::info
Other venue MICs are in the `venue` field of responses from
the [metadata.list_publishers](https://databento.com/docs/api-historical/metadata/metadata-list-publishers) endpoint.
:::

## Timestamps

Databento data includes these timestamp fields:

- `ts_event`: Matching-engine-received timestamp in nanoseconds since the UNIX epoch.
- `ts_in_delta` is a signed offset in nanoseconds. Subtract a populated, unclamped offset from `ts_recv` to
  recover the source timestamp, usually the publisher sending time. For `DBIX.CORE.MSTAR` MBP-1, trades, and
  statistics, the offset refers to `TransactTime`, and `ts_event` holds the gateway's `SendingTime`.
  `EQUS.MINI` and `OPRA.PILLAR` leave the offset at zero.
- `ts_recv`: Capture-server-received timestamp in nanoseconds since the UNIX epoch.
- `ts_out`: Databento sending timestamp (live only).

Nautilus data requires at least two timestamps (per the `Data` contract):

- `ts_event`: UNIX timestamp (nanoseconds) when the data event occurred.
- `ts_init`: UNIX timestamp (nanoseconds) when the data instance was created.

Market data such as quotes, trades, and book updates uses Databento `ts_recv` as Nautilus `ts_event`.
Databento synchronizes its capture clocks to UTC and documents `ts_recv` as monotonic per symbol. Publisher
event timestamps can come from different clocks and arrive out of order. For BBO and CBBO, `ts_recv` marks
the end of the interval. Bars use the DBN bar interval timestamp.
`bars_timestamp_on_close` controls whether Nautilus bars use the interval open or close timestamp.
`InstrumentStatus` uses the DBN record header `ts_event`. `DatabentoImbalance` and `DatabentoStatistics`
keep the Databento `ts_event` and `ts_recv` fields separately. `DatabentoStatistics` also keeps `ts_ref`
and `ts_in_delta` from the record for reference-time and latency analysis.

:::info
See these Databento docs for details:

- [Databento standards and conventions - timestamps](https://databento.com/docs/conventions/common-fields-enums-types#timestamps)
- [Databento timestamping guide](https://databento.com/docs/architecture/timestamping-guide)

:::

## Data types

This section maps Databento schemas to Nautilus data types.

:::info
See Databento [schemas and data formats](https://databento.com/docs/schemas).
:::

### Instrument definitions

Databento uses a single schema for all instrument classes. The decoder maps each
to the appropriate Nautilus `Instrument` type.

| Databento instrument class | Code | Nautilus instrument type |
| -------------------------- | ---- | ------------------------ |
| Stock                      | `K`  | `Equity`                 |
| Future                     | `F`  | `FuturesContract`        |
| Call                       | `C`  | `OptionContract`         |
| Put                        | `P`  | `OptionContract`         |
| Future spread              | `S`  | `FuturesSpread`          |
| Option spread              | `T`  | `OptionSpread`           |
| Mixed spread               | `M`  | `OptionSpread`           |
| FX spot                    | `X`  | `CurrencyPair`           |
| Index                      | `I`  | Not yet available        |
| Bond                       | `B`  | Not yet available        |
| Commodity spot             | `Y`  | Not yet available        |

### Option exercise style and settlement type

The decoder sets `exercise_style` and `settlement_type` on each `OptionContract` from the CFI code
(ISO 10962) in the definition. A field stays `None` when the code leaves that attribute undefined.

`OPRA.PILLAR` definitions arrive without a CFI code. For this dataset, the decoder applies US listing
conventions by option root when the CFI code supplies neither term.

- **Cash-settled, European-style**: a fixed list of index, currency, and index binary option roots,
  such as `SPX`, `SPXW`, `XSP`, `VIX`, `NDX`, `RUT`, and `DJX`.
- **Cash-settled, American-style**: `OEX` and `XAU`.
- **Ambiguous roots**: both fields stay `None`. A five-letter root that starts with `K` can be a binary
  option on a company metric or an equity option such as `KELYA`, and the definition does not tell them
  apart. A root with a digit before a letter, such as `B1SPY`, is treated the same way.
- **Any other root**: American-style and physically settled, the convention for equity and ETF options.

:::warning
The OPRA root lists are fixed in the adapter. A cash-settled root that the adapter does not list, such as
one listed after the adapter release, decodes as American-style and physically settled unless it is an
ambiguous root.
:::

### Option expiration correction

OPRA option definitions (dataset `OPRA.PILLAR`) carry the expiration with date-level precision: the
time-of-day is zeroed to midnight UTC. Thus, an option expiring at 16:00 New York time arrives stamped on the
prior evening in New York. The matching engine then treats the contract as expired before its final trading
session. By default, the loader corrects such midnight-UTC OPRA expirations to 16:00 New York time. It leaves
every other dataset untouched. It also leaves expirations with an intraday time, such as CME Globex,
untouched.

Override the default, or set per-underlying times, with `expiration_overrides`. It maps a dataset to
a mapping of underlying symbol to time, where the reserved key `default` sets the dataset-wide time:

```python
loader.load_instruments(
    filepath=path,
    use_exchange_as_venue=False,
    expiration_overrides={
        "OPRA.PILLAR": {"default": "16:00", "SPX": "09:30"},
    },
)
```

Times use `HH:MM` or `HH:MM:SS` in the exchange-local timezone (New York for OPRA). Only a dataset with a
built-in correction rule can be tuned, and `OPRA.PILLAR` is the only such dataset. An unknown or rule-less
dataset such as `GLBX.MDP3` raises a `ValueError`. The correction keys on the option's underlying, so it
cannot distinguish series that share an underlying but settle at different times (for example AM-settled SPX
versus PM-settled SPXW). Set the time that matches the contracts you are loading.

### Price precision

Databento raw prices are fixed-point integers scaled by 1e-9. Instrument definitions supply tick-size
precision, which can be insufficient for reported prices.

- **Market prices**: Must be exactly representable at the resolved precision; decoding never promotes precision.
- **Statistics**: Use at least the native nine-decimal wire precision to preserve VWAP, volatility, and delta.
  Higher resolved precision remains in effect.
- **Option strikes**: Use the precision needed for the wire value, with a floor at the strike currency's
  minor-unit precision. Ordinary USD strikes keep two decimal places; adjusted strikes retain finer digits.

Insufficient market precision causes a decoding error. Historical requests and file loading stop at the first
rejected record. Live feeds log and skip affected market and imbalance records.
Precision errors name the instrument, field, value, and required precision. Select sufficient `price_precision`
explicitly to decode finer prices without rounding.

:::warning[Sub-penny equity prices]
A stock with a `0.01` order tick can trade at `100.005`, the midpoint of a `100.00` bid and a `100.01` offer.
Strict decoding rejects trade prices and OHLCV values that need more decimal places than the resolved precision.

- For **combined records** with trade decoding enabled, the decoder rejects the whole quote/trade record if it
  cannot represent the trade price. Historical and loader MBP-1/CMBP-1 quote-only paths omit trade decoding, so
  their quotes are unaffected by trade prices.
- For **live data**, pass `price_precision` in `params` on `subscribe_quotes()` or `subscribe_trades()` for the
  affected instrument. The decoder uses the override even after the instrument definition arrives. The
  published definition keeps the exchange tick metadata.
- For **historical requests and files**, set `price_precision` to cover the reported prices, such as `4` for
  four-decimal equity prices.
- For **backtests**, the decode override leaves the instrument definition at its original precision. Use the
  same `price_precision` for the registered instrument and the data you decode for matching. The matching
  engine skips quote ticks, trade ticks, and executable bars with a different price precision, even when the
  strategy receives them. Keep the original tick size by writing it at the higher precision. For example,
  set `price_precision=4` and `price_increment=Price.from_str("0.0100")` for a one-cent tick. The matching
  engine still checks submitted order prices against that increment.

:::

#### Live feeds

The feed handler populates a per-instrument precision map from `InstrumentDefMsg` records as they arrive.
Market data handlers resolve precision in this order:

1. Subscription-supplied precision matched to the record `instrument_id` through a symbol mapping message.
1. Subscription-supplied precision matched directly on the native Databento symbol.
1. Precision from an `InstrumentDefMsg` already seen for the Databento record `instrument_id`.
1. The USD default precision of 2.

Pass `price_precision` in the node's `subscribe_quotes()` or `subscribe_trades()` parameters. Those are the
only node subscriptions that read this parameter. The direct `DatabentoLiveClient.subscribe()` accepts a
`price_precisions` list aligned with `instrument_ids`, including for parent and continuous subscriptions.

**Definition timing**: Without an explicit override, definitions must arrive before market data for instruments
with non-standard tick sizes, such as treasury futures with 1/256 ticks. Subscribe to the `definition` schema
before or alongside market data subscriptions.

:::tip
At strategy start, call `subscribe_instrument()` for each instrument to populate the live precision map.
A symbol's `price_precision` override lasts for the dataset session. An override on a quote or trade
subscription also covers its order book deltas, depth, bars, statistics, and imbalance. Statistics keep their
nine-decimal minimum; `InstrumentStatus` has no prices and needs no precision.
:::

#### Historical requests and file loading

Precision is resolved per record in this order:

1. An explicit `price_precision` argument on the call.
1. A per-symbol cache populated by loading definitions (`load_instruments`
   on the file loader, `get_range_instruments` on the historical client) or
   by an explicit `set_price_precision(symbol, precision)` call using the Nautilus symbol, such as `ESZ26`.

The node data client attempts to seed a missing precision from definitions, starting at UTC midnight on
the request's first query date to include any definition snapshot. The market data query keeps its own time
range. The direct Python `DatabentoHistoricalClient.get_order_book_depth()` has no per-call precision argument.
Load definitions or call `set_price_precision()` before requesting depth through that method.

The USD default applies only to live feeds. Historical client and loader calls return an error if precision
remains unresolved. For node requests, the engine logs the failure and reports it in `on_data_response()`
with any results from successful dataset groups.

### MBO (market by order)

MBO is the highest granularity data from Databento, representing full order book
depth. Some messages include trade data. The decoder produces an `OrderBookDelta`
and optionally a `TradeTick`. The live client emits a `TradeTick` only after the
instrument has received its first delta.

The live client buffers MBO messages until a record's `F_LAST` flag closes the match event, then passes one
`OrderBookDeltas` container to the handler. It honors the raw flag even on records that produce no delta
(fill attribution, status actions, or trades) or fail decoding. This prevents partial events from being
stranded and merged into the next event.

When MBO carries the instrument's trades (see
[Real-time client architecture](#real-time-client-architecture)), a trade that closes the event follows the
flushed container.

Snapshot records (`F_SNAPSHOT`) accumulate into the same buffer and flush with the first non-snapshot event
boundary. Thus, a snapshot reaches the handler as one `OrderBookDeltas` container. With a replay `start`
anchor, the client buffers replayed MBO deltas and drops replayed MBO trades. It flushes the deltas when a
non-snapshot `F_LAST` boundary has a `ts_recv` later than the session start time. If that record carries a
live delta, the client includes it in the same batch as the replayed deltas.

### MBP-1 (market by price, top-of-book)

MBP-1 represents top-of-book quotes and trades. Some messages carry trade data.
The decoder produces a `QuoteTick` and also a `TradeTick` when the message is
a trade.

### TBBO and TCBBO (top-of-book with trades)

TBBO and TCBBO provide both quote and trade data in each message. Both schemas
emit a `TradeTick` per message plus a `QuoteTick`, more efficient than separate
quote and trade subscriptions. The quote is skipped when either the bid or ask
price is undefined. TCBBO provides consolidated data across venues.

#### Trade IDs

The CMBP-1 and TCBBO schemas do not publish a native trade identifier. The decoder derives a deterministic
`TradeId` by FNV-1a hashing the instrument ID, `ts_event`, `ts_recv`, price, size, and aggressor side of the
trade. The same record yields the same trade ID across replays when it resolves to the same Nautilus
instrument ID. This lets consumers deduplicate replayed trades. Distinct trades with identical input fields
also share an ID.

Trades decoded from `trades`, `mbp-1`, and `tbbo` use the DBN `sequence` as their trade ID. `EQUS.MINI`
sets `sequence` to `0`, so trade IDs repeat on that dataset.

### OHLCV (bar aggregates)

Databento timestamps bar messages at the **open** of the interval. By default,
the decoder normalizes bar `ts_event` to the bar **close**: the original
`ts_event` plus the interval. `ts_init` uses the live receipt time, or the close
time for historical and file-based loads when no explicit init timestamp is
supplied. To set `ts_event` to the interval open, use `bars_timestamp_on_close=False` in the data client
config or direct live client, or `timestamp_on_close=False` on `get_range_bars()` or `load_bars()`.

### Imbalance and statistics

The `imbalance` and `statistics` schemas have no built-in Nautilus equivalents.
The adapter defines `DatabentoImbalance` and `DatabentoStatistics` in Rust, and
Python bindings expose both types from `nautilus_trader.adapters.databento`.

Subscribe to live records with `subscribe_data()` and a `DataType` named `DatabentoStatistics` or
`DatabentoImbalance` whose identifier is the instrument ID. Historical requests and files use:

- `DatabentoDataLoader.load_imbalance` and `load_statistics` for DBN files.
- `DatabentoHistoricalClient.get_range_imbalance` and `get_range_statistics` for historical ranges.

`DatabentoLiveClient.subscribe` with the `imbalance` or `statistics` schema also streams them
outside a node.

Request a bounded range of `statistics` for the `ES.FUT` parent symbol
(all active E-mini S&P 500 futures). Both `get_range_*` methods are asynchronous.
Use Databento's Historical
[`metadata.get_cost`](https://databento.com/docs/api-historical/metadata/metadata-get-cost)
endpoint before real historical pulls:

```python
import os

from nautilus_trader.adapters.databento import DatabentoHistoricalClient
from nautilus_trader.core import dt_to_unix_nanos
from nautilus_trader.model import InstrumentId


client = DatabentoHistoricalClient(
    key=os.environ["DATABENTO_API_KEY"],
    publishers_filepath="publishers.json",
    use_exchange_as_venue=False,
)

statistics = await client.get_range_statistics(
    dataset="GLBX.MDP3",
    instrument_ids=[InstrumentId.from_str("ES.FUT.GLBX")],
    start=dt_to_unix_nanos("2024-03-06T00:00:00Z"),
    end=dt_to_unix_nanos("2024-03-07T00:00:00Z"),
    price_precision=2,
)
```

The example uses `price_precision=2` for lookup; returned statistic prices have nine decimal places.
A fresh historical client has no cached precision:

- **Required setup**: Supply `price_precision` or first call `get_range_instruments` for the same range.
  Otherwise, the first decoded record aborts the request.
- **Parent symbols**: `set_price_precision` cannot seed `ES.FUT`; records resolve to individual contract symbols.

### Arrow encoding for imbalance and statistics

Both types implement Arrow record batch encoding and decoding. The
`nautilus_databento::arrow` module exposes it in Rust behind the `arrow` feature flag:

```rust
use nautilus_databento::arrow::imbalance::{
    decode_imbalance_batch,
    imbalance_to_arrow_record_batch,
};

let batch = imbalance_to_arrow_record_batch(&imbalances)?;

let metadata = batch.schema().metadata().clone();
let decoded = decode_imbalance_batch(&metadata, &batch)?;
```

The `statistics` module follows the same pattern with
`decode_statistics_batch` and `statistics_to_arrow_record_batch`. Call
`get_databento_arrow_schema_map(DatabentoImbalance)` from Python to inspect the
Arrow field map for either type.

- **Batch consistency**: `statistics_to_arrow_record_batch` and registered custom-data encoding reject mixed
  instruments, price precisions, or quantity precisions. Null fields do not constrain precision. Align
  precisions explicitly before batching.
- **Explicit metadata**: In Rust, supply `DatabentoStatistics::encode_batch` with sufficient precision metadata.
  It must name the same instrument for every row and represent every value exactly. Decoded values use that precision.

Importing the Python adapter registers Arrow codecs for both types. In Rust, call
`nautilus_databento::types::register_databento_custom_data()` with the `arrow` feature enabled before writing
or querying these types as custom data in a `ParquetDataCatalog`. To replay them, query the `CustomData`
records and pass them to `BacktestEngine.add_data()`. `BacktestDataConfig` rejects custom types in
`BacktestNode` runs.

:::info
Python loader and historical methods return the underlying Databento objects. Wrapping them with
`CustomData(...)` in Python does not produce the native payload the registered Arrow encoder expects.
Use the Rust catalog API to write those records to Parquet, or process the loaded records directly.
:::

## Performance considerations

Two options for backtesting with DBN data:

- Store data as DBN (`.dbn.zst`) files and decode to Nautilus objects every run.
- Convert DBN files to Nautilus objects once and write to the data catalog (Nautilus Parquet format).

For repeated backtests, convert the data once and replay from the Parquet catalog. [DataFusion](https://datafusion.apache.org/)
streams the converted data from disk, so each run avoids decoding DBN again.

:::note
Measured decode and client throughput for this adapter is recorded in
[`crates/adapters/databento/benches/BENCHMARKS.md`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/databento/benches/BENCHMARKS.md),
along with the command that reproduces it. Absolute numbers vary by machine, so
only same-machine deltas are meaningful.
:::

For live data, decoded delivery from the feed handler to Nautilus uses an unbounded queue so slow consumers
do not stall decoding. If a consumer falls behind for long enough, queued data can exhaust memory and stop
the process.

## Loading DBN data

`DatabentoDataLoader` decodes zstd-compressed DBN files (`.dbn.zst`) directly into Nautilus objects. It exposes
a method for each supported output type, including `load_instruments`, `load_order_book_deltas`,
`load_order_book_depth`, `load_quotes`, `load_trades`, `load_bars`, `load_status`,
`load_imbalance`, and `load_statistics`.

Pass the publisher metadata file when it is not available beside the running executable:

```python
from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.model import InstrumentId


loader = DatabentoDataLoader(publishers_filepath="publishers.json")

instruments = loader.load_instruments(
    filepath="equity-definitions.dbn.zst",
    use_exchange_as_venue=True,
)
trades = loader.load_trades(
    filepath="aapl-trades.dbn.zst",
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
)
```

Store the definitions alongside market data in a `ParquetDataCatalog` so a backtest can load the associated
instruments.

```python
from nautilus_trader.persistence import ParquetDataCatalog


catalog = ParquetDataCatalog(base_path="catalog")
catalog.write_instruments(instruments)
catalog.write_trades(trades)
```

`load_quotes` reads MBP-1 or TBBO quotes. For other quote schemas or trades in a combined record, use the
schema-specific method.

- `load_bbo_quotes` for BBO interval quotes.
- `load_cmbp_quotes` for CMBP-1 quotes.
- `load_cbbo_quotes` for CBBO quotes.
- `load_tbbo_trades` for TBBO trades.
- `load_tcbbo_trades` for TCBBO trades.

Call `schema_for_file` to read a file's schema from its DBN metadata header when picking the loader
method.

Optional `instrument_id` and `price_precision` arguments bypass symbology or precision lookup when
those values are already known. An explicit `instrument_id` is used as given; match its year digits to the
loader's `futures_year_digits`. The bar loader also accepts `timestamp_on_close`.

## Real-time client architecture

The `DatabentoDataClient` wraps the other Databento adapter classes. It creates one live feed handler per
dataset on the first subscription for that dataset, and every schema for that dataset shares the handler. The
handler runs one async task that races the next gateway record against the next engine command. Thus, later
subscriptions reach the running session without a reconnect.

The `trades`, `mbp-1`, `tbbo`, `cmbp-1`, `tcbbo`, and `mbo` schemas all carry an instrument's trades. The
handler forwards trades only from the DBN record type that delivers the instrument's first trade, until the
handler reconnects. For example, with `mbp-1` for quotes alongside `trades`, both subscriptions stay active
and only the selected source emits trade ticks. Quotes and book deltas from all subscriptions still flow.
Which schema delivers first can differ between sessions, so with `mbo` beside another trade schema, a trade
does not always follow its event's book deltas. Limitations:

- The `mbp-1` and `tbbo` schemas share a record type, so subscribing to both for one instrument still
  delivers each trade, and the quote at each trade, twice.
- A continuous symbol stops carrying a contract when it rolls. If a schema subscribed on a continuous symbol
  carries a contract's trades, the other schemas' trades for that contract stay dropped after the roll.

A single `DatabentoHistoricalClient` serves every historical request from the
data client, including the instrument definitions fetched to seed price precision.

:::warning
Queue direct live client subscriptions with a replay `start` before calling `start()`. Databento accepts
[replay anchors](https://databento.com/docs/api-live/basics/intraday-replay) only before the session starts.
For a late anchor, the feed handler logs an error and still sends the subscription. If the gateway accepts
the subscription, it streams live data without replaying history. The handler strips the anchor from saved
subscriptions after sending them, so automatic reconnection resubscribes without replaying history.
:::

## Configuration

Create `DatabentoDataClientConfig` from the adapter's public Python module. The API key and
`publishers.json` path are required:

```python
import os
from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataClientConfig


config = DatabentoDataClientConfig(
    api_key=os.environ["DATABENTO_API_KEY"],
    publishers_filepath=Path("publishers.json"),
    use_exchange_as_venue=False,
)
```

Download the canonical
[`publishers.json`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/databento/publishers.json)
and point `publishers_filepath` at the local copy.

| Option                    | Default  | Description                                             |
| ------------------------- | -------- | ------------------------------------------------------- |
| `api_key`                 | Required | Databento API key.                                      |
| `publishers_filepath`     | Required | Local path to Databento publisher metadata.             |
| `use_exchange_as_venue`   | `False`  | Use exchange MIC venues for GLBX instruments.           |
| `futures_year_digits`     | `2`      | Use two-digit years, or `1` for existing futures IDs.   |
| `share_class_space`       | `True`   | Spell share classes with a space (`BRK B`).             |
| `bars_timestamp_on_close` | `True`   | Timestamp bars on close instead of the interval open.   |
| `venue_dataset_map`       | `None`   | Override venue-to-dataset mappings from publisher data. |
| `historical_base_url`     | `None`   | Override the Historical API base URL.                   |
| `live_gateway_addr`       | `None`   | Override the live gateway socket address (`host:port`). |

Use `DatabentoDataClientConfig` with `DatabentoDataClientFactory`. The current
[Python example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/databento/data_tester.py)
shows the complete `LiveNode.builder(...)` configuration.

### Connection stability

The live client reconnects automatically on:

- **Network interruptions**: Temporary connectivity issues.
- **Gateway restarts**: Databento scheduled live gateway restarts. See the
  [maintenance schedule](https://databento.com/docs/api-live/basics/maintenance-schedule).
- **Market closures**: Sessions ending during off-hours.

#### Reconnection strategy

The factory-backed live client uses an internal 10-minute reconnection window with exponential
backoff from 1 second, capped at 60 seconds. The Python `DatabentoDataClientConfig` constructor does
not expose a reconnection timeout. Once the window elapses without a successful session, the client
gives up and reports an error rather than retrying indefinitely.

Stalled connections are detected by the upstream Databento client, which raises a heartbeat timeout
when no data arrives within the heartbeat interval plus 5 seconds. The feed handler treats that as a
connection error and enters the same backoff loop.

All reconnections include:

- **Jitter**: Random delay (up to 1 second) to prevent simultaneous reconnection storms.
- **Automatic resubscription**: Restores all active subscriptions after reconnecting.
- The handler resets the retry window and backoff when the gateway closes the connection or a read fails
  at least 60 seconds after the connection attempt began.
- **Command buffering**: Commands received during backoff are applied to the next session.

Databento supports [granular unsubscribe](https://databento.com/docs/api-live/client/unsubscribe). NautilusTrader's
adapter logs individual unsubscribe requests at debug level and keeps the gateway subscriptions active.
Stop the session to remove its subscriptions from the gateway.

#### Scheduled maintenance

Databento restarts live gateways on this schedule (all clients disconnect):

| Dataset            | Restart time      |
| ------------------ | ----------------- |
| CME Globex         | Saturday 02:15 CT |
| All ICE venues     | Sunday 09:45 UTC  |
| All other datasets | Sunday 10:30 UTC  |

Databento recommends waiting 15 minutes after the scheduled restart time. The node's 10-minute automatic
retry window can expire during maintenance. If it expires, restart the live node once the gateway is available.

## Contributing

:::info
To contribute, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
