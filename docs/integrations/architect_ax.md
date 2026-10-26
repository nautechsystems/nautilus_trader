# AX Exchange

[AX Exchange](https://architect.exchange) is a centralized and regulated derivatives exchange for traditional
underlying asset classes. Architect Bermuda Ltd. operates AX under a license from the
[Bermuda Monetary Authority (BMA)](https://www.bma.bm). AX lists perpetual and dated futures contracts in both
its production and sandbox catalogs.

This integration supports live market data ingest and order execution with AX Exchange.

## Overview

This adapter is implemented in Rust and exposed to Python through PyO3 bindings. It does not
require external AX client libraries.

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The AX Exchange adapter includes multiple components, which can be used together or separately
depending on the use case.

- `AxHttpClient`: Low-level HTTP API connectivity.
- `AxMdWebSocketClient` and `AxOrdersWebSocketClient`: Low-level WebSocket connectivity for Rust
  callers.
- `AxDataClient`: A market data feed manager.
- `AxExecutionClient`: An account management and trade execution gateway.
- `AxDataClientFactory`: Factory for AX data clients.
- `AxExecutionClientFactory`: Factory for AX execution clients.

:::note
Most users configure a live trading node as shown below. They do not need to use these lower-level components
directly.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/architect_ax/examples/)

## AX Exchange documentation

AX Exchange provides documentation for users at the
[Architect documentation site](https://docs.architect.exchange/).
Refer to the AX Exchange documentation in conjunction with this NautilusTrader integration guide.

## Products

The production catalog lists perpetual and dated futures contracts in these venue categories:

| Venue category   | Examples                     | Nautilus asset class |
| ---------------- | ---------------------------- | -------------------- |
| Foreign exchange | `EURUSD-PERP`, `JPYUSD-PERP` | FX                   |
| Equities         | `AAPL-PERP`, `NVDA-PERP`     | Equity               |
| Metals           | `XAU-PERP`, `XAU-2026-DEC`   | Commodity            |
| Energy           | `WTIOIL-PERP`                | Commodity            |
| Treasuries       | `UST10Y-PERP`                | Debt                 |
| Compute          | `NVDA-H100-2027-MAR`         | Alternative          |

The sandbox catalog differs. It also lists `energy_etfs` contracts such as `USO-PERP` and
`UNG-PERP`, which map to the Equity asset class, and compute perpetuals such as `OCPI-H100-PERP`.

The adapter maps a `crypto` venue category to the `CRYPTOCURRENCY` asset class, and any category it
does not recognize to `ALTERNATIVE`.

### Perpetual contracts

A perpetual contract (perpetual swap) is a derivative that tracks the price of an underlying
asset without expiring. Unlike standard futures, there is no settlement date, which eliminates
rollover costs and simplifies position management. A funding rate mechanism keeps the contract
price aligned with the underlying index price through periodic payments between long and short
holders. See the [Architect documentation](https://docs.architect.exchange/) for details on
funding rate mechanics and contract specifications.

Characteristics of AX perpetual contracts:

- **Cash-settled in USD**: No physical delivery. All profit and loss is settled in USD.
- **Funding rates**: Periodic payments keep the contract price aligned with the underlying.
- **Multiplier of 1**: Each contract represents one unit of exposure to the underlying.
- **Whole contracts only**: Fractional quantities are not supported.
- **Margin**: A position requires initial margin to open and maintenance margin to stay open.

The adapter represents an AX instrument without an expiration as `PerpetualContract` and an
instrument with an expiration as `FuturesContract`. The venue category determines the Nautilus
asset class. The adapter uses `MARGIN` account type and `NETTING` order management.

## Symbology

The adapter preserves each AX symbol and appends the Nautilus venue identifier `.AX`. Perpetual
symbols use the `-PERP` suffix. Dated symbols include their year and contract month.

| Contract     | AX Symbol      | Nautilus InstrumentId |
| ------------ | -------------- | --------------------- |
| EUR/USD perp | `EURUSD-PERP`  | `EURUSD-PERP.AX`      |
| Gold perp    | `XAU-PERP`     | `XAU-PERP.AX`         |
| Dated gold   | `XAU-2026-DEC` | `XAU-2026-DEC.AX`     |

The venue identifier is `AX`. To construct a Nautilus `InstrumentId`:

```python
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("EURUSD-PERP.AX")
```

## Environments

AX Exchange provides two trading environments. Configure the appropriate environment using the
`environment` parameter in your client configuration.

| Environment    | Config                                 | Description                            |
| -------------- | -------------------------------------- | -------------------------------------- |
| **Sandbox**    | `environment=AxEnvironment.SANDBOX`    | Test environment with simulated funds. |
| **Production** | `environment=AxEnvironment.PRODUCTION` | Live trading with real funds.          |

### Sandbox

The default environment for development and testing with simulated funds.
All sandbox endpoints are resolved automatically when `environment=AxEnvironment.SANDBOX`.

#### 1. Create a sandbox account

Follow the [Architect documentation](https://docs.architect.exchange/) to create a sandbox
account. An invite code is required during registration.

#### 2. Create API keys and fund the account

Use the AX sandbox UI to generate API keys and deposit simulated funds into your account.
Store the `api_key` and `api_secret` securely.

#### 3. Set environment variables

```bash
export AX_API_KEY="your-sandbox-api-key"
export AX_API_SECRET="your-sandbox-api-secret"
```

#### 4. Configure the live node

Set `environment=AxEnvironment.SANDBOX` on the data and execution client configs. See the
[Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax/)
for complete `LiveNode` setup.

### Production

For live trading with real funds. Requires a verified AX Exchange account.

```python
config = AxExecutionClientConfig(
    environment=AxEnvironment.PRODUCTION,
)
```

:::warning
Ensure you are using the correct environment before placing orders.
Sandbox is the default to prevent accidental live trading.
:::

## Market data

The adapter provides real-time market data via WebSocket subscriptions, with HTTP endpoints
for historical data backfill.

### Data types

| AX Data           | Nautilus Data Type  | Notes                                                          |
| ----------------- | ------------------- | -------------------------------------------------------------- |
| Order book (L1)   | `QuoteTick`         | Best bid/ask top-of-book from L1 book subscription.            |
| Order book (L2)   | `OrderBookDelta`    | Aggregated price levels.                                       |
| Order book (L3)   | `OrderBookDelta`    | Per-snapshot order quantities with synthetic IDs.              |
| Trades            | `TradeTick`         | Real-time trade events from trade-only WebSocket subscription. |
| Mark price        | `MarkPriceUpdate`   | Extracted from L1 ticker subscription.                         |
| Bars/candles      | `Bar`               | OHLCV data (total volume only, no buy/sell breakdown).         |
| Funding rates     | `FundingRateUpdate` | Polled via HTTP; interval configurable.                        |
| Instrument status | `InstrumentStatus`  | State changes from L1 ticker subscription.                     |

AX instrument states map to `MarketStatusAction` as follows:

| AX state                            | `MarketStatusAction`        |
| ----------------------------------- | --------------------------- |
| Pre-open                            | `PRE_OPEN`                  |
| Open                                | `TRADING`                   |
| Closed, closed-frozen               | `CLOSE`                     |
| Halted                              | `HALT`                      |
| Match-and-close auction             | `CROSS`                     |
| Suspended                           | `SUSPEND`                   |
| Delisted, or any unrecognized state | `NOT_AVAILABLE_FOR_TRADING` |

:::note
Historical quote tick requests are not supported by AX Exchange. Only real-time quote
data is available via WebSocket L1 book subscriptions. The adapter does not expose AX index prices
or instrument close events, so those subscriptions log a warning and yield no data.
:::

:::note
AX L3 snapshots contain per-order quantities but no venue order IDs. The adapter assigns synthetic
IDs within each snapshot. It cannot track the same individual order across snapshots.
L2 and L3 processing requires full snapshots (`st: true`). The adapter rejects incremental
frames (`st: false`) to avoid clearing unchanged book levels.
:::

:::note
AX publishes no trade identifier for market data. Thus, the adapter derives `TradeTick.trade_id` from the
trade's timestamp, price, size, and aggressor side. REST and WebSocket agree on the same trade whenever both
report its aggressor side. Prints that AX reports identically share an ID. This affects only consumers that
deduplicate market data on `trade_id`, because fills carry the venue's own trade IDs.
:::

### WebSocket subscription behavior

AX market data WebSocket subscriptions use one active stream per symbol. The adapter selects the
smallest stream that covers the active Nautilus subscriptions:

- A trades-only subscription uses AX `level: "TRADES"`, which delivers trade prints only.
- Book-only and quote-only subscriptions set AX `trades: false` and `ticker: false` to suppress
  unrequested trade and ticker events.
- Mark price and instrument status subscriptions require AX ticker events. Thus, the adapter enables ticker
  delivery on the active book stream. If no book subscription exists, it opens an L1 stream.
- Book deltas subscribe at the AX level matching the Nautilus book type. `L1_MBP` has no
  delta-capable AX equivalent, so the adapter logs a warning and subscribes at L2 instead.
- If multiple Nautilus data types are active for a symbol, the adapter resubscribes only for these changes:

  - The required AX level or delivery flags change.
  - An order book needs a fresh snapshot (see [Order book recovery](#order-book-recovery)).
- Subscription changes reach AX in the order the data engine issues them, so an unsubscribe
  followed by a resubscribe leaves the stream subscribed.

AX documents estimated funding rates on ticker events and an estimated-funding request on the orders
WebSocket. Ticker models retain estimated-funding metadata. Nautilus exposes settled funding-rate updates
through HTTP polling. The adapter does not emit a separate estimated-funding data type or request standalone
estimates.

### Order book recovery

The data client tracks each order book delta subscription with the
[shared book recovery machinery](../developer_guide/adapters.md#order-book-recovery-ownership). AX L2 and L3
messages carry a full snapshot and no sequence number. AX sends one immediately after each subscribe
acknowledgement, including for an empty or unchanged book. The client accepts every message as a snapshot. It
suppresses book output while a subscription write is in flight and drops frames for a book that is no longer
subscribed.

Recovery replaces the symbol's subscription with an unsubscribe and a subscribe on the same connection. The
replacement retains its current level and trade and ticker flags. AX carries all market data for a symbol on
one stream, so trades, quotes, mark prices, and instrument status for that symbol pause while the replacement
runs. Recovery starts when:

- An initial subscription write fails.
- No snapshot arrives within `book_snapshot_timeout_secs` (default 10 seconds) after the initial
  subscription write completes or the connection is re-established. A data client `connect` after
  `disconnect` counts too: the client keeps its books, and the WebSocket client replays their
  subscriptions.
- An L2 or L3 frame cannot be converted, such as an incremental (`st: false`) frame. The book stops
  emitting until a replacement snapshot arrives, and a running recovery's current attempt fails
  without waiting for its snapshot deadline.

A subscription AX rejects delivers no snapshot, so its snapshot deadline starts recovery. A
subscription the client cannot queue because the WebSocket handler has stopped starts no recovery,
and its book emits nothing. Subscribing to deltas for a book whose stream is already open replaces
the stream, so the book starts from a fresh snapshot.

Each recovery makes up to eight attempts within 180 seconds, with exponential backoff. It then retries until
it accepts a snapshot. The retry interval doubles from one minute to fifteen minutes. A running recovery
continues across reconnects with its remaining budget, and unsubscribe or shutdown cancels it. A recovery
waiting between attempts after its budget retries at once on the new connection. Recovery never ends in a
failed state.

Subscription writes run in order while the handler continues processing commands and frames. Cancellation
skips unsent book subscription and recovery writes and keeps their snapshot gates closed. A write already
admitted to the network can still complete before later subscription changes.

The client does not correlate subscription acknowledgements with recovery attempts. A snapshot
queued before a replacement can complete recovery once the replacement write finishes. AX sends no
frames while a book is unchanged, so the client does not treat a silent book as stale.

Setting `book_snapshot_timeout_secs` to `0` disables snapshot deadlines. Recovery then starts only from a
failed initial write or an invalid frame. Within the retry budget, a replacement that delivers no snapshot
waits until one of these events:

- The client accepts a snapshot.
- An invalid frame fails the attempt.
- Recovery is cancelled.
- The 180-second initial budget ends.

### Live recovery validation

The `ax-book-stress` harness is a development tool for changes to book synchronization and recovery. It uses
AX sandbox market data and submits no orders. It checks five perpetual books against the book stream contract
and each relayed raw L2 frame.

The AX market data stream requires authentication, so the harness reads sandbox API credentials
from `AX_API_KEY` and `AX_API_SECRET` and runs without `scripts/strip-adapter-env.bash`. From the
repository root, run:

```bash
CARGO_BUILD_JOBS=16 cargo test -p nautilus-architect-ax --features examples --test ax-book-stress -- --timeout 10 --rounds 14
```

`--scenario` selects the run:

- `churn` (default): rotates through these conditions:

  - Invalid frames that each book recovers without a reconnect.
  - Snapshots held past their deadlines after a reconnect.
  - Late snapshots after a reconnect.
  - A rejected replacement.
  - Reconnects cut before their snapshots.
  - A restart during recovery.
  - A 40-second traffic freeze that closes no socket.
- `initial`: drops each book's first snapshot and silences its stream, in a fresh session per
  round. With `--timeout 0`, the books stay dark until a reconnect replays their subscriptions.
- `turnover`: unsubscribes and resubscribes a recovering book just after recovery starts, after its
  replacement reaches the venue, or after a rejected replacement's deadline. The new subscription
  must keep streaming once the venue settles, with no further replacement.
- `boundaries`: rejects every attempt in the retry budget, then checks the retry ceiling, a
  reconnect that ends the ceiling wait, unsubscribe during recovery, and shutdown during a
  reconnect. It requires a nonzero `--timeout`, since snapshot deadlines end each rejected attempt,
  and at least three `--symbols`.

`--timeout` sets the snapshot timeout in seconds, where `0` disables snapshot deadlines, and
`--rounds` sets the number of rounds (14 by default). `--symbols` takes a comma-separated list of
symbols to check. The sandbox market maker quotes only some instruments, and a book that stops
streaming fails the run, so choose books that stream.

The harness requires the sandbox market data WebSocket and REST API. See
[Stress harnesses](../developer_guide/spec_data_testing.md#stress-harnesses) for the shared flags
and output format.

### HTTP API behavior

- `GET /tickers` returns limit/offset page metadata and supports `limit`, `offset`, and `sort`
  query parameters.
- `GET /ticker` returns the ticker under a top-level `ticker` response field.
- `GET /open-orders` uses limit/offset pagination. Open-order reconciliation traverses all pages
  and validates totals, offsets, duplicates, and completeness so detected response drift fails the
  request.
- `GET /fills` and `GET /funding-rates` use cursor pagination. The adapter traverses each cursor chain as a
  best-effort historical read. AX corrections during traversal are not an atomic snapshot.
- `GET /orders` exposes cursor metadata and supports `order_id`, `order_ids`, `account_id`, and
  optional timestamp filters. Order-report history traversals follow the cursor chain,
  accept partial pages, and validate declared limits and totals. Empty continuation pages,
  repeated cursors, duplicate order IDs, and incomplete declared totals fail the request.
- Open-order, historical-order, fill, and position report requests resolve an uncached symbol
  through `GET /instrument` and cache the result. An instrument request or parse failure fails that
  entire report request instead of dropping venue state.
- `GET /transactions` requires `start_timestamp_ns` and `end_timestamp_ns` with a range no wider
  than 7 days. The low-level client exposes its cursor and account selectors.
- `GET /order-status` can include `reject_reason` and `reject_message` for rejected orders.
- When an account selector is omitted, AX uses the primary account. The high-level execution client owns one
  primary account. Low-level request models expose documented account selectors.

### Bar intervals

| Interval | Description |
| -------- | ----------- |
| `1s`     | 1-second    |
| `5s`     | 5-second    |
| `1m`     | 1-minute    |
| `5m`     | 5-minute    |
| `15m`    | 15-minute   |
| `1h`     | 1-hour      |
| `1d`     | 1-day       |

## Orders capability

The AX order-entry API has no order-type selector. Its single native order shape requires a price,
which the adapter maps to a Nautilus `LIMIT` order. The adapter simulates a Nautilus `MARKET` order
by previewing an aggressive price and submitting that priced shape with IOC.
Tracked fills retain the submitted Nautilus order type and side, including simulated market orders.

The official [REST place-order](https://docs.architect.exchange/api-reference/order-management/place-order)
and [orders WebSocket](https://docs.architect.exchange/api-reference/order-management/orders-ws) request
schemas contain no `order_type` or `trigger_price` field. Sandbox stop-limit submissions with unbreached
triggers executed immediately at the active limit price. With conditional execution unconfirmed, the adapter
rejects venue-native stop-limit orders before sending them.

Nautilus can still emulate a stop-limit order locally. The common order emulator waits for the
configured trigger, then sends a plain limit order to this adapter.

### Order types

| Order Type             | Supported | Notes                                           |
| ---------------------- | --------- | ----------------------------------------------- |
| `MARKET`               | ✓         | Adapter-simulated with an aggressive IOC price. |
| `LIMIT`                | ✓         | Maps to the native AX priced order shape.       |
| `STOP_LIMIT`           | -         | *Not supported by AX Exchange*.                 |
| `LIMIT_IF_TOUCHED`     | -         | *Not supported by AX Exchange*.                 |
| `STOP_MARKET`          | -         | *Not supported by AX Exchange*.                 |
| `MARKET_IF_TOUCHED`    | -         | *Not supported by AX Exchange*.                 |
| `TRAILING_STOP_MARKET` | -         | *Not supported by AX Exchange*.                 |

### Execution instructions

| Instruction      | Supported | Notes                                                         |
| ---------------- | --------- | ------------------------------------------------------------- |
| `post_only`      | ✓         | Maker-only; rejected if the order would take.                 |
| `reduce_only`    | -         | Rejected locally; AX exposes no reduce-only field.            |
| `quote_quantity` | -         | Rejected locally; the adapter wire path encodes base only.    |
| `display_qty`    | -         | Rejected locally; the adapter wire path has no display field. |

The adapter omits `rb` on place and replace requests, using AX's default `rej` behavior for
post-only orders. Selecting `bo` (back off one tick from the opposite side) or `tbl` (best price on
the same side) is not supported. See the [AX changelog](https://docs.architect.exchange/changelog).

Sandbox crossing-order validation confirms that omitted `rb` and explicit `rej` reject with
`post-only order would cross the book`. External orders that opt into `bo` or `tbl` report the
adjusted limit in `p`; reconciliation uses that price. Post-only orders can fill after resting.

Order-history, open-order, and WebSocket order responses retain `rb` as optional typed adapter metadata on
`AxOrderDetail`, `AxOpenOrder`, and `AxWsOrder`. It describes the latest place or replace request and takes
effect only when that request has `po: true`. Missing or null values remain absent. Unrecognized strings map
to `Unknown`, not `Reject`. `OrderStatusReport` does not expose this metadata, so reconciliation reports
cannot distinguish these repricing policies for externally placed or replaced orders.

The reduce-only boundary matters because AX has no reduce-only field. In sandbox, the venue accepted and
filled an order as ordinary after its reduce-only instruction was dropped from the wire payload. Such an order
can open or increase exposure instead of closing it. Production behavior was not verified. The adapter
therefore denies reduce-only orders before submission rather than sending an instruction the venue cannot
honor.

The adapter also rejects quote-quantity and display-quantity instructions because its AX wire path
cannot encode those semantics. This is an adapter boundary, not a claim that AX Exchange rejects
equivalent venue-native features.

### Time in force

| Time in Force  | Supported | Notes                            |
| -------------- | --------- | -------------------------------- |
| `GTC`          | ✓         | Good Till Canceled.              |
| `GTD`          | -         | Rejected locally by the adapter. |
| `DAY`          | ✓         | Valid until end of trading day.  |
| `IOC`          | ✓         | Immediate or Cancel.             |
| `FOK`          | -         | Rejected locally by the adapter. |
| `AT_THE_OPEN`  | -         | Rejected locally by the adapter. |
| `AT_THE_CLOSE` | -         | Rejected locally by the adapter. |

The venue deprecates `DAY` and recommends `GTC` instead.

### Advanced order features

| Feature            | Supported | Notes                                                              |
| ------------------ | --------- | ------------------------------------------------------------------ |
| Order modification | ✓         | Atomic replace; AX returns a new venue order ID.                   |
| Cancel order       | ✓         | Single order cancellation.                                         |
| Cancel all orders  | ✓         | Cancel all open orders for an instrument.                          |
| Batch cancel       | -         | The adapter sends individual cancels.                              |
| Order lists        | ✓         | Sequential submission (orders submitted individually, non-atomic). |

The adapter restores cached local open and in-flight orders before processing the orders stream. An uncertain
send or request failure retains local tracking until a terminal event or a terminal reconciliation report
identifies the active venue order. After replacement, queries, modifications, and cancels use the active
venue order ID. Stale parent updates and malformed tracked updates wait for reconciliation without being
routed as external orders.

A cancel that already has a venue order ID is forwarded, including a second cancel of a
terminal order. A strategy does not send that second cancel after the local order is already
closed or pending cancel. The adapter emits `OrderCancelRejected` only when AX sends
`CancelRejected`. Sandbox answers a resend of an already canceled order with WebSocket error
`404` (`order not found`). The adapter logs that error and does not turn it into
`OrderCancelRejected`.

**Side filter**: AX cancel-all has no side parameter. Thus, a `CancelAllOrders` command with `order_side` set
sends individual cancel requests. It cancels only open orders on that side for the instrument. A side-filtered
request selects from open orders only, so an inflight (`SUBMITTED`) order not yet acknowledged by AX survives
one.

### Position management

| Feature         | Supported | Notes                                |
| --------------- | --------- | ------------------------------------ |
| Query positions | ✓         | Real-time position updates.          |
| Position mode   | -         | Netting mode only.                   |
| Cross margin    | ✓         | Cross-margin across all instruments. |

### Order querying

| Feature              | Supported | Notes                                                   |
| -------------------- | --------- | ------------------------------------------------------- |
| Query open orders    | ✓         | List all active orders.                                 |
| Query single order   | ✓         | By venue order ID or client order ID (any order state). |
| Order status reports | ✓         | Open-order checks and historical startup mass status.   |
| Fill reports         | ✓         | Execution and fill history.                             |

:::note
Bulk open-order checks use `/open-orders` when `open_check_open_only` is enabled, which is the
default. Otherwise, they combine `/orders` with an unbounded `/open-orders` snapshot. Startup mass-status
reconciliation uses the same combination, including historical terminal orders such as filled and canceled
orders. The merged result applies history bounds only to closed reports. Active and in-flight reports
survive the bounds and take precedence over historical rows with the same venue order ID.

Single-order queries via `query_order` use `/order-status`, which works for any order state. Canceled,
expired, and rejected orders with no remaining quantity also use `/orders` to recover their original
quantity. Targeted reconciliation probes use `/order-status` by venue or client order ID, then fetch the
complete `/orders` record. Only AX's HTTP 404 `no matching orders` error establishes absence. Other error
responses, transport or decode failures, and missing detail records fail the probe. Instrument resolution,
report parsing, and identity mismatches also fail the probe.

AX open and historical order payloads do not expose a stop order type or trigger price.
REST-derived reconciliation therefore reports every visible external order as a limit order. The
adapter does not submit venue-native conditional orders.

Historical order reports carry the venue reject reason (`r`, falling back to `txt`), so reconciled
`OrderRejected` events keep the same reason strings as their real-time counterparts. Reconciled
`OrderCanceled` events can also carry the venue reason where a live cancel carries none. Reports retain the
venue's post-only flag and subsecond timestamp.

AX `/orders` payloads expose creation time without a terminal timestamp. When history contains a
closed order with fills, the adapter also requests `/fills` in the same window (at most seven days).
Historical reports keep `ts_accepted` at creation and raise a closed order's `ts_last` to its latest
matching fill. With no matching fill, `ts_last` stays at creation. The fill time is a lower bound for
cancellation or expiry. Targeted probes use the latest seven days for this fill lookup.

Replaced historical order IDs are terminal. Day-complete orders expire, and expired IOC orders cancel,
matching the WebSocket event path. An unknown order state fails the reconciliation request instead of omitting
an order from the snapshot. A fill with an unknown sibling order state still reaches execution.

Startup mass-status reconciliation bounds its `/orders` and `/fills` requests by
`reconciliation_lookback_mins`, and positions are always reported as a current snapshot. A lookback longer
than seven days still yields only seven days of fills. The adapter sets the mass status's `lookback_start`
to the applied fill-window start, including the seven-day boundary when no lookback is configured.
Without a configured lookback, the adapter still fetches every historical order. An explicit zero-minute
lookback uses a one-nanosecond fill window because AX rejects zero-width queries.

After a successful complete positions snapshot, the adapter emits explicit flat reports for instruments
absent from AX's response but present in this account's cached open positions. It preserves explicit flat
and non-flat venue rows. These cached flat reports let reconciliation close retained positions whose cached
resting orders fill while the node is offline, without leaving offsetting external positions. Fills for
instruments without a position report update orders without opening positions, so incomplete history for an
uncached flat instrument does not require a position correction. An invalid position row fails mass-status
collection instead of implying that its instrument is flat.

Position reports divide the signed fill-ledger cost basis by signed net quantity. The
[AX gateway schema](https://docs.architect.exchange/openapi/api-gateway.json) defines `signed_notional`
as a deprecated alias for `signed_cost_basis`; reports prefer `signed_cost_basis` when present. This
ledger average can change on reductions and reversals independently of Nautilus's opening-fill average,
and it can be zero or negative.

Reports use [`AvgPxReconciliation::OpeningOnly`](../concepts/execution/reconciliation.md#reported-entry-averages),
so reconciliation uses the value for synthetic openings and reversals without comparing it with the cached
average. Reports preserve the signed decimal quotient without rounding it to instrument price precision
or declaring an average-price precision.
:::

:::warning[Position ownership during recovery]
Closing fills linked to cached orders retain their strategy ownership. If closing order evidence is
unavailable, a flat snapshot alone cannot identify which strategy owns a correction. Verify retained
strategy positions against AX before resuming trading in that case.

Special fills use external order identities. A settlement that closes strategy exposure can therefore
leave offsetting strategy and external positions. Configure the strategy's `external_order_instrument_ids`
when it should own those external orders. See
[External order creation](../concepts/execution/reconciliation.md#external-order-creation).

Explicit flat reports enable position reconciliation. If history starts mid-lifecycle, reconciliation may
need a synthetic correction. With `generate_missing_orders=False`, an unresolved discrepancy prevents
startup.
:::

:::warning
After a restart without cached replacement history, reconciliation can apply an older order's state or fields
to its replacement. This can make a working order appear canceled locally. Recovery remains incomplete. Check
replaced orders against AX before resuming trading.
:::

### Account state

The `/balances` endpoint carries no margin data, so account state also requests `/risk-snapshot`. Its USD
`initial_margin_required_total` populates the USD balance's locked funds, capped at the USD balance. A USD
`MarginBalance` entry pairs initial with maintenance margin. When `/risk-snapshot` fails, account state falls
back to balances-only with zero locked margin and a warning.

## Authentication

AX Exchange uses bearer token authentication:

1. API key and secret obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.
3. The adapter requests one-hour session tokens and refreshes them every 30 minutes.
4. A refresh updates REST authentication and the token used by the next WebSocket reconnect without
   interrupting the active connection.

## Configuration

### Environments and endpoints

| Environment | HTTP API                                         | HTTP API (orders)                                   | Market Data WS                                   | Orders WS                                            |
| ----------- | ------------------------------------------------ | --------------------------------------------------- | ------------------------------------------------ | ---------------------------------------------------- |
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `https://gateway.sandbox.architect.exchange/orders` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `https://gateway.architect.exchange/orders`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

:::info
Order management endpoints (place, cancel, replace, cancel-all, order status, open orders,
historical orders, and initial margin requirement) use the orders base URL. Every other REST
endpoint, including authentication, account state, fills, transactions, and market data, uses the
API base URL. The adapter resolves both from the configured environment.
:::

### Data client configuration options

| Option                             | Default   | Description                                                                     |
| ---------------------------------- | --------- | ------------------------------------------------------------------------------- |
| `api_key`                          | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.                         |
| `api_secret`                       | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.                   |
| `environment`                      | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                                |
| `base_url_http`                    | `None`    | Override for the REST base URL.                                                 |
| `base_url_ws_public`               | `None`    | Override for the market data WebSocket URL.                                     |
| `base_url_ws_private`              | `None`    | Override for the private orders WebSocket URL.                                  |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports.                           |
| `http_timeout_secs`                | `60`      | Timeout (seconds) for REST requests.                                            |
| `max_retries`                      | `3`       | Maximum retry attempts for idempotent REST requests (`GET`, `HEAD`, `OPTIONS`). |
| `retry_delay_initial_ms`           | `1,000`   | Initial delay (milliseconds) between retries.                                   |
| `retry_delay_max_ms`               | `10,000`  | Maximum delay (milliseconds) between retries (exponential backoff).             |
| `heartbeat_interval_secs`          | `20`      | Heartbeat interval (seconds) for WebSocket connections.                         |
| `recv_window_ms`                   | `5,000`   | Reserved; AX uses bearer tokens and the adapter sends no window.                |
| `update_instruments_interval_mins` | `60`      | Interval (minutes) between instrument catalog refreshes.                        |
| `funding_rate_poll_interval_mins`  | `15`      | Interval (minutes) between funding rate poll requests.                          |
| `book_snapshot_timeout_secs`       | `10`      | Initial, reconnect, and recovery book snapshot wait; `0` disables.              |
| `transport_backend`                | `Sockudo` | WebSocket transport backend.                                                    |

### Execution client configuration options

| Option                    | Default   | Description                                                                     |
| ------------------------- | --------- | ------------------------------------------------------------------------------- |
| `account_id`              | `AX-001`  | Account ID for the execution client.                                            |
| `api_key`                 | `None`    | API key; loaded from `AX_API_KEY` env var when omitted.                         |
| `api_secret`              | `None`    | API secret; loaded from `AX_API_SECRET` env var when omitted.                   |
| `environment`             | `SANDBOX` | Trading environment (`SANDBOX` or `PRODUCTION`).                                |
| `base_url_http`           | `None`    | Override for the API REST base URL.                                             |
| `base_url_orders`         | `None`    | Override for the orders REST base URL.                                          |
| `base_url_ws_private`     | `None`    | Override for the orders WebSocket URL.                                          |
| `proxy_url`               | `None`    | Optional proxy URL for HTTP and WebSocket transports.                           |
| `http_timeout_secs`       | `60`      | Timeout (seconds) for REST requests.                                            |
| `max_retries`             | `3`       | Maximum retry attempts for idempotent REST requests (`GET`, `HEAD`, `OPTIONS`). |
| `retry_delay_initial_ms`  | `1,000`   | Initial delay (milliseconds) between retries.                                   |
| `retry_delay_max_ms`      | `10,000`  | Maximum delay (milliseconds) between retries (exponential backoff).             |
| `heartbeat_interval_secs` | `30`      | Heartbeat interval (seconds) for WebSocket connections.                         |
| `recv_window_ms`          | `5,000`   | Reserved; AX uses bearer tokens and the adapter sends no window.                |
| `cancel_on_disconnect`    | `False`   | Cancel this WebSocket session's open orders on disconnect.                      |
| `transport_backend`       | `Sockudo` | WebSocket transport backend.                                                    |

When `transport_backend=None`, the compiled Rust default selects Sockudo when the
`transport-sockudo` Cargo feature is enabled and Tungstenite otherwise.

Use `AxDataClientConfig` with `AxDataClientFactory` and `AxExecutionClientConfig` with
`AxExecutionClientFactory`. The Python examples show the complete `LiveNode.builder(...)`
configuration for data and execution clients.

### API credentials

There are two options for supplying your credentials to the AX Exchange clients.
Either pass the corresponding `api_key` and `api_secret` values to the configuration objects, or
set the following environment variables:

- `AX_API_KEY`
- `AX_API_SECRET`

:::tip
We recommend using environment variables to manage your credentials.
:::

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

## Implementation notes

- **Whole contracts only**: AX uses integer contract quantities. The adapter models a one-contract
  size increment and lot size, while enforcing each instrument's separate `minimum_order_size`.
  Fractional quantities generate `OrderDenied` locally.
- **Dated futures activation**: AX publishes expiration but not activation timestamps. The adapter
  uses zero for the unknown activation time and preserves that limitation in instrument metadata.
- **Rate limiting**: The adapter applies a conservative rate limit of 10 requests/second with
  automatic exponential backoff on rate limit responses.
- **Market orders**: AX does not support native market orders. The adapter uses a preview endpoint
  to determine the take-through price and submits an aggressive IOC limit order. Because the book
  can move between the preview and the submission, a simulated market order may fill partially.
- **Stop-limit orders**: The adapter rejects venue-native stop-limit submissions because sandbox
  testing did not confirm conditional semantics. Use local order emulation when a strategy
  requires a stop-limit order.
- **Order modification**: AX supports atomic order replacement via `POST /replace-order`. The execution client
  maps `modify_order` to this endpoint. The HTTP response contains a proposed venue order ID; the orders
  stream confirms whether replacement succeeds. Queries and cancels retain the working order ID until
  the stream, a child fill, or a query confirms the replacement. A query of a replaced parent checks proposed IDs
  and returns the confirmed replacement's status, including fills or cancellation. It leaves the result
  unresolved when no replacement is confirmed. A cancel before confirmation targets the parent and
  can be rejected; recovery then needs a late stream confirmation, a child fill, or an explicit query.
  A crossing post-only replacement emits `OrderModifyRejected` with the venue reason
  and leaves the original order working. The adapter rejects a modification locally if it carries a
  trigger price, because AX has no field for one, or if the order has no venue order ID yet.
- **Funding rate polling**: The data client polls `GET /funding-rates` for each subscribed instrument at
  `funding_rate_poll_interval_mins`. It requests a seven-day lookback to find a rate across weekends and
  holidays. It emits the latest rate only when it differs from the last one emitted.
- **Cancel on disconnect**: Set `cancel_on_disconnect=True` in the execution client config
  to have the exchange cancel all open orders if the orders WebSocket disconnects.
- **Account fee rates**: Instruments do not carry maker or taker fee rates. An
  authenticated client resolves account rates from `GET /whoami` and fails to connect
  if that lookup fails. The execution client caches rates per AX account, apart from
  instruments, and refreshes them each time it connects. Automatic WebSocket reconnects
  keep the cached rates.
- **Fill commissions**: Real-time fill events from the WebSocket do not include fee data. The adapter
  estimates commission for both tracked fill events and untracked fill reports using the fill quantity,
  absolute fill price, contract multiplier, and account's maker or taker rate. It uses the trade's account ID,
  then the order's account ID if absent, or the first `/whoami` account if neither ID is present. It keeps the
  calculation in decimal arithmetic and rounds only the final fee into USD `Money`, whose precision is 2.

  A fill whose instrument or complete account fee schedule is not cached, or whose commission cannot be
  represented, is logged and skipped. Tracked order metadata remains available for reconciliation. Recovery
  depends on configured reconciliation checks or an explicit order query. Skipped fills do not trigger an
  immediate REST request.

  Reconciliation does not replace an applied commission, so a streamed estimate remains even when the
  venue-reported `/fills` fee differs. That endpoint supplies the fee for a fill that was not already applied
  from the stream. The adapter converts it with `Money::from_decimal` into USD, so a sub-cent fee such as
  `0.012188` is stored as `0.01`. An unrepresentable REST fee fails fill-report requests, historical
  order-status requests that need fills, and mass status. During startup, that error prevents the node
  from starting.
- **Fill reconciliation window**: The `/fills` endpoint requires a bounded time range and caps the span at
  seven days. Fill requests cover seven days by default, or the shorter configured lookback. Order history
  uses the configured lookback and has no start bound by default.

  Older order reports can still produce inferred fills with `commission=None`. Order history does not
  supply fees, and the adapter does not estimate historical fees from current account rates. Position
  accounting excludes unavailable fees, so reconstructed PnL for that history is incomplete. A reported
  zero fee remains a zero commission.
- **Fill order identity**: AX can omit `order_id` for block trades and final settlement fills. The
  adapter derives a deterministic reconciliation order ID from `trade_id` for those classified
  records and includes a matching `FILLED` market order report in startup mass status. Classification
  fields are optional for regular fills with a valid `order_id`. The adapter rejects rows with neither
  an order ID nor explicit special-fill classification, and rejects inconsistent classification.
- **Unfilled IOC/FOK**: AX reports an unfilled immediate order as an expiry. The adapter maps it to
  `OrderCanceled` to match NautilusTrader semantics.
- **One-tick quotes**: Example testers place post-only limits one tick from top of book.
  Those quotes can still fill. Flatten leftovers with
  `cargo run --bin ax-flatten -p nautilus-architect-ax` (`AX_IS_SANDBOX` defaults to true).
  That binary cancels all open orders on the account, then closes every position.

## Contributing

:::info
For additional features or to contribute to the AX Exchange adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
