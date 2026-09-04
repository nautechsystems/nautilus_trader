# Fixture source

`rtds_crypto_twap_sixty_update.json` is a constructed protocol vector, not a
live capture. Its timestamps, symbol, display value, and exact signed-E18 value
come from Polymarket's official SDK regression vectors at these immutable
commits; the 60-second topic and `window_s` use those suites' 60-second case:

- TypeScript SDK: <https://github.com/Polymarket/ts-sdk/blob/fd830725e5a6e7f6181d519a13f76918559e4b34/packages/bindings/src/subscriptions/rtds.test.ts>
- Python SDK: <https://github.com/Polymarket/py-sdk/blob/c8fb84bb51e60f790239056be7be0f5cc337d2e0/tests/unit/test_streams_rtds_events.py>

The fixture verifies NT's public data-client routing and exact wire decoding.
It is not evidence of live RTDS delivery or boundary behavior.
