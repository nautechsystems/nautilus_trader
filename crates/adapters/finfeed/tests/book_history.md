# Historical Book Contracts

The adapter replays returned L2 events under the assumptions below. Complete initialization, REST
action semantics, and lossless continuation remain unverified.

## Sources and fixtures

The [history endpoint](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/orderbooks/orderbook/exchange_id/market_id/history/get)
documents raw flat-file updates filtered by processing time, in ascending order, with a 10000-record cap
including date queries. It documents hourly files with daily fallback but does not identify the files
behind a response. The [published OpenAPI][openapi] declares an untyped response array without a cursor
or offset.

The [fixture guide](../test_data/README.md) records published examples and authored scenarios. Historical
event fixtures are independently authored: `book-history-actions.json` covers six records with both
sides, action codes 0/1/3, and zero sizes; `book-history-batch.json` covers 100 ordered records.
They test exact raw-field preservation, not actual outcome history, closure, or complete date coverage.
The opt-in live comparison in [book_history.rs](integration/book_history.rs) uses a separately supplied
raw response; that payload is not part of the fixture corpus.

## Raw event boundary

| Field           | JSON type     | Behavior exercised by authored fixtures                  |
| --------------- | ------------- | -------------------------------------------------------- |
| `symbol_id`     | String        | Preserve exact spelling and outcome suffix.              |
| `time_exchange` | String        | Preserve nanoseconds and response order, including ties. |
| `price`         | Number/string | Preserve exact values; reject overflow and underflow.    |
| `size`          | Number/string | Preserve zero and fractional sizes.                      |
| `is_sell_ask`   | Boolean       | Preserve both sides.                                     |
| `update_type`   | Integer       | Retain raw codes; check replay actions separately.       |
| `id`            | String        | Preserve empty and nonempty IDs.                         |
| `time_coinapi`  | String        | Preserve optional processing clocks.                     |

Authored cases do not establish provider requiredness, nullability, a closed action enum, stable IDs,
or snapshot-completion markers. Market/exchange IDs, sequence, and measured receive clocks cannot be
inferred from a symbol or exchange timestamp.

Related formats differ:

