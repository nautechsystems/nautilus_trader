# FinFeed

[FinFeedAPI Prediction Markets API](https://www.finfeedapi.com/products/prediction-markets-api?ref=nautilus)
by [API Bricks](https://www.apibricks.io/?ref=nautilus) supplies REST metadata, activity, books, and OHLCV.
The NautilusTrader adapter supports **Polymarket binary Yes/No instruments** through requests and polling.

**Data only**: Client ID `FINFEED`; no execution client, account operations, or order routing.
FinFeed's currencies, stock, SEC, MCP, and JSON-RPC products are outside scope.

## Overview

The Rust adapter includes Python bindings and needs no external FinFeed library.
`nautilus_trader.adapters.finfeed` exports:

| Export                         | Purpose                                             |
| ------------------------------ | --------------------------------------------------- |
| `FinFeedDataClientConfig`      | Startup selections, HTTP controls, polling.         |
| `FinFeedInstrumentSelection`   | Exact outcome IDs from one exchange.                |
| `FinFeedDataClientFactory`     | `LiveNode` factory registration.                    |
| `FinFeedDataLoader`            | Cached definitions and standalone Nautilus history. |
| `FinFeedHttpClient`            | Async read-only REST queries and typed results.     |
| `FINFEED`, `FINFEED_CLIENT_ID` | Default client name and routing ID.                 |

Start with [Python configuration](#python-configuration) or the [examples](#examples).

## Supported capabilities

| Capability                 | Interface            | Support / limit                                             |
| -------------------------- | -------------------- | ----------------------------------------------------------- |
| Discovery / metadata       | REST                 | Raw listings; native Polymarket binary Yes/No definitions.  |
| Quote / trade ticks        | Polling              | Sampled activity; bounded deduplication, no gap recovery.   |
| L2 deltas                  | Polling              | Full replacement of supplied levels.                        |
| L2 depth                   | Polling              | Snapshots, locally limited depth.                           |
| Current book               | REST                 | L2 market-by-price (MBP).                                   |
| Historical quotes / trades | REST                 | Covering UTC hours; checked overlapping continuation.       |
| Historical bars            | REST                 | Per-market external `LAST` OHLCV.                           |
| Flat-file OHLCV            | Raw Rust/Python REST | Explicit source; no native conversion.                      |
| Exchange-wide OHLCV        | Raw Rust/Python REST | Up to 24 hours; no definition loading or native conversion. |
| Closed bars                | Polling              | Completed per-market periods.                               |
| Resolution                 | Metadata refresh     | Closed/resolved ends polling; no settlement event.          |
| Historical books           | REST                 | L2 deltas, depth, point snapshots; seed required.           |
| L3 books                   | Unsupported          | No order-level replay.                                      |
| Execution                  | Unsupported          | No accounts or order routing.                               |

## Integrator attribution

- **API requests**: Every managed attempt, including retries and reconnects, sends
  `X-Referrer: nautilus`. This fixed header has no override and contains no credential or account data.
- **Signup**: Open an [API Bricks referral link](https://www.apibricks.io/?ref=nautilus) before creating
  an account in the same browser. Signup and request attribution are separate;
  see the [referral documentation](https://www.apibricks.io/platform/docs/general/referral?ref=nautilus).

## Examples

The [Polymarket order book backtest tutorial](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_finfeed.py)
collects paginated historical L2 data, caches it in Parquet, and runs an imbalance strategy with plotted results.

### Running the examples

Before running:

- Configure [prediction-market authentication](#authentication-and-environment).
- For Python, install a build with FinFeed bindings in `python/.venv`;
  see [installation](../getting_started/installation.md).
- Check provider retention for the historical examples' fixed past windows.

Run from the repository root:

```bash tab="Rust"
CARGO_BUILD_JOBS=8 cargo run --locked -p nautilus-finfeed --example node_data_tester
```

```bash tab="Python"
uv run --project python --no-sync python examples/live/finfeed/data_tester.py
```

The [Rust](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/finfeed/examples/node_data_tester.rs)
and [Python](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/data_tester.py)
examples register the factory with `LiveNode` and route `DataTester` through `FINFEED`.

### Defaults and request cost

| Setting           | Example value                                             |
| ----------------- | --------------------------------------------------------- |
| Outcome           | `2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO`.          |
| Subscriptions     | Quotes, trades, L2 book replacements; logs incoming data. |
| Stop timer        | 60 seconds including startup; shutdown can take longer.   |
| Managed allowance | Eight attempts; no application retries.                   |
| Cadence           | 10-second shared pacing; 60-second poll delay.            |

No orders, bars, or history requests run. Bootstrap costs two attempts; initial activity/book polls cost one each.
For longer runs, [size the allowance](#request-cost-and-retries).

### Selecting another outcome

Use `POLYMARKET` with the provider's
[active-market endpoint](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/markets/markets/exchange_id/active/get?ref=nautilus).
Set `MARKET_ID` to an exact binary Yes/No outcome ID, including its suffix.
Full metadata must pass the [definition rules](#supported-venues-and-outcomes).

Use the loaded precision and FinFeed instrument ID. Do not round data or substitute a native
Polymarket ID. Closed or unavailable markets may produce no new activity.

### Discovering an outcome in Python

The [discovery example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/discovery.py)
reads one active-ID page, checks at most three full definitions, and selects the first open outcome
accepted by `to_instrument()`. It creates an explicit selection, requests its definition, and subscribes to quotes.

```bash
uv run --project python --no-sync python examples/live/finfeed/discovery.py
```

- **Allowances**: Four managed attempts for discovery and four separately for the node.
- **Timing**: Both clients use 10-second pacing, 10-second HTTP timeouts, and no application retries.
  Discovery has a 60-second deadline; the node's 60-second timer includes bootstrap. Shutdown can take longer.
- **No match**: Raises `RuntimeError` before building the node. Change the script constants for other bounds.

`DataTesterConfig(request_instruments=True)` refreshes the explicit selection; it does not discover a catalog.

### Historical book replay in Python

The [historical-book example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/historical_book.py)
replays a 115-second window for a provider-classified closed outcome into an L2 `OrderBook`:

```bash
uv run --project python --no-sync python examples/live/finfeed/historical_book.py
```

| Control                       | Bound                                                           |
| ----------------------------- | --------------------------------------------------------------- |
| Managed attempts              | Three: exchange discovery, full definition, whole-hour history. |
| Pacing / HTTP timeout         | 10 seconds / 60 seconds.                                        |
| Attempt / invocation deadline | 75 seconds including admission / 90 seconds.                    |
| Response deadline             | 150 seconds including bootstrap; no retries.                    |

The example checks integrity at `F_LAST`, prints batch counts and final supplied levels, and disposes
the node after stopping. No subscriptions or execution client run; a missing response fails the run.
These deadlines can still reject [slow responses](#shared-pacing-and-deadlines).

Set `MARKET_ID`, `START`, and `END` for another window. The start hour must supply a seed at/before `START`
on the ordering clock. Increase allowance for busier/larger windows; missing seeds, ambiguous continuation,
or exhaustion fail. See [replay rules](#historical-books).

### Historical research and source selection in Python

The [research example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/historical_research.py)
prints raw IDs, period bounds, closing prices, and volumes from one explicit source:

```bash
uv run --project python --no-sync python examples/live/finfeed/historical_research.py market
uv run --project python --no-sync python examples/live/finfeed/historical_research.py flat-file
uv run --project python --no-sync python examples/live/finfeed/historical_research.py exchange
```

| Selection   | Source                                        | Scope                                                             |
| ----------- | --------------------------------------------- | ----------------------------------------------------------------- |
| `market`    | DB-backed `/ohlcv`, `period_id=1HRS`.         | One outcome; native bar source.                                   |
| `flat-file` | `/activity/.../history/ohlcv`, `period=1HRS`. | One outcome; raw access only.                                     |
| `exchange`  | DB-backed `/ohlcv`.                           | Keep each `market_id_exchange`, including missing or unknown IDs. |

All selections use the same UTC day. Each run permits **one GET and five rows**, a 20-second HTTP
timeout, a 30-second caller deadline, and no retries; completion or failure cancels the client.
Prices and volumes remain `Decimal`; times and identifiers stay raw, with no local filtering or
instrument mapping. Sources do not fall back to one another.

Run one command at a time, with at least ten seconds between runs on the account. After a timeout or
HTTP 429, stop and check provider usage. Five rows can be truncated; empty results can reflect missing
or delayed data. Read the [flat-file](#flat-file-ohlcv), [exchange-wide](#exchange-wide-ohlcv), and
[price-source limits](#prices-and-finality) before expanding retrieval.

For Nautilus bars, load a supported definition and call `request_bars` with explicit start/end and an
external `LAST` bar type. Native pagination selects complete periods, including matching calendar
periods; a small local limit does not reduce retrieval. See [historical bars](#historical-bars).

### Sizing a production collection

Use the [bounded tester](#defaults-and-request-cost), then choose selections, allowance, poll delay,
run duration, and both connection deadlines from [sizing a bounded collection](#sizing-a-bounded-collection).
For one outcome, update `ATTEMPT_BUDGET` and `RUN_SECONDS` in `examples/live/finfeed/data_tester.py`.
Larger selections need exact IDs and room for latency and account-wide quota.
[Resume deliberately](#exhaustion-and-deliberate-resume) when the finite allowance runs out.

## FinFeed documentation

- [Prediction Markets API reference](https://www.finfeedapi.com/products/prediction-markets-api/docs?ref=nautilus).
- [Authentication](https://www.finfeedapi.com/products/prediction-markets-api/docs/authentication?ref=nautilus).

## Instruments and symbology

### Supported venues and outcomes

Native definitions and subscriptions support **Polymarket binary Yes/No outcomes only**.
The label must match `_YES` or `_NO` and a distinct source token.

Raw REST identity also recognizes `KALSHI`, `MYRIAD`, `MANIFOLD`, `HYPERLIQUIDOUTCOME`, `PASCAL`,
`GEMINIPM`, `CRYPTOCOMPM`, and `FORECASTEX`. Those venues and non-binary contracts have no native mapping.

### Explicit selections

`FinFeedInstrumentSelection` names exact outcome IDs under one exchange. Select both `_YES` and `_NO`
when needed; related outcomes and markets are not loaded implicitly.
See [factory registration](#python-configuration).

- **Construction**: Empty selections, duplicate IDs, unknown exchanges, and invalid IDs fail before requests.
- **Bootstrap**: Unavailable or unsupported definitions and ambiguous token mappings fail without partial publication.

### Discovery

Collection methods require explicit `Pagination` (Rust) or `FinFeedPagination` (Python) page and
record bounds, including the terminal page.

| Collection | Continuation / termination                           | Rejected conditions                 |
| ---------- | ---------------------------------------------------- | ----------------------------------- |
| Listings   | Opaque cursor; null or absent cursor ends traversal. | Repeated cursor or exceeded bounds. |
| Active IDs | One-based pages; empty page ends traversal.          | Duplicate ID or exceeded bounds.    |

Discovery has no point-in-time catalog guarantee. Full metadata must still pass native definition checks.

### Identity and precision

| Field         | Definition                                  | Meaning                                                                                               |
| ------------- | ------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Instrument ID | `<exact outcome ID>.FINFEED_POLYMARKET`     | Preserves the provider's case and dots.                                                               |
| Raw symbol    | Exact outcome ID, including `_YES` or `_NO` | Must agree with the normalized Yes/No label and a distinct source token.                              |
| Currency      | `pUSD`                                      | Follows the native Polymarket convention.                                                             |
| Price         | Four decimal places                         | Positive source `MinimumTickSize` in `(0, 1)`; prices must fit exactly in `[0, 1]`.                   |
| Size          | Six decimal places                          | Share increment `0.000001`; raw sizes must fit exactly; [bar volume exception](#prices-and-finality). |
| Activation    | Zero                                        | Source activation time is unavailable.                                                                |
| Expiration    | Source `EndDateIso`                         | Simulated trading cutoff, not authoritative venue closure or settlement.                              |

Definition metadata retains description, outcome, condition/token IDs, provider status, and available
question/tags fields. Additional order quantity, notional, and price limits stay unset.
Metadata does not imply execution compatibility.

### Coexistence with native adapters

FinFeed IDs use `FINFEED_POLYMARKET`; native Polymarket IDs use `POLYMARKET` and separate condition/token
identity. Register each factory separately, retain both definitions, and route FinFeed requests with
`FINFEED_CLIENT_ID`. Native data or execution needs the native client and instrument IDs.

:::warning
Do not overwrite one definition with the other or route FinFeed IDs to native execution.
:::

## Standalone data loader

`FinFeedDataLoader` retains checked outcome definitions and returns Nautilus market data without a node.
Construction sends no requests and reads no credentials. The first network operation creates a shared
HTTP client using `FinFeedDataClientConfig`, including its pacing, retries, and attempt allowance.

Call `await loader.request_instrument(instrument_id)` to fetch and retain one definition, or
`await loader.request_instruments()` to fetch the configured selections atomically. `add_instrument`
registers an existing definition. Failed refreshes preserve the retained definitions. Market-data calls
use cached metadata; they do not refresh it implicitly.

`request_book_deltas`, `request_quotes`, `request_trades`, and `request_bars` take integer UNIX nanosecond
`start` and `end` bounds and an optional positive limit. Bounds are inclusive on the adapter's ordering
clock. These methods share the native client's historical processing and return no partial result on
failure. Book limits preserve complete batches. See [historical books](#historical-books) for snapshot
requirements, replay clocks, and continuation limits.

The [Polymarket backtest tutorial](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_finfeed.py) shows definition retrieval,
book download, catalog writes, and replay with `BacktestNode`.

## Python REST access

`FinFeedHttpClient(config)` uses `FinFeedDataClientConfig` credentials, endpoint, proxy, HTTP limits,
and retries. Selections, bootstrap deadlines, and polling settings do not apply. Construction resolves
credentials without sending requests; the client keeps no instrument metadata cache.

**Selectors and validation**:

- Dates: `YYYY-MM-DD`. Timestamps: RFC 3339 with `Z` or an offset, at 100 ns request precision; getters retain
  sub-microsecond precision. A start must precede its end.
- History: Supply `date` or `time_start` (optional `time_end`), never a date with time bounds.
  `get_ohlcv_latest` rejects bounds.
- Raw trade/quote/book history: Use a date or covering UTC-hour bounds, then filter locally. Explicit
  bounds within one UTC hour fail before spending an attempt; date and start-only selectors are accepted.

Constructors parse dates/times; awaiting the call checks remaining query rules before admission.
Query objects and result properties have no setters.

| Query or bounds type        | Constructor inputs                                         | Rules                                                                                                      |
| --------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `FinFeedListingsQuery`      | `limit, cursor=None`                                       | Limit in `[1, 10000]`; optional nonempty opaque cursor.                                                    |
| `FinFeedActiveQuery`        | `limit, page`                                              | Limit in `[1, 10000]`; page in `[1, 2147483647]`.                                                          |
| `FinFeedActivityQuery`      | `limit`                                                    | Limit in `[1, 10000]` for each activity list.                                                              |
| `FinFeedHistoryQuery`       | `limit, date=None, time_start=None, time_end=None`         | Date or start required; limit in `[1, 10000]`.                                                             |
| `FinFeedFlatFileOhlcvQuery` | `period, limit, date=None, time_start=None, time_end=None` | `period` is `1SEC`, `1MIN`, or `1HRS`; date or start required; limit in `[1, 10000]`.                      |
| `FinFeedOhlcvQuery`         | `period_id, limit, time_start=None, time_end=None`         | Nonblank `period_id`; per-market limit in `[1, 100000]`; see [exchange-wide bounds](#exchange-wide-ohlcv). |
| `FinFeedPagination`         | `max_pages, max_records`                                   | Both positive; exceeded bounds raise an error.                                                             |

All methods return awaitables. Collection methods use `bounds: FinFeedPagination` and return a complete
bounded traversal or raise without partial results. Raw history methods return **one response**;
[automatic historical pagination](#coverage-and-truncation) is available through `FinFeedDataLoader`
and the native data client.

| Method                                                       | Query type                  | Awaited result             |
| ------------------------------------------------------------ | --------------------------- | -------------------------- |
| `get_exchanges()`                                            | None                        | `list[FinFeedExchange]`    |
| `get_exchange(exchange_id)`                                  | None                        | `FinFeedExchange`          |
| `get_listings(exchange_id, query)`                           | `FinFeedListingsQuery`      | `FinFeedMarketsPage`       |
| `get_active(exchange_id, query)`                             | `FinFeedActiveQuery`        | `list[str]`                |
| `get_listings_all(exchange_id, query, bounds)`               | `FinFeedListingsQuery`      | `list[FinFeedMarket]`      |
| `get_active_all(exchange_id, query, bounds)`                 | `FinFeedActiveQuery`        | `list[str]`                |
| `get_market(exchange_id, market_id)`                         | None                        | `FinFeedMarket`            |
| `get_current(exchange_id, market_id)`                        | None                        | `FinFeedActivityCurrent`   |
| `get_latest(exchange_id, market_id, query)`                  | `FinFeedActivityQuery`      | `FinFeedActivityBatch`     |
| `get_trades_history(exchange_id, market_id, query)`          | `FinFeedHistoryQuery`       | `list[FinFeedTrade]`       |
| `get_quotes_history(exchange_id, market_id, query)`          | `FinFeedHistoryQuery`       | `list[FinFeedQuote]`       |
| `get_book(exchange_id, market_id)`                           | None                        | `FinFeedBook`              |
| `get_book_history(exchange_id, market_id, query)`            | `FinFeedHistoryQuery`       | `list[FinFeedBookEvent]`   |
| `get_periods()`                                              | None                        | `list[FinFeedOhlcvPeriod]` |
| `get_ohlcv_history(exchange_id, market_id, query)`           | `FinFeedOhlcvQuery`         | `list[FinFeedOhlcv]`       |
| `get_ohlcv_flat_file_history(exchange_id, market_id, query)` | `FinFeedFlatFileOhlcvQuery` | `list[FinFeedOhlcv]`       |
| `get_ohlcv_exchange_history(exchange_id, query)`             | `FinFeedOhlcvQuery`         | `list[FinFeedOhlcv]`       |
| `get_ohlcv_latest(exchange_id, market_id, query)`            | `FinFeedOhlcvQuery`         | `list[FinFeedOhlcv]`       |

**Results**:

- Prices, quantities, sizes, and volumes use `decimal.Decimal`; nested `source_specific_data` JSON
  preserves values with `Decimal` instead of binary floats.
- Nullable IDs, absent activity, and absent versus empty lists remain distinct. Returned lists/source dictionaries are copies.
- `FinFeedMarketStatus`, `FinFeedOutcomeType`, and `FinFeedTradeSide` map unknown labels to `UNKNOWN`,
  `OTHER`, and `OTHER`; unknown text is discarded, but other fields and sibling records remain available.
  Status/side labels infer no settlement or aggressor side.
- `market.to_instrument(ts_init=0)` returns a checked `BinaryOption`; `ts_init` is Unix nanoseconds.
  Unsupported or incomplete definitions, including `OTHER` outcome types, raise `ValueError`.
  Raw access remains available regardless of mapping support.
- Historical book rows retain numeric action codes and provider order. Raw REST access does not replay books
  or establish complete history.

| Failure                                                                                  | Python exception                        |
| ---------------------------------------------------------------------------------------- | --------------------------------------- |
| Construction, invalid selectors or local query rules                                     | `ValueError`.                           |
| Integer outside its Rust type's range                                                    | `OverflowError`.                        |
| HTTP, transport, decoding, budget, traversal/response bounds, continuation, cancellation | `RuntimeError` with sanitized messages. |

`client.attempts_remaining` exposes the local allowance. Canceling an asyncio awaitable cancels that call;
`client.cancel()` permanently cancels pending and future calls, including admission/retry waits.
Construct another client to resume.

### Flat-file OHLCV

Use `get_ohlcv_flat_file_history` with `FinFeedFlatFileOhlcvQuery` for the provider's trade-generated
activity source at `/v1/activity/{exchange_id}/{market_id}/history/ohlcv`.
Select `period` (`1SEC`, `1MIN`, or `1HRS`), a date or time bounds, and a limit in `[1, 10000]`, including dates.
With an existing client:

```python
from nautilus_trader.adapters.finfeed import FinFeedFlatFileOhlcvQuery

query = FinFeedFlatFileOhlcvQuery("1MIN", 3, date="2026-10-11")
rows = await client.get_ohlcv_flat_file_history("KALSHI", "CONTROLH-2026-D_NO", query)
```

- **Source**: All `/v1/ohlcv` methods and native bars instead use DB-backed OHLCV with `period_id` and
  the full catalog. Sources can return different rows for the same window; neither falls back to the other.
- **Results**: Raw `FinFeedOhlcv` rows, exact decimal tokens, no mapping or local time filtering.
  Filter locally for exact bounds; a saturated response has no continuation and may omit rows.
  Missing or delayed data can also leave gaps. See [price-source limits](#prices-and-finality).

### Exchange-wide OHLCV

`get_ohlcv_exchange_history(exchange_id, query)` requires both bounds in `FinFeedOhlcvQuery`:
**at most 24 hours and `[1, 10000]` rows**. It returns one response in provider order, with every field
and raw `market_id_exchange`, including unknown, empty, or missing IDs. It loads no definitions and
performs no native bar conversion; native requests require a loaded supported definition.

For longer retrieval:

1. Request `time_start=A, time_end=B`, then `time_start=B, time_end=C`, each at most 24 hours.
1. Keep boundary rows: the provider does not specify bound inclusion. Deduplicate only identical full
   rows with nonempty market IDs; retain missing-ID or conflicting rows for explicit resolution.
1. Check saturation. Advancing from the last timestamp alone can skip other markets at that time.
   Missing or delayed source data can leave gaps even in short or empty responses.

See the [provider contract](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/ohlcv/ohlcv/exchange_id/history/get?ref=nautilus).

## Requests

Load a definition before history or current-book requests.

### Instrument metadata

- **Single instrument**: Fetch fresh full metadata, including outcomes outside startup selections.
- **Instrument list**: Refresh configured selections for the requested venue.
- **Publication**: Publish updates and the correlated response on success. No periodic metadata refresh runs.

### Historical data

#### Time windows and limits

- **Bounds**: Start is required; an omitted end uses request time. Windows are inclusive `[start, end]`.
- **Allowance**: Bootstrap, history, polling, and retries share `attempt_budget`. Trade/quote/book history
  rejects windows whose touched UTC hours exceed the remaining allowance, reporting both counts.
  This reserves nothing: continuation, retries, or concurrent polling can still exhaust it without partial delivery.
- **Output limit**: Selects the earliest records after local filtering and stable ordering; retrieval cost is unchanged.
- **Cancellation**: Disconnect cancels retrieval.

| Data   | Retrieval                                | Selection / timestamps                                                                  |
| ------ | ---------------------------------------- | --------------------------------------------------------------------------------------- |
| Quotes | Covering UTC hours; 10000 rows per page. | Order/filter by `recv_time` (`ts_init`); `entry_time` supplies `ts_event`.              |
| Trades | Covering UTC hours; 10000 rows per page. | Sole `timestamp` for ordering and both clocks; `NO_AGGRESSOR`.                          |
| Bars   | Period lookup; 100000 rows per page.     | Complete periods inside the window, closed by request time; both clocks use period end. |

#### Coverage and truncation

**Native history paginates automatically**. Raw REST history remains single-response; partial-hour native
windows expand retrieval and filter locally. Raw explicit bounds within one UTC hour are rejected because
FinFeed can return empty arrays despite data in a covering response.

Trades/quotes/books need at least one attempt per touched UTC hour, including the end hour. For 31 days:
745 attempts midnight-to-midnight, or 744 ending before the final midnight. Default 10-second pacing alone
exceeds two hours in either case; local attempts are not billing units.

- **Full pages**: Keep the upper selector fixed. Rewind the lower selector to one second before the largest
  returned clock rounded down to whole seconds, never moving it backward or reaching the upper selector.
  Continuation clocks are trade `timestamp`, quote `recv_time`, and book `time_exchange`.
- **Overlap**: Retained rows must form a contiguous suffix repeated as the next page's exact ordered prefix,
  comparing every decoded field. Skip only that prefix; identical new events keep their multiplicity.
  Full pages must add records and advance the lower selector.
- **Short/empty pages**: Extend the upper selector by an hour, carrying and checking overlap. Empty hours do
  not end traversal; a short page in the final touched hour does. Caller limits do not reduce this work.

:::warning
**Pagination does not guarantee complete source coverage.** The rewind is empirical, not a provider cursor.
FinFeed [selects trades by processing time](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/market-activity/activity/exchange_id/market_id/history/trades/get?ref=nautilus)
but exposes one trade clock, can omit processing clocks on other records, and specifies no maximum delay.
Processing outside selected hours can omit in-window events; overlap checks cannot detect excluded rows.
Missing files, delayed data, and short/empty pages can leave gaps.

Dense/nonprogressing intervals and noncontiguous overlap fail without partial delivery. A reversal smaller
than the rewind can still straddle its selector and force rejection. Restarting from the last event clock can skip ties/delays.
:::

#### Historical bars

Bars require `EXTERNAL`, `LAST`, and a matching provider period:

- **Durations**: Second, minute, hour, day, month, year; `1-WEEK` maps to seven days.
  `12-MONTH` and `1-YEAR` use the one-year period.
  Milliseconds are unsupported. Calendar and multi-month periods must fit completely inside the window
  and close by request time.
- **Pagination**: Retrieve the full window in pages of up to 100000 rows. Advance to the greatest validated
  period end with a fixed upper bound, including after short pages. Stop on an empty response or a period
  reaching the upper bound; an empty final request may be needed.
- **Rejection**: Repeated starts, overlap with earlier pages, invalid durations, and nonprogress fail
  without partial delivery. Within-page ordering and overlap are not checked.
- **Coverage**: Output is sorted locally, but continuation advances to the greatest returned period end.
  This assumes pages do not omit earlier periods; newest-first or incomplete pages can skip older data.
- **Conversion**: Omit periods with absent prices, zero volume, and zero trades. Prices remain exact;
  [volume normalization and price-source/finality limits](#prices-and-finality) apply.

#### Historical books

L2 market-by-price (MBP) history supports deltas, depth, and point snapshots.
For a historical point, use an offset-bearing RFC 3339 timestamp in
`params={"end": "2026-10-08T00:40:00Z"}` with `request_book_snapshot`; omit it for a current snapshot.

**Seed and groups**:

- Retrieval starts at the containing UTC hour, with the next hour as the initial upper selector and no
  earlier lookback. A returned snapshot must seed state at or before `start` on the ordering clock below.
  Leading updates are validated, then discarded until that seed.
- Each exchange-timestamp change or same-time transition between snapshot and incremental records ends
  an `F_LAST` group. Snapshot groups contain `Clear` then zero or more `Add` records, all `F_SNAPSHOT`.
  Every applied group must leave a valid book; a later same-time group cannot repair it.
- Actions `0`, `1`, and `3` mean relative ADD, absolute SET (zero deletes), and complete replacement.
  Source symbols match case-insensitively. Replay reconstructs supplied records, not guaranteed venue state.

**Ordering and bounds**:

- Preserve source order and original `ts_event`, including backward exchange clocks. A group's `ts_init`
  is the cumulative maximum of all its rows' processing clocks (`time_coinapi`, otherwise `time_exchange`)
  and the preceding group. Every delta shares that value; **it is not measured receipt time**.
- Groups at or before `start` build the seed; groups in `(start, end]` publish. Bounds use this derived clock,
  so an in-window exchange timestamp can be excluded and an earlier one emitted. The generated seed uses
  `start` for both clocks; point snapshots use the requested time as `ts_last`.
- Retrieval uses [checked continuation](#coverage-and-truncation) and stops once any source clock exceeds
  `end`: later ordering cannot return to the window. Skip action/integrity validation of the first post-end
  group, even if the page cuts it. Snapshot-boundary checks still reject replacements spanning timestamps
  across `end`.

**Depth, limits, and errors**:

- Depth defaults to 10, supports `[1, 10]` levels per side, and emits one snapshot per group; timestamps can repeat.
- Delta limits keep whole batches and must fit the initial replacement; depth limits count snapshots.
  Validate all levels in groups through inclusive `end` before applying either local limit.
- Missing seeds, invalid actions, L3 requests, and [continuation/budget failures](#coverage-and-truncation)
  fail without partial delivery. Delayed processing can omit events despite in-window exchange times.

### Current books and unsupported requests

Current books require `L2_MBP`: both sides must be present (empty sides stay empty), and every level
is validated before local depth limits. The book timestamp is `time_exchange`; the response envelope
uses local publication time.

Funding, custom data, and option-chain reference-price requests are unsupported.

### Failed requests

Validation errors return immediately. **Asynchronous failures log an error and emit no response**:
set a caller response timeout. Disconnect cancels admitted work and prevents stale publication.

## Polling subscriptions

### Shared requests and cadence

| Scope                           | Shared request                                   |
| ------------------------------- | ------------------------------------------------ |
| Quotes + trades for one outcome | Latest activity.                                 |
| Deltas + depth for one outcome  | Current book.                                    |
| Each bar type                   | Period lookup, then separate latest-OHLCV polls. |

One scheduler polls scopes serially. `poll_interval_ms` is the delay after each endpoint poll completes;
pacing, other outcomes, response time, and retries add delay. Raising HTTP concurrency does not parallelize
polling. With all three scope types, 10-second pacing alone gives each at least 30 seconds between starts
in a saturated cycle.

:::warning
REST subscriptions sample data. There is no WebSocket stream, continuity, automatic backfill, or completeness guarantee.
:::

`DEBUG` diagnostics report activity/OHLCV counts, limits, and saturation. Saturation does not trigger recovery.

### Timestamps and deduplication

Quotes use entry/receive times for `ts_event`/`ts_init`; trades use their sole timestamp for both.
**Provider timestamps do not measure local arrival or polling latency.** Unseen older activity can publish.

- **Retention**: FIFO caches hold 2048 identities for `poll_limit` in `[1, 1024]`, or 20000 for `[1025, 10000]`.
  Each subscribed quote/trade list allocates its full tier independently, so memory jumps above 1024.
  Unsubscribing releases that list's cache.
- **Duplicates**: Duplicate records do not refresh retention; eviction can allow old events to publish again.
- **Corrections**: Trades require IDs. The first valid record for a retained ID wins; later corrections are omitted.

### Polling errors

| Condition                                                         | Effect                         |
| ----------------------------------------------------------------- | ------------------------------ |
| Invalid activity record                                           | Log and omit the record.       |
| Wire-decoding failure                                             | Reject the response.           |
| Empty list                                                        | Publish nothing.               |
| Exhausted transient retries, decode failure, or HTTP 404          | Allow a later scheduled poll.  |
| Budget/configuration failure or other permanent HTTP client error | Suspend the affected endpoint. |

HTTP 408 and 429 remain eligible for later polls. See [resuming polls](#changing-subscriptions-and-resuming-polls).
For an exhausted allowance, follow [exhaustion and deliberate resume](#exhaustion-and-deliberate-resume).

### Book snapshots

#### Events and depth

Both feeds require `L2_MBP`:

- **Deltas**: `Clear`, then sorted bid/ask `Add` records replace the book. All carry `F_SNAPSHOT`, the final
  record also `F_LAST`. Omitted depth retains all supplied levels.
- **Depth**: Defaults to 10 levels per side, supports more than 10, and returns actual levels without padding.
  Events carry `F_SNAPSHOT` and `F_LAST`.

Sequence IDs, order IDs, and depth order counts are zero; the source supplies none.

#### Validation and timestamps

Validate the full snapshot before limiting depth, including levels outside the requested depth.
Missing sides, crosses, duplicate prices, invalid timestamps, non-positive sizes, or inexact values
reject the entire snapshot. **Empty and locked books are valid.**

| Timestamp  | Source           |
| ---------- | ---------------- |
| `ts_event` | `time_exchange`. |
| `ts_init`  | `time_coinapi`.  |

#### Duplicate and stale snapshots

- Suppress consecutive identical full snapshots; changes at equal timestamps can publish.
- Drop snapshots older by `(time_coinapi, time_exchange)`. Clock regressions can suppress updates until
  the prior watermark returns or reconnect clears the book scope.
- Adding/removing a feed, changing depth, or refreshing a definition can republish an unchanged book.

### Closed bars

#### Formats and polling

Subscriptions require `EXTERNAL`, `LAST`, and [matching durations](#historical-bars).
Unsupported aggregation/price types fail at admission; a missing catalog duration logs an error and
suspends that bar type after lookup.

Long periods still poll at `poll_interval_ms` and can exhaust the allowance before closing.

#### Publication rules

Subscribe before the desired period closes:

- **Eligibility**: Start the watermark at subscribe time. Publish only periods ending after it and at or
  before the poll's request-start clock. Running periods wait for another poll.
- **Duplicates**: Publish identical raw periods once; compare before volume normalization. Invalid records
  or conflicting duplicates reject the whole batch without advancing the watermark.
- **Late data**: Omit revisions and late periods behind the advanced watermark. Never fabricate or backfill missing periods.

#### Prices and finality

`LAST` preserves provider OHLCV; it does **not** establish trade-only, midpoint, or mark prices.
Completed periods can be revised; subscriptions omit revisions behind their watermark.

| Value                              | Conversion rule                                                                                                 |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| All prices; quote/trade/book sizes | Exact representation required.                                                                                  |
| Inexact bar volume                 | Half-to-even rounding at size precision; adjustment at most `1e-15` of reported volume, below 500000000 shares. |
| Exact bar volume                   | Quantity range checks; no inexact-volume ceiling.                                                               |

The ceiling keeps tolerance below half a size unit. Larger mismatches, negative volume, and positive-to-zero
rounding reject the entire request or poll batch. These rules also apply to historical bars.

## Lifecycle and market resolution

### Connect and market status

Connect publishes all selected definitions before readiness; invalid or unavailable metadata fails bootstrap.
Only explicit definition refreshes apply lifecycle changes. An absent active ID or expired `EndDateIso`
alone does not prove resolution.

| Refreshed status               | Polling behavior                                          |
| ------------------------------ | --------------------------------------------------------- |
| `Closed`, `Resolved`           | End subscriptions; reject new ones, including at startup. |
| `Open`, `Suspended`, `Unknown` | Keep polling; no venue trading constraints inferred.      |

Loaded terminal definitions remain available for explicit data requests, subject to provider availability.

No `InstrumentStatus`, `InstrumentClose`, authoritative settlement price, or settlement event is inferred.
Backtests use `BinaryOption.expiration_ns` to stop simulated trading and cancel open orders at the metadata
cutoff; this does not establish real venue closure or payout.

### Unsubscribe and disconnect

Unsubscribe blocks further publication before returning and signals cancellation; the scheduler drops
the request when next run, without refunding an admitted attempt.
Disconnect closes admission, cancels polling/requests, and drains owned tasks.

### Reconnect

Reconnect reuses the resolved credential with a fresh local allowance:

- **Definitions**: Reload startup selections and outcomes with retained intent before restoring subscriptions.
- **Ad hoc outcomes**: Re-request non-startup outcomes without active subscriptions before history/book requests,
  including terminal outcomes.
- **Retained state**: Keep activity identities and bar watermarks; publish the next valid book afresh.
  No cached data is replayed.

### Changing subscriptions and resuming polls

After fixing a non-budget suspension, unsubscribe/subscribe or reconnect.
An [exhausted allowance](#exhaustion-and-deliberate-resume) requires a new connection generation.

- **Runtime changes**: Unsubscribe before changing depth or resubscribing after terminal metadata becomes
  nonterminal; duplicate active subscriptions are suppressed.
- **State reset**: Unsubscribe clears that activity type's identities or bar watermark; reset clears all intent.
- **Definition refresh**: Nonterminal updates invalidate in-flight polls and request fresh samples of active scopes.

## Limitations

| Area       | Limit                                                                                                              |
| ---------- | ------------------------------------------------------------------------------------------------------------------ |
| Polling    | Samples can miss events; no stream or automatic gap recovery.                                                      |
| History    | Outcome/date/retention dependent; [checked continuation](#coverage-and-truncation) cannot prove complete coverage. |
| Books      | Returned records and a seed only; no guaranteed full venue state.                                                  |
| Bars       | No trade-only/final-price guarantee; late revisions behind the watermark are omitted.                              |
| Resolution | Metadata controls polling; no authoritative settlement or payout.                                                  |

## Configuration

Rust uses `FinFeedDataClientConfig`, nested `http: FinFeedHttpConfig`, and `http.retry: RetryConfig`.
Python uses flat keywords; omitted or `None` values keep Rust defaults.

- **Config construction**: No connection or credential reads; Python rejects unknown keywords and wrong types.
- **Data-client construction**: Validate selections, URLs, and controls before requests.

### Authentication and environment

#### Credentials

Use a Prediction Markets API key via explicit `api_key` or `FINFEED_API_KEY` in the launched process.
The client reads it once at construction and reuses it on reconnect. `COINAPI_API_KEY` is not a fallback.

Read-only GETs send the key directly in `Authorization`. No trading account, private key, or JWT is needed;
the provider's [JWT instructions](https://www.finfeedapi.com/products/prediction-markets-api/docs/authentication?ref=nautilus)
cover unsupported trading/market-creation operations.

**Keep keys out of endpoint URLs.** Keys and proxy URLs are redacted; Python exposes presence indicators,
not plaintext getters.

#### Endpoint and proxy

Default: `https://api.prediction-markets.finfeedapi.com/v1/` (production).

- Redirects are rejected; ambient proxies are disabled. Set `proxy_url` for an explicit HTTP(S) proxy.
- Custom origins require HTTPS, except plaintext HTTP for loopback peers.

### Python configuration

Register the factory with bounded settings:

```python
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId

config = FinFeedDataClientConfig(
    instruments=[
        FinFeedInstrumentSelection(
            "POLYMARKET",
            ["2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO"],
        ),
    ],
    attempt_budget=8,
    http_timeout_secs=10,
    max_retries=0,
    poll_interval_ms=60_000,
    connect_timeout_secs=45,
)
node = (
    LiveNode.builder("FINFEED-DATA-TESTER", TraderId("TESTER-001"), Environment.LIVE)
    .with_reconciliation(False)
    .with_timeout_connection(60)
    .with_timeout_portfolio(0)
    .add_data_client(None, FinFeedDataClientFactory(), config)
    .build()
)
```

`None` selects the default client name, `FINFEED`. The [examples](#examples) add `DataTester`, subscriptions,
and a 60-second stop timer. For general setup, see [Configure live trading](../how_to/configure_live_trading.md).

### Public controls and defaults

#### Connection and instruments

| Python keyword            | Rust field                     | Default                   | Meaning                                                 |
| ------------------------- | ------------------------------ | ------------------------- | ------------------------------------------------------- |
| `api_key`                 | `api_key`                      | `None`                    | Resolve `FINFEED_API_KEY` at client construction.       |
| `instruments`             | `instruments`                  | `[]`                      | Nonempty explicit selections for the data client.       |
| `base_url_http`           | `http.base_url`                | Production endpoint above | Include the `/v1/` prefix.                              |
| `proxy_url`               | `http.proxy_url`               | `None`                    | Explicit HTTP(S) proxy.                                 |
| `http_timeout_secs`       | `http.timeout_secs`            | `300`                     | Transport timeout per request, in seconds.              |
| `max_concurrent_requests` | `http.max_concurrent_requests` | `1`                       | Managed requests in flight on this instance.            |
| `attempt_budget`          | `http.attempt_budget`          | `100`                     | Lifetime managed attempts per connection generation.    |
| `min_request_interval_ms` | `http.min_request_interval_ms` | `10000`                   | Shared spacing between attempt starts, in milliseconds. |
| `connect_timeout_secs`    | `connect_timeout_secs`         | `120`                     | Entire bootstrap deadline in seconds.                   |

Each `FinFeedInstrumentSelection` requires `exchange_id` and `market_ids`, with no selection defaults.
Use `POLYMARKET` and exact binary Yes/No outcome IDs; see [supported venues and outcomes](#supported-venues-and-outcomes).

Request timeouts and pacing are positive and limited to one day. `connect_timeout_secs` accepts `[1, 86400]`;
concurrency and attempt allowance must be positive.

#### Polling

| Python keyword     | Rust field         | Default | Meaning                                    |
| ------------------ | ------------------ | ------- | ------------------------------------------ |
| `poll_interval_ms` | `poll_interval_ms` | `10000` | Delay after each endpoint poll completes.  |
| `poll_limit`       | `poll_limit`       | `100`   | Maximum latest quotes and trades per list. |

Polling accepts `[100, 86400000]` milliseconds and a `poll_limit` in `[1, 10000]`.
OHLCV requests use `poll_limit + 1` rows for a running period: at most 10001, within the market
endpoint's 100000-row limit.

At most 128 outcomes and 128 distinct bar types across them can retain intent. Books have no
`poll_limit`; apply depth only after validating the full snapshot.

#### Retries

| Python keyword               | Rust field                        | Default  | Meaning                                                    |
| ---------------------------- | --------------------------------- | -------- | ---------------------------------------------------------- |
| `max_retries`                | `http.retry.max_retries`          | `3`      | Retries after the first attempt.                           |
| `retry_delay_initial_ms`     | `http.retry.initial_delay_ms`     | `1000`   | Initial retry backoff in milliseconds.                     |
| `retry_delay_max_ms`         | `http.retry.max_delay_ms`         | `10000`  | Maximum backoff in milliseconds.                           |
| `retry_backoff_factor`       | `http.retry.backoff_factor`       | `2.0`    | Backoff multiplier.                                        |
| `retry_jitter_ms`            | `http.retry.jitter_ms`            | `0`      | Maximum retry jitter in milliseconds.                      |
| `retry_operation_timeout_ms` | `http.retry.operation_timeout_ms` | `330000` | Per-attempt deadline including admission waits.            |
| `retry_immediate_first`      | `http.retry.immediate_first`      | `false`  | Skip backoff before the first retry.                       |
| `retry_max_elapsed_ms`       | `http.retry.max_elapsed_ms`       | `600000` | Whole invocation deadline including retries and admission. |

Retry deadlines and maximum backoff are positive and limited to one day. Both Rust deadlines must be
`Some` with positive values; Python `None` retains defaults rather than disabling a deadline.

Backoff requires a positive initial delay, a maximum delay at least as large as the initial delay,
and a finite factor in `[1, 100]`. Jitter can be zero.

### Request cost and retries

#### Bootstrap budget

Bootstrap costs **one exchange-discovery GET plus one metadata GET per distinct startup or retained outcome**,
excluding retries. Refreshes, history, period lookups, and polling share the allowance:

| Operation                | Minimum cost                                                            |
| ------------------------ | ----------------------------------------------------------------------- |
| Quote/trade/book history | One attempt per touched UTC hour, plus continuation.                    |
| Historical bars          | Period lookup and OHLCV pages, possibly a final empty page.             |
| Bar subscription         | Period lookup before first OHLCV poll per generation or resubscription. |

Default 10-second pacing alone consumes the 120-second bootstrap deadline at 12 outcomes.
At 128 outcomes, 129 bootstrap attempts exceed the default 100-attempt allowance.
Size both adapter and node connection deadlines for selections, latency, and retries.

#### Sizing a bounded collection

`attempt_budget` is a finite **connection-generation lifetime allowance**, not a daily/minute quota.
Increasing it does not increase provider quota. Count shared [endpoint scopes](#shared-requests-and-cadence),
not subscriptions; each bar type adds a lookup before its first poll. Failed or invalidated lookups can
repeat after subscription changes or definition refreshes.

For unchanged subscriptions and no refreshes:

- **Bootstrap**: `1 + N` attempts for `N` outcomes, without retries.
- **Polling ceiling**: `F * (1 + floor(T / P))` attempts for `F` scopes during `T` seconds **after readiness**,
  with poll delay `P` seconds. Response/admission time can slow polls; this is not an update-rate guarantee.
- **Headroom**: Add period lookups, explicit requests, refreshes, and retries. Changes/refreshes can make
  scopes immediately due. Each operation can use up to `1 + max_retries` attempts, subject to deadlines.

These 24-hour examples use 10-second pacing and HTTP timeouts, no retries, bars, or explicit requests;
allowances reserve extra room:

| Outcomes | Polling scopes per outcome | Poll delay | Bootstrap attempts | Total attempt ceiling | Explicit allowance | Adapter / node connection timeout |
| -------- | -------------------------- | ---------- | ------------------ | --------------------- | ------------------ | --------------------------------- |
| 1        | Activity and book          | 60 s       | 2                  | 2884                  | 3200               | 45 s / 60 s                       |
| 12       | Activity                   | 600 s      | 13                 | 1753                  | 2000               | 300 s / 330 s                     |
| 128      | Activity                   | 3600 s     | 129                | 3329                  | 4000               | 1500 s / 1560 s                   |

For the first row, retain the [tester](#defaults-and-request-cost) selection and subscriptions, set
`attempt_budget=3200`, and keep `poll_interval_ms=60000`, `http_timeout_secs=10`,
`max_concurrent_requests=1`, `max_retries=0`, `connect_timeout_secs=45`, and node
`.with_timeout_connection(60)`. Set `RUN_SECONDS` to 86400 plus allowed startup time: its timer includes startup.

For larger selections, replace the IDs, subscriptions, poll delay, allowance, and both deadlines with the
chosen row. See [public controls](#public-controls-and-defaults) for equivalent Rust fields.

#### Shared pacing and deadlines

**Pacing is per client**, across all managed attempts. At 10 seconds, 128 activity scopes need at least
1280 seconds per sweep, or 2560 with 128 book scopes, before latency. A 60-second poll delay cannot make
those scopes update once per minute; reduce selections or increase the delay.

`max_concurrent_requests` bounds in-flight work but does not parallelize serial bootstrap/polling or bypass
pacing. Separate clients have independent controls; coordinate their account-wide load.

| Deadline                     | Covers                                                           |
| ---------------------------- | ---------------------------------------------------------------- |
| `http_timeout_secs`          | One transport request.                                           |
| `retry_operation_timeout_ms` | One attempt, including admission waits.                          |
| `retry_max_elapsed_ms`       | Invocation, retries, and admission.                              |
| `connect_timeout_secs`       | Entire adapter bootstrap.                                        |
| Node connection timeout      | Must leave time for the adapter bootstrap.                       |
| Caller response timeout      | Entire paginated request; async failures log without a response. |

Queued work can expire without an attempt. Longer bootstrap deadlines do not override shorter request deadlines.

:::warning
**Book history can be slow.** Live checks include a 508-record response taking about 105 seconds and a
native two-hour request timing out with a 180-second HTTP timeout. Neither establishes a reliable upper
bound; 100 seconds still rejects the observed 105-second response.
:::

Set a caller response timeout for the full traversal. If you override request deadlines, adjust HTTP,
per-attempt, and total retry deadlines together. Raising only the HTTP timeout can leave a shorter
attempt deadline that still cancels the request.

#### Exhaustion and deliberate resume

At `FinFeed local attempt budget exhausted`, affected polls suspend but retain intent. The client can
remain connected with no further polls. Calling `connect()` again or repeating a subscription does not refill it.

| Owner                           | Deliberate restart                                                                                          |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `LiveNode`                      | Stop; await run completion; dispose and drop; check usage; build a new bounded node with explicit settings. |
| Direct Rust `FinFeedDataClient` | `disconnect()` then `connect()` for a new generation, metadata reload, and eligible retained intent.        |
| Standalone HTTP client          | Construct a new instance after cancellation; use a new configuration to change allowance.                   |

In Python, await `run_async()` before disposal and release all old node references before rebuilding
(`del node` if it is the last); see [ownership and lifecycle](../concepts/python.md#ownership-and-lifecycle).
`LiveNode` exposes no per-client FinFeed reconnect. Budget bootstrap and period lookups again;
construct a revised client to change its allowance.

**Do not automate reconnects to refill allowance**: they neither restore provider quota nor coordinate
other clients. Check provider usage before starting the next bounded window.

#### Retryable failures

Retry transport/timeouts and HTTP 429, 500, 502, 503, and 504, honoring integer `Retry-After` seconds.
Admission can expire before sending.

#### Local attempts and provider quota

Every admitted attempt counts, including failures/cancellations. **No refunds.**

:::note
Native HTTP/2 can resend twice after `REFUSED_STREAM` or `GOAWAY(NO_ERROR)` without managed allowance
or pacing. Local attempts are not provider billing units.
:::

Client instances and reconnects do not coordinate or reset account quota. Check provider entitlement
and usage before raising the allowance or reducing pacing.

## Troubleshooting

| Symptom / log                                                               | Action                                                                                                                       |
| --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `set api_key or FINFEED_API_KEY`, HTTP 401/403                              | Set `FINFEED_API_KEY` in the launched process; check Prediction Markets entitlement.                                         |
| `FinFeed bootstrap timed out` / bootstrap failure                           | Check exact binary outcome IDs, metadata, availability, budget, pacing, and both connection deadlines.                       |
| No fresh data                                                               | Check activity/timestamps, pacing, suspension, and refreshed status; inactive markets may yield no ticks.                    |
| Local attempt budget exhausted                                              | [Size the allowance](#sizing-a-bounded-collection), then [restart deliberately](#exhaustion-and-deliberate-resume).          |
| HTTP 429                                                                    | Stop; follow provider retry/reset guidance.                                                                                  |
| Request error, no response                                                  | Inspect the log; set a caller timeout for asynchronous failures.                                                             |
| Inexact data / invalid book                                                 | Preserve the error; check metadata. Do not round data or substitute native definitions.                                      |
| Empty history / bars                                                        | Check UTC bounds, complete periods, caps, availability. Empty does not establish resolution.                                 |
| `is inactive or resolved`                                                   | Choose a supported active outcome; terminal metadata blocks subscriptions.                                                   |
| `overlap changed`, `not a contiguous suffix`, `exceeds its hourly selector` | Check [continuation limits](#coverage-and-truncation); inconsistent clocks/pages reject the whole result.                    |
| `cannot advance`                                                            | Check [continuation](#coverage-and-truncation) and [bar rules](#historical-bars): dense/repeated/overlapping pages can fail. |
| `history time bounds must cross a UTC hour boundary`                        | Use a date or covering hours for raw history; filter locally.                                                                |
| `requires at least ... requests`                                            | Shorten the window or increase allowance; compare required/remaining attempts.                                               |
| `FinFeed has no matching OHLCV period`                                      | Select a catalog duration; see [historical bars](#historical-bars).                                                          |

## Contributing

:::info
To contribute to the FinFeed adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
