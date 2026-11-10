# Kalshi

[Kalshi](https://kalshi.com) is an exchange for event contracts and
[perpetual futures](https://help.kalshi.com/en/articles/14679970-what-are-perpetual-futures).
The `nautilus-kalshi` adapter supports binary event contracts in Kalshi's demo and production
environments, with live and historical market data, account reports, and limit-order execution.

## Overview

The adapter is implemented in Rust and exposed to Python through PyO3. It provides:

- `KalshiHttpClient`: Typed REST discovery, public and account reports, and V2 order requests.
- `KalshiWebSocketClient`: Authenticated public and private streams, subscription recovery, and
  order book sequence validation.
- `KalshiInstrumentProvider`: Fresh market, event, and series definitions mapped to Nautilus
  instruments.
- `KalshiDataClient`: L2 market-by-price books, quotes, trades, candles, instrument status, and
  final market closes.
- `KalshiExecutionClient`: Limit-order submission, order lists, modification, cancellation,
  account state, and execution recovery.
- `KalshiDataClientFactory` and `KalshiExecutionClientFactory`: Live-node client construction.

### Low-level client contracts

- **REST pagination**: `get_all_*` methods bound pagination and expose `complete` and `next_cursor`
  when the page budget ends. A successful result with `complete=false` is partial and must not be
  treated as complete reconciliation evidence.
- **REST cancellation**: `KalshiHttpClient::cancel()` permanently cancels typed requests on the
  client and all clones. There is no reset path; create a new client for further requests.
- **Batch identities**: Create batches require unique, non-empty client order IDs. Cancel batches
  require unique venue order IDs.
- **Batch reply validation**: A mismatched result count or identity returns
  `KalshiHttpError::Response` with the raw response. The orders may have succeeded and require
  reconciliation.
- **Write outcome evidence**: `is_definitely_not_sent()` is the only proof that a write was not
  sent. Duplicate identities, malformed successes, and transport failures remain uncertain.
- **Discovery errors**: A quota-discovery failure before the first attempt returns `Discovery` and
  is proven unsent. After an earlier attempt, it returns `AfterSend` and may have been transmitted.
- **Exchange status**: `get_status()` returns the typed availability body, including maintenance
  responses with HTTP 500, 503, or 504. `get_exchange_status()` exposes the HTTP status and response
  headers.
- **WebSocket subscription identity**: Pass the original subscription arguments to `unsubscribe`
  and `update_subscription`, even after an update changes the retained market filter. The original
  arguments identify the subscription being changed or removed.
- **WebSocket references**: Duplicate subscriptions share a reference count. The final unsubscribe
  removes replay intent.
- **WebSocket clones**: Clones share lifecycle and subscription state. Resetting or disposing one
  clone affects every consumer.
- **WebSocket stop**: `stop` closes the transport and drains adapter-owned tasks while retaining
  subscription intent.
- **WebSocket reset**: `reset` stops the transport and clears subscription intent and reference
  counts.
- **WebSocket dispose**: `dispose` clears intent and reference counts and permanently rejects new
  connections and subscriptions.
- **Disconnected recovery**: Automatic reconnects are disabled. After an unexpected disconnect,
  call `stop()` and then `connect()` on the same Kalshi client. The new connection signs fresh
  headers and replays retained intent. Terminal channel errors 9, 10, and 25 require this recovery.
- **Command rejections**: Market-limit and rate-limit errors 26 and 27 emit errors and retain
  pending intent for a later connection instead of triggering that replay.

Python live nodes use the configuration and factory classes from `nautilus_trader.adapters.kalshi`.
The lower-level clients and provider are Rust components.

## Examples

The Rust and Python testers use `LiveNode` and the public factories. Both select the demo
environment. Replace the example instrument constants with open demo markets before running them.
The execution testers bypass shared pre-trade risk on a dedicated demo node; see
[Limit-order submission](#limit-order-submission) for this limitation.

The execution tester uses these settings and actions:

- **Market**: Requires a single-step price grid.
- **Shard**: The Rust tester reads the market's `exchange_index` at startup. In the Python tester,
  set `EXCHANGE_INDEX` to that value.
- **Prices**: Places BUY five ticks below the best bid and SELL five ticks above the best ask. Both
  resulting prices must remain in `(0, 1)` and on the grid; price-range clamping is disabled.
- **Orders**: Submits a post-only GTC BUY/SELL pair as an independent order list for `0.01`
  contracts.
- **Reconciliation**: Sets `reconciliation_lookback_mins` to `180`.
- **Shutdown**: Cancels its orders but does not close positions because its built-in close operation
  uses an unsupported market order.

:::warning
Both execution testers set `DRY_RUN` to false and place demo orders. Set it to true to connect
without submitting orders.
:::

Run the examples from the repository root:

```bash tab="Rust"
cargo run --locked -p nautilus-kalshi --example kalshi-data-tester
cargo run --locked -p nautilus-kalshi --example kalshi-exec-tester
```

```bash tab="Python"
uv run --project python --no-sync python examples/live/kalshi/data_tester.py
uv run --project python --no-sync python examples/live/kalshi/exec_tester.py
```

## Kalshi documentation

Use these official Kalshi resources with this guide:

- [API environments](https://docs.kalshi.com/getting_started/api_environments) and
  [API keys](https://docs.kalshi.com/getting_started/api_keys).
- [Order direction](https://docs.kalshi.com/getting_started/order_direction) and
  [fixed-point representation](https://docs.kalshi.com/getting_started/fixed_point_migration).
- [WebSocket connection](https://docs.kalshi.com/websockets/websocket-connection) and
  [order book updates](https://docs.kalshi.com/websockets/orderbook-updates).
- [Exchange sharding](https://docs.kalshi.com/getting_started/exchange_sharding).
- [Historical data](https://docs.kalshi.com/getting_started/historical_data) and
  [rate limits](https://docs.kalshi.com/getting_started/rate_limits).
- [Fee rounding](https://docs.kalshi.com/getting_started/fee_rounding) and
  [market settlement](https://docs.kalshi.com/getting_started/market_settlement).
- [V2 order creation](https://docs.kalshi.com/api-reference/orders/create-order-v2).

## Product support

Capability matrices describe adapter support. *Not supported by Kalshi* identifies a venue API
limitation. *Not currently implemented* identifies an adapter limitation.

| Product type           | Data feed | Trading | Notes                                                    |
| ---------------------- | --------- | ------- | -------------------------------------------------------- |
| Binary event contracts | ✓         | ✓       | USD 1 contracts which satisfy the admission rules below. |
| Perpetual futures      | -         | -       | *Not currently implemented*.                             |

### Accounts and shards

- **Account scope**: Each execution client uses one Direct primary account, subaccount `0`, and one
  exchange shard. Direct membership does not use a futures commission merchant (FCM).
- **Shard resources**: Each shard has its own matching engine and collateral.
- **Shard identity**: The market's `exchange_index` identifies its shard; the ticker does not. The
  execution client rejects orders for any other shard.
- **Multiple shards**: Configure separate execution clients, with a distinct `account_id` for each
  client in the same runtime.
- **Collateral movement**: The adapter does not transfer or rebalance collateral between shards.

### Market admission

The instrument provider accepts only markets with:

- `market_type=binary`.
- `settlement_bounds_type=default`.
- USD 1 contract notional.
- No multivariate collection or selected legs.
- The documented empty `collateral_return_type`, indicating no collateral-return netting.

#### Rejection rules and unsupported operations

- **Rejected economics**: The provider rejects unknown product or collateral behavior, `MECNET`,
  `DIRECNET`, scalar payouts, and nonzero settlement floors.
- **Netting test**: `mutually_exclusive=false` does not prove that an event has no netting.
- **Settlement floors**: A missing, null, or zero `settlement_floor_dollars` does not exclude an
  ordinary contract.
- **Payouts**: Admission does not guarantee a USD 0 or USD 1 payout. Contract-specific rules can
  produce a fractional settlement value. See [Settlement and restart](#settlement-and-restart).
- **Other operations**: The adapter does not support FCM accounts, sub-user accounts, numbered
  subaccounts, transfers, automatic rebalancing, RFQ order entry, multivariate market creation, or
  native order groups.

## Symbology

Each admitted market maps to one YES-priced `BinaryOption` with netting OMS:

- The instrument ID is `<ticker>.KALSHI`, and the raw symbol is the market ticker.
- A positive account net represents YES exposure; a negative account net represents NO exposure.
- BUY increases the signed net, and SELL decreases it.
- The account uses USD and `AccountType::Cash`, with borrowing and local account-state calculation disabled.

This mapping preserves Kalshi's single signed position and the YES price scale used by V2 orders,
fills, and public trades. Separate YES and NO instruments are not used.

### Instrument fields

| Instrument fields                                   | Source and mapping                                                               |
| --------------------------------------------------- | -------------------------------------------------------------------------------- |
| `id`, `raw_symbol`, `event_id`                      | Market ticker, the same raw ticker, and `event_ticker`.                          |
| `asset_class`, `outcome`                            | `Alternative` and `YES`.                                                         |
| `currency`                                          | USD; exact amount constructors preserve fractional USD balances and fees.        |
| `activation_ns`, `expiration_ns`                    | `open_time` and required `latest_expiration_time`, converted to UTC nanoseconds. |
| `description`, `info`                               | Market and event descriptions, rules, shard, grid, and series metadata.          |
| `price_precision`, `price_increment`, `tick_scheme` | Six decimals and a scheme derived from the market's complete `price_ranges`.     |
| `size_precision`, `size_increment`, `min_quantity`  | Two decimals, `0.01`, and `0.01` contracts.                                      |
| `margin_init`, `margin_maint`                       | Zero for the fully paid cash model.                                              |
| `ts_event`, `ts_init`                               | Metadata update time when available, otherwise receipt time; local receipt time. |

`BinaryOption` has multiplier one, no base currency, and USD as its quote and settlement currency.
`close_time` controls order entry but does not replace `latest_expiration_time` or supply a
settlement timestamp.

### Prices, quantities, and direction

- **Response precision**: The adapter stores instrument prices at six decimal places and preserves
  received precision. Kalshi price-grid values use up to four decimals; monetary and fee values can
  require six.
- **Contract counts**: Counts use up to two decimal places.
- **V2 requests**: YES prices accept up to four decimal places after normalization and serialize
  with exactly four. Counts accept at most two. The adapter rejects values which require rounding.
- **Fractional trading**: Kalshi supports fractional orders and fills on active Predictions markets,
  including fills from whole-contract orders.

| Surface                                        | Mapping                                                                      |
| ---------------------------------------------- | ---------------------------------------------------------------------------- |
| V2 create `side`                               | BUY -> `bid`; SELL -> `ask`.                                                 |
| V2 create `price`, `count`                     | Unchanged YES price and absolute contract count.                             |
| REST `orderbook_fp.yes_dollars` level `(p, h)` | YES bid `(p, h)`.                                                            |
| REST `orderbook_fp.no_dollars` level `(n, h)`  | YES ask `(1 - n, h)`.                                                        |
| WebSocket NO level with `use_yes_price=true`   | YES ask with unchanged price and size.                                       |
| WebSocket NO level with `use_yes_price=false`  | YES ask at `1 - price`, with unchanged size.                                 |
| Public trade                                   | YES execution price, fractional count, native trade ID, and taker direction. |
| Private order or fill                          | `book_side=bid` -> BUY; `book_side=ask` -> SELL; unchanged YES price.        |

- **Direction validation**: Canonical direction fields must agree when more than one is present.
  The adapter rejects inconsistent direction or complementary price fields. Legacy `action`, `side`,
  and `taker_side` do not replace canonical V2 fields.
- **Exposure context**: Direction alone does not show whether a fill opens or reduces exposure. The
  preceding signed position supplies that context.
- **NO cash flows**: Opening NO exposure at YES price `p` costs `1 - p` per contract. Closing that
  exposure at YES price `p` also returns `1 - p` before fees. For example, opening at `0.60` costs
  `0.40`, and closing at `0.70` returns `0.30`.

### Dynamic price grids

- **Grid source**: Use each market's `price_ranges` bands for price validation. The
  `price_level_structure` label and base `price_increment` do not describe every valid price.
- **Base increment**: Calculate the greatest common divisor of the exact band starts and steps. The
  complete grid still rejects off-grid prices.
- **Registration**: Fresh definition parsing registers and verifies a tick scheme. A grid change
  publishes an updated instrument.
- **Restart**: Restore the retained grid and scheme after deserializing an instrument for correct
  price navigation. Without the scheme, navigation falls back to the base increment.
- **Live orders**: Submission independently fetches fresh metadata and validates the complete grid,
  rejecting an off-grid price rather than rounding it.

## Environments and authentication

### API hosts

The adapter defaults to Kalshi's recommended external API hosts.

| Environment | REST base URL                                      | WebSocket URL                                          |
| ----------- | -------------------------------------------------- | ------------------------------------------------------ |
| Production  | `https://external-api.kalshi.com/trade-api/v2`     | `wss://external-api-ws.kalshi.com/trade-api/ws/v2`     |
| Demo        | `https://external-api.demo.kalshi.co/trade-api/v2` | `wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2` |

Explicit overrides also support `api.elections.kalshi.com` for production and `demo-api.kalshi.co`
for demo. Known hosts must match the selected environment.

### Credentials

Production and demo use separate credentials:

| Environment | API key ID               | Private key               |
| ----------- | ------------------------ | ------------------------- |
| Production  | `KALSHI_API_KEY_ID`      | `KALSHI_PRIVATE_KEY`      |
| Demo        | `KALSHI_DEMO_API_KEY_ID` | `KALSHI_DEMO_PRIVATE_KEY` |

- **Pairing**: Supply `api_key_id` and `private_key` together, or omit both to use the selected
  environment's pair. The adapter does not combine explicit and environment credentials or
  credentials from different environments.
- **Validation**: Missing, empty, incomplete, malformed, or unsupported credentials fail before
  authenticated traffic.
- **Key formats**: Private keys are PEM-encoded text, not file paths. The adapter accepts RSA
  PKCS#1, RSA PKCS#8, and Ed25519 PKCS#8 keys. Encrypted keys are unsupported.
- **Signing**: The decoded key type selects RSA-PSS with SHA-256 or Ed25519. RSA uses MGF1-SHA256
  and a salt length equal to the digest length. Each adapter-owned REST retry and each new WebSocket
  client connection signs afresh. Lower-level HTTP retries can reuse the original signed headers.
- **Signature input**: Concatenate the timestamp in milliseconds, uppercase HTTP method, and full
  request path without separators. Exclude the hostname and query string.
- **Headers**: Send `KALSHI-ACCESS-KEY` for the API key ID, `KALSHI-ACCESS-TIMESTAMP` for the
  millisecond timestamp, and `KALSHI-ACCESS-SIGNATURE` for the Base64 signature.
- **Paths**: The default WebSocket handshake signs `GET` and `/trade-api/ws/v2`. Endpoint overrides
  use the canonical path of the transmitted URL.

### Access

- **Public REST**: Market, event, series, book, trade, and candle reads do not require
  authentication.
- **Authenticated REST**: Portfolio reads, account-limit reads, and order writes require
  authentication.
- **WebSocket**: Every connection requires authentication, including public ticker, trade, and
  lifecycle channels.

:::warning
Rust `Debug` and Python representations redact credentials, endpoint overrides, and proxy values.
Serialization emits plaintext values. Do not log serialized configurations.
:::

## Instrument loading

- **Factory selection**: The data factory requires a nonempty `instrument_ids` list containing
  distinct Kalshi binary-option IDs. These markets bound bootstrap and streaming. Explicit
  instrument, book, and history requests can query other admitted markets.
- **Standalone selection**: `KalshiDataClient` takes selected markets as a constructor argument.
  Its `config.instrument_ids` still requires validation but does not select those markets.
- **Fresh definitions**: `KalshiInstrumentProvider` fetches fresh market, event, and series
  metadata for every request. It does not serve definitions from a private cache.

### Request filters

- **Selectors**: Request parameters accept `tickers` (comma-separated), `event_ticker`,
  `series_ticker`, `status`, and `exchange_index`.
- **Time filters**: Requests also accept `min_created_ts`, `max_created_ts`, `min_updated_ts`,
  `max_updated_ts`, `min_close_ts`, `max_close_ts`, `min_settled_ts`, and `max_settled_ts`.
- **Selected-market fallback**: The data client adds selected tickers only when none of `tickers`,
  `event_ticker`, `series_ticker`, `min_updated_ts`, or `max_updated_ts` is present. `status` or
  `exchange_index` alone does not override that fallback.
- **Live queries**: The provider sets `mve_filter=exclude` on every live bulk query and sends the
  remaining selectors unchanged. Kalshi does not permit `tickers` and update-time filters in one
  query; local filtering applies only to records Kalshi returns. This filter also excludes on-demand
  `KXRFQ*` markets from bulk discovery, while direct ticker requests can still load them.
- **Unsupported markets**: Bulk results omit unsupported economics; a direct request rejects them.
  Multivariate-only requests are rejected.

### Pagination and archived definitions

`max_pages` defaults to `100`. Exhausting the budget returns an error without a partial response.
A nonempty starting `cursor` is rejected.

- **Archive selectors**: The archive accepts one selector per request, chosen in this order:
  `tickers`, `event_ticker`, then `series_ticker`. With no selector, the provider sends
  `mve_filter=exclude`.
- **Direct live requests**: Try the archive after HTTP 404. If both sources return 404, retry at
  most three times, with delays of 100, 200, and 400 milliseconds. Other live-market errors do not
  trigger an archive lookup.
- **Explicit historical requests**: Set `historical=true` to select the archive directly. These
  requests do not retry 404.
- **Bulk requests**: Query live and archived sources separately. The provider does not merge across
  the historical cutoff.

### Time bounds

- **Bounds**: `start` and `end` filter `ts_event` inclusively, using `updated_time` when available
  and local receipt time otherwise. Inverted bounds fail before transport. They do not reconstruct
  earlier instrument definitions.
- **REST filters**: Timestamp filters compare full metadata timestamps with whole-second bounds.
  Minimum bounds are strict. Maximum bounds are strict except `max_updated_ts`, which includes its
  bound.
- **Mismatches**: A direct request rejects filter or time-bound mismatches without a response; bulk
  results omit those records.
- **Response identity**: Response parameters, correlation IDs, and time bounds retain the request
  values.

### Metadata refreshes

The data client refreshes metadata after each configured interval elapses from the preceding
refresh's completion. It also refreshes before processing events from a new WebSocket generation
and after lifecycle subscription acknowledgment. A failed refresh stops the stream.

## Market data

| Data type            | Live | Snapshot | Historical | Nautilus type      | Notes                                                        |
| -------------------- | ---- | -------- | ---------- | ------------------ | ------------------------------------------------------------ |
| Instrument metadata  | REST | ✓        | ✓          | `BinaryOption`     | Fresh live and archived definitions.                         |
| Order book deltas    | ✓    | ✓        | -          | `OrderBookDeltas`  | Full `L2_MBP`; WebSocket snapshots and deltas.               |
| Order book snapshots | -    | ✓        | -          | `OrderBook`        | Correlated REST response; not inserted into the live stream. |
| Quote ticks          | ✓    | -        | -          | `QuoteTick`        | WebSocket ticker; managed-book quotes require configuration. |
| Trade ticks          | ✓    | -        | ✓          | `TradeTick`        | WebSocket trades and bounded REST history.                   |
| Bars                 | -    | -        | ✓          | `Bar`              | Native REST candles; live bars use engine aggregation.       |
| Instrument status    | ✓    | ✓        | -          | `InstrumentStatus` | Lifecycle channel and REST metadata.                         |
| Instrument close     | ✓    | ✓        | -          | `InstrumentClose`  | Published only after authoritative final settlement.         |

The adapter does not provide historical quotes or books, native streaming candles, L3
market-by-order books, Greeks, funding rates, or unrelated index feeds. `request_quotes` and
unsupported funding, book-depth, book-delta, and custom-data requests return explicit errors.

### Live subscriptions

- **Connection**: `connect()` fetches and publishes fresh definitions before streaming. It
  authenticates the WebSocket and waits for subscription confirmation and lifecycle metadata
  recovery. Lifecycle recovery must finish within `ws_timeout_secs` after confirmation.
- **Demand**: Subscriptions can be recorded before connection and are reference-counted per
  instrument and data type.
- **Venue subscriptions**: Each quote, trade, and book channel shares one subscription covering the
  selected markets. It is removed when the final demand for that channel retires.
- **Lifecycle channel**: The client retains one global `market_lifecycle_v2` subscription for
  selected-market metadata.
- **Independent requests**: Instrument, book, and history requests run independently of WebSocket
  recovery.
- **Disconnected recovery**: Automatic reconnects are disabled. Call `stop()` and then `connect()`
  after an unexpected disconnect. The explicit connection replays retained subscriptions.
- **Shutdown**: `stop()` and `disconnect()` cancel pending requests and reject new requests until
  the next `connect()`. `stop()` preserves demand, reset clears it, and disposal permanently rejects
  new work.

### Quotes

Direct quote subscriptions use the WebSocket ticker channel. The data engine can also derive quotes
from a managed book when `LiveDataEngineConfig.emit_quotes_from_book=true`; this option defaults to
false.

- **Required fields**: Each ticker message must contain YES bid and ask prices and positive sizes.
  A missing, null, or empty side produces no quote. Earlier values are not carried forward.
- **Validation**: Crossed quotes, out-of-range prices, and excess precision are logged and dropped.

Every valid complete ticker message emits a quote, including unchanged values.

### Trades

Trades preserve the native trade ID, fractional count, YES execution price, and venue timestamp.

- **Direction**: Canonical `bid` or `yes` direction maps to buyer aggression; `ask` or `no` maps to
  seller aggression. The legacy `taker_side` is not a fallback, and book-leg complements do not
  apply.
- **Validation**: Missing or conflicting direction, inconsistent YES/NO prices, invalid values, and
  precision loss are logged and dropped while the stream continues.
- **Exclusions**: The adapter drops flagged block trades. Ticker last-price changes do not emit
  trades.

Trade timestamps use `ts_ms`, falling back to `ts` seconds when absent. `ts_init` records local
conversion time.

### Order books

`subscribe_book_deltas` accepts full `BookType::L2_MBP` books with no depth limit or extra
parameters. Ticker quotes remain independent of book subscriptions.

- **Snapshots**: WebSocket snapshots replace every level, including empty and one-sided books. Each
  replacement starts with `Clear`, sets `F_SNAPSHOT`, and marks its final delta with `F_LAST`.
- **Deltas**: Signed venue size changes become absolute level sizes with `Add`, `Update`, or
  `Delete` actions. Malformed payloads and invalid levels or timestamps suppress the affected book
  until recovery.
- **SID-wide failures**: A missing market identity invalidates every book on the SID. Protocol or
  command errors on any channel also pause every demanded book.
- **Managed rejection exception**: A correlated nonterminal managed `get_snapshot` rejection reaches
  only that book's recovery and leaves the confirmed subscription active.

#### Recovery

- **Sequence scope**: Track one counter per server subscription ID (`sid`) across its markets and
  sequenced control replies. Duplicate and older frames produce no updates.
- **Gap handling**: A gap suppresses every demanded book on that SID until a matching snapshot
  arrives.
- **Retry policy**: Recovery retains one owner and retry budget per book across explicit reconnects.
  It makes up to eight attempts within 180 seconds, then retries at jittered intervals growing from 1
  to 15 minutes until a snapshot is accepted or recovery is canceled.
- **Snapshot deadlines**: `book_snapshot_timeout_secs` controls initial and recovery deadlines.
  Set `0` to disable these deadlines while retaining retry limits. Managed snapshot requests use
  this deadline separately from the WebSocket command-acknowledgment timeout.

Quiet markets do not trigger stale-feed recovery.

#### REST book snapshots

- **Request**: `request_book_snapshot` fetches a fresh instrument definition and returns a
  correlated `OrderBook`. Omit `depth` for the full book or select `[1, 100]`. Extra parameters are
  rejected. Markets which fail provider admission produce a warning without a response.
- **Price scale**: REST books contain YES bids and NO bids; NO bids are complemented once to obtain
  YES asks. WebSocket subscriptions set `use_yes_price=true`, so NO levels already represent YES
  asks.
- **Watermark**: REST responses use sequence `0` and local conversion timestamps because the
  endpoint supplies no sequence watermark or time bounds.
- **Stream separation**: REST responses are never inserted into the live book stream.

### Instrument status and final closes

#### Lifecycle metadata

- **Coverage**: The global lifecycle feed is filtered to selected markets. Kalshi omits on-demand
  `KXRFQ*` markets from this unscoped feed, so lifecycle-driven status and close updates do not
  cover those instruments. Explicit ticker and trade subscriptions still receive them.
- **Precedence**: Lifecycle overlays take precedence over older REST metadata; newer versioned REST
  definitions supersede them. Duplicate or older notifications are ignored.
- **Stream failures**: Unknown states, contradictory metadata, and malformed selected-market
  notifications stop the stream. An unavailable definition never implies closure.
- **Response guard**: A correlated instrument response which contradicts newer lifecycle metadata
  fails before it can replace the data engine's instrument cache. A selected-market request which
  overlaps recovery can fail without a correlated response. Retry it after recovery completes.
- **Refresh triggers**: The client refreshes REST definitions after a selected notification and
  after a lifecycle sequence gap.
- **Missing timestamps**: A notification without `sending_ts_ms` triggers fresh REST verification
  without applying an unordered overlay.
- **Unchanged data**: Unchanged definitions and statuses produce no additional events. Snapshot
  prices and volumes alone do not count as definition changes.

#### Status mapping

| Market state                        | Action                   | `is_trading` |
| ----------------------------------- | ------------------------ | ------------ |
| `initialized`, before `open_time`   | `PreOpen`                | `false`      |
| `active`, before `close_time`       | `Trading`                | `true`       |
| `inactive`, before `close_time`     | `Pause`                  | `false`      |
| `closed`                            | `Close`                  | `false`      |
| `determined`, `disputed`, `amended` | `PostClose`              | `false`      |
| `finalized`                         | `NotAvailableForTrading` | `false`      |

- **Open and close**: Initialized markets open at `open_time`. Initialized, active, and inactive
  markets close at `close_time`.
- **Quoting**: `is_quoting` matches `is_trading`.
- **Finality**: Elapsed expected expiry does not establish a final payout.
- **Reason**: `InstrumentStatus.reason` carries the effective lifecycle state.
- **Event timestamps**: Implicit open and close events use `open_time` and `close_time`. Other
  updates use `sending_ts_ms` or REST `updated_time`, with local initialization time when the REST
  timestamp is absent.

#### Final closes

- **Evidence**: A final `InstrumentClose` requires `status=finalized`, `settlement_value_dollars`,
  and `settlement_ts`.
- **Verification**: A `settled` notification triggers REST verification without replacing a prior
  `PostClose` status.
- **Rechecks**: If final metadata is not ready, the client rechecks after 250 milliseconds, 500
  milliseconds, 1 second, and 2 seconds. Scheduled metadata refreshes provide recovery if these
  checks fail.
- **Deduplication**: Repeated final evidence is deduplicated across explicit reconnects; conflicting evidence
  fails explicitly.
- **Execution barrier**: Execution-enabled runtimes add the reconciliation barrier described in
  [Settlement and restart](#settlement-and-restart).

### Public block trades

- **Adapter output**: Live `TradeTick` output and ordinary trade history exclude flagged block
  trades.
- **Venue default**: Kalshi includes block trades in live and historical REST responses and flags
  them on the WebSocket `trade` channel.
- **Low-level filtering**: REST endpoints expose `is_block_trade=true|false` for selecting block or
  non-block records.
- **Candle policy**: Kalshi does not document whether block trades contribute to candle prices or
  volume.
- **RFQ**: RFQ order entry is not supported.

## Historical data

Historical trade and bar requests fetch fresh instrument, event, and series context, including
archive fallback after a live-definition 404.

- **Bounds**: `start` is required. An omitted `end` uses the client clock when the request is
  accepted. Both bounds include their timestamps.
- **Ordering**: Results sort chronologically. `limit` selects the earliest matching records after
  complete collection, and equal-time trades sort by trade ID.
- **Parameters**: `max_pages` is the only adapter-specific parameter. It defaults to `100` and caps
  each live or archived trade tier. Other adapter-specific keys return an error.
- **Failures**: Synchronous validation failures return errors. After asynchronous collection starts,
  exhausted page budgets, repeated cursors, conflicting duplicate IDs, and source failures are
  logged and produce no partial or correlated response.
- **Timestamps**: Historical trades and bars use event time for `ts_event` and `ts_init`. The
  response envelope uses request-acceptance time.

### Live and archived tiers

- **Cutoff fields**: `market_settled_ts` partitions markets and candles, `trades_created_ts`
  partitions trades and fills, `orders_updated_ts` partitions orders, and
  `market_positions_last_updated_ts` is the position backfill horizon. Active orders remain on the
  live endpoint.
- **Trade routing**: Read the cutoff once. If the window reaches it, collect live pages before a
  full-window archive probe to cover records moving between reads.
- **Candle routing**: Compare the market's settlement timestamp with `market_settled_ts`,
  independently of the requested candle window. A settled market still in the live tier also
  receives an archive probe.
- **Candle fallback**: An archive 404 preserves a successful live result, including an empty window.
  Two missing sources fail. Native single-market candle endpoints are unpaginated; `max_pages`
  applies only to trades.
- **Shard filtering**: Historical orders, fills, positions, and live settlements have no shard-query
  filter. Low-level callers must filter returned records by `exchange_index` before reconciliation.
  A single-order response must also match the client's account and shard.
- **Atomicity**: Cursor pages and separate source reads do not form an atomic snapshot. Repeat a
  request at the handoff when point-in-time completeness is required.
- **Position handoff**: For complete position history, page through live positions with
  `settlement_status=all` before archived positions, then deduplicate by account scope, subaccount,
  and ticker. A settled position can remain live after its last update predates the cutoff.
- **Settlement records**: `/portfolio/settlements` contains only settled positions still in the live
  data set. Archived positions do not expose the same settlement-record fields.

### Bars

| Bar specification | `period_interval` in minutes |
| ----------------- | ---------------------------- |
| `1-MINUTE`        | `1`                          |
| `1-HOUR`          | `60`                         |
| `1-DAY`           | `1440`                       |

- **Price selection**: Historical bars require a standard `EXTERNAL` bar type with `LAST`, `BID`,
  or `ASK` prices. These select trade-price, YES-bid, or YES-ask candles.
- **Interval timestamps**: `ts_event` is the venue interval end, including daily ends which differ
  from UTC midnight. Partial intervals ending after the request-acceptance clock are excluded.
- **Volume and prices**: Volume preserves the venue contract count, including zero. BID and ASK
  candles preserve boundary prices `0` and `1`; they do not reconstruct historical quotes or sizes.
- **Missing data**: An all-null trade-price candle with zero volume produces no bar. Other missing
  selected OHLC values fail the request.
- **Live bars**: Historical quotes and external live-bar subscriptions are unsupported. Aggregate
  subscribed trades or quotes with the data engine instead.

## Orders capability

### Order types

| Order type             | Binary options | Notes                        |
| ---------------------- | -------------- | ---------------------------- |
| `LIMIT`                | ✓              | Native V2 limit orders.      |
| `MARKET`               | -              | *Not currently implemented*. |
| `STOP_MARKET`          | -              | *Not currently implemented*. |
| `STOP_LIMIT`           | -              | *Not currently implemented*. |
| `MARKET_IF_TOUCHED`    | -              | *Not currently implemented*. |
| `LIMIT_IF_TOUCHED`     | -              | *Not currently implemented*. |
| `TRAILING_STOP_MARKET` | -              | *Not currently implemented*. |
| `TRAILING_STOP_LIMIT`  | -              | *Not currently implemented*. |

### Time in force

| Time in force  | Binary options | Notes                                                       |
| -------------- | -------------- | ----------------------------------------------------------- |
| `GTC`          | ✓              | Rests until cancellation or venue termination.              |
| `GTD`          | ✓              | Requires a future expiry in whole Unix seconds.             |
| `IOC`          | ✓              | Fills available size immediately and cancels the remainder. |
| `FOK`          | ✓              | Fills completely or ends without a fill.                    |
| `DAY`          | -              | *Not currently implemented*.                                |
| `AT_THE_OPEN`  | -              | *Not currently implemented*.                                |
| `AT_THE_CLOSE` | -              | *Not currently implemented*.                                |

- **Expiry validation**: Only `GTD` accepts an expiration. The adapter denies subsecond expiry
  rather than rounding it and rechecks expiry after asynchronous preparation.
- **Venue management**: Kalshi enforces GTD expiry, so leave `StrategyConfig.manage_gtd_expiry=false`.
- **Recovery mapping**: Private recovery reports venue expiry as `OrderCanceled`; it does not infer
  `OrderExpired` from the local clock.

### Execution instructions and params

| Option                       | Binary options | Notes                                                              |
| ---------------------------- | -------------- | ------------------------------------------------------------------ |
| `post_only`                  | ✓              | Available with `GTC` and `GTD`; denied with `IOC` and `FOK`.       |
| `reduce_only`                | ✓              | Available with `IOC` under the reduction rules below.              |
| `quote_quantity`             | -              | *Not currently implemented*; quantities use contracts.             |
| `display_qty`                | -              | *Not currently implemented*.                                       |
| `self_trade_prevention_type` | ✓              | `taker_at_cross` by default, or `maker`; set through order params. |
| `cancel_order_on_pause`      | ✓              | Boolean order param; defaults to `false`.                          |

- **Self-trade outcomes**: `taker_at_cross` stops matching, cancels the taker, and retains completed
  fills. `maker` cancels the resting maker, then matching continues.
- **Unknown params**: Unknown `SubmitOrder.params` keys are denied.
- **Other features**: Other order types and time-in-force values, contingencies, emulation, and
  execution algorithms are unsupported.
- **Pause behavior**: `cancel_order_on_pause` controls cancellation when trading pauses.
- **Reactivation**: Kalshi cancels all resting orders when an inactive market returns to active,
  independently of the pause setting.

### Limit-order submission

Prerequisites:

- A cached binary instrument and a cached order routed to the execution client.
- A reported USD cash account with borrowing and local account calculation disabled.
- Netting OMS and completed private recovery for the account session.
- Direct client calls, or a dedicated Kalshi-only node with `LiveRiskEngineConfig.bypass=true`.
  The ordinary risk-enabled `LiveNode` pipeline does not support submission, order-list submission,
  or amendment with Kalshi's signed-binary cash model.

:::warning
Risk bypass disables every shared pre-trade risk check, including configured price, quantity,
notional, trading-state, and rate checks. The adapter retains its own validation and cash
reservations. Do not enable bypass on a mixed-adapter node.
:::

Before dispatch, the adapter checks:

- **Quantity and price**: Positive quantity with at most two decimals, and YES price in `(0, 1)`
  with at most four decimals after normalization and on the market grid. Instrument quantity limits
  also apply.
- **Trading status**: Fresh market and exchange status which permits trading on the configured
  shard.
- **Identity**: Matching command, cached order, instrument, primary subaccount `0`, and
  execution-client identity.
- **Position binding**: Command, order, and cached position binding equal to
  `<instrument_id>-<strategy_id>` when a position ID is present.
- **Fee metadata**: `quadratic` or `quadratic_with_maker_fees` with a nonnegative multiplier. Event
  overrides must supply both the fee type and multiplier.
- **Cash**: Sufficient free cash, including principal and the conservative fee allowance.

#### Admission and readiness

- **Admission failures**: Invalid orders are denied before `OrderSubmitted`.
- **Readiness**: Private order, fill, and market-position notices and uncertain submission outcomes
  temporarily remove readiness.
  Orders submitted during that window are denied; retry with a new client order ID after recovery
  publishes a fresh account balance and the engine consumes it.
- **Reservations**: Each admission reserves principal and fees before dispatch. Concurrent
  submissions cannot reuse the same cash or reducible contracts, and stale balance responses cannot
  erase newer holds.

#### Reduce-only orders

- **Required reduction**: A reduce-only IOC must reduce both the account/subaccount/shard net and
  the submitting strategy's open canonical netting position.
- **Account capacity**: Quantity must fit the unreserved account net after subtracting same-side
  remaining quantities from external and tracked pending, ambiguous, and open orders.
- **Strategy capacity**: Quantity must separately fit the strategy's canonical position; those
  quantities are not subtracted from the canonical position again. Another strategy's position and
  unclaimed external inventory do not qualify.
- **Exclusive flow**: The strategy and instrument must have no unresolved order, and external orders
  on the reducing side must resolve first. Until the reduction resolves, the client denies other
  orders for that strategy and instrument.
- **Reservations**: Reductions retain their full opening principal and fee reservation. Expected
  proceeds cannot fund another order.

:::warning
Reduce-only admission requires this runtime to be the sole venue writer for the instrument until
terminal recovery completes. Stop independent submissions and amendments and resolve writes already
in flight first. Keep external-order claims unchanged throughout this interval.

The operator must maintain these conditions; the adapter does not enforce them. Kalshi constrains
the account net but cannot reserve a strategy position against an external fill.
:::

### Order operations

| Operation           | Binary options | Notes                                                               |
| ------------------- | -------------- | ------------------------------------------------------------------- |
| Submit order        | ✓              | One adapter-attempt V2 limit request.                               |
| Submit order list   | ✓              | Independent lists; linked and bracket lists are denied.             |
| Modify order        | ✓              | Change price or total quantity; trigger and expiry are unsupported. |
| Batch modify orders | -              | *Not currently implemented*.                                        |
| Cancel order        | ✓              | Cancel one known venue order on the configured shard.               |
| Batch cancel orders | ✓              | Cancel selected venue order IDs in quota-sized chunks.              |
| Cancel all orders   | ✓              | Cancel a scoped snapshot of owned active orders.                    |
| Query order         | ✓              | Requires cached identity and private recovery.                      |
| Query account       | ✓              | Requests recovery and emits `AccountState`.                         |
| Mass status         | ✓              | Scoped order, fill, and position reports.                           |

- **Params**: Order and account queries require `params=None`; an explicitly empty params object is
  also rejected.
- **Reports**: Use report-generation methods for returned reports; queries emit recovered execution
  and account events.
- **Readiness**: When no native change is pending, `query_order` also restores cached context and
  clears readiness until recovery observes the new account session.

#### Order lists and batch cancellation

- **Pre-dispatch validation**: Every list child must pass validation, preparation, and cash
  admission before the first write. A validation or preparation failure denies the whole list
  without a venue write.
- **Admission failure**: If admission fails after earlier children reach `OrderSubmitted`, those
  children receive `OrderRejected` with `Order was not sent`, and the remaining children receive
  `OrderDenied`. No venue write is sent.
- **List params**: Params apply to every child. A reduce-only order must be the only list child for
  its instrument and satisfy the single-order reduction rules.
- **Chunking**: The client splits lists and selected-ID cancellations into chunks which fit the
  account's write capacity and endpoint cost.
- **Per-child outcomes**: Responses correlate by child identity even when reordered. Missing,
  duplicate, conflicting, or uncertain results retain those children for recovery.
- **Later chunks**: If a chunk has an unresolved outcome or is not sent, later chunks are not sent.
  Each chunk has at most one adapter-owned attempt.
- **Unsent later chunks**: Known unsent submissions receive `OrderRejected`; cancellations receive
  `OrderCancelDenied`. A whole-request error without per-child results does not prove that every
  child was rejected.
- **Cancel-all scope**: `cancel_all_orders` snapshots owned active orders matching trader, strategy,
  instrument, and optional side. It includes submitted orders awaiting venue IDs, never uses
  Kalshi's account-wide deletion endpoint, and does not add later orders.
- **Cancellation-claim lifetime**: A captured submission without a venue ID retains its claim only
  until it resolves, the client stops, or the client explicitly reconnects. Stop and explicit
  reconnect fence queued work from the old transport generation; do not assume that cancellation
  survives reconnection.
- **Command params**: Batch cancel and cancel-all reject nonempty command parameters.

#### Modification and cancellation

`modify_order` changes price or total quantity on an owned or recovered limit order:

- **Quantity reduction**: Same-price reductions use `decrease`. Reducing a total of `2.00` to
  `0.50` sends `reduce_by=1.50`. If `0.25` already filled, `0.25` remains unless more fills race
  the change.
- **Zero leaves**: To remove all remaining quantity, set the requested total equal to the filled
  quantity. Authoritative recovery can then cancel the order. A requested total of zero is invalid
  when fills exist.
- **Price changes and quantity increases**: Use `amend` with the requested total, including fills.
  Expiry and client identity are preserved.
- **Unsupported changes**: Trigger-price and expiration-only changes are unsupported. Modification
  and cancellation reject nonempty command parameters.
- **Market close**: After `close_time`, Kalshi rejects all order operations, including cancellation,
  with `MARKET_INACTIVE` and cancels resting orders shortly after close.
- **Admission window**: A single change requires an active connection and current private recovery.
  It has 15 seconds from capture to complete recovery and preparation. Batch cancellation waits for
  recovery and order identities until it completes, the client stops, or the client reconnects.
- **Retry margin**: The default node inflight retry budget leaves at least 10 seconds for dispatch
  and recovery; custom settings must preserve that margin.
- **Concurrent changes**: A second distinct change is denied while the first is in flight.
- **Order drift**: A modification is denied if the private order's price, total quantity, or filled
  quantity changes while it waits.
- **Reservation update**: An amendment reserves the greater of its requested hold and the existing
  hold plus the cost of any quantity increase at the new price. This covers fills at the old terms
  racing the amendment. Authoritative recovery replaces the holds with venue-reported terms.
- **Post-dispatch**: Wait for fresh recovery and account observation before another submission. Use
  order lists for submission bursts and batch cancellation for cancellation bursts.

:::warning
The shared inflight watchdog can emit `OrderCanceled` after status-query retries even when the
venue order still rests. This event does not prove venue cancellation. Reconcile venue state before
managing an uncertain modification or cancellation again.
:::

#### Write outcomes

- **Acknowledgments**: Order writes use one adapter-owned attempt. Lower-level transport retries
  can replay the signed request. Acknowledgments establish acceptance, not individual fills or final
  order state. Private streams and REST reports supply authoritative updates.
- **Identity defect**: Kalshi's [API changelog](https://docs.kalshi.com/changelog.md) documents that
  V2 single-cancel and amend responses can contain another order's `order_id`, `client_order_id`,
  and quantities even when the intended mutation executed correctly. Validate returned identities
  and obtain order and fill state from private streams and REST reports, not response quantities.
- **Definitive outcomes**: A proven local failure before transmission or definitive venue rejection
  emits the matching rejection or denial and releases its hold unless private evidence already
  establishes a venue order. Only explicitly classified venue errors count as definitive; other
  errors remain ambiguous. A post-only crossing rejection sets `due_post_only=true`.
- **Local unsent changes**: Proven unsent modifications and cancellations emit `OrderModifyDenied`
  or `OrderCancelDenied` with reason `VALIDATION_FAILED: <detail>` and the command ID in
  `causation_id`.
- **Ownership failures**: Commands whose ownership cannot be established fail without an order
  event.
- **Shutdown**: Shutdown denies orders still in admission.
- **Session changes**: A session change before transmission stops the queued write. Submissions
  receive `OrderRejected`, and modifications or cancellations receive matching denials. Retry
  proven-unsent commands after readiness returns.
- **Uncertain outcomes**: Timeouts, cancellation after transmission, malformed success responses,
  and other uncertain outcomes retain order identity, command context, and funds for recovery. They
  do not create a rejection. Reconcile venue evidence before retrying.
- **Unresolved commands**: An unresolved outcome has no automatic expiry or operator override.
  Expiry alone never releases an ambiguous hold. The outcome can block writes after an explicit
  reconnect and keep `is_connected()` false, preventing a `LiveNode` restart.
- **Unacknowledged changes**: An open report matching requested modification terms does not prove
  completion. Without a valid acknowledgment, the command remains unresolved until terminal order
  evidence arrives.
- **Batch responses**: Check every child's identity and result. HTTP success alone does not prove
  that every child succeeded.

## Reconciliation and recovery

### Cash and reservations

#### Balance model

- **Scope**: Cash and reservations belong to one Direct primary account, subaccount `0`, and shard.
- **Account net**: Positive net represents YES exposure; negative net represents NO exposure.
  Strategy positions remain scoped by instrument and strategy, while cash and reservations use the
  account-level net.
- **Balance mapping**: The adapter maps the shard's `balance_dollars` available balance to Nautilus
  `total` and sets `free = total - locked`.
- **Locks and cash updates**: Conservative locks cover open orders and unresolved commands. Fill and
  settlement cash changes apply to `total`; fills not confirmed by a balance snapshot remain
  tracked.
- **Cash mismatches**: A non-fill cash change concurrent with unconfirmed fills can keep recovery
  blocked. Verify the cash change and fill fees before recreating the client with complete cached
  orders and positions. Reconnecting alone preserves the retained cash anchor.
- **Funding boundaries**: The adapter never uses aggregate account value or another shard's balance
  to fund an order.
- **Opening reservations**: BUY reserves `quantity * price`; SELL reserves
  `quantity * (1 - price)`. Both include a conservative fee allowance. Orders cannot be funded from
  an expected reduction, opposite order, or future settlement.
- **Cancellation release**: A reservation is released only after a confirmed terminal outcome with
  concurrent fills accounted for.

**Worked reservation example**:

Starting from flat cash of USD `100.00` with no fees, a BUY for `2.00` at `0.60` and a SELL for
`3.00` at `0.70` reserve `1.20 + 0.90 = 2.10`. Cash of `1.50` cannot fund both orders, although
each order alone fits.

| Scenario                              | Cash     | Locked | Free    | Signed net |
| ------------------------------------- | -------- | ------ | ------- | ---------- |
| Both orders accepted                  | `100.00` | `2.10` | `97.90` | `0.00`     |
| SELL canceled                         | `100.00` | `1.20` | `98.80` | `0.00`     |
| One BUY contract fills after cancel   | `99.40`  | `0.60` | `98.80` | `1.00`     |
| Both orders fill from the first state | `99.90`  | `0.00` | `99.90` | `-1.00`    |

The last row starts again from the original two-order state rather than following the cancellation.

#### Signed fill accounting

For a fill, let `q` be the prior signed account net, `d` the signed fill count (positive for BUY
and negative for SELL), `p` the YES price, and `f` the net fee. If `C` is cash after filled
premiums and fees but before open-order reservations, the transition is:

```text
q_next = q + d
cash_change = -p * d + min(q_next, 0) - min(q, 0) - f
C_next = C + cash_change
```

- **NO collateral**: The `min` terms adjust collateral backing NO exposure; they are not another
  commission.
- **Reversals**: A reversal applies its reduction and new opening in one transition rather than
  creating a second venue fill. It resets the new side's average to the reversal fill's YES price.
- **Reductions**: Reductions retain the opening average.
- **Realized PnL**: For closing count `h` and average opening YES price `a`, gross realized PnL is
  `h * (p - a)` for a long position and `h * (a - p)` for a short position.
- **Fees**: Fees remain separate. Net cash profit subtracts every authoritative fee exactly once.

**Worked fill sequence**:

This sequence starts with cash `100.0000` and a flat position. Fees are illustrative reported net
amounts, and no orders rest during the sequence.

| Fill        | YES price | Fee        | Signed net after fill | Cash after fill |
| ----------- | --------- | ---------- | --------------------- | --------------- |
| BUY `2.00`  | `0.60`    | `0.010000` | `2.00`                | `98.7900`       |
| SELL `0.75` | `0.70`    | `0.003000` | `1.25`                | `99.3120`       |
| SELL `2.00` | `0.40`    | `0.008000` | `-0.75`               | `99.3540`       |
| BUY `0.25`  | `0.30`    | `0.001000` | `-0.50`               | `99.5280`       |
| BUY `0.50`  | `0.20`    | `0.002000` | `0.00`                | `99.9260`       |

Gross realized PnL is `-0.050000`, fees are `0.024000`, and the final cash change is `-0.074000`.

#### Precision and fees

- **USD precision**: Direct balances align to USD `0.0001`; fees and price-times-count values can
  require six decimals. The adapter preserves exact amounts and applies four-decimal alignment only
  to reported balances.
- **Exact constructors**: `Money::from_decimal` quantizes to USD's two-decimal currency precision.
  Use `Money::from_decimal_exact` or `Money::from_str_exact` for fractional balances and commissions;
  both reject precision loss and overflow.
- **Currency identity**: Exact constructors preserve standard USD identity and default precision.
  Do not overwrite the global USD registry, relabel USD as USDC, or invent a venue alias. Currency
  equality and hashing use the code, and reconstruction resolves the registered currency.
- **Venue fee rounding**: Kalshi rounds each model trade fee up to six decimals, then rounds signed
  revenue minus trade fee down to the member's balance grid. An accumulator spans all fills of one
  order across maker and taker changes. Rebates use the member's balance increment and cannot make a
  fill's net fee negative.
- **Adapter fee handling**: The adapter interprets fill `fee_cost` as the net fee and checks that
  interpretation against authoritative balance evidence. Kalshi's schema does not define whether it
  includes every rounding and rebate component. A disagreement or missing or malformed fee keeps
  recovery incomplete. Maker and taker totals are reconciliation checks, not additional commissions.

**Worked rounding example**:

Three fills each buy `0.01` at `0.1234`. The illustrative model fees are `0.000017276`,
`0.000017276`, and `0.000117276`. The third model fee is higher so the net-fee cap permits a
`0.0001` rebate.

| Fill | Trade fee  | Rounding fee | Rebate     | Accumulator after rebate | Cash change | Net fee    |
| ---- | ---------- | ------------ | ---------- | ------------------------ | ----------- | ---------- |
| 1    | `0.000018` | `0.000048`   | `0.000000` | `0.000048`               | `-0.0013`   | `0.000066` |
| 2    | `0.000018` | `0.000048`   | `0.000000` | `0.000096`               | `-0.0013`   | `0.000066` |
| 3    | `0.000118` | `0.000048`   | `0.000100` | `0.000044`               | `-0.0013`   | `0.000066` |

Combined revenue is `-0.003702`, fees are `0.000198`, and cash change is `-0.0039`. This example
explains the rounding mechanics; it does not establish `fee_cost` semantics.

#### Simulation limits

- **Live accounting**: Generic local cash calculations do not implement Kalshi's signed-binary cash
  transitions. Keep `calculate_account_state=false` for live accounts.
- **Backtests and sandbox**: These modes do not support opening NO exposure with this model.
- **Grid validation**: Their base-increment checks do not replace full-grid validation.

### Private recovery

#### Startup and ownership

- **Instrument set**: Load every execution instrument into the cache before connecting. If markets
  are loaded later, disconnect and reconnect the execution client; a fresh WebSocket connection
  does not refresh that set.
- **Private streams**: Recovery subscribes to `user_orders`, `fill`, and `market_positions` before
  collecting REST evidence.
- **Ownership**: Recovery validates account, subaccount, and shard ownership and requires received
  fill IDs, order fill counts, and position economics to agree with REST evidence. An absent
  subaccount field does not prove subaccount `0`; venue records must establish ownership before
  publication.
- **Incomplete recovery**: Missing identities, fees, pagination, or conflicting evidence keep
  recovery incomplete.

#### Recovered orders

| Recovered state                         | Behavior                                                                                                   |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Node-owned orders                       | Retain command metadata, identity, and ownership.                                                          |
| Unknown filled limit orders             | Recover only when complete fills and current position agree with retained opening context or a flat start. |
| Unknown unfilled resting orders         | Recover with current totals and conservative cash holds.                                                   |
| Unknown canceled orders with fills      | Require enough history to establish final quantity; otherwise recovery fails.                              |
| Nonzero positions without opening proof | Require complete fill and position evidence from a flat start or matching cached authority.                |

- **Recovered defaults**: Without instructions, use GTD when an expiration exists or GTC otherwise,
  with `post_only=false` and `reduce_only=false`. These defaults do not establish the original
  instructions.
- **Recovered identity**: An empty client order ID uses the venue order ID. Recovered IDs must be
  unique across the node.
- **Owned unmapped classifications**: Independently valid fills for owned limit orders can publish
  even when the order type or status is unmapped, unless a same-market settlement or unconfirmed
  quantity prevents the pass. Replay does not repeat fill effects.
- **Admission and reservations**: An unmapped classification keeps command admission blocked and
  reservations unresolved after collateral consumed by those fills is accounted for.
- **External unmapped orders**: An unmapped external order keeps the entire recovery pass
  incomplete.

#### Startup readiness

- **Connection**: `connect()` waits for the current account session and recovered protocol
  generation. Node startup reconciliation must also finish before order flow. A failed data or execution
  connection aborts node startup; the startup error names the failed client without provider error text.
- **Context errors**: `KALSHI_ORDER_CONTEXT_REQUIRED` identifies a filled order without sufficient
  cached context. `KALSHI_OPENING_CONTEXT_REQUIRED` identifies a nonzero position without opening
  authority. The adapter logs each code with its recovery action. Restore complete cached orders
  and positions, not only order records.
- **Concurrent consumption**: Keep the account cache consumer running while `connect()` waits.
  Direct applications must process execution events concurrently; awaiting connection before
  consuming events causes an account-observation timeout.
- **Reset**: Execution-client `reset()` clears instrument definitions but preserves private order,
  fill, position, and cash authority. It requires the canonical cache to remain intact. Do not reuse
  the client after an engine-wide reset clears that cache.
- **External claims**: When unclaimed-order filtering is enabled, register claims before startup or
  disable filtering so the engine accepts recovered orders.
- **Queued reports**: `LiveNode::run()` and `run_with_mode()` apply queued recovered reports and
  events before startup mass reconciliation. Direct applications must apply them once before
  requesting mass status.

#### Lookback and history

- **Unbounded lookback**: The default can include historical fills without cached order context and
  fail startup.
- **Bounded lookback**: `reconciliation_lookback_mins` limits inactive-order and fill replay while
  retaining every resting order on the configured shard, every order with a fill inside the window,
  and every other order updated inside it.
- **Context requirement**: Any retained filled order still needs sufficient context.
- **Position-only history**: Notifications can require coverage of the market's full cumulative
  traded volume, including offsetting fills. Purged orders, restarts without retained history, or
  trades through another client can leave gaps. These gaps keep the client unready and withhold
  final closes on its data channel. Starting a new runtime does not reconstruct that history.
- **Recovery budget**: Each source has a 100-page budget. Recovery reads balance twice and requests
  fee metadata for open orders. Shared rate limits can delay readiness; budget other clients against
  the same account limits.
- **Polling**: A one-second timer requests serial recovery while the private WebSocket is active.
  Passes can run back to back when reads take longer than the interval. Basic and Advanced accounts
  have one balance read per second; each open order also requires event and series fee reads.
- **Account history**: Each successful recovery publishes `AccountState`, including unchanged
  balances. Set `LiveExecutionEngineConfig.purge_account_events_interval_mins` and
  `purge_account_events_lookback_mins` to bound in-memory history. Redis and PostgreSQL retain
  persisted account events for replay.
- **Client lifetime**: Private terminal ownership and command identities remain for the execution
  client's lifetime. Purging the shared cache does not prune that state.

### Order and fill reports

- **Targeted reports**: Order reports require cached order context.
- **Aggregate omissions**: An external order without command metadata can be omitted only after
  validating zero filled quantity and no associated fills in complete live and archived history.
  Nonzero fills, associated fills, or incomplete history fail the aggregate request.
- **Report completeness**: Bulk reports warn about omissions, and mass status sets
  `reports_complete=false` while preserving independent cash holds.
- **Omission cost**: Each omission requires a live fill lookup and a full ticker-scoped archived
  fill scan, including history before the requested report window.
- **Deduplication**: Stable fill IDs deduplicate REST, stream, query, and explicit reconnect evidence.
  Position snapshots do not create fills.
- **Flat reports**: An explicit flat report is required when a tracked market becomes flat; a
  missing paginated record does not prove flatness.
- **Opening average**: `PositionStatusReport` leaves `avg_px_open` and its precision unset.
  Higher-level reconstruction requires complete original fill and reduction history; venue cost and
  exposure totals are not substitutes.
- **Position checks**: Each private recovery publishes position reports. The engine compares them
  with cached positions and warns about discrepancies outside tolerance. These reports count as
  activity and can defer periodic position repair. Investigate repeated mismatch warnings instead
  of waiting for automatic correction.

#### Report field mapping

| Report fields                                              | Source or absence rule                                                                                                                         |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Shared `account_id`, `instrument_id`                       | Configured primary account and shard, with the confirmed market ticker.                                                                        |
| Shared `report_id`, `ts_init`                              | Local report UUID and initialization time.                                                                                                     |
| Order/fill `client_order_id`, `venue_order_id`             | Supplied or cached client ID; recovery uses `order_id` when empty. Venue ID is `order_id`.                                                     |
| Order `order_side`, `price`, `order_type`                  | Canonical direction, `yes_price_dollars`, and supported venue `type=limit`.                                                                    |
| Order `quantity`, `filled_qty`                             | Active total is `fill_count_fp + remaining_count_fp`; canceled totals require cached final-quantity evidence. Filled count is `fill_count_fp`. |
| Order `order_status`                                       | `resting` -> accepted or partially filled; `executed` -> filled only with zero leaves; `canceled` -> canceled.                                 |
| Order `time_in_force`, `expire_time`                       | Cached TIF or recovered GTD/GTC default; expiration comes from venue `expiration_time`.                                                        |
| Order `ts_accepted`, `ts_last`                             | REST creation/update times or WebSocket `created_ts_ms`/`last_updated_ts_ms`.                                                                  |
| Order `avg_px`                                             | Absent from adapter order reports.                                                                                                             |
| Order `post_only`, `reduce_only`                           | Cached instructions; private recovery defaults both to false when unavailable.                                                                 |
| Order `cancel_reason`                                      | Absent; the adapter does not map WebSocket `last_update_reason`.                                                                               |
| Order contingency, trigger, display, and activation fields | Absent from the supported order slice.                                                                                                         |
| Order `order_list_id`                                      | Retained local independent-list identity, not venue contingency.                                                                               |
| Fill `trade_id`, `last_qty`, `last_px`, `order_side`       | REST `fill_id` or equivalent `trade_id`, count, YES price, and canonical direction.                                                            |
| Fill `commission`, `liquidity_side`                        | Exact `fee_cost` subject to balance verification; `is_taker` selects taker or maker.                                                           |
| Fill `ts_event`, `avg_px`                                  | REST `created_time` or WebSocket `ts_ms`; no cumulative average for one fill.                                                                  |
| Order/fill/position `venue_position_id`                    | Absent; no fabricated venue position identifier.                                                                                               |
| Position `position_side`, `quantity`, `signed_decimal_qty` | Sign, absolute value, and exact value of `position_fp`; zero maps to flat.                                                                     |
| Position `ts_last`                                         | REST `last_updated_ts`; a stream without a timestamp triggers a REST refresh.                                                                  |
| Position `avg_px_open` and precision                       | Absent; no opening average inferred from cost, exposure, or traded totals.                                                                     |

`user_orders.status=unknown` supplies no reportable state and requires REST reconciliation. Venue
`canceled` maps to `OrderStatus::Canceled`; the adapter does not map `ExpiryCancel` to `OrderExpired`.
For a cached post-only rejection, canceled zero-fill venue evidence preserves `Rejected`; resting or
filled venue evidence is a conflict.

### Settlement and restart

#### Final payout

- **Final evidence**: A final close uses authoritative `settlement_value_dollars` and
  `settlement_ts`. Determination, dispute, amendment, and elapsed expiration do not prove final
  settlement.

For final YES payout `s` and signed account net `q` immediately before settlement, gross settlement
cash is:

```text
payout = s * max(q, 0) + (1 - s) * max(-q, 0)
```

- **Cash credit**: Settlement credits the account's signed net payout once and consumes the
  remaining exposure.
- **No synthetic events**: Settlement does not create a synthetic trade, order, fill, or fee.
- **Strategy PnL**: Strategy-level position PnL remains separate from the account cash credit.
- **Exact credit**: Reconcile the actual credited amount and any rounding adjustment from venue
  evidence. Cent-valued settlement revenue is insufficient for fractional positions or payouts.
- **Missing evidence**: Missing authoritative fractional-settlement evidence blocks reconciliation
  and order flow.

#### Payout examples

These independent one-contract cases start with cash `100.0000`, an opening net fee of `0.002000`,
and no resting orders. A NO opening uses SELL at the same YES price of `0.60`.

| Exposure   | Cash before settlement | Final cash at YES `1.00` | Final cash at YES `0.00` | Final cash at YES `0.33` |
| ---------- | ---------------------- | ------------------------ | ------------------------ | ------------------------ |
| YES `1.00` | `99.3980`              | `100.3980`               | `99.3980`                | `99.7280`                |
| NO `1.00`  | `99.5980`              | `99.5980`                | `100.5980`               | `100.2680`               |

Every case ends flat. For the fully filled reservation example, settlement leaves `99.90` for a YES
outcome or `100.90` for a NO outcome.

#### Close preconditions

- **Fill barrier**: Before a final close is published or applied to an execution-enabled account,
  reconcile every fill through `settlement_ts`.
- **Restart**: Restore the complete position cycle before applying the close.
- **Reconciliation**: Deduplicate fills by account scope and fill ID, replay each once, and compare
  the signed account net and shard cash with fresh venue evidence.

:::warning
Final closes wait for every native execution client on the same runtime data channel. Each client
must reconcile settlement and cash, observe the current account session, and confirm queued fills on
cached orders. Keep clients active until their positions settle.

- **Stopped client**: Final closes are withheld until it explicitly reconnects.
- **Disposed or dropped client**: Final closes are permanently blocked on that data channel.
  Recreate the runtime data channel rather than replacing only the execution client.
- **Data-only runtime**: Market-level close delivery remains available.

:::

#### Irreversible blocks

- **Engine barrier**: The first cached close is authoritative and freezes position changes,
  including late fills whose event time precedes settlement.
- **Triggers**: Unapplied execution evidence found after settlement is prepared or cached, and
  conflicting settlement signatures, permanently block that execution scope and its data channel's
  final closes.
- **Persistence**: Stop, reset, and explicit reconnect do not clear the block. Ordinary fill replay cannot
  repair the settled position.
- **Adapter limit**: The adapter does not perform the required reconstruction. Do not clear the
  close or claim equivalence through ordinary replay.
- **Recovery**: Stop order flow, restore the complete position cycle and cash with verified
  reconstruction, and start a runtime with a new data channel.

## Rate limiting

### Token budgets

- **Read and Write budgets**: Kalshi uses separate budgets for each account and environment. Other
  API clients on the same account consume them. The default request cost is 10 tokens;
  `/account/endpoint_costs` and `/account/limits` supply overrides and capacities.
- **Discovery**: Authenticated transports discover limits and costs before ordinary requests.
  Discovery is valid for at most 60 seconds and expires earlier with a temporary grant. Writes wait
  for valid discovery.
- **Discovery fallback**: Failed discovery starts a five-second conservative read fallback: 200
  tokens per second, capacity 200, cost 50 per request, and one balance request per second. Writes
  remain disabled until both discovery reads succeed.
- **Balance bucket**: Basic and Advanced tiers allow one balance read per second; higher known tiers
  allow five.
- **Batch cost**: Cost scales per item, and the full chunk must fit before transmission.

### Shard and account budgets

- **Nonzero shard**: A single-order write with a specified nonzero shard uses that shard's Write
  bucket.
- **Unscoped writes**: Shard-zero and batch writes use the unscoped bucket.
- **Auto-routed writes**: These consume the unscoped and every nonzero shard bucket.
- **Reads**: Read budgets are not shard-scoped.
- **Quota sharing**: Use `new_authenticated_with_quota` to share a `KalshiQuota` across account
  data and execution transports with distinct API keys. Use `new_with_quota` for public traffic
  which must consume the same budget. Default authenticated construction shares quota for the same
  key and origin.

### Retries and writes

- **Safe reads**: The adapter retries transport failures, timeouts, HTTP 500, 502, 503, 504, and
  recognized rate-limit 429 responses up to twice with bounded backoff.
- **Quota and signing**: Each adapter attempt acquires quota; authenticated attempts then sign.
- **Other 429 responses**: The adapter does not retry these responses.
- **Order writes**: The adapter makes one attempt and does not automatically retry. Lower-level
  transport retries can replay the signed request without entering adapter quota or signing. Every
  uncertain outcome requires reconciliation.

## Configuration

### Data client configuration options

| Option                             | Default      | Description                                                                      |
| ---------------------------------- | ------------ | -------------------------------------------------------------------------------- |
| `environment`                      | `Production` | Select production or demo endpoints and credential variables.                    |
| `instrument_ids`                   | `[]`         | Bootstrap and streaming markets; factories require a nonempty list.              |
| `api_key_id`                       | `None`       | API key ID; must be paired with `private_key`.                                   |
| `private_key`                      | `None`       | PEM private-key text; must be paired with `api_key_id`.                          |
| `base_url_http`                    | `None`       | Optional REST URL override.                                                      |
| `base_url_ws`                      | `None`       | Optional WebSocket URL override.                                                 |
| `proxy_url`                        | `None`       | Optional explicit HTTP or HTTPS proxy.                                           |
| `http_timeout_secs`                | `60`         | REST attempt timeout in seconds.                                                 |
| `ws_timeout_secs`                  | `30`         | WebSocket connection, subscription-restoration, and lifecycle-bootstrap timeout. |
| `book_snapshot_timeout_secs`       | `10`         | Initial and recovery snapshot deadline; `0` disables deadlines.                  |
| `update_instruments_interval_mins` | `60`         | Delay between completed instrument refreshes.                                    |
| `transport_backend`                | `Sockudo`    | Required WebSocket backend; `Tungstenite` is rejected.                           |

### Execution client configuration options

| Option              | Default      | Description                                                                                 |
| ------------------- | ------------ | ------------------------------------------------------------------------------------------- |
| `environment`       | `Production` | Select production or demo endpoints and credential variables.                               |
| `account_id`        | `KALSHI-001` | Nautilus account ID; use a distinct value for each colocated shard client.                  |
| `exchange_index`    | `0`          | Exchange shard for orders, reports, and collateral.                                         |
| `api_key_id`        | `None`       | API key ID; must be paired with `private_key`.                                              |
| `private_key`       | `None`       | PEM private-key text; must be paired with `api_key_id`.                                     |
| `base_url_http`     | `None`       | Optional REST URL override.                                                                 |
| `base_url_ws`       | `None`       | Optional WebSocket URL override.                                                            |
| `proxy_url`         | `None`       | Optional explicit HTTP or HTTPS proxy.                                                      |
| `http_timeout_secs` | `60`         | REST attempt timeout in seconds.                                                            |
| `ws_timeout_secs`   | `30`         | WebSocket connection, subscription-restoration, bootstrap, and account-observation timeout. |
| `transport_backend` | `Sockudo`    | Required WebSocket backend; `Tungstenite` is rejected.                                      |

### Shared validation

- **Input validation**: Python constructors validate inputs. Rust callers use `validate()`, and
  authenticated HTTP construction validates before building the transport. Unknown decoded fields
  are rejected.
- **Known hosts**: Kalshi hosts, including hosts with a trailing dot, must match `environment` and
  use HTTPS or WSS.
- **Custom hosts**: Local peers and gateways can use HTTP or WS.
- **Endpoint components**: Overrides cannot contain user information, query parameters, or
  fragments.
- **Timing**: Values must be positive and fit their transport units, except
  `book_snapshot_timeout_secs=0`, which disables book deadlines.

### Transport behavior

- **HTTP**: The client ignores ambient proxy variables and does not follow redirects. Lower-level
  transport retries can replay a signed request without entering adapter quota or signing.
- **WebSocket**: The client uses the configured URL, timeout, backend, and explicit proxy. Automatic
  reconnects are disabled; call `stop()` and then `connect()` after an unexpected disconnect.
- **Tungstenite**: This backend is rejected because its TRACE handshake logs can expose
  authentication headers.
- **Sockudo**: WARN and ERROR handshake diagnostics can include the endpoint authority and a static
  failure category. They do not include Kalshi authentication headers.

### Live node setup

- **Reconciliation**: Set `LiveExecutionEngineConfig.reconciliation_lookback_mins` to bound
  inactive-order and fill replay.
- **Account model**: The execution factory constructs a USD cash account with netting OMS.
- **Execution**: Use a dedicated Kalshi-only node with `LiveRiskEngineConfig.bypass=true` for order
  writes. Ordinary risk-enabled submission and amendment are unsupported.
- **Examples**: The maintained examples show complete live-node wiring for both clients.

## Contributing

:::info
To contribute to the Kalshi adapter or request a feature, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
