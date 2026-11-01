# FinFeed Conformance

Run these checks from the repository root. They cover the adapter's market-data surfaces against
local peers; live checks require separate authorization and provider access. Execution and other
FinFeed products are out of scope.

See the [data-testing specification](../../../../docs/developer_guide/spec_data_testing.md),
[field contracts](market_contracts.md), [book replay contracts](book_history.md), and
[fixture provenance](../test_data/README.md). The [integration guide](../../../../docs/integrations/finfeed.md)
describes supported capabilities and provider limits.

## Test gates

### Native and Python

Test the adapter alone to avoid feature unification from other workspace members. These commands cover
standard precision, default high precision, Python bindings, and shared trader/data-engine behavior:

```bash
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=8 cargo nextest run --locked \
  -p nautilus-finfeed --lib --test integration --no-default-features \
  --cargo-profile nextest --profile ci
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=8 cargo nextest run --locked \
  -p nautilus-finfeed --lib --test integration --cargo-profile nextest --profile ci
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=8 cargo nextest run --locked \
  -p nautilus-finfeed --lib --test integration --features python \
  --cargo-profile nextest --profile ci
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=4 cargo nextest run --locked \
  -p nautilus-system --lib --cargo-profile nextest --profile ci \
  -E 'test(/trader::tests::test_.*(retir|dispos)/)'
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=4 cargo nextest run --locked \
  -p nautilus-data --test integration --no-default-features --cargo-profile nextest --profile ci \
  -E 'test(=engine::book::test_process_book_snapshot_publish_for_multiple_intervals_same_instrument)'
uv run --project python --no-sync pytest python/tests/unit/adapters/finfeed -q
bash .pre-commit-hooks/check_dst_conventions.sh
```

The Python suite requires the built extension in `python/.venv`. It checks configuration, validation,
secret redaction, factory wiring, and node requests, subscriptions, interval books, and retirement.
Local peers verify `Authorization` and `X-Referrer` on every request.

`test_discovery_replay_subscriptions_and_fresh_node_restart` reuses the discovery function and
historical-book actor from the examples. It checks definitions apart from construction timestamps,
final book ladders, every quote field, stopped caches, and all twelve request paths. It disposes and
drops the first node before building another on the same thread; this tests a fresh node, not client reconnect.

### Simulation

Enable both `simulation` and `cfg(madsim)`. `make cargo-test-sim` includes compatible FinFeed library
and integration tests; native TLS/proxy, credential subprocess, and live tests remain excluded.

```bash
for seed in 1 7 42 999; do
  CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=8 MADSIM_TEST_SEED="$seed" \
    MADSIM_TEST_NUM=5 MADSIM_TEST_JOBS=1 \
    RUSTFLAGS="--cfg madsim ${RUSTFLAGS:-}" cargo nextest run --locked \
    -p nautilus-finfeed --lib --test integration --no-default-features --features simulation \
    --cargo-profile nextest --profile ci
done
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=8 MADSIM_TEST_SEED=7 \
  MADSIM_TEST_NUM=5 MADSIM_TEST_JOBS=1 \
  RUSTFLAGS="--cfg madsim ${RUSTFLAGS:-}" cargo nextest run --locked \
  -p nautilus-finfeed --lib --test integration --no-default-features \
  --features simulation,high-precision --cargo-profile nextest --profile ci
```

Each test repeats five times with consecutive seeds, matching the nightly gate. Repetitions run
sequentially within a process; nextest runs processes concurrently. Bar tests reset the global clock
before tasks start. Madsim controls scheduling, time, and local HTTP transport; tests use synthetic
credentials and control delays, retries, admission, and refreshes without production test controls.

For fresh-process reproducibility, run either test twice with `MADSIM_TEST_SEED=7` and `--no-capture`:

- `dst::all_endpoint_paths_queries_and_models`: Compare its `wire:` and `model:` records.
- `polling::shared_latest_poll_preserves_every_field_ties_duplicates_and_older_events`: Compare its
  `wire:` and `domain:` records, including exact fields and request order.

### Live checks

Run one authorized check at a time, with at least ten seconds between checks on the same account.
Local attempt limits do not coordinate clients or guarantee billing or account-wide quota.
Stop after HTTP 429 and follow provider retry/reset guidance. Empty history does not prove source coverage.

The ignored `live::bounded_*` checks accept `FINFEED_API_KEY` or `COINAPI_API_KEY`. Select one test:

```bash
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=1 cargo nextest run --locked \
  -p nautilus-finfeed --test integration --cargo-profile nextest --profile ci \
  --run-ignored only -E 'test(=live::bounded_read_only_rest_smoke)'
```

