# Betfair v2

The Betfair Rust adapter is in active parity work. This page tracks the current Rust behavior and
the planned cutover from the stable guide in [Betfair](betfair.md).

This page mirrors the main section order from [Betfair](betfair.md). When the Rust adapter becomes
the primary Betfair path, this file can replace `betfair.md` with small edits instead of a full
rewrite.

## Scope

- Source of truth for this page: `crates/adapters/betfair`
- Stable guide today: [Betfair](betfair.md)
- Purpose of this page: track the current Rust surface, the known gaps, and the cutover path

## Current Rust status

| Area                     | Current Rust behavior                                                                                        | Difference from `betfair.md` today                                        | Cutover work                                        |
|--------------------------|--------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------|-----------------------------------------------------|
| Order types              | `MARKET` only supports `AT_THE_CLOSE`; `LIMIT` supports BSP on close flows.                                  | Stable guide is still Python shaped in this area.                         | Decide final Betfair market order model.            |
| Batch operations         | `SubmitOrderList` and `BatchCancelOrders` are implemented.                                                   | Stable guide used to mark these as unsupported.                           | Keep and promote.                                   |
| Reconciliation scope     | `reconcile_market_ids_only` uses `reconcile_market_ids`; otherwise falls back to `stream_market_ids_filter`. | Stable guide says stream filtering and reconciliation are separate.       | Decide if Rust keeps or removes this coupling.      |
| Full image cache checks  | Rust uses `generate_mass_status()` at startup and does not run `check_cache_against_order_image`.            | Stable guide describes the Python full image cache check.                 | Add parity or document the Rust path as final.      |
| External order filtering | `ignore_external_orders` only skips OCM updates with no `rfo`.                                               | Python also uses it during full image cache checks.                       | Decide final filtering behavior.                    |
| Config surface           | No `certs_dir`, no `instrument_config`, fixed keep alive, required heartbeat value.                          | Stable guide still documents the Python config surface.                   | Decide whether to add parity or bless Rust surface. |
| SSL certificates         | Stream client currently hardcodes `certs_dir=None`.                                                          | Stable guide documents certificate configuration and `BETFAIR_CERTS_DIR`. | Add support or remove from the future guide.        |

## Orders capability

### Order types

| Order Type             | Supported | Notes                                                                       |
|------------------------|-----------|-----------------------------------------------------------------------------|
| `MARKET`               | ✓*        | Rust only supports `AT_THE_CLOSE`, which maps to Betfair `MARKET_ON_CLOSE`. |
| `LIMIT`                | ✓         | Rust supports regular limit orders and BSP on close limit orders.           |
| `STOP_MARKET`          | -         | Not supported.                                                              |
| `STOP_LIMIT`           | -         | Not supported.                                                              |
| `MARKET_IF_TOUCHED`    | -         | Not supported.                                                              |
| `LIMIT_IF_TOUCHED`     | -         | Not supported.                                                              |
| `TRAILING_STOP_MARKET` | -         | Not supported.                                                              |

### Time in force

| Time in force  | Supported | Notes                                                        |
|----------------|-----------|--------------------------------------------------------------|
| `GTC`          | ✓         | Maps to Betfair `PERSIST`.                                   |
| `DAY`          | ✓         | Maps to Betfair `LAPSE`.                                     |
| `FOK`          | ✓         | Maps to Betfair `FILL_OR_KILL`.                              |
| `IOC`          | ✓         | Maps to `FILL_OR_KILL` with `min_fill_size=0`.               |
| `AT_THE_CLOSE` | ✓         | Used for Betfair BSP `LIMIT_ON_CLOSE` and `MARKET_ON_CLOSE`. |

Rust currently also accepts `LIMIT` orders in `AT_THE_OPEN` mode and routes them through Betfair
`LIMIT_ON_CLOSE` instructions. Treat that as current behavior, not a settled public contract.

### Batch operations

