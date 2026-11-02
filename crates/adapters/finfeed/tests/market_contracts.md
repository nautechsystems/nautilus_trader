# Prediction Market Field Contracts

Use this reference before extending FinFeed instrument or data projections. Discovery does not establish
instrument support: the adapter accepts only checked Polymarket Yes/No `BinaryOption` definitions.
Execution and other FinFeed products are out of scope.

## Sources and fixtures

The [published OpenAPI][openapi] defines the raw models and examples. The
[market reference](https://www.finfeedapi.com/products/prediction-markets-api/docs/rest-api/markets/markets/exchange_id/market_id/get)
does not establish venue payout units, source increments, snapshot derivation, or final settlement.
Native venue rules alone do not prove FinFeed normalization.

The [fixture guide](../test_data/README.md) distinguishes licensed published examples from authored
scenarios. The published market example lacks the source fields needed for checked conversion.
Authored metadata tests verify the parser's required fields and error paths; they do not prove that
the provider supplies those fields for every market.

## Currency, payout, and increments

[Polymarket](https://docs.polymarket.com/concepts/pusd) defines six-decimal `pUSD`; the
[native parser](../../polymarket/src/http/parse.rs) supplies the approved share convention.
The FinFeed mapping uses price precision four, share precision six, and a share increment of `0.000001`.
It requires a fetched tick strictly inside `(0, 1)` and representable without rounding.
Ordinary binary redemption rules do not establish authoritative settlement for a FinFeed record.

The [definition parser](../src/http/parse.rs) rejects the other eight venues until their complete field
rules are verified. Their authored listing fixtures exercise that rejection and exact ID preservation.

| Family              | Required field rules before native mapping                                                          | Boundary to preserve                                                              |
| ------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Kalshi              | Currency, payout units, full per-market price grid, activation, cutoff, expiry, and early close.    | Zero `TickSize` cannot replace `price_ranges` or `price_level_structure`.         |
| Myriad              | Network/token identity, share/payout units, increments, outcome IDs, date format, and trade clock.  | Token decimals do not establish share precision; paused status needs precedence.  |
| Manifold            | MANA/CASH identity, payout/cancellation, answer IDs, classification, and positive share quantities. | A binary label cannot establish a multiple-choice answer mapping.                 |
| Hyperliquid Outcome | Network, template tradability, outcome encoding, units, increments, and lifecycle.                  | Native asset encoding does not establish spot/perpetual defaults.                 |
| Pascal              | Outcome/payout mapping, joint tick/significant-figure rules, size increments, and lifecycle.        | Preserve significant dots in provider IDs.                                        |
| Gemini              | Outcome identity, units, increments, timezone, lifecycle, and final resolution.                     | Quote-asset precision alone does not establish contract precision or payout.      |
| Crypto.com          | Outcome suffix rules, units, increments, status precedence, lifecycle, and usable activity.         | Empty discovery or a missing activity response does not establish venue coverage. |
| ForecastEx          | Stable provider IDs, activation, cutoff, expiry, units, increments, and payout/finality.            | A nonterminal settlement-price field cannot establish final payout.               |

[Kalshi's fixed-point rules](https://docs.kalshi.com/getting_started/fixed_point_migration) include dollar
strings, fractional contracts, and market-specific price ranges. Do not infer a universal tick from
trade differences. Currency identity needs network/token identity or an explicit mapping; a matching
code such as bridged USDC does not establish the built-in currency. [Manifold](https://docs.manifold.markets/api)
distinguishes MANA and CASH and permits probability resolution; do not silently assign USD.

Preserve source prices separately when their relationship to normalized prices is unclear. Zero prices
do not establish increments, payout, or a usable snapshot. Polymarket `Rates[].asset_address`, `MinSize`,
and `MaxSpread` are [rewards metadata](../../polymarket/src/http/models.rs), not collateral or order constraints.

## Identity, lifecycle, and outcome rules

[Symbol conversion](../src/common/symbol.rs) preserves the exact provider `market_id` under
`FINFEED_<exchange_id>`. Native IDs, titles, and slugs are not substitutes. Embedded times, yearless
titles, and ambiguous dates cannot establish lifecycle timestamps without provider format and timezone
rules. Metadata and activity are separate observations; neither supplies an atomic market snapshot.

The parser requires normalized `Binary`, a Yes/No label, a matching ID suffix, a valid `ConditionId`,
and exactly two distinct numeric token IDs with distinct Yes/No labels. It rejects other binary labels,
`MultipleChoice`, and `Numeric`. Wider support needs verified grouping, units, bounds, and payout rules.
Native [events](https://docs.polymarket.com/concepts/markets-events) group independently resolvable
binary markets; several candidates do not prove a FinFeed multiple-choice instrument, and an
above/below question does not prove scalar payout.

Do not merge definitions or relax the [duplicate-outcome guard](../src/instruments.rs) when records
conflict about token/outcome identity. Changes to prices, ticks, or rewards minima alone do not establish
an identity conflict. `NegRisk` fields do not establish FinFeed grouping semantics.

## Projection decisions

Keep data without a justified domain mapping in the [raw models](../src/http/models.rs).
The [checked projections](../src/data/parse.rs) enforce these boundaries:

| Field        | Checked behavior                                                  | Remaining limit                                                                  |
| ------------ | ----------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Prices       | Decode exact `Decimal`; reject rounding, overflow, and underflow. | Provider derivation and price precision remain unverified.                       |
| Sizes        | Require exact share precision and positive trade size.            | Currency decimals do not establish increments or quantity.                       |
| Trade IDs    | Preserve nullable IDs raw; require an ID for a checked tick.      | Origin, stability, uniqueness, corrections, and revisions are unverified.        |
| Trade clocks | Copy the single trade timestamp to both native clocks.            | `ts_init` does not measure local receipt; per-venue clock meaning is unverified. |
| Trade side   | Preserve raw side and optional outcome; use `NoAggressor`.        | A side or matching outcome does not establish maker/taker semantics.             |
| Settlement   | Retain metadata without an authoritative settlement event.        | Status, price, winner, and end date do not establish final payout.               |

[Polling](../src/data/poll.rs) retains the first valid event per instrument/trade ID; corrections under
retained IDs are ignored. A UUID shape or tightly spaced timestamps proves neither ID stability,
event splitting, processing-time assignment, nor invalid trades.

Native [displayed prices](https://docs.polymarket.com/concepts/prices-orderbook) use midpoint with a
last-trade fallback above spread `0.10`; FinFeed does not promise that rule. A `0.0001`/`0.0002` midpoint
would be `0.00015` without changing the tradable tick. Snapshot precision and tradable grid are separate:
projection checks representability, not grid membership. Require a provider definition and finer-price
sample before changing precision. The fetched tick can age; FinFeed has no periodic metadata refresh.

[Fixed-point types](../../../model/src/types/fixed.rs) permit nine decimals in standard mode, 16 in
high precision, and up to 18 with `defi`. Finer prices need provider normalization or an explicit model
decision. Zero-size trades remain raw; do not invent quantity to satisfy `TradeTick::new_checked`.

## Settlement boundary

[Polymarket resolution](https://docs.polymarket.com/concepts/resolution) distinguishes proposals,
disputes, and final resolution, including 50/50 payout. FinFeed winner and `Is5050Outcome` flags lack
verified payout/finality semantics. No verified final payout vector, finality time, or authoritative
resolution reference supports a settlement event.

Keep `EndDateIso` as the supported Polymarket simulation cutoff and unknown activation as zero.
Do not transfer that fallback to other venues or infer settlement from cutoff, status, winner, or
absence from active IDs. Wider support needs field-specific finality, cancellation, payout-unit,
and revision rules.

[openapi]: https://github.com/api-bricks/api-bricks-sdk/blob/20ae84c54320b32b8af4fda0515de047ce689fa0/finfeedapi/prediction-markets-api-rest/spec/openapi.json
