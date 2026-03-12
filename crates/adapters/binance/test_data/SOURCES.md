# Binance fixture sources

This file maps Binance parser surfaces to their primary docs sources.

Use the docs examples first. Fall back to live capture for SBE wire payloads and
for cases where a docs example is missing or stale.

## Spot HTTP

| Surface          | Parser functions                                                                            | Primary source                                                                                                      |
| ---------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Market data REST | `decode_ping`, `decode_server_time`, `decode_depth`, `decode_trades`, `decode_klines`, `decode_exchange_info` | [Spot REST market data docs](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints) |
| Account REST     | `decode_account`, `decode_account_trades`, `decode_orders`                                 | [Spot REST account docs](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/account-endpoints)         |
| Trading REST     | `decode_new_order_full`, `decode_cancel_order`, `decode_order`, `decode_cancel_open_orders` | [Spot REST trading docs](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/trading-endpoints)       |

## Spot WebSocket

| Surface          | Parser functions                                                                  | Primary source                                                                                                      |
| ---------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| User data stream | `parse_spot_exec_report_to_order_status`, `parse_spot_exec_report_to_fill`, `parse_spot_account_position` | [Spot user data stream docs](https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream)              |
| WS API trading   | Spot WebSocket API request and response parsing around trading flows.            | [Spot WebSocket API trading docs](https://developers.binance.com/docs/binance-spot-api-docs/websocket-api/trading-requests) |

## Spot SBE

| Surface                               | Parser functions                                                                     | Primary source                                                                                          |
| ------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| HTTP SBE responses                    | Spot `decode_*` functions in `src/spot/http/parse.rs`.                              | Spot REST docs above for semantic examples, then live SBE capture for raw payloads.                     |
| Market data stream SBE                | `decode_market_data`, `parse_trades_event`, `parse_bbo_event`, `parse_depth_snapshot`, `parse_depth_diff` | [Spot SBE market data docs](https://developers.binance.com/docs/binance-spot-api-docs/sbe-market-data) |
| SBE schema and payload interpretation | Shared SBE decoding and fixture derivation work.                                     | [Spot SBE FAQ](https://developers.binance.com/docs/binance-spot-api-docs/faqs/sbe_faq)                 |

## Futures

| Surface             | Parser functions                                                                                         | Primary source                                                                                           |
| ------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| Futures HTTP        | HTTP model and response fixtures under `src/futures/http`.                                               | Official Binance Futures REST docs for each endpoint we cover.                                           |
| Market data streams | `parse_agg_trade`, `parse_trade`, `parse_book_ticker`, `parse_depth_update`, `parse_mark_price`, `parse_kline`, `extract_symbol`, `extract_event_type` | USD-M Futures market stream docs for aggregate trade, book ticker, diff depth, mark price, and klines. |
| User data streams   | `parse_futures_order_update_to_order_status`, `parse_futures_order_update_to_fill`, `parse_futures_algo_update_to_order_status`, `parse_futures_account_update`, `decode_order_client_id`, `decode_algo_client_id` | USD-M Futures user-data docs for balance and position updates, order updates, and algo order updates.   |

## Notes

- As of March 12, 2026, Binance spot REST, user-data stream, and WS API docs
  publish canonical JSON examples for the major parser surfaces.
- As of March 12, 2026, Binance SBE docs publish schema and transport guidance,
  but not a complete set of raw binary fixture payloads.
- As of March 12, 2026, the Spot user-data docs examples are wrapped in a
  `subscriptionId` and `event` envelope. The WebSocket handler in this crate
  now accepts both wrapped docs payloads and legacy top-level event payloads.
- The low-level Spot user-data message structs still deserialize the inner
  event object directly, so wrapped docs fixtures should use the shared
  `load_event_fixture` helper in tests.
- For SBE coverage, the docs define the expected fields. Live capture supplies
  the canonical wire bytes.
- As of March 12, 2026, Binance does not appear to publish a separate USD-M
  individual trade stream example on the market-stream docs pages. The fixture
  `futures/market_data_json/trade_stream.json` is derived from the published
  aggregate trade example and should be replaced with a live sample if we
  capture one.
- The fixtures `futures/market_data_json/kline_stream_closed.json`,
  `futures/user_data_json/order_update_trade.json`, and
  `futures/user_data_json/algo_update_new.json` are narrow derived variants of
  a published docs example so we can test alternate parser branches that the
  docs do not show directly.
