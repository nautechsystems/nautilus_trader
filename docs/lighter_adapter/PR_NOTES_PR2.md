# PR2 Notes (Public Market Data)

## Summary

- Implemented Lighter public WebSocket client (order_book/trade/market_stats) with reconnect/resubscribe and instrument index cache.
- Added WS message models and parsers to normalize depth snapshots/deltas, trades, and mark/index/funding updates into Nautilus events.
- Wired Python data client + factory to new HTTP/WS clients, including gap detection and REST snapshot resync.
- Exposed REST order book snapshot path and PyO3 bindings (HTTP + WS) for Python surface.
- Fixture-backed Rust tests cover parsing for order book, trades, and market stats.

## Tests

- `cargo test -p nautilus-lighter`

## Follow-ups / Assumptions

- Live WS schema/channel names still based on captured fixtures; adjust after validation spike.
- Further reconnect/reconcile hardening will be addressed in PR5.