| Operation    | Supported | Notes                                      |
|--------------|-----------|--------------------------------------------|
| Batch Submit | ✓         | Implemented through `SubmitOrderList`.     |
| Batch Modify | -         | Not supported.                             |
| Batch Cancel | ✓         | Implemented through `BatchCancelOrders`.   |

## Execution control flow

The current Rust execution path is:

1. Connect the HTTP client and fetch initial account funds.
2. Seed OCM state from cached orders.
3. Connect the Betfair execution stream and subscribe to order updates.
4. Generate startup mass status from `listCurrentOrders`.
5. Reconcile order and fill reports into the execution engine.

Current Rust notes:

- `stream_market_ids_filter` filters live OCM updates.
- `reconcile_market_ids_only=True` uses explicit `reconcile_market_ids`.
- When `reconcile_market_ids_only=False` and `reconcile_market_ids` is unset, Rust currently
  falls back to `stream_market_ids_filter` for startup reconciliation.
- Rust does not yet implement the Python `check_cache_against_order_image` full-image cache check.
- `ignore_external_orders=True` currently skips only OCM updates with no `rfo`.

## Session management and reconnection

Betfair sessions expire every 12-24 hours. The Rust adapter handles session recovery
automatically through three mechanisms:

| Mechanism           | Trigger                           | Action                                                               |
|---------------------|-----------------------------------|----------------------------------------------------------------------|
| Periodic keep‑alive | Every 10 hours.                   | Renew session token, push to all stream watch channels.              |
| Keep‑alive fallback | Keep‑alive returns `LoginFailed`. | Full re‑login via `reconnect()`, push fresh token to streams.        |
| Stream reconnect    | `Connection` message after drop.  | Try keep‑alive, fall back to re‑login on `LoginFailed`, update auth. |

Transient errors (network timeouts, 5xx responses) during keep-alive are logged and
skipped. The existing session token is preserved and the next keep-alive interval
retries. Only `LoginFailed` errors (session expiry) trigger a full re-login.

Both the data and execution clients run identical reconnection logic. Each spawns:

- A **keep-alive task** that periodically refreshes the session and pushes updated
  auth bytes to the stream watch channels.
- A **reconnect handler** that listens for `Connection` messages after a stream
  reconnect, refreshes the session, and pushes the new token.

The stream client stores auth bytes in a `tokio::sync::watch` channel. The
`post_reconnection` closure reads from this channel on each TCP reconnect, so a token
refreshed by either the keep-alive task or reconnect handler is picked up on the next
connection attempt.

The data client reconnect handler also updates the race stream auth when a race stream
is active.

## Tick scheme and pricing

Betfair uses a tiered tick scheme with varying increments across price ranges:

| Price range    | Tick size |
|----------------|-----------|
| 1.01 - 2.00    | 0.01      |
| 2.00 - 3.00    | 0.02      |
| 3.00 - 4.00    | 0.05      |
| 4.00 - 6.00    | 0.10      |
| 6.00 - 10.00   | 0.20      |
| 10.00 - 20.00  | 0.50      |
| 20.00 - 30.00  | 1.00      |
| 30.00 - 50.00  | 2.00      |
| 50.00 - 100.00 | 5.00      |
| 100.00 - 1000  | 10.00     |

Minimum price is 1.01, maximum is 1000.00.

## Order modification

- Price and size cannot change atomically; these require separate operations.
- Price modification uses `ReplaceOrders` (cancel + new order at new price).
- Size reduction uses `CancelOrders` with a `size_reduction` parameter.
- Size increase is not supported; submit a new order instead.

A replace operation generates both a cancel event for the original order and an accepted
event for the replacement. The adapter tracks pending replacements to suppress synthetic
cancel events.

## Order stream fill handling

The execution client processes order updates from the Betfair Exchange Streaming API.
Two configuration options control how updates are filtered:

- `stream_market_ids_filter`: filters at the market level (early exit, silent skip).
- `ignore_external_orders`: filters at the order level (skips OCM updates with no `rfo`).

