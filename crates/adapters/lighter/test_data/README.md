# Lighter Test Data

## Sources

- `http_order_book_details.json`, `http_order_books.json`, `http_order_book_orders.json`, and
  `http_recent_trades.json` were captured from Lighter Mainnet REST endpoints on
  2026-05-05.
- `http_order_book_depth.json` and `http_orders.json` are schema fixtures for REST models whose
  exact public endpoint response was not available without auth or was blocked for unauthenticated
  access during fixture collection.
- `ws_*.json` fixtures follow the official Lighter WebSocket documentation examples and message
  field definitions.
- `ws_spot_market_stats_subscribed_single_empty_mid.json` is a verbatim live Testnet
  `subscribed/spot_market_stats` frame captured on 2026-09-18, pinning the venue's empty-string
  mid price on a market with no resting quotes on one side.
- `ws_*_subscribed_*_bad_body.json` fixtures are constructed malformed confirmation frames
  (valid `type`/`channel` header, mistyped body) proving subscriptions complete from the
  header instead of hanging.
- `http_order_book_details_widened_ids.json` and `http_order_books_widened_ids.json` cover the
  post-September-2026 64-bit market-ID allocation: the 4095/4098 rows mirror live Lighter Testnet
  responses captured on 2026-09-18, while the 40000/50000 rows are synthetic future IDs above the
  legacy `i16` range in the same venue shape. The `ws_*_widened.json` frames apply the same
  widened IDs to the documented WebSocket shapes.

## References

- REST OpenAPI: <https://raw.githubusercontent.com/elliottech/lighter-python/main/openapi.json>
- WebSocket docs: <https://apidocs.lighter.xyz/docs/websocket-reference>
- Public REST base URL: <https://mainnet.zklighter.elliot.ai/api/v1>
