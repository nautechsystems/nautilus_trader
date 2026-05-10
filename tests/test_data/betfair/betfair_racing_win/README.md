# Betfair racing WIN dataset

Horse racing WIN market at Nottingham (market ID `1.245077076`) recorded from
the Betfair Exchange Streaming API on 2025-06-26.

- **Market type:** WIN (6 runners, 2 removed pre-race)
- **Event type:** Horse Racing, Flat (event type ID 7)
- **Settlement:** Runner 75925986 won (BSP 2.446)
- **Data:** 27,458 MCM lines, ~942 KB gzip

## Setup

Copy `1.245077076.gz` to `tests/test_data/local/betfair/`.

## Usage

```bash
cargo run -p nautilus-betfair --example betfair-load-file -- \
  tests/test_data/local/betfair/1.245077076.gz
```
