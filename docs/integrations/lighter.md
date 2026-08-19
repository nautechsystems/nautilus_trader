# Lighter

[Lighter](https://lighter.xyz) is a decentralized central-limit-order-book exchange for spot and
perpetual futures. The venue settles through an Ethereum zero-knowledge rollup, while matching and
sequencing run off-chain.

The NautilusTrader Lighter adapter is implemented by the `nautilus-lighter` crate. It provides
Rust data and execution clients, typed REST and WebSocket models, and an in-tree L2 transaction
signer for the venue's Schnorr / ECgFp5 signing flow.

Measured L2 signing cost, including a comparison with the official Go SDK, is recorded in
[`crates/adapters/lighter/benches/BENCHMARKS.md`](../../crates/adapters/lighter/benches/BENCHMARKS.md).
Absolute numbers vary by machine, so only same-machine deltas are meaningful.

## Overview

The main components are:

- `LighterRawHttpClient`: low-level REST client for the public and account endpoints.
- `LighterHttpClient`: domain client which parses instruments, trades, books, orders, and account
  state into Nautilus model types.
- `LighterWebSocketClient`: reconnecting WebSocket client for public market and private account streams.
- `LighterDataClient`: Nautilus data client for instruments, trades, quotes, and L2 MBP books.
- `LighterExecutionClient`: Nautilus execution client for account streams, order submission,
  modification, cancellation, and reconciliation reports.
- `LighterDataClientFactory` and `LighterExecutionClientFactory`: live-node factory wiring.

The Python surface is intentionally narrow. The Python extension exposes configuration,
environment selection, factory classes, and integrator revocation; data and execution clients are
consumed through the Rust trait surface.

## Examples

Python examples live in
[`examples/live/lighter/`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/lighter/)
and default to a dry build. Pass `--run` to connect; the execution tester also requires
`--live-orders` to disable `dry_run`.

From the repository root:

```bash
.venv/bin/python examples/live/lighter/data_tester.py --lighter-environment testnet
.venv/bin/python examples/live/lighter/exec_tester.py --lighter-environment testnet
```

From the repository root, connect to mainnet with explicit instruments:

```bash
.venv/bin/python examples/live/lighter/data_tester.py \
    --lighter-environment mainnet \
    --instrument BTC-PERP.LIGHTER \
    --run
.venv/bin/python examples/live/lighter/exec_tester.py \
    --lighter-environment mainnet \
    --instrument DOGE-PERP.LIGHTER \
    --run
```

Rust examples live under `crates/adapters/lighter/examples/`. Both testers connect when run. The
execution tester has `DRY_RUN = false` in its source, so the command below can submit live mainnet
orders:

```bash
cargo run --example lighter-data-tester --package nautilus-lighter --features examples
cargo run --example lighter-exec-tester --package nautilus-lighter --features examples
```

:::warning
Examples can connect to live venues. Execution examples with live order flow enabled can submit
orders when pointed at a funded mainnet account. Review the selected instrument, quantity, and
environment before running them.
:::

For emergency account cleanup, `cargo run --bin lighter-flatten -p nautilus-lighter` cancels open
orders and closes positions for the configured Lighter account. It scans all registered markets, so
the standard 60 req/min REST quota can make the run take several minutes. Review the active account
and positions first because it is account-wide, not strategy-scoped.

## Product support

| Product type      | Data feed | Trading | Notes                                                        |
| ----------------- | --------- | ------- | ------------------------------------------------------------ |
| Spot              | ✓         | ✓       | Spot markets using Lighter market indexes 2048-4094.         |
| Perpetual futures | ✓         | ✓       | Linear perpetual markets using Lighter market indexes 0-254. |
| Dated futures     | -         | -       | *Not supported*.                                             |
| Options           | -         | -       | *Not supported*.                                             |

## Limitations

The current adapter scope is deliberately narrower than the venue's full transaction surface:

- Grouped order lists, OCO/OTO groups, brackets, TWAP, trailing stops, and iceberg display size are
  not implemented. Batch submit does not use `CreateGroupedOrders`.
- Order-list submit and batch cancel fan out independent transactions sequentially over WebSocket.
  Both operations are capped at 15 transactions per command.
- `CancelAllOrders` uses cached open orders for the requested instrument. The adapter does not use
  Lighter's native account-wide cancel-all transaction because it can affect unrelated markets.
- Spot trading supports market and limit orders. Conditional stop-loss and take-profit orders are
  limited to perpetual markets.
- Account state and position reports come from private WebSocket streams. `query_account` and
  position status generation replay the latest cached stream state.
- Unscoped order reconciliation is bounded to configured or observed active markets to avoid a full
  venue-wide fan-out under the standard REST quota.

## Symbology

Lighter identifies markets by numeric `market_index` values. The adapter bootstraps the mapping from
`GET /api/v1/orderBookDetails`, then converts the raw venue symbol into a Nautilus `InstrumentId`.

| Venue product     | Nautilus symbol format        | Example                 | Notes                        |
| ----------------- | ----------------------------- | ----------------------- | ---------------------------- |
| Perpetual futures | `{BASE}-PERP.LIGHTER`         | `BTC-PERP.LIGHTER`      | Raw venue symbol `BTC`.      |
| Spot              | `{BASE}/{QUOTE}-SPOT.LIGHTER` | `ETH/USDC-SPOT.LIGHTER` | Raw venue symbol `ETH/USDC`. |

The suffix separates spot and perpetual listings. Outbound requests strip it and use the cached
`market_index`; spot symbols retain the venue pair.

## Environments

| Environment | REST URL                              | WebSocket URL                              | Chain ID |
| ----------- | ------------------------------------- | ------------------------------------------ | -------- |
| Mainnet     | `https://mainnet.zklighter.elliot.ai` | `wss://mainnet.zklighter.elliot.ai/stream` | 304      |
| Testnet     | `https://testnet.zklighter.elliot.ai` | `wss://testnet.zklighter.elliot.ai/stream` | 300      |

Use `LighterEnvironment::Mainnet` or `LighterEnvironment::Testnet` in data and execution
configuration. URL overrides are available for private gateways or local test fixtures.

## Integrator attribution

Create and modify transactions carry the NautilusTrader integrator account index in
`L2TxAttributes` to measure adapter usage. Maker and taker integrator fees are zero. The execution
client submits the required **zero‑fee** `ApproveIntegrator` approval during startup when the API
key is not maker‑only.

Maker‑only API keys cannot submit `ApproveIntegrator`. The execution client detects these keys and
skips automatic approval. Approval is account‑scoped, so a non‑maker‑only key on the same account
must approve the integrator before a maker‑only key can trade through the adapter.

### Revoking the approval

Use revocation as cleanup when leaving the adapter. It sends `ApproveIntegrator` with
`approval_expiry = 0` and zero max fees. The next execution‑client startup with a non‑maker‑only
key records a new zero‑fee approval.

```bash
export LIGHTER_API_KEY_INDEX=5
export LIGHTER_API_SECRET=REPLACE_ME
export LIGHTER_ACCOUNT_INDEX=123456
cargo run -p nautilus-lighter --bin lighter-integrator-revoke           # mainnet
cargo run -p nautilus-lighter --bin lighter-integrator-revoke testnet   # testnet
```

Script source:
[`crates/adapters/lighter/bin/integrator_revoke.rs`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/lighter/bin/integrator_revoke.rs).

```python
# Python (PyO3 binding) - reads the same env vars as the Rust bin
from nautilus_trader.adapters.lighter import revoke_lighter_integrator
from nautilus_trader.adapters.lighter import LighterEnvironment

await revoke_lighter_integrator()  # mainnet (default)
await revoke_lighter_integrator(LighterEnvironment.TESTNET)  # testnet
```

The Rust script prints a summary of the action and pauses for an Enter keypress before signing or
sending; abort with `Ctrl+C` before that point if anything in the summary looks wrong. The Python
binding does not prompt: review the active env vars yourself before calling.

## Data subscriptions

| Data type            | Sub.         | Snapshot | Hist. | Nautilus type       | Notes                                                    |
| -------------------- | ------------ | -------- | ----- | ------------------- | -------------------------------------------------------- |
| Instrument metadata  | Cache replay | ✓        | -     | `InstrumentAny`     | Loaded from `orderBookDetails`.                          |
| Trade ticks          | ✓            | -        | ✓     | `TradeTick`         | WebSocket trades; public `recentTrades` REST history.    |
| Quote ticks          | ✓            | -        | -     | `QuoteTick`         | Best bid and ask ticker stream.                          |
| Order book deltas    | ✓            | ✓        | -     | `OrderBookDeltas`   | `L2_MBP` only.                                           |
| Order book depth10   | ✓            | -        | -     | `OrderBookDepth10`  | Live top-10 view from maintained book; no REST snapshot. |
| Order book snapshots | -            | ✓        | -     | `OrderBook`         | REST snapshot, max depth 250.                            |
| Mark prices          | ✓            | -        | -     | `MarkPriceUpdate`   | Perp market stats stream.                                |
| Index prices         | ✓            | -        | -     | `IndexPriceUpdate`  | Market and spot stats streams.                           |
| Funding rates        | ✓            | -        | ✓     | `FundingRateUpdate` | Current estimates and REST hourly history.               |
| Bars                 | ✓            | -        | ✓     | `Bar`               | WebSocket candle stream; REST history for backfill.      |
| Instrument status    | REST         | ✓        | -     | `InstrumentStatus`  | `active` / `inactive` snapshots.                         |

Only `BookType::L2_MBP` is accepted for book-delta and depth10 subscriptions. Other book types
return an error before subscribing.

The WebSocket order book initializes only from `subscribed/order_book`. If an `update/order_book`
arrives before that snapshot, the adapter drops it and waits for the real snapshot because
incremental updates do not contain the full visible book.

Depth10 subscriptions use the same WebSocket `order_book` stream as deltas. The adapter emits a
refreshed top-10 view after each accepted snapshot or incremental update.

Bar subscriptions use the venue's `candle/{market_id}/{resolution}` WebSocket channel. Lighter
batches in-progress updates for the open bar every ~500 ms; the adapter emits a Nautilus `Bar`
only when the candle start timestamp advances, so consumers see one event per closed period. The
in-progress cache is cleared on reconnect and on unsubscribe.

The stream supports `1m`, `5m`, `15m`, `30m`, `1h`, `4h`, `12h`, and `1d`. `1w` is REST-only via
`request_bars`; subscribing to a `1-WEEK` bar type returns an error.

REST bar history omits venue gap rows whose open, high, low, or close is missing, null, zero, or
negative. These rows cannot form valid Nautilus bars and do not stop later valid rows from loading.

Instrument status subscriptions replay the latest cached `orderBookDetails` status when available
and otherwise fetch a REST snapshot. Lighter does not expose a WebSocket status-change stream.

See [Funding rates](#funding-rates) for live and historical funding semantics.

Trade subscriptions use the public WebSocket trade stream. Historical trade requests use the
public `/api/v1/recentTrades` endpoint, which needs no credentials; the adapter clamps the
request to the venue per-call cap and filters the returned ticks to the requested time range.

### Unsupported data requests

`request_quotes` is not implemented. Lighter exposes best bid and offer data through the
WebSocket `ticker` stream, but the REST endpoints available to the adapter do not provide a
timestamped quote snapshot or quote history that can map safely to `QuoteTick`.

`request_book_depth` is not implemented. The documented REST book endpoints do not provide a
venue event timestamp for `OrderBookDepth10.ts_event`; use `subscribe_book_depth10` for a live
depth10 stream or `request_book_snapshot` for a REST `OrderBook` snapshot.

## Orders capability

### Order identification

Lighter uses a numeric venue order index and a caller-supplied `client_order_index`.
The adapter derives a 31‑bit index from the Nautilus `ClientOrderId` and probes forward on a
collision. Because the collision‑probed value cannot be re‑derived after restart, order
reconciliation resolves each raw venue order ID through the core cache and restores its actual
`client_order_index` before translating order and fill reports. Open cached orders return to active
tracking, while terminal orders use bounded replay tracking.

Recovery never infers a client order ID from the integer alone: the cached venue order ID must
match. It requires reconciliation to include the order and the core cache to retain its
venue‑order‑ID mapping; otherwise reports use the unique venue order ID as their external client
order ID.

Query paths use the numeric venue order ID for active or terminal history. Before that ID is known,
a Nautilus client order ID can query active orders by its derived client index. Client-index-only
queries do not search terminal history, and duplicate active matches fail as ambiguous.

### Order types

| Order type             | Perpetuals | Spot | Notes                                                   |
| ---------------------- | ---------- | ---- | ------------------------------------------------------- |
| `MARKET`               | ✓          | ✓    | Cap derived from cached far‑side quote + slippage.      |
| `LIMIT`                | ✓          | ✓    | Requires a limit price.                                 |
| `STOP_MARKET`          | ✓          | -    | Perp only; cap derived from `trigger_price` + slippage. |
| `STOP_LIMIT`           | ✓          | -    | Perp only; maps to Lighter stop‑loss limit orders.      |
| `MARKET_IF_TOUCHED`    | ✓          | -    | Perp only; cap derived from `trigger_price` + slippage. |
| `LIMIT_IF_TOUCHED`     | ✓          | -    | Perp only; maps to Lighter take‑profit limit orders.    |
| `MARKET_TO_LIMIT`      | -          | -    | *Not supported*.                                        |
| `TRAILING_STOP_MARKET` | -          | -    | *Not supported*.                                        |
| `TRAILING_STOP_LIMIT`  | -          | -    | *Not supported*.                                        |
| `TWAP`                 | -          | -    | *Not supported*; no Nautilus mapping.                   |

Conditional orders require `trigger_price`. The adapter rejects missing triggers for `STOP_MARKET`
and `MARKET_IF_TOUCHED`, any trigger that truncates to `0` ticks at the instrument's price
precision, and spot conditionals that Lighter does not support.

Lighter requires a worst‑acceptable `price` for market‑style orders. The adapter starts from the
cached far‑side `QuoteTick` for `MARKET`, or `trigger_price` for `STOP_MARKET` and
`MARKET_IF_TOUCHED`, then applies `market_order_slippage_bps` (default 50 bps) and rounds at the
instrument's price precision, up for buys or down for sells. A `MARKET` order without a cached
quote is denied. Override the slippage with `SubmitOrder.params["market_order_slippage_bps"]`.

### Contingent orders

| Feature               | Perpetuals | Spot | Notes                                              |
| --------------------- | ---------- | ---- | -------------------------------------------------- |
| Stop‑loss market      | ✓          | -    | `STOP_MARKET` maps to Lighter `STOP_LOSS`.         |
| Stop‑loss limit       | ✓          | -    | `STOP_LIMIT` maps to Lighter `STOP_LOSS_LIMIT`.    |
| Take‑profit market    | ✓          | -    | `MARKET_IF_TOUCHED` maps to Lighter `TAKE_PROFIT`. |
| Take‑profit limit     | ✓          | -    | `LIMIT_IF_TOUCHED` maps to `TAKE_PROFIT_LIMIT`.    |
| Trigger price         | ✓          | -    | Required for every supported conditional order.    |
| Trigger price type    | -          | -    | *Not supported*; no trigger source selector.       |
| Grouped order lists   | -          | -    | *Not supported*.                                   |
| OCO / OTO orders      | -          | -    | *Not supported*.                                   |
| Bracket orders        | -          | -    | *Not supported*.                                   |
| `CreateGroupedOrders` | -          | -    | *Not supported*; order lists use independent txs.  |

### Order options

| Option           | Perpetuals | Spot | Notes                                                                     |
| ---------------- | ---------- | ---- | ------------------------------------------------------------------------- |
| `post_only`      | ✓          | ✓    | Maps to Lighter's post‑only time‑in‑force.                                |
| `reduce_only`    | ✓          | -    | Passed through to `CreateOrder`; use only to reduce an existing position. |
| `quote_quantity` | -          | -    | *Not supported*; submit base quantity instead.                            |
| `display_qty`    | -          | -    | *Not supported*; Lighter exposes no iceberg display quantity field.       |

### Adapter order params

| Param                                      | Perpetuals | Spot | Notes                                               |
| ------------------------------------------ | ---------- | ---- | --------------------------------------------------- |
| `market_order_slippage_bps`                | ✓          | ✓    | Overrides the config default for market‑style caps. |
| `post_only` through `SubmitOrder.params`   | -          | -    | *Not supported*; use the Nautilus order flag.       |
| `reduce_only` through `SubmitOrder.params` | -          | -    | *Not supported*; use the Nautilus order flag.       |

### Time in force

| Time in force  | Perpetuals | Spot | Notes                                                                         |
| -------------- | ---------- | ---- | ----------------------------------------------------------------------------- |
| `GTC`          | ✓          | ✓    | Limit‑style uses `GoodTillTime`; market‑style uses `IOC`.                     |
| `DAY`          | ✓          | ✓    | Limit‑style and conditional orders use a positive order expiry.               |
| `GTD`          | ✓          | ✓    | Supplied expiry must be 5 minutes to 30 days from submission.                 |
| `IOC`          | ✓          | ✓    | Plain `MARKET`/`LIMIT` use expiry `0`; conditional limit uses trigger expiry. |
| `FOK`          | -          | -    | *Not supported*.                                                              |
| `AT_THE_OPEN`  | -          | -    | *Not supported*.                                                              |
| `AT_THE_CLOSE` | -          | -    | *Not supported*.                                                              |

The adapter sends `MARKET`, `STOP_MARKET`, and `MARKET_IF_TOUCHED` as Lighter
`ImmediateOrCancel`; the venue rejects market‑style `GoodTillTime` orders. Plain `MARKET` uses
`OrderExpiry = 0`, while conditional market orders keep a positive expiry until triggered.
The adapter denies Nautilus `IOC` for conditional market orders because Lighter reserves IOC for
post‑trigger execution. Conditional limit orders can use `IOC`: their trigger rests with a positive
expiry, then the child uses `ImmediateOrCancel`.

Without an explicit GTD expiry, limit‑style `GTC`, `DAY`, and `GTD` orders default to the current
time plus 28 days; conditional `GTC`, `DAY`, and limit‑style `IOC` use the same default. Lighter
rejects `-1` and accepts expiries from 5 minutes to 30 days after submission. The adapter enforces
that window with a one‑second signing and transport margin, so an expiry of exactly 5 minutes is
denied locally before signing; tester configurations expressed in whole minutes should use at least
6 minutes.

### Execution instructions

| Instruction   | Perpetuals | Spot | Notes                                                     |
| ------------- | ---------- | ---- | --------------------------------------------------------- |
| `post_only`   | ✓          | ✓    | Overrides the TIF and sends Lighter `PostOnly`.           |
| `reduce_only` | ✓          | -    | Position‑reducing flag for existing derivative positions. |

Use `post_only` on limit-style orders. The adapter does not synthesize maker-only market orders.
Live mainnet testing confirms `reduce_only=true` for closing perpetual positions. Invalid
reduce-only opens can be dropped by Lighter without a venue order report; the adapter reconciles
them as `INFLIGHT_TIMEOUT` rather than a venue-supplied rejection reason.

### Advanced order features

| Feature            | Perpetuals | Spot | Notes                                                      |
| ------------------ | ---------- | ---- | ---------------------------------------------------------- |
| Order modification | ✓          | ✓    | Modify quantity, price, and trigger price on a live order. |
| Bracket orders     | -          | -    | *Not supported*.                                           |
| Iceberg orders     | -          | -    | *Not supported*.                                           |
| Trailing stops     | -          | -    | *Not supported*.                                           |
| Pegged orders      | -          | -    | *Not supported*.                                           |
| TWAP orders        | -          | -    | *Not supported*; no Nautilus mapping.                      |
| Leverage update    | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.           |
| Native cancel‑all  | -          | -    | *Not supported*; adapter scopes cancel‑all per instrument. |
| Dead man's switch  | -          | -    | *Not supported*.                                           |

### Order operations

| Operation           | Perpetuals | Spot | Notes                                                          |
| ------------------- | ---------- | ---- | -------------------------------------------------------------- |
| Submit order        | ✓          | ✓    | Sends a signed `L2CreateOrder` transaction over WebSocket.     |
| Submit order list   | ✓          | ✓    | Sequential fanout of up to 15 independent create transactions. |
| Modify order        | ✓          | ✓    | Sends a signed `ModifyOrder`; reports may restate accepts.     |
| Cancel order        | ✓          | ✓    | Sends a signed `L2CancelOrder` transaction.                    |
| Cancel all orders   | ✓          | ✓    | Iterates cached open orders for the requested instrument.      |
| Set leverage        | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.               |
| Batch cancel orders | ✓          | ✓    | Sequential fanout of up to 15 independent cancel transactions. |
| Query order         | ✓          | ✓    | Requires credentials and REST lookup.                          |
| Query account       | ✓          | ✓    | Replays the latest private WebSocket account state.            |
| Mass status         | ✓          | ✓    | Bounded to account‑active markets from WS and REST reports.    |

`SubmitOrderList` and `BatchCancelOrders` sign and hand off each child transaction in order through
the hash-correlated WebSocket `sendTx` path. The adapter allocates each nonce only after the prior
child handoff completes. Each transaction therefore receives normal acknowledgement, rejection,
and nonce recovery handling. Fanout is not atomic: it does not create grouped venue orders or
provide OCO/OTO or bracket semantics.

`UpdateLeverage` is exposed as `LighterExecutionClient::update_leverage(instrument_id,
initial_margin_fraction, margin_mode)`. The `initial_margin_fraction` is in venue ticks
(1e-4 fraction): `500` is 5% initial margin (20x leverage), `1000` is 10% (10x), and so on.

`UpdateLeverage`, `CancelAllOrders`, modify orders with integrator attributes, and conditional
create orders are byte‑pinned against the signer distributed with the official `lighter-python`
SDK version 1.1.2.

### Order querying and reconciliation

| Feature              | Perpetuals | Spot | Notes                                                        |
| -------------------- | ---------- | ---- | ------------------------------------------------------------ |
| Query open orders    | ✓          | ✓    | REST `accountActiveOrders` scoped by market.                 |
| Query order history  | ✓          | ✓    | REST `accountInactiveOrders` with cursor pagination.         |
| Order status updates | ✓          | ✓    | Private WebSocket order streams plus status reports.         |
| Trade history        | ✓          | ✓    | REST `trades`; credentials are required for account history. |
| Fill reports         | ✓          | ✓    | REST and private WebSocket trade payloads.                   |
| Position reports     | ✓          | -    | Perp only; replays cached position stream.                   |
| Account state        | ✓          | ✓    | Replays the cached merged account state snapshot.            |
| Mass status          | ✓          | ✓    | Combines orders, fills, and cached positions.                |

Authenticated inactive-order and fill pagination rejects repeated cursors and stops after 1,000
pages. Fill reconciliation remains repeatable across calls while suppressing fills already emitted
from the live WebSocket stream. Historical order and fill reports bind a mapped client index only
to its matching venue order ID so reused numeric indexes cannot merge unrelated lifecycles.

Each bounded mass status captures one cutoff for its inactive orders and fills. The adapter marks
the report set complete only when the required order, fill, and position sources succeed and every
historical fill maps to its order. If a historical source fails, active orders remain available for
reconciliation while historical fills follow the engine's
[order‑only projection](../concepts/execution.md#orderonly-fill-projection) rules.

The `trades` endpoint retains only the most recent 3,000 trades per `account_index`, so a bounded
lookback can request more fill history than the venue serves. Pagination walks back from the newest
trade, and only a trade older than the lookback start proves the window was served:

- Trade older than the start: the report set stays complete.
- Cursor exhausted first: the adapter logs the uncovered span and marks the report set incomplete.
- No retained trades: nothing can have been truncated, so the report set stays complete.

An exhausted cursor cannot distinguish truncation from an account with no older trades, so a young
account reports incomplete even though nothing is missing. Choose a lookback the venue can serve.
The `export` endpoint serves full trade history for auditing fills the lookback cannot cover, and
the adapter does not read it.

A strategy that opens a position immediately on start can trigger a transient position-check
discrepancy warning (`cached=0, venue=N`) when the venue's `account_all_positions` frame arrives a
few milliseconds before the matching fill event is processed. The warning self-resolves once the
fill applies; no reconciliation orders are generated.

## Account and position management

Authenticated execution clients subscribe to these private streams:

- `account_all_orders`: order status reports.
- `account_all_trades`: fill reports.
- `account_all_positions`: initial position snapshot and live updates.
- `account_all_assets`: per-asset balance snapshots (spot balance plus perp collateral).
- `user_stats`: perp-account margin rollup (collateral and available balance).

The adapter merges `account_all_assets` and `user_stats` into a single account state and emits it
only after both streams have delivered their first frame.

The execution client requires credentials before connecting because private account streams and
nonce refresh are mandatory. A client can be constructed without credentials, but live execution
will not connect until `private_key`, `account_index`, and `api_key_index` resolve.

Perpetual positions use netting mode with one position per market; spot balances use account asset
state. A `subscribed/account_all_positions` frame is an authoritative snapshot: omitted markets and
rows with a zero `position` value flatten cached positions, and an empty `positions` map flattens the
entire cache. Cached positions for rows the adapter cannot map or parse are retained, so they do not
cause false flat reports.

For bounded reconciliation, the adapter also records which markets the current connection's
snapshot covers. A reconnect invalidates that coverage. An absent touched market produces an
explicit flat report only after a current snapshot covers it; an unmapped or malformed row leaves
the mass status incomplete instead of proving flat.

An `update/account_all_positions` frame is incremental. Non‑zero rows replace the cached position for
their market, explicit zero rows flatten that market, and omitted markets remain cached. An empty
update retains all cached positions.

| Feature                 | Perpetuals | Spot | Notes                                                        |
| ----------------------- | ---------- | ---- | ------------------------------------------------------------ |
| Account balances        | ✓          | ✓    | Merged assets + `user_stats`, replayed from cache on query.  |
| Position state          | ✓          | -    | Perp only; initial snapshot plus live updates.               |
| Netting positions       | ✓          | -    | One Nautilus position per perpetual market.                  |
| Cross margin            | ✓          | -    | Passed through `LighterPositionMarginMode::Cross`.           |
| Isolated margin         | ✓          | -    | Passed through `LighterPositionMarginMode::Isolated`.        |
| Leverage updates        | ✓          | -    | Signed `UpdateLeverage` transaction.                         |
| Spot margin / borrowing | -          | -    | *Not supported*.                                             |
| Deposits / withdrawals  | -          | -    | Use venue tools or Lighter APIs outside the trading adapter. |

## Liquidation and ADL handling

| Event or field              | Support | Notes                                                         |
| --------------------------- | ------- | ------------------------------------------------------------- |
| Liquidation trades          | ✓       | Account trade rows can parse as fills, with no special event. |
| Deleverage trades           | ✓       | Account trade rows can parse as fills, with no special event. |
| Liquidation price reporting | -       | *Not supported*; reports omit this field.                     |
| ADL event stream            | -       | *Not supported*.                                              |

## Funding rates

Perpetual `market_stats` frames emit `MarkPriceUpdate`, `IndexPriceUpdate`, and
`FundingRateUpdate`. The live funding update uses `current_funding_rate` as the upcoming estimate;
`funding_rate` and `funding_timestamp` describe the last completed payment. Because market stats
provide no future settlement time, live updates leave `interval` and `next_funding_ns` unset. Spot
`spot_market_stats` frames emit `IndexPriceUpdate`.

Historical requests use public `/api/v1/fundings` rows at `1h` resolution and set `interval=60`.
`direction=long` stays positive, while `short` becomes negative. Pagination covers the requested
range up to the adapter's page cap, subject to an explicit `limit`; see
[Rate limiting](#rate-limiting). Account‑specific `positionFunding` is not used.

## Account tiers

Lighter account tiers set latency, rate limits, and fees. The execution client reads the tier from
`GET /api/v1/account` and logs it, including unknown raw `account_type` values. It does not raise
limits automatically because a local quota override does not grant a higher venue limit.

| Tier     | Latency (maker / taker) | REST weighted limit | `sendTx` limit       | Fees (maker / taker)      | Notes                                   |
| -------- | ----------------------- | ------------------- | -------------------- | ------------------------- | --------------------------------------- |
| Standard | 200 ms / 300 ms         | 60 req/min          | 60 req/min           | 0 / 0                     | Zero‑fee default tier.                  |
| Premium  | 0 ms / 140-200 ms       | 24,000 req/min      | 4,000-40,000 req/min | 0.28-0.40 / 1.96-2.80 bps | Lowest latency; scales with staked LIT. |
| Plus     | 200 ms / 300 ms         | 120,000 req/min     | 8,000 req/min        | 0.5 / 0.5 bps             | Raised limits, standard latency.        |
| Builder  | -                       | 240,000 req/min     | -                    | -                         | Highest REST throughput.                |

Premium figures scale with staked LIT and can change. Before raising a local quota, confirm that
Lighter applies the matching tier limit to the client's traffic, then set the quota explicitly
(see [Rate limiting](#rate-limiting)).

## Rate limiting

Lighter limits both IP and L1 addresses. Each data and execution client owns a separate REST
limiter and defaults to the standard‑account quota. Configure their combined traffic within the
venue limit.

Higher [account tiers](#account-tiers) still require explicit client quotas:

- `rest_quota_per_min`: REST read‑bucket quota in requests per minute. Unset keeps 60 req/min.
  Available on both the data and execution clients.
- `sendtx_quota_per_min`: transaction quota in requests per minute, metered in a bucket separate
  from reads. Unset keeps it at the standard 60 req/min, independent of `rest_quota_per_min`.
  Execution client only.

These options change local pacing only. Public data requests remain unauthenticated, so setting a
higher local quota does not make those requests eligible for an account‑level venue limit.

### L1-address transaction limit

The venue also enforces a 40 req/min limit per L1 address on transaction traffic, below the
default `sendtx_quota_per_min` of 60. A mainnet quoting session amending on every quote drift hit
`code=23000` (`Too Many Requests`) after roughly 40 modify transactions in a minute; see
[Volume quota and no-fill quoting](#volume-quota-and-no-fill-quoting) for the related quota that
modify transactions also spend. Set `sendtx_quota_per_min` to 40 or lower for transaction‑heavy
quoting workloads. The limiter is shared across all `sendTx` traffic, so a lower quota also paces
creates and cancels.

The REST limiter counts one token per call rather than venue endpoint weights. Set
`rest_quota_per_min` for the effective endpoint mix: a 24,000 weighted req/min premium limit yields
40 calls/minute to endpoints with weight 600, such as `/api/v1/trades` and
`/api/v1/recentTrades`.

The venue meters transactions per account across both transports in one bucket. The execution
client enforces `sendtx_quota_per_min` with a single shared limiter across WebSocket `sendTx`
(including order-list and cancel fanout) and the HTTP `sendTx` used for startup integrator
approval. Low‑level raw `sendTx` and `sendTxBatch` calls use that limiter when the client is
constructed with it; otherwise, they fall back to the raw client's REST limiter.

The clients share one WebSocket message limiter per venue URL. It paces non‑transaction control
frames at 200 messages/minute across both clients. A closed‑loop subscription gate caps
unacknowledged requests at 35, below the venue's 50‑message per‑IP ceiling; this count depends on
acknowledgement latency, not send rate. `sendTx` does not count against the client‑message bucket.

| Scope                                | Venue limit                 | Adapter behavior                                     |
| ------------------------------------ | --------------------------- | ---------------------------------------------------- |
| REST, standard account               | 60 req/min                  | Default; set `rest_quota_per_min` to override.       |
| REST, premium account                | 24,000 weighted req/min     | Local override required; venue attribution applies.  |
| REST, plus account                   | 120,000 weighted req/min    | Local override required; venue attribution applies.  |
| REST, builder account                | 240,000 weighted req/min    | Local override required; venue attribution applies.  |
| `sendTx` / `sendTxBatch`, standard   | 60 req/min                  | Execution orders use WebSocket `sendTx`.             |
| `sendTx` / `sendTxBatch`, plus       | 8,000 req/min               | Set `sendtx_quota_per_min` to use it.                |
| `sendTx` / `sendTxBatch`, premium    | 4,000-40,000 req/min        | Set `sendtx_quota_per_min` (scales with staked LIT). |
| Default transaction type limit       | 40 req/min                  | Applies to tx types not covered by volume quota.     |
| `L2UpdateLeverage` transaction limit | 40 req/min                  | Relevant to `update_leverage`.                       |
| Pending orders                       | 500/account, 16/market      | Venue limit; adapter does not pre‑count it.          |
| Active orders                        | 1,500/account, 1,000/market | Venue limit; adapter does not pre‑count it.          |

Common REST endpoint weights from the official docs:

| Endpoint group                       | Weight | Adapter behavior                                |
| ------------------------------------ | ------ | ----------------------------------------------- |
| `sendTx`, `sendTxBatch`, `nextNonce` | 6      | Tx calls use tx limiter; `nextNonce` uses REST. |
| `accountInactiveOrders`              | 100    | Adapter counts one REST token per HTTP call.    |
| `trades`, `recentTrades`             | 600    | Adapter counts one REST token per HTTP call.    |
| Other endpoints                      | 300    | Adapter counts one REST token per HTTP call.    |

| Endpoint or transport                  | Limit      | Notes                                                |
| -------------------------------------- | ---------- | ---------------------------------------------------- |
| `/api/v1/trades`                       | 100 rows   | Adapter paginates reconciliation at this cap.        |
| `/api/v1/accountInactiveOrders`        | 100 rows   | Adapter follows `next_cursor` at this cap.           |
| `/api/v1/orderBookOrders`              | 250 levels | Snapshot depth is clamped to the venue cap.          |
| `/api/v1/candles`                      | 500 rows   | Adapter caps REST bar pages at this venue maximum.   |
| `/api/v1/fundings`                     | 100 rows   | Adapter paginates funding pages at this venue cap.   |
| WebSocket connections                  | 255 / IP   | Venue limit.                                         |
| WebSocket subscriptions / connection   | 500        | Venue limit.                                         |
| WebSocket unique accounts / connection | 500        | Venue limit.                                         |
| WebSocket connections / minute         | 255        | Venue limit.                                         |
| WebSocket client messages / minute     | 200        | Paces non‑tx frames; heartbeat pings bypass it.      |
| WebSocket inflight messages            | 50         | Venue cap; subscriptions use a 35-frame closed loop. |
| WebSocket `sendTxBatch` batch size     | 15 txs     | Venue limit; adapter fanout is also capped at 15.    |
| WebSocket keepalive                    | 2 minutes  | Adapter sends heartbeats every 30 seconds.           |
| WebSocket outbound command queue       | Not capped | Paced before writes; no queue‑depth cap.             |

Historical bar and funding‑rate requests stop after 500 REST pages. This covers up to 250,000 bars
or 49,500 hourly funding intervals. If the cap leaves part of the requested range uncovered, the
HTTP client returns `LighterHttpError::HistoryIncomplete` instead of partial history and does not
retry the capped request. Completion on the final allowed page remains successful. A request with
an explicit `start` also remains successful when its explicit `limit` is satisfied. The data client
logs the incomplete error and emits no response; narrow the requested range to continue.

## Volume quota and no-fill quoting

Volume quota is separate from transport limits. `L2CreateOrder`, `L2CancelAllOrders`,
`L2ModifyOrder`, and `L2CreateGroupedOrders` spend it; completed volume and any free allowance
replenish it. The adapter does not inspect remaining quota. See Lighter's
[Volume Quota](https://apidocs.lighter.xyz/docs/volume-quota-program) documentation for current
rules and figures.

Repeated no‑fill quote refreshes can exhaust this quota even when the WebSocket and `sendTx`
limiters work. For live tests, prefer slower one‑sided quoting, wider refresh thresholds, testnet,
or a bounded strategy that earns enough fills to replenish its quota.

## Connection management

The WebSocket client sends heartbeats every 30 seconds and reconnects with exponential backoff from
250 milliseconds to 30 seconds. It treats a connection carrying no inbound frame for 90 seconds as
dead and reconnects, which recovers a stalled socket that the venue never closes. The venue answers
each heartbeat with a pong, so a healthy connection refreshes that window even when no market data
flows. Private subscriptions use auth tokens with an 8‑hour maximum TTL;
the adapter mints 7‑hour tokens, rotates them every 6 hours, and resubscribes. A transparent
reconnect triggers a fresh token and account resubscription after tracked subscriptions start
replaying.

On execution reconnect, the adapter starts a nonce‑baseline refresh through
`GET /api/v1/nextNonce`. New signed transaction dispatch is rejected until that refresh, or its
background retry, installs the replacement connection's nonce baseline.

Within a session, venue confirmations advance the local nonce window, definitive rejections or
pre‑handoff failures may roll back its latest nonce, and stale state triggers a
`GET /api/v1/nextNonce` resync. Outcomes that may have reached the venue retain their pending nonce
and order identity for WebSocket or reconciliation recovery.

`LighterExecutionClient::connect()` waits up to 30 seconds for every account stream
(`account_all_orders`, `account_all_trades`, `account_all_positions`, `account_all_assets`,
`user_stats`) to satisfy its readiness condition. For positions, only the
`subscribed/account_all_positions` snapshot satisfies this wait; a live update does not. The adapter
does not use REST account payloads as a fallback, so `connect()` blocks on these streams as its ground
truth. Each attempt clears old position and account caches before awaiting the session's frames.
Transparent WebSocket reconnects and auth‑token rotations do not re‑enter `connect()`. Both retain
cached positions until the next `subscribed/account_all_positions` frame applies the snapshot
replacement rules. Live update frames merge into the retained cache without evicting omitted
markets.

## API credentials

Lighter signing requires all three credential values:

- Account index: numeric Lighter account identifier.
- API key index: numeric API key slot. Lighter reserves indexes `0-3`; use a user‑created key in the
  `4-254` range. Do not use `255`; it is an `apikeys` query sentinel, not a signing key.
- API private key: 40-byte hex private key, with or without a `0x` prefix.

Config values take precedence. A missing config field, or a blank API private key (empty or
whitespace only), falls back to the corresponding environment variable for the selected
environment.

| Environment | API key index                   | API private key              | Account index                   |
| ----------- | ------------------------------- | ---------------------------- | ------------------------------- |
| Mainnet     | `LIGHTER_API_KEY_INDEX`         | `LIGHTER_API_SECRET`         | `LIGHTER_ACCOUNT_INDEX`         |
| Testnet     | `LIGHTER_TESTNET_API_KEY_INDEX` | `LIGHTER_TESTNET_API_SECRET` | `LIGHTER_TESTNET_ACCOUNT_INDEX` |

Execution rejects incomplete credentials. The data client runs without credentials: its
subscriptions and REST requests (instruments, book, trades, bars, funding) all use public
endpoints.

## Configuration

### Data client configuration options

| Option                             | Default   | Description                                              |
| ---------------------------------- | --------- | -------------------------------------------------------- |
| `base_url_http`                    | `None`    | Optional REST URL override.                              |
| `base_url_ws`                      | `None`    | Optional WebSocket URL override.                         |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket.               |
| `environment`                      | `Mainnet` | `LighterEnvironment::Mainnet` or `Testnet`.              |
| `account_index`                    | `None`    | Optional factory field; public data calls do not use it. |
| `api_key_index`                    | `None`    | Optional factory field; public data calls do not use it. |
| `private_key`                      | `None`    | Optional factory field; public data calls do not use it. |
| `http_timeout_secs`                | `60`      | HTTP request timeout in seconds.                         |
| `ws_timeout_secs`                  | `30`      | WebSocket connection and reconnection timeout.           |
| `update_instruments_interval_mins` | `60`      | Instrument metadata refresh interval in minutes.         |
| `rest_quota_per_min`               | `None`    | REST quota override; unset keeps 60 req/min.             |
| `transport_backend`                | Default   | WebSocket transport backend.                             |

### Execution client configuration options

| Option                      | Default       | Description                                              |
| --------------------------- | ------------- | -------------------------------------------------------- |
| `trader_id`                 | `TRADER-001`  | Nautilus trader identifier.                              |
| `account_id`                | `LIGHTER-001` | Nautilus account identifier for the venue.               |
| `account_index`             | `None`        | Lighter account index.                                   |
| `api_key_index`             | `None`        | Lighter API key slot.                                    |
| `private_key`               | `None`        | Hex private key for auth and L2 transaction signing.     |
| `base_url_http`             | `None`        | Optional REST URL override.                              |
| `base_url_ws`               | `None`        | Optional WebSocket URL override.                         |
| `proxy_url`                 | `None`        | Optional proxy URL for HTTP and WebSocket.               |
| `environment`               | `Mainnet`     | `LighterEnvironment::Mainnet` or `Testnet`.              |
| `http_timeout_secs`         | `60`          | HTTP request timeout in seconds.                         |
| `ws_timeout_secs`           | `30`          | WebSocket connection and reconnection timeout.           |
| `market_order_slippage_bps` | `50`          | Slippage cap (bps) for `MARKET` / `STOP_MARKET` / `MIT`. |
| `rest_quota_per_min`        | `None`        | REST quota override; unset keeps 60 req/min.             |
| `sendtx_quota_per_min`      | `None`        | Transaction quota override; unset keeps 60 req/min.      |
| `transport_backend`         | Default       | WebSocket transport backend.                             |

### Configuration example

```rust
use nautilus_lighter::{
    common::enums::LighterEnvironment,
    config::{LighterDataClientConfig, LighterExecClientConfig},
};
use nautilus_model::identifiers::{AccountId, TraderId};

let data_config = LighterDataClientConfig::builder()
    .environment(LighterEnvironment::Testnet)
    .build();

let exec_config = LighterExecClientConfig::builder()
    .trader_id(TraderId::from("TRADER-001"))
    .account_id(AccountId::from("LIGHTER-001"))
    .environment(LighterEnvironment::Testnet)
    .build();
```

The execution config resolves credentials from the matching testnet environment variables; set its
credential fields directly to override them. Use
`LiveExecEngineConfig.reconciliation_instrument_ids` to scope reconciliation and
`reconciliation_lookback_mins` to bound inactive order and fill replay.

## Official documentation

- [Get started](https://apidocs.lighter.xyz/docs/get-started)
- [Trading and signing](https://apidocs.lighter.xyz/docs/trading)
- [API keys](https://apidocs.lighter.xyz/docs/api-keys)
- [Account types](https://apidocs.lighter.xyz/docs/account-types)
- [Rate limits](https://apidocs.lighter.xyz/docs/rate-limits)
- [Volume quota](https://apidocs.lighter.xyz/docs/volume-quota-program)
- [Data structures, constants, and errors](https://apidocs.lighter.xyz/docs/data-structures-constants-and-errors)
- [REST OpenAPI](https://raw.githubusercontent.com/elliottech/lighter-python/main/openapi.json)
- [WebSocket reference](https://apidocs.lighter.xyz/docs/websocket-reference)

## Contributing

:::info
For additional features or to contribute to the Lighter adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
