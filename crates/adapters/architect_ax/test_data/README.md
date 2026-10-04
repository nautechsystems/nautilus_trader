# Architect AX Test Data

## Captured responses

`captured/` contains sanitized AX sandbox responses captured on 2026-09-24. HTTP captures retain
complete response envelopes, including pagination. The instruments response contains all 29 returned
instruments. WebSocket files retain one complete frame for each observed message or request response.

Account, user, order, trade, and transaction identifiers use consistent fixture replacements. Client
order IDs, account labels, email addresses, and deposit references are also replaced. Prices,
quantities, timestamps, nulls, and protocol fields retain their captured values. Authentication tokens
and request credentials are excluded.

The HTTP captures cover identity, balances, positions, risk, instruments, tickers, books, trades,
candles, funding rates, funding slots, transactions, fills, orders, and open orders. The
`open-reject`, `open-backoff`, `open-top`, `open-default`, and `open-inactive` captures come from
minimum-size, noncrossing sandbox orders. They cover `rej`, `bo`, `tbl`, omitted `rb`, and `po: false`.
All orders created for these captures were canceled or replaced, with zero executed quantity.

The WebSocket captures cover L1/L2/L3 books, tickers, trades, candles, heartbeats, subscription
responses, login, open orders, and errors. `ws-md-bc.json` and `ws-md-response-5.json` retain a BBO
candle and its subscription response; BBO candle subscriptions are not implemented by the adapter.

The additional lifecycle captures include acknowledged, replaced, canceled, expired IOC, rejected,
and filled orders. HTTP captures include nonzero long and short positions, fill history, initial
margin, aggressive-order preview, and single-order status responses. Both test-created long and short
positions are closed after capture. AX WebSocket fills omit commission; the HTTP fill report supplies
the fee and can appear several seconds after the WebSocket event. Captured empty cancellation reasons
map to the model's `Unknown` variant.

## Specification sources

The wire models and enum values use these official specifications, retrieved on 2026-09-24:

- [HTTP API gateway, version 16.3.0](https://docs.architect.exchange/openapi/api-gateway.json).
- [HTTP order gateway, version 16.3.0](https://docs.architect.exchange/openapi/order-gateway.json).
- [Market data AsyncAPI, version 0.1.0](https://docs.architect.exchange/openapi/asyncapi-marketdata-publisher.bundled.json).
- [Orders AsyncAPI, version 0.1.0](https://docs.architect.exchange/openapi/asyncapi-order-gateway.bundled.json).

Their SHA-256 digests, in the same order, are:

```text
71fbdbfa668e372d2186c6356d9e8f5cd999daee4f2b56aca9822d1780eaf7e2
74752f809058be444f3459457af654215f11535fd3cd9770df78a85dcaf1b068
eb918d9af73bc83ecd7be7f38cb5057b4826281928c23405bdfc8ecff3f80fd1
6693af61471868d54175adec6c0af1d6ed2b7cf5877af7554c90c3f3b191838d
```

`wire_enum_values.json` records the documented values of modeled response enums and the request,
event, and environment variants supported by the adapter. Cancel reason strings are open-ended;
the fixture lists the recognized values, including their fallback.

## Compatibility and derived cases

The top-level JSON files predate this capture set. Their original capture provenance is not recorded;
use them as targeted examples or compatibility cases. Required account, post-only, position cost-basis,
margin, and instrument metadata fields are aligned with the current schemas where applicable.

`ws_order_amended.json` derives an in-place amendment from the replacement example, following the
schema's `ro`-only form. `ws_order_filled_unknown_state.json` and `ws_order_filled_unknown_tif.json` substitute an unknown
sibling classification to verify that a fill still reaches execution. Files containing `invalid` test obsolete or
malformed shapes. Tests also derive missing, null, and future enum values from these fixtures.

The captures preserve these observed differences from the published schemas:

- HTTP candle timestamps are integer epoch seconds; OpenAPI declares RFC 3339 strings.
- Ticker price fields and estimated funding amounts can be null.
- Login responses can contain `o: null`.
- Instrument `additional_product_specs` can be null.
- Funding-slot responses include `slot_interval_minutes`, which OpenAPI does not list.

Older ticker captures also omit mark price and instrument state. The decoder keeps those fields
optional for compatibility. L2/L3 `st: false` frames are rejected because the adapter currently builds
books from full snapshots; treating incremental data as a snapshot would clear unchanged levels.
