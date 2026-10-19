# Fixture source

The `polybolt_*.json` fixtures were captured read-only from the production PolyBolt endpoint
`wss://ws-live-v2.polymarket.com/ws` on 2026-10-07 with CLOB API credentials, subscribing to
`price.crypto` (`btcusd`), `price.crypto.twap` (`btcusd`, 60 s) and `price.equity` (`aapl`).

- `polybolt_authed.json`, `polybolt_subscribed.json`: the venue acknowledgements, unchanged.
- `polybolt_*_snapshot.json`: the first envelope after each `subscribed` ack. The venue sent
  117 points (crypto, TWAP) and 599 points (equity) covering the previous two minutes; the
  `data` array is trimmed to the first two and the last point. All retained values are unchanged.
- `polybolt_*_update.json`: the first live envelope per channel, unchanged.
- `polybolt_error_auth_required.json`, `polybolt_error_auth_invalid.json`: constructed from the
  `error` schema in <https://ws-live-v2.polymarket.com/asyncapi.json>, not live captures.

The auth frame is never recorded. `full_accuracy_value` is a decimal string with up to 18
fractional digits (not the signed E18 integer RTDS used for TWAP).
