# Fixture source

`gamma_market_crypto_twap.json` is a reduced, field-preserving subset of the
Gamma response from:

<https://gamma-api.polymarket.com/markets?slug=btc-updown-5m-1787414400>

It was captured read-only on 2026-08-23. The original response fixture had
SHA-256 `e6f7216e8364eb7bac549c0a2cac29d78d806a65e4082283c2434f553fa2cbb9`.
Only fields not needed to deserialize or verify the transported metadata were
removed; the retained values are unchanged.
