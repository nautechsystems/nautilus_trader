# Lighter Test Data

## Sources

- `http_order_book_details.json`, `http_order_books.json`, `http_order_book_orders.json`, and
  `http_recent_trades.json` were captured from public Lighter mainnet REST endpoints on
  2026-05-05.
- `http_order_book_depth.json` and `http_orders.json` are schema fixtures for REST models whose
  exact public endpoint response was not available without auth or was blocked for unauthenticated
  access during fixture collection.
- `ws_*.json` fixtures follow the official Lighter WebSocket documentation examples and message
  field definitions.

## References

- REST OpenAPI: <https://raw.githubusercontent.com/elliottech/lighter-python/main/openapi.json>
- WebSocket docs: <https://apidocs.lighter.xyz/docs/websocket-reference>
- Public REST base URL: <https://mainnet.zklighter.elliot.ai/api/v1>
