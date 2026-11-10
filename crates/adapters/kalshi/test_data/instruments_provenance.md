# Instrument Fixture Provenance

`instruments.json` combines public production REST captures from 17 October 2026 with controlled inputs.
The `definitions` entries contain these market/event pairs:

- `live`: an active `KXBTC15M` market and its event, including fractional counts and a tapered grid.
- `closed`: a determined NCAA team-total market and its event.
- `settled`: a finalized `KXBTC15M` market and its event.
- `archived`: a finalized Brazilian soccer total from the historical markets endpoint.
- `unsupported`: an esports soccer market whose event uses `MECNET`.
- `decimal_strike`: a native BTC daily strike ticker with a decimal point and `DIRECNET` event.

The separate `series` entry contains the `KXBTC15M` series response envelope.

The responses come from `/markets`, `/markets/{ticker}`, `/historical/markets`,
`/events/{event_ticker}`, and `/series/{series_ticker}`. They contain no credentials or account data.
The fixture retains public rules and outcome descriptions. Unknown response fields remain in the
capture even when the typed REST model does not consume them.

The whole-cent, sub-cent, half-cent, and nonuniform grid cases reproduce structures documented in
[fixed-point representation](https://docs.kalshi.com/getting_started/fixed_point_migration).
The `float_boundary` case is a controlled subinterval of the cent lattice that detects a floating-point
endpoint error. Invalid grids and selected legs are controlled negative inputs based on the
[market schema](https://docs.kalshi.com/api-reference/market/get-market).

The tests mutate captured metadata for unsupported economics, identity failures, shard mismatches,
missing update times, and grid changes. These mutations are test cases, not additional venue observations.
