# Kraken test fixtures

## Spot order rows

`http_open_orders_conditional.json`, `http_closed_orders_conditional_canceled.json` and
`http_closed_orders_conditional_triggered.json` are OpenOrders and ClosedOrders responses Kraken
returned on 2026-10-13 for 5 USDC buy orders on USDC/EUR, placed on a live account and canceled
there. Each limit sat at half the market price or less, so no order could fill. The order IDs are
replaced with IDs of the same form (`OCAP01-STOPL-000001` onward); every other value is as Kraken
returned it, with two-space indentation.

- `http_open_orders_conditional.json` holds eight untriggered orders: two stop-losses, one placed
  at `+50%`, a take-profit, a stop-loss-limit, a take-profit-limit, a trailing-stop at `+50%`, a
  trailing-stop-limit at `+50%` with a `+1%` limit offset, and a limit placed at `-50%`. Kraken
  resolves the relative prices of non-trailing orders when it accepts them and keeps the trailing
  offsets.
- `http_closed_orders_conditional_canceled.json` holds the same eight orders once canceled, the
  first stop-loss amended to a new trigger (`misc` is `amended`).
- `http_closed_orders_conditional_triggered.json` holds a stop-loss-limit and a take-profit-limit
  canceled untriggered, and a stop-loss-limit, a take-profit-limit and a trailing-stop-limit
  canceled after triggering (`misc` is `stopped` or `touched`).

`http_open_orders_doc_example.json` and `http_query_orders_doc_example.json` are the complete
response examples from Kraken's
[Get Open Orders](https://docs.kraken.com/api-reference/account-data/get-open-orders) and
[Query Orders Info](https://docs.kraken.com/api-reference/account-data/query-orders-info)
documentation, retrieved on 2026-10-13 and unchanged apart from whitespace. A QueryOrders row has
the ClosedOrders row shape: the 2026-10-13 capture read the same canceled orders through both
endpoints and received equal rows. The query example's stop-loss-limit row `OBCMZD-JIEE7-77TH3F`
describes its order as `@ limit 27500.0` with `price2` `0`, a form neither capture returned for a
canceled or triggered stop-loss-limit, so no test asserts it.

Two negative tests change one field of a captured row, a malformed `descr.price` and an empty
`stopprice`. Those variants are not venue responses.

## Spot executions frames

`ws_executions_pending_new_doc_example.json` and `ws_executions_trade_doc_example.json` are the
`pending_new` and `trade` update frames from Kraken's
[executions channel documentation](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/executions),
retrieved on 2026-10-13 and unchanged apart from whitespace.