- The [dataset schema](https://www.coinapi.io/datasets/prediction-market-limitbook-full) defines
  `is_sell_ask=true` as ask and includes a receive clock; this does not establish the REST projection.
- The [FinFeed CSV sample](https://www.finfeedapi.com/products/prediction-markets-api/data-samples)
  uses `is_buy`; its side convention is not the REST field's convention.
- The [Flat Files guide](https://www.finfeedapi.com/products/flat-files/docs/datasets/limitbook) maps
  codes 0/1/2/3/4 to ADD/SUB/SET/DELETE/MATCH, including Prediction Markets Flat Files, without a numeric
  SNAPSHOT code. The adapter does not use this conflicting map for REST replay.

## Ordering and continuation

The raw client preserves response order and equal-time ties. Live probes of daily BTC outcome history
return exchange timestamps that move backwards, empty IDs, and no processing clock. Inclusive
exchange-time restarts omit late source-order rows. The documented selector uses processing time;
exchange time is neither a supported processing cursor nor measured receive time.

Native book retrieval uses an empirical continuation rule:

1. Request up to 10000 rows, starting at the containing UTC hour and ending at its next boundary.
1. When another page is needed, rewind to one second before the largest exchange timestamp rounded
   down to a whole second, without moving the lower bound backwards.
1. Require the retained rows to form a contiguous suffix. The next response must repeat that exact
   sequence as its prefix, comparing every decoded field and preserving duplicate multiplicity.
1. Skip only the matched prefix. A saturated page must advance the lower bound and add new records.
1. Keep the upper selector fixed across saturated-page continuations. After a short page, advance it
   by one hour while carrying the overlap. A mismatch rejects the entire series.
1. Stop after a short page in the last touched hour, or when a returned source clock exceeds the
   requested end on the derived replay clock.

Unless clamped to the current lower selector, the rewind spans between one and nearly two seconds from the
largest timestamp. Differing page sizes and wider-rewind controls reproduce matching prefixes in the probed
stream, but maximum lateness and general selector behavior remain unverified. Empty IDs and field equality
do not supply a lossless deduplication key. No global set deduplication is used. Records excluded by a lower
bound can remain undetected even when overlap matches. Even a small timestamp reversal can straddle the
rewound selector and make the overlap noncontiguous, which rejects the request without partial delivery.
A dense interval can prevent progress even when a full page happens to contain the complete remainder.
Short or empty arrays do not prove coverage. No documented continuation token, coverage envelope, or
complete reset boundary exists. Current books cannot initialize a past window.

## Bounded replay contract

The [pinned provider processing example](https://github.com/api-bricks/api-bricks-sdk/blob/db3b1d1bdd8b7f5b71ab80663c54aa391595d052/coinapi/flat-files-api-processing-samples/limitbook_full_l2l3/Program.cs)
uses ADD=0, SET=1, SUB=2, SNAPSHOT=3, DELETE=4, MATCH=5. It adds relative ADD sizes, sets absolute
SET/SNAPSHOT sizes, and clears on entry to a contiguous snapshot block. Applying these flat-file
semantics to REST is an inference. The replay implementation is independently written and does not
copy the example's batching or subtraction/clamping behavior. Codes 2, 4, and 5 remain unsupported.

Replay requires a returned seed at or before `start`, retrieving from the hour containing `start`
without lookback. Leading updates are validated and discarded before the seed. It assumes each
contiguous same-time snapshot run is a complete replacement and reconciled responses are continuous.
Each contiguous snapshot or incremental run at a timestamp forms a separate `F_LAST` group.
Every applied group must leave a valid book, even if a later group shares its timestamp. Applied groups at or
before `start` on the derived ordering clock contribute to the generated seed.
Timestamp-spanning snapshots and unmatched page or hour-boundary overlap fail.

A row's source clock is its processing timestamp when present, otherwise its exchange timestamp.
Each completed group's derived `ts_init` is the maximum of those clocks and the preceding group's
ordering time. Every delta in the group uses that value; original `ts_event` and application order
remain unchanged. This is a synthetic ordering clock, not measured receipt time. Seed placement and
inclusive end selection use the same clock. A group that straddles a bound on individual source
clocks is selected atomically at its final derived time. Replay stops before action and integrity
validation of the first group past `end`; a cut post-end group cannot invalidate the requested prefix. See
[request and limit rules](../../../../docs/integrations/finfeed.md#historical-books).

The [authored replay fixture](../test_data/book-replay.json) checks exact ladders after relative adds,
absolute sets, zero-size deletion, tied events, replacements, and empty sides. Rust and Python
`OrderBook` consumers verify replay under these assumptions, not source continuity or venue history.

## Complete replay acceptance case

This stronger claim remains unverified. Use live open and closed outcomes with an initial book at
00:00 UTC and an independent final book at the next UTC midnight:

1. Verify a complete reset's processing boundary, start/end, both sides, and later replacement rules.
1. Apply confirmed actions with exact sizes; check every level after additions, replacements, and deletions.
1. Continue a truncated response across a processing-time tie using a supported total-order key or
   cursor; prove no omissions or duplicates.
1. Repeat retrieval to check ordering and filter bounds, including differing exchange/processing clocks.
1. Compare every final level and size with the independent reference. Reject missing initialization,
   unknown actions, unavailable continuation, and gaps.

The authored fixtures do not pass this live acceptance case. It still needs verified action/reset
semantics, processing clocks, tie ordering, continuation, coverage, and independent historical reference
books. The current-book route cannot supply those past reference books.

[openapi]: https://github.com/api-bricks/api-bricks-sdk/blob/df07cde0d11ff2b87fb893df6a83146b4a26b346/finfeedapi/prediction-markets-api-rest/spec/openapi.json
