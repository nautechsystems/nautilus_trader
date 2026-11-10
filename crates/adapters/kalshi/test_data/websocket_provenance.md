# WebSocket Fixture Provenance

`websocket.json` contains independently authored synthetic command and response frames. Market names,
subscription IDs, timestamps, amounts, and error text are original test inputs. No account data, SDK code,
or packaged fixtures are copied.

The frames follow the [official WebSocket API 2.0.0 schema](https://docs.kalshi.com/asyncapi.yaml),
retrieved on 17 October 2026 with SHA-256 `9edefced7ca008b3e5225a11b8db2b56974efc8efc2c6918b66de118f048259f`.
The [connection contract](https://docs.kalshi.com/websockets/websocket-connection) defines command correlation,
and the [keepalive contract](https://docs.kalshi.com/websockets/connection-keep-alive) defines Ping/Pong.
Invalid IDs, mismatched filters, terminal errors, and malformed frames are adversarial test inputs.
These fixtures prove local protocol behavior.

`market_data.json` contains independently authored synthetic trade and ticker payloads with
prices, fractional sizes, and timestamps. The field contracts follow [market ticker](https://docs.kalshi.com/websockets/market-ticker),
[public trades](https://docs.kalshi.com/websockets/public-trades), and [order direction](https://docs.kalshi.com/getting_started/order_direction),
retrieved on 18 October 2026. No SDK source or packaged fixtures are reused.

`websocket_messages.json` holds synthetic notification, command, and rejection cases extracted from
the tests. Tests replace market and order identities, timestamps, and selected values for each scenario.
The private notification fields follow [user fills](https://docs.kalshi.com/websockets/user-fills),
[market positions](https://docs.kalshi.com/websockets/market-positions), and
[user orders](https://docs.kalshi.com/websockets/user-orders), checked on 5 November 2026.
The WebSocket schema retrieved that day remains version `2.0.0`, with SHA-256
`33fd603e0f7a95acd0da82a70cfc9126c31ddc27528a6e696efb2c3806b56b23`.
The schema defines error code 27 for the subscription command rate limit.
Missing fields, stale IDs, future timestamps, and malformed economics remain controlled negative inputs.
These cases are not venue captures or verbatim documentation examples.

`orderbook_delta_incomplete.json` retains the fuzz target's synthetic delta with required payload fields
omitted. The `one_sided` case in `books.json` retains the stress test's synthetic snapshot payload.
Their field names follow [orderbook updates](https://docs.kalshi.com/websockets/orderbook-updates);
their omissions exercise local parsing and book handling.
