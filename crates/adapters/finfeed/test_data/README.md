# FinFeed Fixture Provenance

Fixtures use API Bricks' published examples and independently authored scenarios. This directory does
not contain independently captured API responses. Live checks use caller-supplied samples outside the
fixture corpus.

## Published examples

The [manifest](published/manifest.json) records the pinned OpenAPI source, JSON pointers, source hash,
and fixture hashes. The source explicitly declares the [MIT license](published/LICENSE); retain that
notice when redistributing the extracted examples.

The examples come from API Bricks' [Prediction Markets OpenAPI specification][openapi]. Extraction
preserves every example field and exact decimal value. JSON whitespace and number spelling are
normalized; numeric values remain JSON numbers. These documentation examples do not establish live
provider coverage or original response bytes.

| Fixture                             | Published example                | Test use                                              |
| ----------------------------------- | -------------------------------- | ----------------------------------------------------- |
| [Exchanges](exchanges.json)         | Supported exchange list.         | Discovery and connection bootstrap.                   |
| [Exchange](published/exchange.json) | Single exchange.                 | Every exchange metadata field.                        |
| [Markets](published/markets.json)   | Two binary outcome records.      | Raw fields inside an authored cursor envelope.        |
| [Active IDs](published/active.json) | Active market ID list.           | Exact ID order and pagination parameters.             |
| [Market](published/market.json)     | Single outcome metadata.         | Raw fields and rejection of an incomplete instrument. |
| [Periods](published/periods.json)   | Fixed and calendar periods.      | Every period field.                                   |
| [Book](published/book.json)         | Empty sides and year-one clocks. | Raw decoding without a checked historical-book claim. |

`dst::published_examples_preserve_discovery_and_raw_response_fields` exercises these examples through
the HTTP client. The published market lacks required Polymarket source fields, so it cannot bootstrap
a checked instrument. The published listing array needs the authored `data`/`next_cursor` envelope
used by the client. These adaptations do not establish additional provider guarantees.

## Authored scenarios

All other fixtures are authored test inputs under the repository license. Their names, descriptions,
IDs, prices, sizes, and event sequences describe test cases, not actual markets or API observations.
Venue names and wire field names identify the protocol being tested.

- `polymarket-market.json` and listing pages supply checked binary definitions, distinct outcome
  tokens, descriptions, tags, ticks, expiries, and pagination cursors. Extra token `price` and `winner`
  fields check that instrument conversion ignores unrecognized token fields. Other venue listings
  exercise identity preservation and explicit rejection of unsupported definitions.
- `polymarket-book.json` constructs ten asks and 36 bids with distinct sizes. The period catalog
  uses the 43 IDs asserted by `data::history::tests::every_claimed_catalog_period_maps_to_a_valid_specification`.
  Lengths are calculated from each ID's unit/count; display labels are authored. This catalog tests
  fixed and calendar period mapping, not the provider's returned catalog or advertised coverage.
- `polymarket-quotes-day.json` checks every quote field and separate event/receive clocks.
  `activity-mixed.json` supplies sixteen quotes and two trades in reverse timestamp order;
  `activity-market.json` supplies its independent checked definition.
- `bars-zero-volume.json` supplies out-of-order completed periods with distinct OHLC prices, zero
  volume, and zero trades. It tests conversion and chronological delivery without a trade-only claim.
- `book-history-actions.json` and `book-history-batch.json` exercise raw action codes, zero sizes,
  both sides, empty and nonempty IDs, tied timestamp groups with non-sorted prices, and a 100-record
  response. Exact positional comparisons check response order within each group.
- `client-*`, `poll-*`, `book-replay.json`, and the exact, malformed, overflow, underflow, null, and
  normalization cases retain their boundary purposes. Replay checks exact ladders, not venue history.

These scenarios preserve behavioral regression checks for decoding, checked conversion, ordering,
deduplication, request limits, polling, cancellation, reconnects, and replay. They do not prove that
the provider returns a particular payload, complete history, synchronized metadata, or stable trade IDs.
See the [field contracts](../tests/market_contracts.md) and [book replay contracts](../tests/book_history.md)
for the remaining mapping and continuity limits.

[openapi]: https://github.com/api-bricks/api-bricks-sdk/blob/20ae84c54320b32b8af4fda0515de047ce689fa0/finfeedapi/prediction-markets-api-rest/spec/openapi.json