For raw-sample comparisons, save the provider response, wait at least ten seconds, and replace the filter
with the test below. Both use `FINFEED_API_KEY`, make one managed GET without retries, and compare every
modeled field and response position using exact decimals:

| Test                                                               | Raw capture and sample variable                                                                                                                         | Required result             |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `book_history::bounded_live_history_matches_raw_sample`            | Closed outcome/date selected by [the live check](integration/book_history.rs), `limit=100`; `FINFEED_BOOK_HISTORY_SAMPLE`                               | 100 matching records        |
| `exchange_ohlcv::bounded_live_exchange_history_matches_raw_sample` | `/v1/ohlcv/POLYMARKET/history`, `period_id=1HRS`, `time_start=2026-10-11T00:00:00Z`, `time_end=2026-10-12T00:00:00Z`, `limit=5`; `FINFEED_OHLCV_SAMPLE` | Five distinct matching rows |

Set the sample variable to the saved JSON path. These comparisons do not prove completeness,
action semantics, or query-bound inclusion.

### Delivery checks

Native [delivery checks](integration/delivery.rs) compare callbacks with raw responses forwarded
through a recording peer. Supply a key containing only ASCII letters, digits, hyphens, and underscores,
followed by `\nSOPS_OK\n` on stdin, then close the pipe. Emit the marker only after credential extraction
succeeds. Keep the key out of arguments, environment variables, files, and logs. Use `cargo test` so
the test inherits stdin; run one check per process:

```bash
CARGO_BUILD_JOBS=8 cargo test --locked -p nautilus-finfeed --test integration --profile nextest -- \
  --ignored --exact delivery::bounded_quote_history_matches_received_provider_rows \
  --nocapture --test-threads=1
```

Replace the exact name to select the bar check:

| Test                                                             | GET allowance | Required delivery                                                                                         | Deadlines and cadence                     |
| ---------------------------------------------------------------- | ------------- | --------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `delivery::bounded_quote_history_matches_received_provider_rows` | 4             | Three quotes from the requested hour, selected from whole-date history in receive-time order              | 600 s per GET; 640 s for native delivery  |
| `delivery::bounded_closed_bar_publication_and_reconnect`         | 18 total      | Positive initial and resumed bars; exact returned periods; strict advancement past the retained watermark | 600 s per generation; 100 s poll interval |

The first resumed bar must end before reconnect starts, exposing an incorrect reconnect-time watermark
reset. Overlapping-response duplicate suppression has a separate local test in `bar_polling`.
Both delivery checks use at least ten-second GET spacing, no retries, and stop forwarding after any
non-200 response or transport failure. A captured status of `None` means no provider HTTP status is established;
the peer supplies a synthetic 504. Empty or saturated data, deadlines, and quota failures leave delivery
unverified. The quote check's covering-hour control does not prove native delivery or complete history.

## LiveNode and DataTester checks

Local [Rust node tests](integration/node.rs) route quotes and managed books through `DataTester`, checking
exact values, headers, and four requests. Python's `test_builtin_data_tester_shares_activity_and_stops`
checks every quote/trade field and releases a held response after stop to verify quiet shutdown.

For an authorized live Rust run with `FINFEED_API_KEY` or `COINAPI_API_KEY`:

```bash
CARGO_BUILD_JOBS=8 NEXTEST_TEST_THREADS=1 cargo nextest run --locked \
  -p nautilus-finfeed --test integration --cargo-profile nextest --profile ci \
  --run-ignored only -E 'test(=node::bounded_live_data_tester_publishes_quotes_and_book_and_stops)'
```

This requires quotes, a two-sided managed book, a stopped node, and stable cached data after stop.
Use `test(=node::bounded_live_book_unsubscribe_resubscribe)` instead for two depth-5 subscription cycles;
it requires five levels on each side and no events after unsubscribe or disconnect.

The [Python live runner](../../../../python/tests/integration/finfeed_live.py) requires the built
extension and `FINFEED_API_KEY`:

```bash
uv run --project python --no-sync python python/tests/integration/finfeed_live.py
```

It checks quote/trade/book delivery, distinct FinFeed/native Polymarket definitions, and stable caches
after stop. It also compares exact prices, nanosecond timestamps, and independently normalized volumes
for the first three priced daily bars in a fixed 49-day window against raw REST rows. Each raw volume
must meet the normalization bounds. At least one priced bar must exist; empty results fail. This does
not verify complete history or completed live bars.

