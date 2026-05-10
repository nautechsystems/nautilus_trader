# Betfair MATCH_ODDS dataset

Football match odds market (market ID `1.253378068`) recorded from the Betfair
Exchange Streaming API on 2026-02-17.

- **Market type:** MATCH_ODDS (3 runners: home, draw, away)
- **Event type:** Football (event type ID 1)
- **Settlement:** Runner 2426 won (BSP 2.22)
- **Data:** 82,061 MCM lines, ~2.5 MB gzip

## Setup

Copy `1.253378068.gz` to `tests/test_data/local/betfair/`.

## Usage

```bash
cargo run -p nautilus-betfair --example betfair-load-file -- \
  tests/test_data/local/betfair/1.253378068.gz
```
