# Lifecycle Fixture Provenance

`lifecycle.json` contains controlled lifecycle notifications based on the official
[WebSocket schema](https://docs.kalshi.com/asyncapi.yaml), API version 2.0.0. These are synthetic
protocol inputs, not captured venue events. Decimal strings preserve the documented price-grid format.
The schema is retrieved on 18 October 2026 with SHA-256
`9edefced7ca008b3e5225a11b8db2b56974efc8efc2c6918b66de118f048259f`.
The ticker comes from the captured `live` instrument fixture. The transcript contains no credentials or account data.

The transcript covers creation, activation, pause/resume, an early close-date change, a grid change,
a subtitle update, determination, and a settlement notification. Determination values remain provisional.
REST dispute, amendment, finalized payout, duplicate/stale evidence, and unavailable-metadata tests mutate
market/event definitions from [the instrument fixtures](instruments_provenance.md).