| Check                       | FinFeed managed attempts | Additional raw GETs |
| --------------------------- | ------------------------ | ------------------- |
| Rust LiveNode/DataTester    | At most 4                | 0                   |
| Rust book resubscription    | At most 4                | 0                   |
| Python node and raw history | At most 8                | 2                   |

All use no execution client, ten-second FinFeed pacing, and no FinFeed retries. The Python runner also
loads one native Polymarket condition with that provider's retry policy, without native subscriptions
or periodic refresh. Follow the [shared live-check limits](#live-checks).

## REST endpoint matrix

[Transport tests](integration/dst.rs) cover exact routes, queries, authentication, attribution,
pagination termination/bounds/cycles/duplicates, malformed data, precision, overflow, retries,
redirects, timeouts, cancellation, pacing, concurrency, budgets, and redaction. Native tests cover
explicit proxies and disabled ambient proxies.

| Endpoint family              | Transport evidence                                             | Domain evidence                                                          |
| ---------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Exchanges and metadata       | `exchanges_use_authenticated_shared_transport`                 | Startup and selected-exchange validation                                 |
| Listings and active IDs      | `cursor_and_active_pagination_terminate_only_at_terminal_page` | `dst::paginated_discovery_loads_only_the_explicit_outcome`               |
| Outcome metadata             | Exact route/model tests                                        | `data::fresh_instrument_requests_and_empty_windows_preserve_every_field` |
| Current/latest activity      | Exact route/model tests                                        | Exact quote/trade polling and deduplication                              |
| Current book                 | Exact route/model tests                                        | Atomic L2 MBP snapshots, delta batches, and depth                        |
| Historical book events       | [Book history tests](integration/book_history.rs)              | [Bounded replay](book_history.md#bounded-replay-contract)                |
| OHLCV periods/latest/history | Exact route/model tests                                        | Catalog matching, closed-bar polling, and historical bars                |
| Exchange-wide OHLCV          | [Exchange OHLCV tests](integration/exchange_ohlcv.rs)          | Raw market rows; no native mapping or metadata traversal                 |
| Flat-file OHLCV              | [Flat-file tests](integration/flat_file_ohlcv.rs)              | Explicit raw source; native bars stay DB-backed                          |
| Trade/quote history          | Exact route/model tests                                        | Daily retrieval, filtering, ordering, ties, and limits                   |

`all_endpoint_paths_queries_and_models` supplies the shared route/model checks.
[Model](../src/http/models.rs), [symbol](../src/common/symbol.rs), [parser](../src/http/parse.rs),
and [error](../src/http/error.rs) tests separately pin fields, identity, definitions, retry classification,
and redaction. [Data tests](integration/data.rs) check domain projections and response envelopes.
The [Python HTTP suite](../../../../python/tests/unit/adapters/finfeed/test_finfeed_http.py) covers
query/result projection, both traversal methods, exception conversion, redaction, and cancellation.

## Data acceptance cases

Case IDs follow the shared data-testing specification. Rust cases run in native and simulation legs;
Python cases run through the [node suite](../../../../python/tests/unit/adapters/finfeed/test_finfeed_node.py).

| Case           | Capability                         | Evidence or unsupported reason                                                                       |
| -------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------- |
| TC-D01         | Request instruments                | `data::fresh_instrument_requests_and_empty_windows_preserve_every_field`                             |
| TC-D02         | Subscribe instrument               | Unsupported; fresh requests publish definition updates                                               |
| TC-D03         | Load specific instrument           | `dst::instrument_startup_and_fresh_requests_return_exact_selections`                                 |
| TC-D10, TC-D14 | Book deltas and managed book       | `book_polling::shared_snapshot_polls_replace_consumers_and_reject_invalid_duplicate_stale_books`     |
| TC-D11         | Interval book callbacks            | Python `test_polling_actor_stop_releases_rest_demand[book]`; shared interval cadence test            |
| TC-D12         | Book depth                         | `book_polling` depth-limit, duplicate-command, and above-inline-capacity cases                       |
| TC-D13, TC-D41 | Book snapshots and historical bars | `data::books_and_closed_bars_preserve_complete_envelopes`; future-closing bars excluded              |
| TC-D20, TC-D30 | Quote/trade polling                | `polling::shared_latest_poll_preserves_every_field_ties_duplicates_and_older_events`                 |
| TC-D21, TC-D31 | Quote/trade history                | `data::quotes_and_trades_preserve_envelopes_boundaries_order_and_exact_limits`; empty dates and ties |
| TC-D40         | Bar polling                        | `bar_polling::public_bars_start_at_subscribe_and_restore_without_duplicates`                         |
| TC-D50, TC-D51 | Mark/index prices                  | Unsupported subscriptions                                                                            |
| TC-D52, TC-D53 | Funding rates/history              | Unsupported                                                                                          |
| TC-D60         | Instrument status                  | Unsupported event; lifecycle metadata uses fresh definitions                                         |
| TC-D61         | Instrument close                   | Unsupported event; no inferred settlement price                                                      |
| TC-D62, TC-D63 | Greeks/option chains               | Unsupported subscriptions                                                                            |
| TC-D70         | Unsubscribe on stop                | Python `test_polling_actor_stop_releases_rest_demand`; Rust in-flight cancellation                   |
| TC-D71         | Custom subscribe params            | Unsupported; polling controls use config                                                             |
| TC-D72         | Custom request params              | Book `end`/`book_type`; L3 and snapshot time aliases rejected; opaque params preserved               |
| TC-D73         | Retirement cleanup                 | Python `test_retirement_preserves_shared_internal_bars_until_final_subscriber`; shared trader tests  |
| TC-D74, TC-D75 | DeFi pool demand/bootstrap         | Unsupported subscriptions and snapshots                                                              |

Additional test-to-case mappings:

- **TC-D12**: `book_polling::depth_limits_return_available_levels_and_duplicate_commands_are_idempotent`
  and `book_polling::depth_above_inline_capacity_keeps_sorted_available_levels`.
- **TC-D31**: `data::empty_daily_page_advances_and_point_window_retains_shared_timestamps`.
- **TC-D21/TC-D31**: `data::multi_month_history_crosses_empty_dates_and_keeps_earliest_records`.
- **TC-D21**: `data::oversized_history_preserves_allowance_for_a_smaller_request` rejects oversized
  windows before spending allowance.
- **TC-D41**: `data::periods_closing_after_now_never_become_historical_bars`,
  `data::complete_calendar_bars_use_the_full_window`, and Python
  `test_public_bar_requests_return_complete_calendar_periods`.

For TC-D72, `data::historical_books_preserve_correlation_bounds_and_reconnect_replay` checks snapshot
`end`; `missing_instruments_invalid_windows_and_historical_books_fail_before_io` rejects L3 and time
aliases. The quote/trade and book/bar envelope tests preserve opaque params.

[Activity](integration/polling.rs), [book](integration/book_polling.rs), and [bar](integration/bar_polling.rs)
polling tests cover shared consumers, due times, retry recovery, suspension, idempotence, cancellation,
budgets, reconnect, and late-response suppression. [Lifecycle tests](integration/lifecycle.rs) cover
open/suspended refresh, terminal outcomes, in-flight cancellation, stale generations, and retained history.
In-flight cancellation checks are `polling::unsubscribe_or_disconnect_cancels_inflight_without_late_ticks`,
`book_polling::unsubscribe_or_disconnect_cancels_inflight_book_without_late_publication`, and
`bar_polling::bar_unsubscribe_and_disconnect_cancel_delayed_latest`.
Historical replay checks apply exact ladders to Rust and Python `OrderBook` consumers under the source assumptions.

Shared [trader tests](../../../system/src/trader.rs), including
`test_retirement_removes_component_subscriptions` and
`test_retirement_releases_bar_aggregator_after_final_subscriber`, cover handler removal and shared
aggregator retention until final retirement. Failed stop/fault/disposal hooks preserve registration
and subscriptions until retry. The Python retirement test verifies the factory-to-peer boundary and final polling stop.
The [data-engine interval test](../../../data/tests/integration/engine/book.rs)
`test_process_book_snapshot_publish_for_multiple_intervals_same_instrument` checks exact counts at
successive virtual-clock boundaries, separately from Python snapshot delivery and cancellation.

## Robustness and polling workloads

`fuzz-rest` uses production parsers to check market, listing, activity, book, and bar responses.
Accepted values retain exact prices/timestamps and quote/trade sizes; bars follow the
[volume normalization rule](../../../../docs/integrations/finfeed.md#prices-and-finality).
It checks book integrity at full depth and local depths 1 and 10, OHLC bounds, and definition identity
round trips. Exact-token regression tests separately check decimal decoding.

With the installed nightly toolchain and `cargo-fuzz`, run a bounded campaign:

```bash
mkdir -p crates/adapters/finfeed/corpus/fuzz-rest
CARGO_BUILD_JOBS=8 CARGO_PROFILE_RELEASE_LTO=false cargo +nightly fuzz run fuzz-rest \
  crates/adapters/finfeed/corpus/fuzz-rest crates/adapters/finfeed/test_data \
  --fuzz-dir crates/adapters/finfeed --features fuzz -- \
  -max_total_time=60 -max_len=16384 -rss_limit_mb=4096 -print_funcs=0 -print_final_stats=1
```

Corpus and crash artifacts stay in the repository's ignored adapter directories.

The [Criterion suite](../benches/data.rs) checks exact output fields and complete record counts before
timing REST decoding and conversion. Each workload has a `decode_convert` case measured in bytes per
second and a predecoded `convert` case measured in records or input book levels per second, for each
input size and book depth where applicable.

| Workload                | Input sizes                                     | Checked output                         |
| ----------------------- | ----------------------------------------------- | -------------------------------------- |
| Mixed activity          | 1, 10, or 100 records per quote/trade list      | Every quote and trade, in source order |
| Quote and trade history | 1, 100, or 10,000 records                       | Every tick field and timestamp         |
| Current book            | Authored book; 1, 10, 100, or 1,000 levels/side | Full book and depths 1 and 10          |
| Priced and empty bars   | 1, 100, or 10,000 records                       | Exact bars or omitted empty periods    |
| Normalized-volume bars  | 1, 100, or 10,000 records                       | Exact prices and rounded volume        |

Scale inputs repeat fixture rows or construct book ladders; they do not establish provider limits or
history continuity. Book `convert` throughput counts all input levels because validation precedes truncation.
Fixture preparation and semantic checks stay outside timing. The cases exclude network I/O, scheduling,
deduplication, and publication. The local-peer workload below checks request I/O and scheduling;
[polling integration tests](integration/polling.rs) separately check deduplication and publication.

Smoke-run all 66 cases before measuring:

```bash
CARGO_BUILD_JOBS=8 cargo bench --locked -p nautilus-finfeed --bench data -- --test
```

Use the default `bench` profile for local measurements:

```bash
CARGO_BUILD_JOBS=8 cargo bench --locked -p nautilus-finfeed --bench data --profile bench
```

Pass a name filter after `--`, such as `history/quotes` or `book/convert/levels_1000`.
For published results, use `bench-lto` and the controls in the
[benchmarking policy](../../../../BENCHMARKING.md).

The local-peer workload tests 1, 16, and 128 markets with quotes, trades, both book feeds, and a bar type
per market. It checks two rounds, shared requests, period lookups, and five-second endpoint intervals
with 1 ms request pacing. Simulation also checks admission spacing under controlled network latency.
Use `--no-capture` for sequential cases with printed call counts and elapsed times:

```bash
CARGO_BUILD_JOBS=8 cargo nextest run --locked \
  -p nautilus-finfeed --test integration --cargo-profile nextest --profile ci \
  -E 'test(workload::)' --no-capture
```

For `N` markets without retries, bootstrap costs `1 + N` calls, the first round costs `4N`, and later
rounds cost `3N`. Each bar scope loads its period catalog once per connection generation.
At default ten-second pacing, saturated steady cycles have these minimum start-to-start intervals,
excluding response time and retries:

| Markets | Bootstrap calls | First poll calls | Later poll calls | Steady cycle spacing |
| ------- | --------------- | ---------------- | ---------------- | -------------------- |
| 1       | 2               | 4                | 3                | 30 seconds           |
| 16      | 17              | 64               | 48               | 480 seconds          |
| 128     | 129             | 512              | 384              | 3840 seconds         |

The default 100-attempt allowance cannot bootstrap 128 markets. Sixteen markets need at least 160
seconds of pacing, beyond the default 120-second bootstrap deadline. Budget at least
`1 + N + 4N + 3N * R` attempts for `R` later rounds, plus explicit requests and retries, and size deadlines
for response times. These local measurements do not prove provider throughput or billing; reconnect
does not reset provider quota.

Retention tests cover 128-instrument/bar-type bounds, per-subscribed-list allocation and release,
2048-entry caches through `poll_limit=1024`, and 20000-entry caches above it. They exercise full
10000-record batches, repeats, FIFO eviction, and late-record retention in sliding windows.
Book scopes retain one full valid snapshot per instrument; memory depends on supplied depth.

Large-batch tests cover limits 1000, 1001, and 10000, independent quote/trade caps, exact wire limits,
serial scheduling, pacing, and in-flight disconnect. Saturation checks pin `DEBUG` diagnostics and
per-list/OHLCV flags: two short lists are not saturated merely because their combined count exceeds
one list's cap. Bars reserve an extra running-period slot at limits 1, 1000, 1001, and 10000.
The ignored `live::bounded_activity_publication_and_reconnect::case_2` tests `poll_limit=10000`, retained
IDs after reconnect, and quiet shutdown with the same bounded allowance as its default-limit case.