### Fill handling

The adapter handles several edge cases when processing fills from the stream:

- **Incremental fills**: Betfair reports cumulative matched sizes. The adapter calculates
  incremental fills by tracking the last known filled quantity per order.
- **Overfill protection**: fills that would exceed the order quantity are rejected.
- **Race conditions**: when stream fills arrive before the HTTP order response, the adapter
  caches the venue order ID immediately to ensure correct order matching.
- **Network error recovery**: when an HTTP order submission fails with a network error
  (timeout, connection reset), the order may still have been placed on the venue. The
  adapter leaves the order in SUBMITTED status and retains the customer order reference
  so the stream can confirm the order when it reconnects. API errors (where Betfair
  explicitly rejected) reject immediately.

## Rate limiting

The adapter uses separate rate limit buckets so that account state polling and
reconciliation do not throttle order placement:

| Bucket  | Default | Endpoints                                       |
|---------|---------|-------------------------------------------------|
| General | 5/s     | Account state, reconciliation, keep‑alive.      |
| Orders  | 20/s    | `placeOrders`, `replaceOrders`, `cancelOrders`. |

Order status and fill report queries retry once on session errors after refreshing the
session. `TOO_MANY_REQUESTS` errors retry after a 5-second delay.

## Market version price protection

When `use_market_version=True`, each order request includes the market version last seen
by the adapter. If the market has advanced beyond that version by the time Betfair
processes the order, Betfair lapses the bet rather than matching it against a changed book.

The adapter reads the market version from the instrument's `info` dictionary, which the
Exchange Streaming API's `MarketDefinition` updates populate. Orders submitted before the
first `MarketDefinition` is received do not include a version.

## Custom data types

The Rust adapter emits the same custom data types as the Python adapter through the
market and race streams. All custom data flows automatically when subscribed to markets.

| Type                       | Stream | Description                                       |
|----------------------------|--------|---------------------------------------------------|
| `BetfairTicker`            | Market | Last traded price, traded volume, BSP indicators. |
| `BetfairStartingPrice`     | Market | Realized BSP after market close.                  |
| `BetfairSequenceCompleted` | Market | Marks end of a market change sequence.            |
| `BetfairOrderVoided`       | Order  | Voided order details (size voided, price, side).  |
| `BetfairRaceRunnerData`    | Race   | Live GPS tracking per runner (TPD).               |
| `BetfairRaceProgress`      | Race   | Sectional times, running order, jump data.        |

Race data requires Total Performance Data (TPD) coverage and a Betfair API key with TPD
access. Enable with `subscribe_race_data=True`.

## Multi-node deployment

When multiple trading nodes share a single Betfair account across different markets:

1. Set `stream_market_ids_filter` to include only that node's markets.
2. Set `ignore_external_orders=True` to suppress warnings about orders from other nodes.
3. Set `reconcile_market_ids_only=True` to limit reconciliation scope.

## Current Rust configuration

### Data client configuration

