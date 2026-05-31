# Lighter

[Lighter](https://lighter.xyz) is a decentralized central-limit-order-book exchange for spot and
perpetual futures. The venue settles through an Ethereum zero-knowledge rollup, while matching and
sequencing run off-chain.

The NautilusTrader Lighter adapter is implemented by the `nautilus-lighter` crate. It provides
Rust data and execution clients, typed REST and WebSocket models, and an in-tree L2 transaction
signer for the venue's Schnorr / ECgFp5 signing flow.

## Overview

The adapter consists of the following main components:

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

The adapter includes Python v2 and Rust live-node examples. The Python examples live in
[`python/examples/lighter/`](https://github.com/nautechsystems/nautilus_trader/tree/develop/python/examples/lighter/)
and default to a dry build: they build the node, register the tester, and exit unless `--run` is
passed.

```fish
cd python
.venv/bin/python examples/lighter/data_tester.py --lighter-environment testnet
.venv/bin/python examples/lighter/exec_tester.py --lighter-environment testnet
```

Pass `--run` to connect to Lighter. The execution tester remains in `dry_run` mode unless
`--live-orders` is also passed.

```fish
cd python
.venv/bin/python examples/lighter/data_tester.py \
    --lighter-environment mainnet \
    --instrument BTC-PERP.LIGHTER \
    --run
.venv/bin/python examples/lighter/exec_tester.py \
    --lighter-environment mainnet \
    --instrument DOGE-PERP.LIGHTER \
    --run
```

Rust examples live under `crates/adapters/lighter/examples/` and run immediately:

```fish
cargo run --example lighter-data-tester --package nautilus-lighter --features examples
cargo run --example lighter-exec-tester --package nautilus-lighter --features examples
```

:::warning
Examples can connect to live venues. Execution examples with live order flow enabled can submit
orders when pointed at a funded mainnet account. Review the selected instrument, quantity, and
environment before running them.
:::

## Product support

| Product type      | Data feed | Trading | Notes                                                        |
|-------------------|-----------|---------|--------------------------------------------------------------|
| Spot              | ✓         | ✓       | Spot markets using Lighter market indexes 2048-4094.         |
| Perpetual futures | ✓         | ✓       | Linear perpetual markets using Lighter market indexes 0-254. |
| Dated futures     | -         | -       | *Not supported*.                                             |
| Options           | -         | -       | *Not supported*.                                             |

## Limitations

The current adapter scope is deliberately narrower than the venue's full transaction surface:

- Grouped order lists, OCO/OTO groups, bracket orders, TWAP, trailing stops, and iceberg display
  size are not implemented.
- Native batch submit and native batch cancel are wired for independent order operations only.
  Batch submit sends independent `L2CreateOrder` txs in one `sendTxBatch`, and batch cancel signs
  independent `L2CancelOrder` txs in one `sendTxBatch`. Both are capped at 15 txs per batch.
- Grouped venue orders remain out of scope: batch submit does not use `CreateGroupedOrders` and
  does not provide atomic OCO/OTO or bracket grouping.
- `CancelAllOrders` uses cached open orders for the requested instrument. The adapter does not use
  Lighter's native account-wide cancel-all transaction because it can affect unrelated markets.
- Spot trading supports market and limit orders. Conditional stop-loss and take-profit orders are
  limited to perpetual markets.
- Account state and position reports come from private WebSocket streams. `query_account` and
  position status generation replay the latest cached stream state.
- Unscoped order reconciliation is bounded to configured or observed active markets to avoid a full
  venue-wide fan-out under the standard REST quota.
- Historical trade requests require credentials for master accounts and sub-accounts.

## Symbology

Lighter identifies markets by numeric `market_index` values. The adapter bootstraps the mapping from
`GET /api/v1/orderBookDetails`, then converts the raw venue symbol into a Nautilus `InstrumentId`.

| Venue product      | Nautilus symbol format | Example            | Notes                   |
|--------------------|------------------------|--------------------|-------------------------|
| Perpetual futures  | `{BASE}-PERP.LIGHTER`  | `BTC-PERP.LIGHTER` | Raw venue symbol `BTC`. |
| Spot               | `{BASE}-SPOT.LIGHTER`  | `ETH-SPOT.LIGHTER` | Raw venue symbol `ETH`. |

The suffix disambiguates spot and perpetual listings that share the same venue symbol. Outbound
requests strip the suffix and use the cached `market_index`.

## Environments

| Environment | REST URL                              | WebSocket URL                              | Chain ID |
|-------------|---------------------------------------|--------------------------------------------|----------|
| Mainnet     | `https://mainnet.zklighter.elliot.ai` | `wss://mainnet.zklighter.elliot.ai/stream` | 304      |
| Testnet     | `https://testnet.zklighter.elliot.ai` | `wss://testnet.zklighter.elliot.ai/stream` | 300      |

Use `LighterEnvironment::Mainnet` or `LighterEnvironment::Testnet` in data and execution
configuration. URL overrides are available for private gateways or local test fixtures.

## Integrator attribution

Submitted create and modify order transactions carry the NautilusTrader integrator account index in
Lighter's `L2TxAttributes`. This helps us gauge real usage of the integration and prioritize
ongoing maintenance. Maker and taker integrator fees are set to zero, so attribution adds no trading
cost.

Lighter requires an `ApproveIntegrator` approval before these attributes can be attached to orders.
During startup, the execution client submits the required **zero-fee** approval for the configured
L2 account.

### Revoking the approval

Use revocation as cleanup when leaving the adapter. It sends the same `ApproveIntegrator` tx with
`approval_expiry = 0` and every max fee set to zero; the next execution-client startup records a
fresh zero-fee approval.

```bash
export LIGHTER_API_KEY_INDEX=0
export LIGHTER_API_SECRET=REPLACE_ME
export LIGHTER_ACCOUNT_INDEX=123456
cargo run -p nautilus-lighter --bin lighter-integrator-revoke           # mainnet
cargo run -p nautilus-lighter --bin lighter-integrator-revoke testnet   # testnet
```

Script source:
[`crates/adapters/lighter/bin/integrator_revoke.rs`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/lighter/bin/integrator_revoke.rs).

```python
# Python (PyO3 binding) - reads the same env vars as the Rust bin
from nautilus_trader.core.nautilus_pyo3 import revoke_lighter_integrator
from nautilus_trader.core.nautilus_pyo3 import LighterEnvironment

await revoke_lighter_integrator()                            # mainnet (default)
await revoke_lighter_integrator(LighterEnvironment.TESTNET)  # testnet
```

The Rust script prints a summary of the action and pauses for an Enter keypress before signing or
sending; abort with `Ctrl+C` before that point if anything in the summary looks wrong. The Python
binding does not prompt: review the active env vars yourself before calling.

## Data subscriptions

| Data type            | Sub.         | Snapshot | Hist. | Nautilus type       | Notes                                                  |
|----------------------|--------------|----------|-------|---------------------|--------------------------------------------------------|
| Instrument metadata  | Cache replay | ✓        | -     | `InstrumentAny`     | Loaded from `orderBookDetails`.                        |
| Trade ticks          | ✓            | -        | ✓     | `TradeTick`         | WebSocket trades; historical REST trades require auth. |
| Quote ticks          | ✓            | -        | -     | `QuoteTick`         | Best bid and ask ticker stream.                        |
| Order book deltas    | ✓            | ✓        | -     | `OrderBookDeltas`   | `L2_MBP` only.                                         |
| Order book depth10   | ✓            | ✓        | -     | `OrderBookDepth10`  | Full WebSocket book snapshots.                         |
| Order book snapshots | -            | ✓        | -     | `OrderBook`         | REST snapshot, max depth 250.                          |
| Mark prices          | ✓            | -        | -     | `MarkPriceUpdate`   | Perp market stats stream.                              |
| Index prices         | ✓            | -        | -     | `IndexPriceUpdate`  | Market and spot stats streams.                         |
| Funding rates        | ✓            | -        | ✓     | `FundingRateUpdate` | Current estimates and REST hourly history.             |
| Bars                 | ✓            | -        | ✓     | `Bar`               | WebSocket candle stream; REST history for backfill.    |
| Instrument status    | REST         | ✓        | -     | `InstrumentStatus`  | `active` / `inactive` snapshots.                       |

Only `BookType::L2_MBP` is accepted for book-delta and depth10 subscriptions. Other book types
return an error before subscribing.

The WebSocket order book initializes only from `subscribed/order_book`. If an `update/order_book`
arrives before that snapshot, the adapter drops it and waits for the real snapshot because
incremental updates do not contain the full visible book.

Bar subscriptions use the venue's `candle/{market_id}/{resolution}` WebSocket channel. Lighter
batches in-progress updates for the open bar every ~500 ms; the adapter emits a Nautilus `Bar`
only when the candle start timestamp advances, so consumers see one event per closed period. The
in-progress cache is cleared on reconnect and on unsubscribe.

The stream supports `1m`, `5m`, `15m`, `30m`, `1h`, `4h`, `12h`, and `1d`. `1w` is REST-only via
`request_bars`; subscribing to a `1-WEEK` bar type returns an error.

Instrument status subscriptions replay the latest cached `orderBookDetails` status when available
and otherwise fetch a REST snapshot. Lighter does not expose a WebSocket status-change stream.

Funding-rate subscriptions use `market_stats.current_funding_rate`, which is Lighter's estimate
for the upcoming payment. Historical funding-rate requests use `/api/v1/fundings` at `1h`
resolution and map settled rows to `FundingRateUpdate` with `interval=60`. The REST `direction`
field controls the sign: `long` stays positive because longs pay shorts, while `short` is mapped
negative because shorts pay longs. The adapter does not use account-specific `positionFunding`
payloads for public funding history.

Trade subscriptions use the public WebSocket trade stream. Historical trade requests use
`/api/v1/trades`; live mainnet testing shows that endpoint rejects unauthenticated requests. The
data client mints a Lighter auth token for this request when credentials are available. Without
credentials, the data client logs a warning and rejects the request.

### Unsupported data requests

`request_quotes` is not implemented. Lighter exposes best bid and offer data through the
WebSocket `ticker` stream, but the REST endpoints available to the adapter do not provide a
timestamped quote snapshot or quote history that can map safely to `QuoteTick`.

`request_book_depth` is not implemented. The documented REST book endpoints do not provide a
venue event timestamp for `OrderBookDepth10.ts_event`; use `subscribe_book_depth10` for live
depth10 snapshots or `request_book_snapshot` for a REST `OrderBook` snapshot.

## Orders capability

### Order identification

Lighter uses a numeric venue order index and a caller-supplied `client_order_index`.
The adapter derives the Lighter `client_order_index` from the Nautilus `ClientOrderId` and keeps a
local map so private WebSocket reports can recover the original client order ID.

Query paths can use either the Nautilus client order ID or the numeric venue order ID when the
required mapping is available.

### Order types

| Order type             | Perpetuals | Spot | Notes                                                   |
|------------------------|------------|------|---------------------------------------------------------|
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

Conditional order types are available for perpetual markets only. Spot conditional orders are
denied locally because Lighter rejects them at the venue. Conditional order types must include a
`trigger_price`. `STOP_MARKET` and `MARKET_IF_TOUCHED` are denied upfront if the trigger is
missing, and all conditional types are denied if the trigger truncates to `0` ticks at the
instrument's price precision.

Lighter's market-style orders require a worst-acceptable `price` field on the wire. The adapter
derives it automatically: `MARKET` orders read the cached far-side `QuoteTick` (ask for buys,
bid for sells); `STOP_MARKET` and `MARKET_IF_TOUCHED` use the order's `trigger_price`. The base
is widened by `market_order_slippage_bps` (default 50 bps = 0.5%), rounded conservatively at the
instrument's price precision (ceil for buys, floor for sells). A `MARKET` order submitted before
the strategy has subscribed to quotes is denied with a clear error. Override per order via
`SubmitOrder.params["market_order_slippage_bps"]`.

### Contingent orders

| Feature                         | Perpetuals | Spot | Notes                                                  |
|---------------------------------|------------|------|--------------------------------------------------------|
| Stop‑loss market                | ✓          | -    | `STOP_MARKET` maps to Lighter `STOP_LOSS`.             |
| Stop‑loss limit                 | ✓          | -    | `STOP_LIMIT` maps to Lighter `STOP_LOSS_LIMIT`.        |
| Take‑profit market              | ✓          | -    | `MARKET_IF_TOUCHED` maps to Lighter `TAKE_PROFIT`.     |
| Take‑profit limit               | ✓          | -    | `LIMIT_IF_TOUCHED` maps to `TAKE_PROFIT_LIMIT`.        |
| Trigger price                   | ✓          | -    | Required for every supported conditional order.        |
| Trigger price type              | -          | -    | *Not supported*; no trigger source selector.           |
| Grouped order lists             | -          | -    | *Not supported*.                                       |
| OCO / OTO orders                | -          | -    | *Not supported*.                                       |
| Bracket orders                  | -          | -    | *Not supported*.                                       |
| `CreateGroupedOrders`           | -          | -    | *Not supported*; native batches use independent txs.   |

### Order options

| Option           | Perpetuals | Spot | Notes                                                                      |
|------------------|------------|------|----------------------------------------------------------------------------|
| `post_only`      | ✓          | ✓    | Maps to Lighter's post‑only time‑in‑force.                                 |
| `reduce_only`    | ✓          | -    | Passed through to `CreateOrder`; use on derivatives only.                  |
| `quote_quantity` | -          | -    | *Not supported*; submit base quantity instead.                             |
| `display_qty`    | -          | -    | *Not supported*; Lighter exposes no iceberg display quantity field.        |

### Adapter order params

| Param                                      | Perpetuals | Spot | Notes                                               |
|--------------------------------------------|------------|------|-----------------------------------------------------|
| `market_order_slippage_bps`                | ✓          | ✓    | Overrides the config default for market‑style caps. |
| `post_only` through `SubmitOrder.params`   | -          | -    | *Not supported*; use the Nautilus order flag.       |
| `reduce_only` through `SubmitOrder.params` | -          | -    | *Not supported*; use the Nautilus order flag.       |

### Time in force

| Time in force  | Perpetuals | Spot | Notes                                                                        |
|----------------|------------|------|------------------------------------------------------------------------------|
| `GTC`          | ✓          | ✓    | Limit‑style uses `GoodTillTime`; market‑style uses `IOC`.                    |
| `DAY`          | ✓          | ✓    | Limit‑style and conditional orders use a positive order expiry.              |
| `GTD`          | ✓          | ✓    | Limit‑style and conditional orders use the supplied Nautilus expiry.         |
| `IOC`          | ✓          | ✓    | Plain `MARKET`/`LIMIT` use expiry `0`; conditional limit uses trigger expiry. |
| `FOK`          | -          | -    | *Not supported*.                                                            |
| `AT_THE_OPEN`  | -          | -    | *Not supported*.                                                            |
| `AT_THE_CLOSE` | -          | -    | *Not supported*.                                                            |

For `MARKET`, `STOP_MARKET`, and `MARKET_IF_TOUCHED`, the adapter maps the wire
time-in-force to Lighter `ImmediateOrCancel` because the venue rejects market-style orders sent as
`GoodTillTime`. Plain `MARKET` orders set `OrderExpiry = 0`. Conditional market orders
(`STOP_MARKET` and `MARKET_IF_TOUCHED`) keep a positive `OrderExpiry` so the trigger can rest,
and the wire `ImmediateOrCancel` applies only after the trigger fires. Nautilus `IOC` cannot be
represented for conditional market orders, so the adapter denies it locally with a clear error.
Conditional limit orders (`STOP_LIMIT` and `LIMIT_IF_TOUCHED`) can use Nautilus `IOC`: the trigger
rests with a positive `OrderExpiry`, and the child limit order uses Lighter `ImmediateOrCancel`
after the trigger fires.

When no explicit GTD expiry is supplied, limit-style `GTC`, `DAY`, and `GTD` orders default to
the current time plus 28 days. Conditional `GTC`, `DAY`, and limit-style `IOC` orders use the
same default expiry. The venue rejects `-1` as an invalid expiry for these TIFs. Live testing has
also shown that very short GTD expiries can be rejected by the sequencer with
`21711 invalid expiry`; use a venue-accepted expiry horizon for live GTD tests.

### Execution instructions

| Instruction   | Perpetuals | Spot | Notes                                           |
|---------------|------------|------|-------------------------------------------------|
| `post_only`   | ✓          | ✓    | Overrides the TIF and sends Lighter `PostOnly`. |
| `reduce_only` | ✓          | -    | Position‑reducing flag for derivatives.         |

Use `post_only` on limit-style orders. The adapter does not synthesize maker-only market orders.

### Advanced order features

| Feature              | Perpetuals | Spot | Notes                                                       |
|----------------------|------------|------|-------------------------------------------------------------|
| Order modification   | ✓          | ✓    | Modify quantity, price, and trigger price on a live order.  |
| Bracket orders       | -          | -    | *Not supported*.                                            |
| Iceberg orders       | -          | -    | *Not supported*.                                            |
| Trailing stops       | -          | -    | *Not supported*.                                            |
| Pegged orders        | -          | -    | *Not supported*.                                            |
| TWAP orders          | -          | -    | *Not supported*; no Nautilus mapping.                       |
| Leverage update      | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.            |
| Native cancel‑all    | -          | -    | *Not supported*; adapter scopes cancel‑all per instrument.  |
| Dead man's switch    | -          | -    | *Not supported*.                                            |

### Order operations

| Operation           | Perpetuals | Spot | Notes                                                       |
|---------------------|------------|------|-------------------------------------------------------------|
| Submit order        | ✓          | ✓    | Sends a signed `L2CreateOrder` transaction over WebSocket.  |
| Submit order list   | ✓          | ✓    | Batches independent `L2CreateOrder` txs only.               |
| Modify order        | ✓          | ✓    | Sends a signed `ModifyOrder`; reports may restate accepts.  |
| Cancel order        | ✓          | ✓    | Sends a signed `L2CancelOrder` transaction.                 |
| Cancel all orders   | ✓          | ✓    | Iterates cached open orders for the requested instrument.   |
| Set leverage        | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.            |
| Batch cancel orders | ✓          | ✓    | Batches independent `L2CancelOrder` txs only.               |
| Native batch submit | ✓          | ✓    | Uses one `sendTxBatch`, capped at 15 create txs.            |
| Native batch cancel | ✓          | ✓    | Uses one `sendTxBatch`, capped at 15 cancel txs.            |
| Query order         | ✓          | ✓    | Requires credentials and REST lookup.                       |
| Query account       | ✓          | ✓    | Replays the latest private WebSocket account state.         |
| Mass status         | ✓          | ✓    | Bounded to account‑active markets from WS and REST reports. |

The native venue `CancelAllOrders` transaction is account-wide. The adapter deliberately cancels
cached open orders per instrument to avoid touching unrelated markets.

`SubmitOrderList` and `BatchCancelOrders` use `sendTxBatch` for independent operations. They do
not create grouped venue orders, do not provide atomic OCO/OTO or bracket semantics, and do not
use account-wide `CancelAllOrders` for scoped cancels.

The `sendTxBatch` response exposes one top-level API code and a `tx_hash` list; it does not expose
per-order API rejection fields. A successful batch response queues the signed transactions, then
private account streams report the final per-order submit, cancel, fill, and reject outcomes.

`UpdateLeverage` is exposed as `LighterExecutionClient::update_leverage(instrument_id,
initial_margin_fraction, margin_mode)`. The `initial_margin_fraction` is in venue ticks
(1e-4 fraction): `500` is 5% initial margin (20x leverage), `1000` is 10% (10x), and so on.

`UpdateLeverage` has no oracle test vectors in this repo; the body field order is pinned
against the cgo header from the upstream signer, and the wire format was verified by
submitting a signed tx to Lighter mainnet that the sequencer accepted.

### Order querying and reconciliation

| Feature              | Perpetuals | Spot | Notes                                                        |
|----------------------|------------|------|--------------------------------------------------------------|
| Query open orders    | ✓          | ✓    | REST `accountActiveOrders` scoped by market.                 |
| Query order history  | ✓          | ✓    | REST `accountInactiveOrders` with cursor pagination.         |
| Order status updates | ✓          | ✓    | Private WebSocket order streams plus status reports.         |
| Trade history        | ✓          | ✓    | REST `trades`; credentials are required for account history. |
| Fill reports         | ✓          | ✓    | REST and private WebSocket trade payloads.                   |
| Position reports     | ✓          | -    | Perp only; replays cached position stream.                   |
| Account state        | ✓          | ✓    | Replays the cached `account_all_assets` stream.              |
| Mass status          | ✓          | ✓    | Combines orders, fills, and cached positions.                |

## Account and position management

Authenticated execution clients subscribe to these private streams:

- `account_all_orders`: order status reports.
- `account_all_trades`: fill reports.
- `account_all_positions`: position snapshots.
- `account_all_assets`: account balance and margin snapshots.

The execution client requires credentials before connecting because private account streams and
nonce refresh are mandatory. A client can be constructed without credentials, but live execution
will not connect until `private_key`, `account_index`, and `api_key_index` resolve.

Perpetual positions are reported in netting mode: one position per market. Spot balances arrive
through account asset state rather than position reports.

| Feature                 | Perpetuals | Spot | Notes                                                        |
|-------------------------|------------|------|--------------------------------------------------------------|
| Account balances        | ✓          | ✓    | `account_all_assets` stream, replayed from cache on query.   |
| Position snapshots      | ✓          | -    | Perp only; `account_all_positions` stream.                   |
| Netting positions       | ✓          | -    | One Nautilus position per perpetual market.                  |
| Cross margin            | ✓          | -    | Passed through `LighterPositionMarginMode::Cross`.           |
| Isolated margin         | ✓          | -    | Passed through `LighterPositionMarginMode::Isolated`.        |
| Leverage updates        | ✓          | -    | Signed `UpdateLeverage` transaction.                         |
| Spot margin / borrowing | -          | -    | *Not supported*.                                            |
| Deposits / withdrawals  | -          | -    | Use venue tools or Lighter APIs outside the trading adapter. |

## Liquidation and ADL handling

| Event or field              | Support | Notes                                                             |
|-----------------------------|---------|-------------------------------------------------------------------|
| Liquidation trades          | ✓       | Account trade rows can parse as fills, with no special event.     |
| Deleverage trades           | ✓       | Account trade rows can parse as fills, with no special event.     |
| Liquidation price reporting | -       | *Not supported*; reports omit this field.                         |
| ADL event stream            | -       | *Not supported*.                                                  |

## Funding rates

Perpetual `market_stats` frames emit `MarkPriceUpdate`, `IndexPriceUpdate`, and
`FundingRateUpdate` events. Spot `spot_market_stats` frames emit `IndexPriceUpdate` events.

Historical funding-rate requests use the public `/api/v1/fundings` endpoint and emit
`FundingRateUpdate` responses for settled hourly rows.

## Rate limiting

Lighter applies rate limits to both IP address and L1 address. The adapter uses the conservative
standard-account REST quota by default, even when credentials belong to an account tier with higher
venue limits.

| Scope                                | Venue limit                 | Adapter behavior                                      |
|--------------------------------------|-----------------------------|-------------------------------------------------------|
| REST, standard account               | 60 req/min                  | HTTP client quota is fixed at this default.           |
| REST, premium account                | 24,000 weighted req/min     | Not auto‑detected; adapter still uses 60 req/min.     |
| REST, plus account                   | 120,000 weighted req/min    | Not auto‑detected; adapter still uses 60 req/min.     |
| REST, builder account                | 240,000 weighted req/min    | Not auto‑detected; adapter still uses 60 req/min.     |
| `sendTx` / `sendTxBatch`, standard   | 60 req/min                  | Singles use `sendTx`; batches use `sendTxBatch`.       |
| `sendTx` / `sendTxBatch`, plus       | 8,000 req/min               | Higher tiers still need external config planning.     |
| `sendTx` / `sendTxBatch`, premium    | 4,000-40,000 req/min        | Higher tiers still need external config planning.     |
| Default transaction type limit       | 40 req/min                  | Applies to tx types not covered by volume quota.      |
| `L2UpdateLeverage` transaction limit | 40 req/min                  | Relevant to `update_leverage`.                        |
| Pending orders                       | 500/account, 16/market      | Venue limit; adapter does not pre‑count it.           |
| Active orders                        | 1,500/account, 1,000/market | Venue limit; adapter does not pre‑count it.           |

| Endpoint or transport                  | Limit      | Notes                                                    |
|----------------------------------------|------------|----------------------------------------------------------|
| `/api/v1/trades`                       | 100 rows   | Adapter paginates reconciliation at this cap.            |
| `/api/v1/accountInactiveOrders`        | 100 rows   | Adapter follows `next_cursor` at this cap.               |
| `/api/v1/orderBookOrders`              | 250 levels | Snapshot depth is clamped to the venue cap.              |
| `/api/v1/candles`                      | 500 rows   | Adapter caps REST bar pages at this venue maximum.       |
| WebSocket connections                  | 200 / IP   | Venue limit.                                             |
| WebSocket subscriptions / connection   | 500        | Venue limit.                                             |
| WebSocket unique accounts / connection | 500        | Venue limit.                                             |
| WebSocket connections / minute         | 80         | Venue limit.                                             |
| WebSocket client messages / minute     | 200        | Excludes `sendTx` and `sendTxBatch`.                     |
| WebSocket inflight messages            | 50         | Excludes `sendTx` and `sendTxBatch`.                     |
| `sendTxBatch` batch size               | 15 txs     | Applies to native HTTP submit and cancel batches.        |
| WebSocket keepalive                    | 2 minutes  | Adapter sends heartbeats every 30 seconds.               |
| WebSocket outbound command queue       | 1000       | Adapter backpressure starts at this queue depth.         |

Premium volume quota is a separate venue constraint for `L2CreateOrder`, `L2CancelAllOrders`,
`L2ModifyOrder`, and `L2CreateGroupedOrders`. The adapter does not inspect remaining quota; use
venue account tools if a strategy depends on premium or plus limits.

## Connection management

The WebSocket client sends heartbeats every 30 seconds and reconnects with exponential backoff from
250 milliseconds up to 30 seconds. Private account subscriptions use Lighter auth tokens with an
8-hour maximum TTL; the adapter refreshes tokens 15 minutes before expiry and resubscribes account
channels.

On execution reconnect, the adapter refreshes the nonce baseline through `GET /api/v1/nextNonce`
before it resumes signed transaction dispatch.

`LighterExecutionClient::connect()` waits up to 30 seconds for every account stream
(`account_all_orders`, `account_all_trades`, `account_all_positions`, `account_all_assets`) to
deliver its first frame before returning. Lighter has no REST endpoint for account or position
state, so the WebSocket frames are the only ground truth: returning earlier would let strategies
race the venue's initial state and find the venue order id lookup table or position cache empty.
The gate clears any prior-session position and account caches at the start of each connect attempt
so a reconnect cycle observes the new session's frames, not stale data.

## API credentials

Lighter signing requires all three credential values:

- Account index: numeric Lighter account identifier.
- API key index: numeric API key slot, `0..=254`. Indices `0..=3` are reserved for Lighter
  desktop/mobile clients.
- API private key: 40-byte hex private key, with or without a `0x` prefix.

Config values take precedence. When config fields are omitted, the adapter reads environment
variables based on the selected environment.

| Environment | API key index                   | API private key              | Account index                    |
|-------------|---------------------------------|------------------------------|----------------------------------|
| Mainnet     | `LIGHTER_API_KEY_INDEX`         | `LIGHTER_API_SECRET`         | `LIGHTER_ACCOUNT_INDEX`          |
| Testnet     | `LIGHTER_TESTNET_API_KEY_INDEX` | `LIGHTER_TESTNET_API_SECRET` | `LIGHTER_TESTNET_ACCOUNT_INDEX`  |

Execution rejects incomplete credentials. The data client can run without credentials for public
streams and public REST endpoints; authenticated data requests such as `request_trades` use the
same values when all three are available.

## Configuration

### Data client configuration options

| Option                             | Default   | Description                                          |
|------------------------------------|-----------|------------------------------------------------------|
| `base_url_http`                    | `None`    | Optional REST URL override.                          |
| `base_url_ws`                      | `None`    | Optional WebSocket URL override.                     |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket.           |
| `environment`                      | `Mainnet` | `LighterEnvironment::Mainnet` or `Testnet`.          |
| `account_index`                    | `None`    | Lighter account index for authenticated REST data.   |
| `api_key_index`                    | `None`    | Lighter API key slot for authenticated REST data.    |
| `private_key`                      | `None`    | Hex private key for REST auth tokens.                |
| `http_timeout_secs`                | `60`      | HTTP request timeout in seconds.                     |
| `ws_timeout_secs`                  | `30`      | WebSocket connect timeout in seconds.                |
| `update_instruments_interval_mins` | `60`      | Instrument metadata refresh interval in minutes.     |
| `transport_backend`                | Default   | WebSocket transport backend.                         |

### Execution client configuration options

| Option                      | Default   | Description                                                |
|-----------------------------|-----------|------------------------------------------------------------|
| `trader_id`                 | Required  | Nautilus trader identifier.                                |
| `account_id`                | Required  | Nautilus account identifier for the venue.                 |
| `account_index`             | `None`    | Lighter account index.                                     |
| `api_key_index`             | `None`    | Lighter API key slot.                                      |
| `private_key`               | `None`    | Hex private key for auth and L2 transaction signing.       |
| `base_url_http`             | `None`    | Optional REST URL override.                                |
| `base_url_ws`               | `None`    | Optional WebSocket URL override.                           |
| `proxy_url`                 | `None`    | Optional proxy URL for HTTP and WebSocket.                 |
| `environment`               | `Mainnet` | `LighterEnvironment::Mainnet` or `Testnet`.                |
| `http_timeout_secs`         | `60`      | HTTP request timeout in seconds.                           |
| `ws_timeout_secs`           | `30`      | WebSocket connect timeout in seconds.                      |
| `active_markets`            | `[]`      | Lighter market IDs to poll during unscoped reconciliation. |
| `market_order_slippage_bps` | `50`      | Slippage cap (bps) for `MARKET` / `STOP_MARKET` / `MIT`.   |
| `transport_backend`         | Default   | WebSocket transport backend.                               |

### Configuration example

```rust
use nautilus_lighter::{
    common::enums::LighterEnvironment,
    config::{LighterDataClientConfig, LighterExecClientConfig},
};

let data_config = LighterDataClientConfig {
    environment: LighterEnvironment::Testnet,
    ..Default::default()
};

let exec_config = LighterExecClientConfig::builder()
    .trader_id(trader_id)
    .account_id(account_id)
    .environment(LighterEnvironment::Testnet)
    .active_markets(vec![0])
    .build();
```

The execution config above resolves credentials from the matching testnet environment variables.
Set `account_index`, `api_key_index`, and `private_key` directly to override environment lookup.
Set `active_markets` to the venue market IDs that should be checked for open orders during
cold-start reconciliation.

## Official documentation

- Trading and signing: <https://apidocs.lighter.xyz/docs/trading>
- API keys: <https://apidocs.lighter.xyz/docs/api-keys>
- Rate limits: <https://apidocs.lighter.xyz/docs/rate-limits>
- Volume quota: <https://apidocs.lighter.xyz/docs/volume-quota-program>
- Data structures, constants, and errors: <https://apidocs.lighter.xyz/docs/data-structures-constants-and-errors>
- REST OpenAPI: <https://raw.githubusercontent.com/elliottech/lighter-python/main/openapi.json>
- WebSocket reference: <https://apidocs.lighter.xyz/docs/websocket-reference>

## Contributing

:::info
For additional features or to contribute to the Lighter adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
