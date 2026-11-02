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
| `time_coinapi`  | String        | Preserve optional receive clocks.                        |

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

Replay preserves response order and equal-time ties without assuming stability across queries or
revisions. The documented selector uses processing time: exchange time is neither a processing cursor
nor measured receive time. Bound inclusion and stable tie order remain unverified.

Empty IDs and full-field equality do not supply a lossless deduplication key. Saturated arrays can be
truncated; short or empty arrays do not prove exhaustion or coverage. Overlap after a timestamp restart
cannot establish general continuation or an hourly reset boundary. No documented continuation token,
coverage envelope, or complete reset boundary exists. Current books cannot initialize a past window.

## Bounded replay contract

The [pinned provider processing example](https://github.com/api-bricks/api-bricks-sdk/blob/db3b1d1bdd8b7f5b71ab80663c54aa391595d052/coinapi/flat-files-api-processing-samples/limitbook_full_l2l3/Program.cs)
uses ADD=0, SET=1, SUB=2, SNAPSHOT=3, DELETE=4, MATCH=5. It adds relative ADD sizes, sets absolute
SET/SNAPSHOT sizes, and clears on entry to a contiguous snapshot block. Applying these flat-file
semantics to REST is an inference. The replay implementation is independently written and does not
copy the example's batching or subtraction/clamping behavior. Codes 2, 4, and 5 remain unsupported.

Replay requires a returned seed at or before `start`, retrieving from the hour containing `start`
without lookback. Leading updates are validated and discarded before the seed. It assumes each
contiguous same-time snapshot run is a complete replacement and responses are continuous and uncapped.
Each contiguous snapshot or incremental run at a timestamp forms a separate `F_LAST` group.
Every group must leave a valid book, even if a later group shares its timestamp. Applied groups at or
before `start` contribute to the generated seed.
Timestamp-spanning snapshots and overlapping hours fail.

Missing receive clocks fall back to exchange time, without a measured-receipt claim. Emitted receive
clocks must stay ordered inside the requested window because response bounds use `ts_init`;
incompatible clocks reject the whole result. See [request and limit rules](../../../../docs/integrations/finfeed.md#historical-books).

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
1. Repeat retrieval to check ordering and filter bounds, including differing exchange/receive clocks.
1. Compare every final level and size with the independent reference. Reject missing initialization,
   unknown actions, unavailable continuation, and gaps.

The authored fixtures do not pass this live acceptance case. It still needs verified action/reset
semantics, receive clocks, tie ordering, continuation, coverage, and independent historical reference
books. The current-book route cannot supply those past reference books.

[openapi]: https://github.com/api-bricks/api-bricks-sdk/blob/20ae84c54320b32b8af4fda0515de047ce689fa0/finfeedapi/prediction-markets-api-rest/spec/openapi.json
