# FinFeed

[FinFeedAPI Prediction Markets API](https://www.finfeedapi.com/products/prediction-markets-api?ref=nautilus)
by [API Bricks](https://www.apibricks.io/?ref=nautilus) provides prediction-market metadata, activity,
order books, and OHLCV data through REST. The NautilusTrader adapter uses REST requests and polling
for Polymarket binary Yes/No instruments.

The default client ID is `FINFEED`. This data-only adapter has no execution client, account operations,
or order routing. FinFeed's currencies, stock, and SEC products, and its MCP and JSON-RPC interfaces,
are outside the adapter's scope.

## Overview

The adapter is implemented in Rust with Python bindings and needs no external FinFeed client library.
The Python package `nautilus_trader.adapters.finfeed` exports:

- `FinFeedDataClientConfig`: Startup selections, HTTP controls, and polling settings.
- `FinFeedInstrumentSelection`: Exact outcome IDs to load from one exchange.
- `FinFeedDataClientFactory`: Factory registered with the live-node builder.
- `FinFeedHttpClient`: Async, read-only REST access with typed queries and provider results.
- `FINFEED` and `FINFEED_CLIENT_ID`: Default client name and routing identifier.

Configure a `LiveNode` through [Python configuration](#python-configuration) or the
[runnable examples](#examples).

## Supported capabilities

| Capability                   | Interface                   | Support and limits                                                                        |
| ---------------------------- | --------------------------- | ----------------------------------------------------------------------------------------- |
| Discovery and metadata       | REST requests               | Raw listings and metadata; native definitions for Polymarket binary Yes/No outcomes only. |
| Quote and trade ticks        | REST polling                | Sampled activity with bounded deduplication; no automatic gap recovery.                   |
| L2 book deltas               | REST polling                | Complete replacements of supplied levels.                                                 |
| L2 book depth                | REST polling                | Snapshots with a local depth limit.                                                       |
| Current book snapshot        | REST requests               | L2 market-by-price (MBP).                                                                 |
| Historical trades and quotes | REST requests               | Whole UTC dates with local selection and response caps.                                   |
| Historical bars              | REST requests               | External `LAST` bars from per-market OHLCV.                                               |
| Flat-file OHLCV              | Raw Rust/Python REST access | Explicit source selection; no native bar conversion.                                      |
| Exchange-wide OHLCV          | Raw Rust/Python REST access | Windows up to 24 hours; no automatic definition loading or bar conversion.                |
| Closed bars                  | REST polling                | Completed periods from per-market OHLCV.                                                  |
| Market resolution            | Metadata refresh            | Closed/resolved metadata ends polling; no settlement event is inferred.                   |
| Historical books             | REST requests               | L2 deltas, depth, and point snapshots; a seed snapshot is required.                       |
| L3 books                     | Unsupported                 | No order-level replay.                                                                    |
| Execution                    | Unsupported                 | No account operations or order routing.                                                   |

## Integrator attribution

**API requests** include the fixed header `X-Referrer: nautilus` on every managed attempt, including
retries and reconnects. It has no configuration override and contains no account data or credential.

For **signup attribution**, open an [API Bricks referral link](https://www.apibricks.io/?ref=nautilus)
before creating an account in the same browser. Signup and API request attribution are separate
mechanisms; see the
[API Bricks referral documentation](https://www.apibricks.io/platform/docs/general/referral?ref=nautilus).

## Examples

### Running the examples

Before running an example:

- Set up [authentication](#authentication-and-environment) with prediction-market access.
- For Python, install a NautilusTrader build with FinFeed bindings in `python/.venv`;
  see [installation](../getting_started/installation.md).
- Historical examples use fixed past windows and depend on provider retention.

Run from the repository root:

```bash tab="Rust"
CARGO_BUILD_JOBS=8 cargo run --locked -p nautilus-finfeed --example node_data_tester
```

```bash tab="Python"
uv run --project python --no-sync python examples/live/finfeed/data_tester.py
```

The [Rust example](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/finfeed/examples/node_data_tester.rs)
and [Python example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/data_tester.py)
register `FinFeedDataClientFactory` with a `LiveNode` and route the built-in `DataTester` through `FINFEED`.

### Defaults and request cost

The fixed-selection Rust and Python examples use these settings:

- **Outcome**: `No` for `2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949`.
- **Data**: Quotes, trades, and L2 book replacements, logged as they arrive. No orders or bar/history requests.
- **Duration**: Request a clean stop after 60 seconds, including startup. Shutdown can take additional time.
- **Requests**: Eight managed attempts, 10-second shared pacing, a 60-second poll interval, and no application retries.

Successful bootstrap uses two attempts; the first activity and book polls use one each.
See [request cost and retries](#request-cost-and-retries) to size the allowance for longer runs.

### Selecting another outcome

Find IDs through the provider's
[active-market endpoint](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/markets/markets/exchange_id/active/get?ref=nautilus),
using `POLYMARKET` as `exchange_id`. Change `MARKET_ID` to another Polymarket binary Yes/No outcome ID,
including its suffix. Full metadata must still pass the [definition rules](#supported-venues-and-outcomes).

The loaded definition supplies precision: do not round provider data or substitute a native Polymarket ID.

Markets can close or become unavailable. An inactive market can produce no new activity during the run.

### Discovering an outcome in Python

The [discovery example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/discovery.py)
reads one active-ID page, fetches full definitions, and selects the first open outcome that passes
`to_instrument()`. It constructs an explicit `FinFeedInstrumentSelection`, starts a data-only node,
requests the selected definition, and subscribes to quotes.

```bash
uv run --project python --no-sync python examples/live/finfeed/discovery.py
```

Discovery inspects at most three IDs and permits four managed attempts. The node has a separate
four-attempt allowance. Both clients use 10-second pacing, 10-second HTTP timeouts, and no application
retries. Discovery has a 60-second deadline; the node runs for 60 seconds including bootstrap.
Shutdown can take additional time. Change the constants in the example to choose other bounds.

If the sample has no supported open outcome, the example raises `RuntimeError` before building the node.
`DataTesterConfig(request_instruments=True)` refreshes the explicit selection; it does not discover or load a catalog.

### Historical book replay in Python

Run the [historical-book example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/historical_book.py)
to load a provider-classified closed outcome and replay a 115-second window into an L2 `OrderBook`:

```bash
uv run --project python --no-sync python examples/live/finfeed/historical_book.py
```

The example permits three managed GET attempts: exchange discovery, one full definition, and one
whole-hour history request. It uses 10-second pacing, a 60-second HTTP timeout, a 75-second per-attempt
deadline including admission, a 90-second invocation deadline, no retries, and a 150-second response
deadline including bootstrap. A missing response fails the run. Successful delivery
prints the completed batch count and final supplied bid/ask levels after checking book integrity at
`F_LAST` boundaries. It starts no subscriptions or execution client and disposes the node after stopping.

Set `MARKET_ID`, `START`, and `END` to select another outcome or window. The returned start hour must
contain a replacement snapshot at or before `START`. Larger windows can require more attempts; missing
seeds, saturated responses, or incompatible clocks cause the request to fail. See
[historical books](#historical-books) for replay rules, depth requests, and point snapshots.

### Historical research and source selection in Python

The [research example](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/finfeed/historical_research.py)
selects one source and prints raw identifiers, period bounds, closing prices, and volumes. Run one
command at a time, with at least ten seconds between runs on the same account. Stop after a timeout
or HTTP 429 and check provider usage before another run.

```bash
uv run --project python --no-sync python examples/live/finfeed/historical_research.py market
uv run --project python --no-sync python examples/live/finfeed/historical_research.py flat-file
uv run --project python --no-sync python examples/live/finfeed/historical_research.py exchange
```

| Selection   | Source and scope                                      | Bounds and interpretation                                                                 |
| ----------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `market`    | DB-backed `/ohlcv`, one outcome.                      | One UTC day with `period_id=1HRS`; native historical bars use this source.                |
| `flat-file` | Flat-file `/activity/.../history/ohlcv`, one outcome. | Same window with `period=1HRS`; raw access only, no native bar conversion.                |
| `exchange`  | DB-backed `/ohlcv`, exchange-wide.                    | Same window; retain each row's `market_id_exchange`, including missing or unknown values. |

Each invocation permits one managed GET attempt, a five-row sample, a 20-second HTTP timeout, no
retries, and a 30-second caller deadline. The client is canceled on completion or failure. Prices
and volumes remain `Decimal`; raw rows retain provider times and are not locally filtered or mapped
to instruments. A five-row response can be truncated; empty responses can reflect missing or delayed
source data. Source selection is explicit, with no fallback. See [prices and finality](#prices-and-finality)
for price-source restrictions.
See [flat-file OHLCV](#flat-file-ohlcv) and [exchange-wide OHLCV](#exchange-wide-ohlcv) before expanding a window.

For Nautilus historical bars, load a supported definition and use a data actor's `request_bars` with
an external `LAST` bar type and explicit start/end. One OHLCV request covers the entire caller window,
including complete calendar-month, multi-month, and year periods when the provider period catalog matches.
It returns only complete periods inside the window and rejects saturated responses. A small local
result limit does not reduce retrieval; see [historical bars](#historical-bars).

### Sizing a production collection

Start with the bounded [data-tester example](#defaults-and-request-cost), then apply the explicit
allowance, poll delay, selection, run duration, and both connection deadlines in
[sizing a bounded collection](#sizing-a-bounded-collection). For the one-outcome row, update
`ATTEMPT_BUDGET` and `RUN_SECONDS` in `examples/live/finfeed/data_tester.py` to match that table,
then run that script. For larger selections, supply exact outcome IDs and allow for response latency
and account-wide quota. [Resume deliberately](#exhaustion-and-deliberate-resume) after the finite
allowance is exhausted.

## FinFeed documentation

- [Prediction Markets API reference](https://www.finfeedapi.com/products/prediction-markets-api/docs?ref=nautilus).
- [Authentication](https://www.finfeedapi.com/products/prediction-markets-api/docs/authentication?ref=nautilus).

## Instruments and symbology

### Supported venues and outcomes

Nautilus definitions support **Polymarket binary Yes/No outcomes only**. The outcome label must match
its `_YES` or `_NO` suffix and identify a distinct source token.

The REST identity layer also recognizes `KALSHI`, `MYRIAD`, `MANIFOLD`, `HYPERLIQUIDOUTCOME`, `PASCAL`,
`GEMINIPM`, `CRYPTOCOMPM`, and `FORECASTEX` for raw REST access. Native instrument definitions
and data-client subscriptions do not support those venues or non-binary contracts.

### Explicit selections

`FinFeedInstrumentSelection` selects exact outcome IDs under one exchange. It does not load every
outcome of a market or every market in an event. Select `_YES` and `_NO` explicitly when both are needed.
See [Python configuration](#python-configuration) for a complete selection and factory registration.

Validation occurs at two stages:

- **Client construction**: Empty selections, duplicate IDs, unknown exchanges, and invalid IDs fail before requests start.
- **Bootstrap**: Ambiguous token mappings, unsupported definitions, and unavailable outcomes fail without
  publishing a partial selection.

### Discovery

The Rust and Python `FinFeedHttpClient` APIs expose listings and active-ID discovery. Collection methods require explicit page
and record bounds: `Pagination` in Rust or `FinFeedPagination` in Python. The page allowance includes
the terminal page.

- **Listings** use opaque cursors. Repeated cursors and exceeded bounds fail explicitly.
- **Active IDs** use one-based pages and finish on an empty page. Duplicate IDs and exceeded bounds fail explicitly.

A changing catalog has no point-in-time consistency guarantee. Discovery alone does not validate a
Nautilus instrument: full metadata must pass the definition rules.

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

FinFeed uses `FINFEED_POLYMARKET`; the native adapter uses `POLYMARKET` with its own condition/token
identity. Their definitions are independent, and their instrument IDs are not interchangeable.

1. Register each factory separately and retain both definitions.
1. Pass `FINFEED_CLIENT_ID` to FinFeed requests and subscriptions.
1. Use the native client's ID and instrument ID for native data or execution.

:::warning
Do not overwrite one definition with the other or route FinFeed IDs to native execution.
:::

## Python REST access

Construct `FinFeedHttpClient(config)` with a `FinFeedDataClientConfig`. The client uses its API key,
endpoint, proxy, HTTP limits, and retry settings. Instrument selections, bootstrap deadlines, and polling
settings do not apply to direct REST calls. Construction resolves credentials without sending requests.
The client retains no instrument metadata cache.

Constructors parse dates and timestamps; the remaining query rules are checked when the call is awaited,
before an attempt is consumed. Invalid selectors or query rules raise `ValueError`; integer conversion
outside the Rust type's range raises `OverflowError`.

Query objects and result properties have no setters. Dates use `YYYY-MM-DD`; timestamps use RFC 3339
strings with an offset. Timestamp getters preserve sub-microsecond precision. Request timestamps must
fit the provider's 100 ns precision, and a start must precede its end. History dates and time bounds
are mutually exclusive. `get_ohlcv_latest` rejects time bounds.

Raw `get_trades_history`, `get_quotes_history`, and `get_book_history` reject explicit bounds within the
same UTC hour before sending a request or spending an attempt. Use `date` or covering UTC-hour bounds
and filter the returned records locally. Date and start-only selectors are also accepted.

| Query or bounds type        | Constructor inputs                                         | Rules                                                                                                      |
| --------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `FinFeedListingsQuery`      | `limit, cursor=None`                                       | Limit in `[1, 10000]`; optional nonempty opaque cursor.                                                    |
| `FinFeedActiveQuery`        | `limit, page`                                              | Limit in `[1, 10000]`; page in `[1, 2147483647]`.                                                          |
| `FinFeedActivityQuery`      | `limit`                                                    | Limit in `[1, 10000]` for each activity list.                                                              |
| `FinFeedHistoryQuery`       | `limit, date=None, time_start=None, time_end=None`         | Date or start required; limit in `[1, 10000]`.                                                             |
| `FinFeedFlatFileOhlcvQuery` | `period, limit, date=None, time_start=None, time_end=None` | `period` is `1SEC`, `1MIN`, or `1HRS`; date or start required; limit in `[1, 10000]`.                      |
| `FinFeedOhlcvQuery`         | `period_id, limit, time_start=None, time_end=None`         | Nonblank `period_id`; per-market limit in `[1, 100000]`; see [exchange-wide bounds](#exchange-wide-ohlcv). |
| `FinFeedPagination`         | `max_pages, max_records`                                   | Both positive; exceeded bounds raise an error.                                                             |

Every method below returns an awaitable. Collection methods take `FinFeedPagination` as `bounds`;
they return a complete bounded traversal or raise an error, without partial results.

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

Results preserve provider values, including nullable identifiers, absent activity, and absent versus
empty lists. Prices, quantities, sizes, and volumes use `decimal.Decimal`. `source_specific_data`
preserves nested JSON values and uses `Decimal` instead of binary floats. Modifying a returned list
or source dictionary does not modify the response object.

`FinFeedMarketStatus`, `FinFeedOutcomeType`, and `FinFeedTradeSide` classify provider labels without
settlement or aggressor-side inference. Unknown labels decode as `UNKNOWN`, `OTHER`, and `OTHER`,
respectively; the original unknown text is not retained. Other fields and sibling records remain available.

`market.to_instrument(ts_init=0)` checks the [Nautilus definition rules](#supported-venues-and-outcomes) and returns a
`BinaryOption`. `ts_init` is Unix time in nanoseconds and defaults to zero.
Unsupported or incomplete definitions, including `OTHER` outcome types, raise `ValueError`.
Raw provider access remains available independently of mapping support. Historical book events retain their numeric action codes and
response order; direct REST access does not replay a book or establish historical completeness.

Construction and local request validation failures raise `ValueError`. Awaited HTTP, transport, or
decoding failures raise `RuntimeError`. The same type covers exhausted attempt budgets, traversal or
response bounds, invalid continuation responses, and client cancellation, with sanitized messages.
`client.attempts_remaining` reports the remaining local managed attempt allowance.
Canceling an asyncio awaitable cancels that call. `client.cancel()` permanently cancels pending and
future requests, including admission and retry waits; construct another client for further calls.

### Flat-file OHLCV

Select `get_ohlcv_flat_file_history` with `FinFeedFlatFileOhlcvQuery` to retrieve activity bars
that the provider describes as trade-generated. It uses `/v1/activity/{exchange_id}/{market_id}/history/ohlcv`,
with required `period` in `1SEC`, `1MIN`, or `1HRS`. Select a date or `time_start` with an
optional `time_end`; date and time bounds are mutually exclusive. The limit is in `[1, 10000]`,
including date-selected requests.

With a constructed `FinFeedHttpClient`, retrieve one bounded response:

```python
from nautilus_trader.adapters.finfeed import FinFeedFlatFileOhlcvQuery

query = FinFeedFlatFileOhlcvQuery("1MIN", 3, date="2026-10-11")
rows = await client.get_ohlcv_flat_file_history("KALSHI", "CONTROLH-2026-D_NO", query)
```

All `/v1/ohlcv` methods, native bar requests, and bar subscriptions use the DB-backed source
with `period_id` and the full period catalog. The two sources can return different rows for the same
market and time window. The adapter preserves each source's prices; see
[prices and finality](#prices-and-finality) for interpretation.
Source selection is explicit; neither source falls back to the other.

Flat-file access returns raw `FinFeedOhlcv` rows without instrument mapping or local time filtering.
Prices and volumes retain the provider's decimal tokens. The client exposes no continuation, so a
saturated response can omit rows. Filter returned rows locally for exact time bounds, and allow for
missing or delayed source data.

### Exchange-wide OHLCV

`get_ohlcv_exchange_history(exchange_id, query)` uses `FinFeedOhlcvQuery` with both `time_start`
and `time_end` required. The maximum window is 24 hours, and the row limit is in `[1, 10000]`.
It returns one response in provider order, preserving all fields and `market_id_exchange` on each
`FinFeedOhlcv`. Unknown, empty, and missing identifiers remain raw values. Retrieval does not load
market definitions or convert rows to Nautilus bars. Load a supported definition before making
native bar requests.

Partition longer workflows explicitly: request `(time_start=A, time_end=B)`, then `(time_start=B,
time_end=C)`, with each window at most 24 hours. Keep returned boundary rows; the provider does not specify
bound inclusion. Deduplicate only identical full rows with a nonempty market identifier. Retain rows with
missing identifiers or conflicting values and resolve them explicitly. A saturated response may omit other
markets at the same timestamp. Avoid advancing from the last timestamp alone, which can skip those
rows. Missing or delayed source data can also leave gaps in short or empty responses.

See the provider's [exchange OHLCV history contract](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/ohlcv/ohlcv/exchange_id/history/get?ref=nautilus).

## Requests

Load a definition before requesting history or a current book.

### Instrument metadata

- **Single-instrument requests**: Fetch fresh full metadata, including outcomes omitted from startup.
- **Instrument-list requests**: Refresh configured selections matching the requested venue.
- **Successful refreshes**: Publish instrument updates and the correlated response. Metadata does not refresh periodically.

### Historical data

#### Time windows and limits

For historical requests:

- **Start**: Required; an omitted end uses the request-time clock.
- **Window**: Inclusive `[start, end]`. All history shares `attempt_budget` with bootstrap, retries, and polling.
  Trade, quote, and book history reject a window before retrieval if the number of touched UTC dates
  (trades and quotes) or UTC hours (books) exceeds the remaining allowance. The error reports both counts. This check reserves no attempts; retries and
  concurrent polling can still exhaust the allowance without partial delivery. Disconnect cancels retrieval.
- **Record limit**: Selects the earliest records after local filtering and stable ordering. It does not reduce retrieval cost.

| Data   | Retrieval                                     | Local selection and timestamps                                                                                      |
| ------ | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Quotes | Whole UTC dates, up to 10000 records per date | Filter/order by `recv_time` (`ts_init`); `entry_time` supplies `ts_event`.                                          |
| Trades | Whole UTC dates, up to 10000 records per date | Filter/order by the sole `timestamp`, used for both timestamps; aggressor is `NO_AGGRESSOR`.                        |
| Bars   | Period lookup and up to 100000 OHLCV records  | Keep complete periods contained in the window and closed by the request-time clock; both timestamps use period end. |

#### Coverage and truncation

Trades and quotes cost one request per touched UTC date, including the date containing the inclusive end.
A daily quote/trade response containing 10000 records or a market OHLCV response containing 100000 periods
**fails the entire request as potentially truncated**, without a partial result or inferred continuation.
A small caller limit does not avoid this failure because the client applies it after retrieval.

The adapter does not paginate saturated responses. Raw sub-day queries can return empty arrays even
when a covering full-day response contains records. Advancing from the last returned timestamp can
skip records with the same timestamp or records selected by a different processing clock.

Empty days do not stop traversal. Missing source files, delayed processing, or a short response can still leave gaps.

The provider documents processing-time selection for
[historical trades](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/market-activity/activity/exchange_id/market_id/history/trades/get?ref=nautilus),
but exposes only one trade timestamp. A trade processed on another UTC date can be omitted even when
its event time falls inside the requested window.

Trades and quotes retrieve full UTC days to avoid unreliable short-window filtering.
Books retrieve whole UTC hours and reject each hourly response at the 10000-record cap.
Native data requests accept partial-hour windows and filter the expanded retrieval locally.
For raw REST calls, use a date or covering UTC-hour bounds; explicit bounds within the same UTC hour
are rejected before sending a request.

#### Historical bars

Bars require `EXTERNAL` and `LAST`, with a matching provider period:

- **Durations**: Second, minute, hour, day, month, or year. `1-WEEK` uses the seven-day period;
  `12-MONTH` and `1-YEAR` use the one-year period. Millisecond aggregation is unsupported.
- **Window**: One OHLCV request covers the entire caller window. Complete calendar months, multi-month periods,
  and years fit when their start and end fall inside the window and their end is at or before the request-time clock.
- **Conversion**: Empty periods with absent prices, zero volume, and zero trades are omitted.
  Inexact prices fail conversion; volume follows the [bounded normalization rule](#prices-and-finality).

The [OHLCV price-source and finality limits](#prices-and-finality) also apply to historical bars.

#### Historical books

Historical deltas and depth replay provider-returned L2 market-by-price (MBP) events. Load the outcome
before requesting a series with `start` and an inclusive `end`. For a point snapshot, pass an RFC 3339
timestamp with `Z` or an explicit offset as `params={"end": "2026-10-08T00:40:00Z"}` to `request_book_snapshot`; omit it for a current book.

The client retrieves whole UTC hours within the requested window and shared attempt budget. The returned records must
supply a snapshot at or before `start`. Retrieval starts at the hour containing `start` without
looking back. Leading updates are validated and discarded until a seed. A generated replacement
at `start` initializes the consumer;
later batches preserve response order and end with `F_LAST` at each exchange-timestamp change and
each same-time transition between snapshot and incremental records. A snapshot group starts with
`Clear` followed by zero or more `Add` deltas, all flagged `F_SNAPSHOT`. Each group must leave a valid
book; a later group at the same timestamp cannot repair an invalid group.
Depth emits one snapshot per group, so timestamps can repeat.
Depth defaults to 10 and supports `[1, 10]` levels per side. Delta limits retain whole batches and
must fit the initial replacement; depth limits count snapshots. Every retrieved level is validated
before local depth or record limits apply. Current-book polling is unchanged.
Each touched hour costs one request. A 31-day midnight-to-midnight window requires 745 requests because
the end is inclusive; ending before the final midnight requires 744. The default 100-attempt lifetime
allowance is shared with bootstrap, polling, and other requests. Increase the existing allowance for
longer windows; default 10-second pacing puts either example above two hours.

Replay interprets numeric actions `0`, `1`, and `3` as relative ADD, absolute SET (zero deletes),
and a complete replacement snapshot, respectively. Reconstructed books reflect the returned events;
missing or delayed records can leave gaps in historical venue state. Processing-time selection can
omit delayed events whose exchange times fall inside the requested window.

Saturated 10000-record responses, missing seeds, overlapping hourly responses, unsupported actions,
and L3 requests fail explicitly. Source exchange clocks drive local bounds. A missing receive clock
uses the exchange clock for `ts_init`; the generated seed uses `start` for both clocks.
Point snapshots use the requested time as `ts_last`. Source symbols match case-insensitively.
Batches in `(start, end]` must also have nondecreasing receive times in `[start, end]`, checked before
local depth or record limits. Incompatible clocks reject the entire series before delivery can trim
or reorder a batch.

### Current books and unsupported requests

Current books use `L2_MBP`:

- Both sides must be present; an empty side remains empty.
- Every level is validated before a requested local depth is applied.
- The book timestamp uses `time_exchange`; the response envelope uses the local publication clock.

Funding, custom data, and option-chain reference-price requests are unsupported.

### Failed requests

Validation errors return immediately. An asynchronous failure is logged and **emits no response**;
set an appropriate caller response timeout. Disconnect cancels admitted work and prevents stale publication.

## Polling subscriptions

### Shared requests and cadence

Polling groups subscriptions by outcome and endpoint:

- **Activity**: Quotes and trades share one latest-activity request.
- **Books**: Delta and depth feeds share one current-book request.
- **Bars**: Each bar type adds a period lookup and separate latest-OHLCV polls.

One scheduler polls all scopes serially. Raising HTTP concurrency does not parallelize it.
`poll_interval_ms` delays each endpoint's next poll after its previous poll completes; shared pacing,
other outcomes, response time, and retries can delay it further.

For one outcome with all three endpoint types active, 10-second pacing alone spaces successive starts
of each endpoint by at least 30 seconds in a saturated cycle.

:::warning
Subscriptions sample REST data. They provide no WebSocket stream, continuity, automatic backfill,
or completeness guarantee.
:::

`DEBUG` polling diagnostics report counts, request limits, and saturation flags for subscribed activity
lists and OHLCV responses. Use these diagnostics to identify responses that reach the configured
limit. Saturation does not trigger historical recovery.

### Timestamps and deduplication

**Provider timestamps are not local arrival times.** Quotes use entry/receive times for
`ts_event`/`ts_init`; trades use their sole timestamp for both. Do not use `ts_init` to measure polling
latency or local arrival order. Unseen older activity can publish.

- **Retention**: Each subscribed activity list keeps 2048 identities for `poll_limit` in `[1, 1024]`,
  or 20000 for `[1025, 10000]`, in FIFO order. Quote and trade caches allocate independently;
  unsubscribing a list releases its cache. Each cache reserves its full tier at subscription, so memory
  increases in a step above `poll_limit=1024`. Duplicates do not refresh retention.
  Eviction can allow an old event to publish again.
- **Trade IDs and corrections**: Trades require an ID. A retained ID keeps the first valid record;
  later corrections are not applied.

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

Both feeds require L2 market-by-price (`L2_MBP`):

- **Deltas**: `Clear`, then sorted bid/ask `Add` records, replacing the whole book. Every delta has
  `F_SNAPSHOT`; the final record also has `F_LAST`. Omitted depth retains all available levels.
- **Depth**: Defaults to 10 levels per side; more than 10 is supported. Events return actual levels
  without padding and carry `F_SNAPSHOT` and `F_LAST`.

Sequence and order IDs are zero because the source exposes neither. Depth order counts are also zero.

#### Validation and timestamps

Validate the full snapshot before applying depth limits. Missing sides, crossed books, duplicate prices,
invalid timestamps, non-positive sizes, or inexact values reject the entire snapshot, including invalid
levels outside the requested depth. **Empty and locked books are valid.**

- `ts_event` uses `time_exchange`.
- `ts_init` uses `time_coinapi`.

#### Duplicate and stale snapshots

- Consecutive identical full snapshots are suppressed.
- Changes at equal timestamps can publish.
- Snapshots older by `(time_coinapi, time_exchange)` are dropped.

Provider clock regressions can suppress updates until the prior watermark is reached or reconnect clears
the book scope. Adding/removing a feed, changing depth, or refreshing a definition can republish an unchanged book.

### Closed bars

#### Formats and polling

Subscriptions require `EXTERNAL` and `LAST`, with [matching durations](#historical-bars).
Unsupported aggregation or price types fail at admission.
A duration absent from the period catalog logs an error and suspends that bar type after lookup.

Long-period subscriptions still poll at `poll_interval_ms` and can exhaust their allowance before a period closes.

#### Publication rules

Subscribe before the desired period closes:

- **Eligibility**: The initial watermark is subscribe time. Only periods ending after it and at or before
  the poll's request-start clock can publish. Running periods wait for a later poll.
- **Duplicates**: Identical raw provider periods publish once; comparison precedes volume normalization.
  Invalid records or conflicting duplicates reject the
  whole batch without advancing the watermark.
- **Late periods**: Revised or late periods behind an advanced watermark are omitted. Missing periods
  are never fabricated or automatically backfilled.

#### Prices and finality

`LAST` selects provider OHLCV. Do not assume that its prices are trade-only, midpoint, or mark prices.

The adapter preserves OHLC exactly. Aggregate volume rounds half to even at size precision when the
adjustment is at most `1e-15` of the reported volume and that volume is below 500000000 shares.
The ceiling applies only to inexact volumes; exact volumes still undergo quantity range checks.
The ceiling keeps the tolerance below half one size unit.
Larger mismatches, negative volumes, and positive-to-zero rounding reject the entire request or poll batch.
Prices and quote, trade, and book sizes remain exact.

The provider can revise completed periods. Subscriptions omit revisions behind their publication
watermark. These price-source and volume rules also apply to historical bars.

## Lifecycle and market resolution

### Connect and market status

Connect publishes all selected definitions before readiness. Invalid or unavailable metadata fails
bootstrap. Explicit definition refreshes apply lifecycle metadata; an absent active ID or expired
`EndDateIso` alone does not prove resolution.

- `Closed` and `Resolved` end all subscriptions for the outcome and reject new ones, including at startup.
- `Open`, provider `Suspended`, and `Unknown` keep polling; these statuses do not establish venue trading constraints.
- Loaded terminal definitions remain available for explicit data requests, subject to provider availability.
- No `InstrumentStatus`, `InstrumentClose`, authoritative settlement price, or settlement event is inferred.

**Backtests** use `BinaryOption.expiration_ns` to close simulated trading and cancel open orders at the
metadata cutoff. It does not establish real venue closure or final payout.

### Unsubscribe and disconnect

Unsubscribe prevents further publication before returning and signals cancellation. The scheduler drops
the affected request when it next runs; an admitted attempt still counts. Disconnect closes admission,
cancels polling and requests, and drains owned tasks.

### Reconnect

Reconnect reuses the resolved credential and starts a fresh local attempt allowance:

- **Definitions**: Reload startup selections and outcomes with retained subscription intent before restoring subscriptions.
- **Ad hoc outcomes**: Request a non-startup outcome again before history/book requests if it has no active
  subscription. This includes ad hoc terminal outcomes.
- **Retained state**: Activity deduplication and bar watermarks survive reconnect; the next valid fetched
  book publishes afresh. No locally cached data is replayed.

### Changing subscriptions and resuming polls

To resume an endpoint suspended by an error other than exhausted allowance, address the cause, then
unsubscribe/subscribe or reconnect. An [exhausted allowance](#exhaustion-and-deliberate-resume) requires a new
connection generation.
Through the runtime, unsubscribe before changing book depth or resubscribing after terminal metadata
becomes nonterminal. Duplicate active subscriptions are suppressed.

Unsubscribe clears the removed activity type's identities or bar watermark; reset clears all intent.
Nonterminal definition refreshes invalidate in-flight polls and request fresh samples of active scopes.

## Limitations

- **Polling**: REST samples can miss activity between polls. There is no streaming transport or automatic gap recovery.
- **Historical coverage**: Availability depends on the outcome, date, and provider retention. Missing or
  delayed records can leave gaps. Requests that reach a [response cap](#coverage-and-truncation) fail without partial delivery.
- **Book replay**: Historical books require a seed snapshot and represent only the returned records;
  they cannot guarantee complete venue state. See [historical books](#historical-books).
- **Bars**: Provider OHLCV does not guarantee trade-only prices or final revisions. Subscriptions omit
  late or revised periods behind the watermark. See [prices and finality](#prices-and-finality).
- **Market resolution**: Metadata updates control polling; the adapter supplies no authoritative settlement event or payout.

## Configuration

Rust uses `FinFeedDataClientConfig` with nested `http: FinFeedHttpConfig` and `http.retry: RetryConfig`.
Python uses flat keywords; omitted or `None` options retain Rust defaults.

- Config construction does not connect or read credentials. Python rejects unknown keywords and wrong types here.
- Building the data client validates selections, URLs, and controls before requests start.

### Authentication and environment

#### Credentials

Use a key entitled to the Prediction Markets API:

- Set `FINFEED_API_KEY` in the launched process through your secret manager, or pass `api_key` explicitly.
- The client reads the environment once at construction and reuses that credential on reconnect.
- `COINAPI_API_KEY` is not a fallback. No trading account, private key, or JWT configuration is used.

Read-only GET requests send the key directly in `Authorization`. The provider's general
[authentication page](https://www.finfeedapi.com/products/prediction-markets-api/docs/authentication?ref=nautilus)
also describes JWT requirements for trading/market creation; those operations are outside this adapter.

**Keep keys out of endpoint URLs.** Keys and proxy URLs stay redacted; Python exposes presence indicators
instead of plaintext getters.

#### Endpoint and proxy

The adapter targets the production endpoint at `https://api.prediction-markets.finfeedapi.com/v1/`.

- Redirects are rejected and ambient proxies are disabled.
- Set `proxy_url` to select an explicit HTTP(S) proxy.
- Custom origins must use HTTPS; plaintext HTTP is limited to loopback peers.

### Python configuration

This configuration uses the runnable examples' bounded request settings and explicitly registers the factory:

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

`None` selects the factory's default client name, `FINFEED`. The complete [examples](#examples) add
`DataTester`, subscribe to data, and request a stop after 60 seconds. For general node setup, see
[Configure live trading](../how_to/configure_live_trading.md).

### Public controls and defaults

#### Connection and instruments

| Python keyword            | Rust field                     | Default                   | Meaning                                                 |
| ------------------------- | ------------------------------ | ------------------------- | ------------------------------------------------------- |
| `api_key`                 | `api_key`                      | `None`                    | Resolve `FINFEED_API_KEY` at client construction.       |
| `instruments`             | `instruments`                  | `[]`                      | Nonempty explicit selections for the data client.       |
| `base_url_http`           | `http.base_url`                | Production endpoint above | Include the `/v1/` prefix.                              |
| `proxy_url`               | `http.proxy_url`               | `None`                    | Explicit HTTP(S) proxy.                                 |
| `http_timeout_secs`       | `http.timeout_secs`            | `30`                      | Transport timeout per request, in seconds.              |
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
| `retry_operation_timeout_ms` | `http.retry.operation_timeout_ms` | `30000`  | Per-attempt deadline including admission waits.            |
| `retry_immediate_first`      | `http.retry.immediate_first`      | `false`  | Skip backoff before the first retry.                       |
| `retry_max_elapsed_ms`       | `http.retry.max_elapsed_ms`       | `120000` | Whole invocation deadline including retries and admission. |

Retry deadlines and maximum backoff are positive and limited to one day. Both Rust deadlines must be
`Some` with positive values; Python `None` retains defaults rather than disabling a deadline.

Backoff requires a positive initial delay, a maximum delay at least as large as the initial delay,
and a finite factor in `[1, 100]`. Jitter can be zero.

### Request cost and retries

#### Bootstrap budget

Bootstrap costs one exchange-discovery request plus one metadata request per distinct startup or retained
outcome, excluding retries. Explicit metadata refreshes and period lookups share the allowance with polling and history.
Trades and quotes cost one request per touched UTC date; historical books cost one per touched UTC hour.
These costs can exceed the default 100-attempt allowance and suspend polling when the shared budget is exhausted.

With default 10-second pacing, 12 outcomes need at least 120 seconds of spacing alone, exhausting the
120-second bootstrap deadline. Set both the adapter deadline and node connection timeout for the selected
count, response times, and retries.

#### Sizing a bounded collection

`attempt_budget` is a finite lifetime allowance for one connection generation. It does not replenish
each minute or day. Choose an explicit allowance for the planned collection duration. Increasing it
does not increase provider quota.

Count each distinct polling scope, rather than each subscription:

- Quotes and trades for one outcome share one activity poll.
- Book deltas and depth for one outcome share one current-book poll.
- Each distinct external bar type adds an OHLCV poll and a period lookup before its first OHLCV poll
  in each connection generation and after resubscription. A lookup that fails or is invalidated by a
  subscription change or definition refresh can repeat on later eligible polls.

Metadata refreshes, history requests, and retries consume the same allowance as polling.
For `N` startup outcomes, bootstrap needs `1 + N` attempts without retries.

With unchanged subscriptions and no definition refreshes, `F` polling scopes use at most
`F * (1 + floor(T / P))` polling attempts during `T` seconds **after readiness**, with a poll delay of
`P` seconds. Reserve that amount, then add bootstrap, period lookups, explicit requests, and retry
headroom. Responses and admission waits can slow polls, so this ceiling does not guarantee an update rate.
Subscription changes and nonterminal definition refreshes can make affected scopes immediately due;
budget those extra polls and any repeated period lookups separately. With retries enabled, each operation
can consume up to `1 + max_retries` attempts, subject to its deadlines.

These 24-hour examples use no application retries, 10-second shared pacing, and a 10-second HTTP timeout.
They subscribe to no bars and issue no explicit requests; the allowance leaves room for planned extra work.

| Outcomes | Polling scopes per outcome | Poll delay | Bootstrap attempts | Total attempt ceiling | Explicit allowance | Adapter / node connection timeout |
| -------- | -------------------------- | ---------- | ------------------ | --------------------- | ------------------ | --------------------------------- |
| 1        | Activity and book          | 60 s       | 2                  | 2884                  | 3200               | 45 s / 60 s                       |
| 12       | Activity                   | 600 s      | 13                 | 1753                  | 2000               | 300 s / 330 s                     |
| 128      | Activity                   | 3600 s     | 129                | 3329                  | 4000               | 1500 s / 1560 s                   |

For the first row, keep the existing one-outcome selections and quote/trade/book subscriptions in the
[data-tester examples](#defaults-and-request-cost), change the allowance to 3200, and set `RUN_SECONDS`
to 86400 plus the startup time you allow. The stop timer includes startup. The equivalent client controls are:

```rust tab="Rust"
let mut config = FinFeedDataClientConfig {
    instruments: vec![FinFeedInstrumentSelection {
        exchange_id: "POLYMARKET".into(),
        market_ids: vec![MARKET_ID.into()],
    }],
    http: FinFeedHttpConfig {
        attempt_budget: 3_200,
        timeout_secs: 10,
        max_concurrent_requests: 1,
        min_request_interval_ms: 10_000,
        ..Default::default()
    },
    poll_interval_ms: 60_000,
    connect_timeout_secs: 45,
    ..Default::default()
};
config.http.retry.max_retries = 0;
```

```python tab="Python"
config = FinFeedDataClientConfig(
    instruments=[FinFeedInstrumentSelection("POLYMARKET", [MARKET_ID])],
    attempt_budget=3_200,
    http_timeout_secs=10,
    max_concurrent_requests=1,
    min_request_interval_ms=10_000,
    max_retries=0,
    poll_interval_ms=60_000,
    connect_timeout_secs=45,
)
```

Keep the node's `.with_timeout_connection(60)` for that row. For a larger selection, use the corresponding
allowance and adapter/node deadlines from the table. Set `RUN_SECONDS` to 86400 plus the startup time you
allow, replace the explicit outcome list, and set the subscription types and poll delay to match.
At 128 outcomes, bootstrap alone needs 129 attempts, exceeding the default allowance. Default pacing
also requires a longer adapter deadline and node connection timeout.

#### Shared pacing and deadlines

Shared pacing permits at most one managed attempt start per configured interval on one HTTP client.
At 10 seconds, sustaining repeated sweeps over 128 activity scopes needs at least 1280 seconds per sweep,
excluding response time. Adding 128 book scopes raises that to 2560 seconds. A 60-second poll delay cannot
make all those scopes update once per minute. Increase the delay or reduce the selection to fit the intended cadence.

`max_concurrent_requests` limits managed requests in flight on one client. Raising it does not parallelize
serial bootstrap or the data client's polling loop, or bypass shared pacing. Separate clients have
independent pacing and concurrency limits; coordinate their total load against account entitlements.

The transport timeout covers one HTTP request. `retry_operation_timeout_ms` also covers its admission
wait; `retry_max_elapsed_ms` covers the invocation and its retries. A queued request can expire without
using an attempt. A generous bootstrap deadline cannot override these shorter request deadlines.
The node connection timeout must leave time for the adapter's entire bootstrap.

#### Exhaustion and deliberate resume

At exhaustion, managed requests return `FinFeed local attempt budget exhausted`; polling scopes suspend
and retain their subscription intent. The client can remain connected while it delivers no further polls.
Calling `connect()` on that connected client or repeating a subscription does not replenish its allowance.

With `LiveNode`, stop the node and wait for its run to finish before disposing and dropping it.
Check provider usage, then build a new node for the next bounded collection window. Supply its
explicit selections, subscriptions, allowance, and deadlines again. In Python, await the `run_async()`
task before disposal and release all references to the old node before the next build. Use `del node`
when it is the last reference; see [Python ownership and lifecycle](../concepts/python.md#ownership-and-lifecycle).
The node does not expose a per-client FinFeed reconnect operation; a new run starts with fresh local state.

For a directly managed Rust `FinFeedDataClient`, explicitly call `disconnect()` and then `connect()`.
The new connection generation receives the configured finite allowance, reloads metadata, and restores
retained polling intent for eligible outcomes. Include bootstrap and bar-period lookup costs again.
To change the allowance, construct a client with a revised configuration. A canceled standalone HTTP
client also requires a new instance. Do not automate reconnects to refresh allowance: reconnecting does
not replenish provider quota or coordinate other clients on the account.

#### Retryable failures

Managed retries cover transport/timeouts and HTTP 429, 500, 502, 503, and 504. Integer `Retry-After` seconds
apply to retryable responses. Admission waits can expire before a request is sent.

#### Local attempts and provider quota

Each admitted attempt consumes the allowance, including failures and cancellations. **There are no refunds.**

:::note
The native HTTP/2 transport can resend twice after remote `REFUSED_STREAM` or `GOAWAY(NO_ERROR)` errors
without consuming managed allowance or pacing. Local attempts are not provider billing units.
:::

Separate client instances and reconnects do not coordinate or reset account-wide quota. Before increasing
the budget or reducing pacing, check your account's entitlement and usage in the provider console.

## Troubleshooting

| Symptom or log text                                      | Check or action                                                                                                                                 |
| -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `set api_key or FINFEED_API_KEY`, HTTP 401/403           | Load `FINFEED_API_KEY` into the launched process and verify prediction-market entitlement.                                                      |
| `FinFeed bootstrap timed out` or other bootstrap failure | Check exact outcome ID, binary metadata, venue availability, budget, pacing, and both connection deadlines.                                     |
| No fresh data                                            | Check provider activity and timestamps, shared pacing, local suspension, and refreshed market status; inactive markets can return no new ticks. |
| `FinFeed local attempt budget exhausted`                 | Size the [allowance](#sizing-a-bounded-collection), then [resume deliberately](#exhaustion-and-deliberate-resume).                              |
| HTTP 429                                                 | Stop requests and respect the provider's retry/reset guidance before resuming.                                                                  |
| Request logs an error but receives no response           | Asynchronous failures emit no response; use a caller timeout and inspect the logged failure.                                                    |
| Inexact data or invalid book                             | Preserve the error and verify source metadata; do not round values or reuse the native adapter's definition.                                    |
| Empty or missing history/bars                            | Check UTC bounds, complete-period rules, truncation caps, and provider availability; empty data alone does not indicate resolution.             |
| `is inactive or resolved`                                | Select another supported outcome; terminal metadata prevents subscriptions.                                                                     |
| `record cap and may be truncated`                        | Check [retrieval caps](#coverage-and-truncation); the adapter returns neither a partial result nor a continuation.                              |
| `history time bounds must cross a UTC hour boundary`     | Use a date or covering UTC-hour bounds for raw trade, quote, or book history, then filter locally.                                              |
| `requires at least ... requests`                         | Shorten the history window or configure a larger local allowance; the error reports required and remaining attempts.                            |
| `FinFeed has no matching OHLCV period`                   | Select a duration in the provider period catalog; see [historical bars](#historical-bars).                                                      |

## Contributing

:::info
To contribute to the FinFeed adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
