# Kraken test fixtures

## Spot order rows

`http_open_orders_conditional.json`, `http_closed_orders_conditional_canceled.json` and
`http_closed_orders_conditional_triggered.json` are OpenOrders and ClosedOrders responses Kraken
returned on 2026-10-13 for 5 USDC buy orders on USDC/EUR, placed on a live account and canceled
there. No order could fill: the untriggered orders had triggers 50% from the market and the limit
order a limit at half of it, and the orders meant to trigger had limits at half the market or less.
The order IDs are replaced with IDs of the same form (`OCAP01-STOPL-000001` onward); every other
value is as Kraken returned it, with two-space indentation.

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

Three robustness tests change one field of a captured row: a malformed `descr.price`, an empty or
relative `stopprice`, and an untriggered trailing-stop-limit's offset written unsigned. Those
variants are not venue responses.

## Spot executions frames

`ws_executions_pending_new_doc_example.json` and `ws_executions_trade_doc_example.json` are the
`pending_new` and `trade` update frames from Kraken's
[executions channel documentation](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/executions),
retrieved on 2026-10-13 and unchanged apart from whitespace.

## Futures split amendment

`ws_futures_split_amend.json` is a synthetic sequence authored on 2026-10-31, not a venue capture.
It combines the wire shapes in Kraken's
[open orders](https://docs.kraken.com/exchange/api-reference/futures-websocket/open_orders) and
[fills](https://docs.kraken.com/exchange/api-reference/futures-websocket/fills) documentation with
the immediate size reduction and delayed reprice described in the
[22 October 2026 changelog](https://docs.kraken.com/exchange/changelog) and
[Maker Protection guide](https://docs.kraken.com/exchange/guides/futures/maker-protection).
These sources were retrieved on 2026-10-31. No source supplies this exact split-amend WebSocket sequence.

The fixture treats open-orders `qty` as the current total size and `filled` as cumulative fills.
The open-orders reference calls `qty` the order's quantity without explicitly distinguishing total
from remaining size. The fills reference separately defines `remaining_order_qty` as unfilled size.
This interpretation matches the adapter; the synthetic sequence does not independently verify it
against venue messages.

The five frames describe one `PF_APEUSD` buy order: placement of 0.0040 at 70000.0, reduction to
0.0025 at 70000.0, a maker fill of 0.0007 at the old price, its cumulative open-order update, and
repricing to 70123.5 with 0.0018 left. Prices, sizes, instrument increments, timestamps, and UUIDs
are invented. The 20 ms interval between reduction and repricing models a hold; it does not measure
venue latency.
The test processes the fill and its cumulative update in both arrival orders. Neither ordering is
claimed as a venue guarantee. The guide says an order can fill at its old price during a held
amend and that a reduction in the amend applies when the hold opens.

This fixture and its test check local dispatch and downstream order-state compatibility. They do
not certify venue activation, feed ordering, margin reservation, REST response envelopes, or batch behavior.