| Option                              | Default  | Notes                                         |
|-------------------------------------|----------|-----------------------------------------------|
| `account_currency`                  | Required | Betfair account currency.                     |
| `username`                          | `None`   | Falls back to `BETFAIR_USERNAME`.             |
| `password`                          | `None`   | Falls back to `BETFAIR_PASSWORD`.             |
| `app_key`                           | `None`   | Falls back to `BETFAIR_APP_KEY`.              |
| `proxy_url`                         | `None`   | Optional proxy URL for HTTP requests.                          |
| `request_rate_per_second`           | `5`      | General HTTP rate limit.                      |
| `default_min_notional`              | `None`   | Optional minimum notional override.           |
| `event_type_ids`                    | `None`   | Optional navigation filter.                   |
| `event_type_names`                  | `None`   | Optional navigation filter.                   |
| `event_ids`                         | `None`   | Optional navigation filter.                   |
| `country_codes`                     | `None`   | Optional navigation filter.                   |
| `market_types`                      | `None`   | Optional navigation filter.                   |
| `market_ids`                        | `None`   | Optional navigation filter.                   |
| `min_market_start_time`             | `None`   | Optional navigation filter.                   |
| `max_market_start_time`             | `None`   | Optional navigation filter.                   |
| `stream_host`                       | `None`   | Optional stream host override.                |
| `stream_port`                       | `None`   | Optional stream port override.                |
| `stream_heartbeat_ms`               | `5,000`  | Required in Rust today.                       |
| `stream_idle_timeout_ms`            | `60,000` | Idle timeout before reconnect.                |
| `stream_reconnect_delay_initial_ms` | `2,000`  | Initial reconnect delay.                      |
| `stream_reconnect_delay_max_ms`     | `30,000` | Maximum reconnect delay.                      |
| `stream_use_tls`                    | `True`   | Use TLS for the stream connection.            |
| `stream_conflate_ms`                | `None`   | Explicit conflation setting.                  |
| `subscription_delay_secs`           | `3`      | Delay before the first market subscription.   |
| `subscribe_race_data`               | `False`  | Subscribe to RCM updates.                     |

Rust does not yet expose `certs_dir` or `instrument_config`. Rust also uses a fixed 36,000 second
keep-alive interval.

### Execution client configuration

| Option                              | Default       | Notes                                                  |
|-------------------------------------|---------------|--------------------------------------------------------|
| `trader_id`                         | `TRADER-001`  | Trader ID for the client core.                         |
| `account_id`                        | `BETFAIR-001` | Account ID for the client core.                        |
| `account_currency`                  | `GBP`         | Betfair account currency.                              |
| `username`                          | `None`        | Falls back to `BETFAIR_USERNAME`.                      |
| `password`                          | `None`        | Falls back to `BETFAIR_PASSWORD`.                      |
| `app_key`                           | `None`        | Falls back to `BETFAIR_APP_KEY`.                       |
| `proxy_url`                         | `None`        | Optional proxy URL for HTTP requests.                                   |
| `request_rate_per_second`           | `5`           | General HTTP rate limit.                               |
| `order_request_rate_per_second`     | `20`          | Order endpoint rate limit.                             |
| `stream_host`                       | `None`        | Optional stream host override.                         |
| `stream_port`                       | `None`        | Optional stream port override.                         |
| `stream_heartbeat_ms`               | `5,000`       | Required in Rust today.                                |
| `stream_idle_timeout_ms`            | `60,000`      | Idle timeout before reconnect.                         |
| `stream_reconnect_delay_initial_ms` | `2,000`       | Initial reconnect delay.                               |
| `stream_reconnect_delay_max_ms`     | `30,000`      | Maximum reconnect delay.                               |
| `stream_use_tls`                    | `True`        | Use TLS for the stream connection.                     |
| `stream_market_ids_filter`          | `None`        | Optional live OCM market filter.                       |
| `ignore_external_orders`            | `False`       | Only skips OCM updates with no `rfo`.                  |
| `calculate_account_state`           | `True`        | Gates periodic account state polling in Rust today.    |
| `request_account_state_secs`        | `300`         | Poll interval for account funds.                       |
| `reconcile_market_ids_only`         | `False`       | When `True`, use `reconcile_market_ids`.               |
| `reconcile_market_ids`              | `None`        | Explicit startup reconciliation market IDs.            |
| `use_market_version`                | `False`       | Attach market version to place and replace requests.   |

Rust does not yet expose `certs_dir` or `instrument_config`.

## Cutover plan

Use this page as the transition tracker until the Rust adapter becomes the primary Betfair path.

At cutover:

1. Decide whether Rust keeps its current reconciliation filter behavior or matches the Python split.
2. Decide whether Rust adds certificate configuration and other Python config fields.
3. Decide whether Rust keeps BSP-only `MARKET` orders or adds the Python aggressive-limit path.
4. Promote this file to `betfair.md`.
5. Move any remaining Python-only notes into a short legacy note or release note.
