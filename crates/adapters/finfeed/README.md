# nautilus-finfeed

[NautilusTrader](https://nautilustrader.io) adapter for the
[FinFeedAPI Prediction Markets API](https://www.finfeedapi.com/products/prediction-markets-api?ref=nautilus).
API Bricks provides FinFeedAPI and CoinAPI.

The adapter provides authenticated REST access, checked Polymarket binary Yes/No instrument definitions,
historical data requests, and book, quote, trade, and closed-bar polling. It is implemented in Rust with
optional Python bindings. Order execution is unsupported.

## Capabilities and limits

- **REST access**: Exchange discovery, bounded listings and active IDs, outcome metadata, current/latest
  activity, L2 books, and trade, quote, book-event, and OHLCV history. Raw results retain exact `Decimal`
  values, nanosecond timestamps, nullable identifiers, and source metadata.
- **Instrument definitions**: Explicitly selected Polymarket Yes/No outcomes map to `BinaryOption` under
  `FINFEED_POLYMARKET`. Other venues and non-binary definitions fail explicitly. FinFeed instrument IDs
  are independent of native adapter IDs and must not be used for native execution.
- **Polling**: Quotes and trades share latest-activity requests; L2 market-by-price (MBP) deltas and depth
  share current-book snapshots. Deltas are complete replacements. Bars publish completed `EXTERNAL` and
  `LAST` periods. Polling can miss events and provides no completeness or automatic backfill guarantee.
- **History**: Trades and quotes retrieve whole UTC dates; L2 book replay retrieves whole UTC hours and
  requires a returned snapshot at or before the requested start. Native history rejects saturated
  responses without partial delivery or inferred continuation. Replay relies on documented source
  action and continuity assumptions; complete source coverage remains unverified.

Provider timestamps are preserved rather than replaced with local arrival times. Trades use
`NO_AGGRESSOR`. Book snapshots do not establish incremental continuity or full venue depth.
OHLCV `LAST` does not establish trade-only prices or final revisions. Prices and raw sizes remain exact;
OHLCV volume permits only [bounded normalization](../../../docs/integrations/finfeed.md#prices-and-finality).

Fresh `Closed` or `Resolved` metadata ends polling and rejects new subscriptions for that outcome.
Metadata has no periodic refresh, and no authoritative settlement or close event is inferred.
The provider end date sets the simulated trading cutoff; it does not establish actual venue closure or settlement.

Historical-trade delivery has a bounded Python comparison; nonempty historical-quote delivery and
positive bar delivery after reconnect remain unverified. See the
[capability matrix](../../../docs/integrations/finfeed.md#supported-capabilities) and
[evidence and limits](../../../docs/integrations/finfeed.md#limitations) before relying on a data surface.

## Configuration

Pass an API key explicitly or set `FINFEED_API_KEY`. The client resolves it once at construction and
reuses it on reconnect. Importing the Python package or constructing a config does not read credentials
or connect. Requests send the key in `Authorization`; keys stay out of URLs and errors.
Redirects are rejected, ambient proxies are disabled, and `proxy_url` selects an explicit HTTP(S) proxy.
Custom origins require HTTPS, except for plaintext loopback peers.

Configure explicit outcomes with `FinFeedInstrumentSelection` and register `FinFeedDataClientFactory`
with a `LiveNode`. Python exports these types and `FinFeedDataClientConfig`, `FinFeedHttpClient`,
`FINFEED`, and `FINFEED_CLIENT_ID` from `nautilus_trader.adapters.finfeed`.
Rust nests HTTP and retry controls under `http` and `http.retry`; Python uses flat keywords, with omitted
or `None` options retaining Rust defaults. Selections, URLs, and controls are validated before requests start.

The defaults allow **100 managed attempts per connection generation**, with **10-second spacing** and
one managed request in flight. Bootstrap, metadata refreshes, history, polling, and retries share that
finite allowance. Bootstrap costs one exchange request plus one metadata request per distinct selected
outcome, excluding retries. The default bootstrap deadline is 120 seconds; 12 outcomes require at least
120 seconds of pacing alone. Size the allowance and deadlines for the workload. Exhaustion can suspend
polling. These local controls do not guarantee billing or account-wide quota.

See [configuration and defaults](../../../docs/integrations/finfeed.md#configuration) and
[production sizing](../../../docs/integrations/finfeed.md#sizing-a-production-collection).

## Feature flags

- `extension-module`: Builds as a Python extension module.
- `fuzz`: Enables the REST decoder and domain conversion libFuzzer target.
- `high-precision` (default): Enables
  [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
  to use 128-bit value types.
- `python`: Enables Python bindings for identity, configuration, factories, and read-only REST access
  with typed queries and results through [PyO3](https://pyo3.rs).
- `simulation`: Routes time and HTTP transport through deterministic simulation with `cfg(madsim)`.
  Simulated transport supports plaintext local peers; HTTPS and explicit proxies require native tests.

## Integrator attribution

Every managed REST attempt, including retries and requests after reconnect, sends `X-Referrer: nautilus`.
The header carries no credential or account data. API Bricks determines usage attribution on its side;
see [attribution details](../../../docs/integrations/finfeed.md#integrator-attribution).

## Documentation

- [Integration guide](../../../docs/integrations/finfeed.md): Setup, API contracts, lifecycle, and provider limits.
- [Rust example](examples/node_data_tester.rs) and [Python example](../../../examples/live/finfeed/data_tester.py):
  Bounded market-data collection through `LiveNode`.
- [Discovery](../../../examples/live/finfeed/discovery.py),
  [historical L2 replay](../../../examples/live/finfeed/historical_book.py), and
  [historical research](../../../examples/live/finfeed/historical_research.py): Python workflows,
  including explicit DB-backed, flat-file, and exchange-wide OHLCV access without source fallback.
- [Conformance tests](tests/README.md): Native, Python, and deterministic simulation checks.
- [Historical-book reference](tests/book_history.md) and [fixture provenance](test_data/README.md):
  Replay assumptions, licensed published examples, and authored boundary cases.

## License

Licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
