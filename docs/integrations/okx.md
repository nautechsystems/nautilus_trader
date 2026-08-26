# OKX

Founded in 2017, OKX is a cryptocurrency exchange that offers spot, margin, perpetual
swap, futures, options, spread, and event contract trading. This integration supports
live market data ingest and order execution on OKX.

## Overview

This adapter is implemented in Rust and exposed to Python through PyO3 bindings. It does not
require external OKX client libraries.

The OKX adapter includes multiple components, which can be used separately or together:

- `OKXHttpClient`: Low-level HTTP API connectivity.
- `OKXWebSocketClient`: Low-level WebSocket API connectivity.
- `OKXDataClient`: Market data feed manager.
- `OKXExecutionClient`: Account management and trade execution gateway.
- `OKXDataClientFactory`: Factory for OKX data clients.
- `OKXExecutionClientFactory`: Factory for OKX execution clients.

:::note
Most users will define a configuration for a live trading node (as shown below),
and won't need to work directly with these lower-level components.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/okx/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/okx/examples/)

## Product support

| Product         | Instrument source            | Data | Exec | Notes                                        |
| --------------- | ---------------------------- | ---- | ---- | -------------------------------------------- |
| Spot            | `public/instruments`         | Yes  | Yes  | Spot trading pairs.                          |
| Margin          | `public/instruments`         | Yes  | Yes  | Spot instruments with margin or leverage.    |
| Perpetual swaps | `public/instruments`         | Yes  | Yes  | Linear and inverse contracts.                |
| Futures         | `public/instruments`         | Yes  | Yes  | Dated futures contracts.                     |
| Options         | `public/instruments`         | Yes  | Yes  | Limit-style orders; requires family filters. |
| Spreads         | `sprd/spreads`               | Yes  | Yes  | Snapshots, quotes, trades on business WS.    |
| Event contracts | `event-contract/*` endpoints | Yes  | Yes  | Parsed as Nautilus `BinaryOption`.           |

Relevant OKX docs:

- [Get instruments](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-instruments).
- [Get limit price](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-limit-price).
- [Get Spreads (Public)](https://www.okx.com/docs-v5/en/#spread-trading-rest-api-get-spreads-public).
- [Spread trading place order](https://www.okx.com/docs-v5/en/#spread-trading-rest-api-place-order).
- [Event contract series](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-series).

:::note
**Options support**: The adapter supports options market data, venue-provided Greeks
(`subscribe_option_greeks`), and order execution for options instruments. See the
[Options trading](#options-trading) section below for details and the
[Options](../concepts/options.md) guide for subscription patterns.
:::

:::info
**Instrument multipliers**: For derivatives (`SWAP`, `FUTURES`, `OPTION`), instrument
multipliers are calculated as the product of OKX's `ctMult` and `ctVal` fields. This
keeps position sizing aligned with OKX contract size and value.
:::

:::info
**Price limits**: OKX exposes `initPxLmtPct`, `floatPxLmtPct`, and `maxPxLmtPct`
on `public/instruments` for spot, margin, swap, and futures instruments. The adapter
preserves non-empty values in the instrument `info` field as `okx_init_px_lmt_pct`,
`okx_float_px_lmt_pct`, and `okx_max_px_lmt_pct`. These fields describe exchange
band percentages, so they are not parsed as static Nautilus `min_price` or `max_price`
values.

Use `OKXHttpClient.request_price_limit(instrument_id)` when you need the current computed
buy and sell limits from OKX's `GET /api/v5/public/price-limit` endpoint. OKX documents
the percentage fields as empty for options and event contracts; the adapter leaves their
instrument `info` unchanged.
:::

:::note
OKX finance-product endpoints such as `/api/v5/finance/okusd/*` are outside the OKX
trading adapter surface.
:::

## Instrument updates

The data client loads its instrument cache over REST at connect and subscribes to the OKX
instruments WebSocket channel for each configured instrument type. All update paths honor
the configured instrument types, families, and contract types, and unchanged definitions
are never republished.

| Source              | Trigger                                         | Publishes downstream                                   |
| ------------------- | ----------------------------------------------- | ------------------------------------------------------ |
| Connect load        | REST at connect                                 | Full cache, once                                       |
| Instruments channel | Venue push (incremental)                        | New or changed definitions, `InstrumentStatus` on each |
| REST reconciliation | `update_instruments_interval_mins` (default 60) | New or changed definitions only                        |

Each update first writes the data client, HTTP, and WebSocket caches, then publishes new or
changed definitions as `DataEvent::Instrument`, so consumers never observe a definition the
caches do not hold. A material change is any serialized field other than `ts_event` and
`ts_init`.

- The instruments channel is incremental rather than a snapshot feed: a subscription or
  reconnect can begin without an initial payload, so reconnect replay alone does not
  reconcile the instrument cache.
- Set the interval to `0` to disable periodic reconciliation; instruments channel updates
  are always applied. One refresh task runs per connection lifecycle and is cancelled on
  disconnect, failed-connect teardown, stop, and dispose. Spread instruments are included
  when `load_spreads` is set.
- Instruments that disappear from a REST response are retained in the cache; they may
  still back open subscriptions. Suspension, expiry, and delisting arrive as
  `InstrumentStatus` events through the instruments channel.

## Symbology

OKX uses specific symbol conventions for different instrument types. Add the `.OKX`
suffix when referencing instruments in Nautilus, for example `BTC-USDT.OKX`.

### Symbol format by instrument type

#### SPOT

Format: `{BaseCurrency}-{QuoteCurrency}`

Examples:

- `BTC-USDT` - Bitcoin against USDT (Tether)
- `BTC-USDC` - Bitcoin against USDC
- `ETH-USDT` - Ethereum against USDT
- `SOL-USDT` - Solana against USDT

To subscribe to spot Bitcoin USD in your strategy:

```python
InstrumentId.from_str("BTC-USDT.OKX")  # For USDT-quoted spot
InstrumentId.from_str("BTC-USDC.OKX")  # For USDC-quoted spot
```

#### SWAP (perpetual swaps)

Format: `{BaseCurrency}-{QuoteCurrency}-SWAP`

Examples:

- `BTC-USDT-SWAP` - Bitcoin perpetual swap (linear, USDT-margined)
- `BTC-USD-SWAP` - Bitcoin perpetual swap (inverse, coin-margined)
- `ETH-USDT-SWAP` - Ethereum perpetual swap (linear)
- `ETH-USD-SWAP` - Ethereum perpetual swap (inverse)

Linear vs inverse contracts:

- **Linear** (USDT-margined): Uses stablecoins like USDT as margin.
- **Inverse** (coin-margined): Uses the base cryptocurrency as margin.

#### FUTURES (dated futures)

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}`

Examples:

- `BTC-USD-261225` - Bitcoin futures expiring December 25, 2026
- `ETH-USD-261225` - Ethereum futures expiring December 25, 2026
- `BTC-USD-270326` - Bitcoin futures expiring March 26, 2027

Futures can be linear or inverse. The adapter derives this from OKX's `ctType` field.

#### SPREADS

Format: `{Leg1InstrumentId}_{Leg2InstrumentId}`

Examples:

- `BTC-USDT_BTC-USDT-SWAP` - Spread between BTC-USDT spot and BTC-USDT perpetual swap
- `ETH-USD-SWAP_ETH-USD-261225` - Spread between ETH-USD perpetual swap and dated future

Set `load_spreads=True` on the data client to load live OKX spread instruments from
the OKX [Get Spreads (Public)](https://www.okx.com/docs-v5/en/#spread-trading-rest-api-get-spreads-public)
endpoint. The adapter maps each OKX `sprdId` to a Nautilus spread instrument ID
with the `.OKX` venue suffix.

Spread instrument notes:

- Spread market data streams on the OKX business WebSocket: quotes (`sprd-bbo-tbt`),
  trades (`sprd-public-trades`), and 5-level book snapshots (`sprd-books5`). Spreads have
  no incremental book channel, so each `sprd-books5` update is a full snapshot delivered
  through the order book subscription (flagged as a snapshot, not incremental L2 deltas).
- The parser represents spot, swap, and futures leg combinations. It also represents
  option-leg spread definitions when OKX returns them through the same spread endpoint.
- OKX option RFQ and block trading workflows are separate from the Nitro spread order
  book API and are not routed by this spread path.

#### OPTIONS

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}-{Strike}-{Type}`

Examples:

- `BTC-USD-261225-100000-C` - Bitcoin call option, $100,000 strike, expiring December 25, 2026
- `BTC-USD-261225-100000-P` - Bitcoin put option, $100,000 strike, expiring December 25, 2026
- `ETH-USD-261225-4000-C` - Ethereum call option, $4,000 strike, expiring December 25, 2026

Where:

- `C` = Call option
- `P` = Put option

#### EVENTS

OKX event contract instrument IDs use the market ID returned by the OKX instruments API.
The adapter represents these markets as Nautilus `BinaryOption` instruments.

Example:

- `BTC-ABOVE-DAILY-261224-1600-65000` - Event contract market in the
  `BTC-ABOVE-DAILY` series.

### Common questions

**Q: How do I know which contract type to use?**
A: Linear and inverse instruments have distinct symbols. The public Python configs do not expose a
contract-type filter, so the adapter loads both for the selected derivative instrument types.

**Q: How do I load event contracts?**
A: Use `OKXInstrumentType.EVENTS`. The public Python configs load all discoverable event contract
series and do not expose a series filter.

## Retail price improvement (RPI)

Use Retail Price Improvement (RPI) to consume OKX's consolidated organic and RPI depth, place RPI
maker orders, or let standard orders take RPI liquidity. The adapter maps these features to existing
Nautilus order book, order, and lifecycle types. RPI routing is opt-in, so standard subscriptions and
orders remain unchanged.

### RPI market data

Pass `params={"rpi": True}` to `subscribe_book_deltas` or
`request_book_snapshot` to use the public `books-rpi` channel or
`GET /api/v5/market/books-rpi`. The feed combines organic quantity with RPI quantity that is
available for execution.

Each raw depth level has the wire shape `[price, totalQty, nonRpiQty, count]`:

| Wire field  | Rust type | Meaning                                      |
| ----------- | --------- | -------------------------------------------- |
| `price`     | `Decimal` | Price level.                                 |
| `totalQty`  | `Decimal` | Organic and available RPI quantity.          |
| `nonRpiQty` | `Decimal` | Quantity available without RPI taker access. |
| `count`     | `u64`     | Aggregated order count at the price level.   |

Nautilus `OrderBookDeltas` and `OrderBook` use `totalQty` as the level quantity. The typed raw
model retains `nonRpiQty`; the difference between the two quantities is the available RPI
liquidity.

WebSocket snapshots and updates retain `seqId` and `prevSeqId`. Emitted deltas carry `seqId` as
their sequence. The data client checks each update's `prevSeqId` against the last accepted `seqId`;
the values do not need to increase by one. On a mismatch, the client:

- Drops the mismatched frame.
- Suppresses later updates for that instrument.
- Replaces the subscription once to request a fresh snapshot.
- Resumes emission after a snapshot with `prevSeqId: -1`.

If the snapshot does not arrive before the configured snapshot timeout, the book monitor logs a
warning and the client remains fail-closed. The adapter applies the same linkage rule to standard
incremental OKX book channels when `prevSeqId` is present. `books-rpi` has no checksum.

For WebSocket subscriptions, `rpi=True` selects `books-rpi` instead of depth or VIP channel
selection. For REST snapshots, the requested depth becomes `sz`; OKX defaults to one level per side
and accepts up to 400.

The low-level Rust clients expose:

- WebSocket: `OKXWebSocketClient.subscribe_book_rpi` and `unsubscribe_book_rpi`.
- REST: `OKXRawHttpClient.get_rpi_order_book` and
  `OKXHttpClient.request_rpi_book_snapshot`.

Public instrument responses expose the venue's RPI spacing thresholds:

| Wire field     | Rust type         | Instrument `info` key |
| -------------- | ----------------- | --------------------- |
| `rpiMinLevel`  | `Option<u64>`     | `okx_rpi_min_level`   |
| `rpiMinPxBand` | `Option<Decimal>` | `okx_rpi_min_px_band` |

`rpiMinLevel` counts organic price levels, while `rpiMinPxBand` measures basis points from the
opposite-side organic best price. The `info` map stores the price band as its exact decimal string.
The adapter does not reject or round an order from these values because OKX applies the
authoritative instrument and account rules. Use `rpi_px_round` or handle the venue rejection.

### RPI execution

Pass RPI controls through the `submit_order`, `submit_order_list`, or `modify_order` command
`params`. These controls work with HTTP and private WebSocket execution:

| Parameter          | Type   | Operations                    | Behavior                                                        |
| ------------------ | ------ | ----------------------------- | --------------------------------------------------------------- |
| `rpi`              | `bool` | Place and batch place         | Sends `ordType: rpi`; the Nautilus order must be `LIMIT`.       |
| `rpi_taker_access` | `bool` | Place and amend, single/batch | Lets a standard order take RPI liquidity.                       |
| `rpi_px_round`     | `bool` | Place and amend, single/batch | Lets OKX round an RPI maker price outward to an eligible level. |

```python
order = strategy.order_factory.limit(
    instrument_id=instrument_id,
    order_side=OrderSide.SELL,
    quantity=instrument.make_qty("250000"),
    price=instrument.make_price("0.0001600"),
)
strategy.submit_order(
    order,
    params={
        "rpi": True,
        "rpi_px_round": True,
    },
)
```

Use `rpi_taker_access` only with regular limit, market, FOK, or IOC orders. When it is enabled,
OKX applies its taker speed bump to eligible orders, including post-only orders. Use `rpi_px_round`
only on RPI maker orders. Omit inapplicable controls instead of passing `False`, because OKX can
reject unsupported combinations. Both controls default to `false`, and `rpi_taker_access` is not
inherited during an amendment. Repeat `rpi_taker_access=True` on every amendment that must retain
access.

The low-level Rust clients expose the same single and batch matrix:

| Operation   | REST method    | WebSocket method      |
| ----------- | -------------- | --------------------- |
| Place       | `place_order`  | `submit_order`        |
| Batch place | `place_orders` | `batch_submit_orders` |
| Amend       | `amend_order`  | `modify_order`        |
| Batch amend | `amend_orders` | `batch_modify_orders` |

The WebSocket batch amend tuple accepts an optional request ID and serializes it as `reqId`; it
does not replace the order's client ID.

### RPI responses and lifecycle

Private order messages parse both `ordType: rpi` and the migration alias `ordType: elp`. If an
unfilled RPI placement first appears on the private order channel as `state: canceled`, with
`accFillSz` zero or empty, the adapter emits a post-only order rejection without first emitting
acceptance. The fallback reason is `RPI order canceled before acceptance`. OKX can use this path
when an RPI price fails its spacing rule and `rpiPxRound` is false. Order reports represent RPI
orders as Nautilus `LIMIT` orders with `post_only=True`.

Use `get_account_instruments` to read the typed `OKXRpiPermission` value:

- `Disabled` maps to `rpi: "0"`.
- `Enabled` maps to `rpi: "1"` and does not grant permission to place RPI orders.
- `Permitted` maps to `rpi: "2"` and grants permission to place RPI orders.

The public instrument endpoint does not return account permissions. Raw fee responses expose
`rpiMaker` as an optional `Decimal`; an empty value means RPI is not applicable.

Responses may contain both RPI and ELP field names during the transition. The adapter prefers `rpi`
and `rpiMaker`, reads `elp` and `elpMaker` as response aliases, and sends only RPI names. Raw trade
messages describe `source: "1"` as an RPI order.

### RPI exclusions

The adapter deliberately excludes the following:

- It does not expose obsolete `books-elp` subscriptions or emit `ordType: elp`.
- It does not treat the published RPI spacing thresholds as authoritative client-side validation.
- It does not apply RPI controls to algo orders. The regular HTTP order path rejects RPI controls
  for spread orders.
- It does not add generic post-only replay deduplication as part of RPI support.

OKX ignores `rpiPxRound` for options and event contracts.

See the [OKX RPI migration changelog](https://www.okx.com/docs-v5/log_en/#2026-07-28)
and [RPI program guide](https://www.okx.com/help/okx-retail-price-improvement-program-rpi).

## Orders capability

Below are the order types, execution instructions, and time-in-force options supported
for linear perpetual swap products on OKX.

### WebSocket order identification

OKX WebSocket order operations use `instIdCode` (a numeric instrument identifier)
instead of the string `instId` parameter. The adapter resolves `instIdCode` values
from the instrument definitions fetched during startup and caches them for the
session lifetime. If the instrument cache is empty (e.g. because of a failed
bootstrap), order submissions fail with a clear error.

### Client order ID requirements

OKX requires client order IDs to be alphanumeric (letters and numbers only) and at most
32 characters. Hyphens (`-`) are rejected, so set the following on your strategy config:

```python
use_hyphens_in_client_order_ids = False
```

Nautilus client order IDs longer than 32 characters are also rejected. When you need UUID-based
identifiers, combine `use_uuid_client_order_ids=True` with `use_hyphens_in_client_order_ids=False`
so the generated value fits within the OKX limit.

### Order types

| Order type             | Linear perpetual swap | Notes                                                       |
| ---------------------- | --------------------- | ----------------------------------------------------------- |
| `MARKET`               | ✓                     | Immediate execution at market price.                        |
| `MARKET_TO_LIMIT`      | ✓                     | Market order converted to IOC limit.                        |
| `LIMIT`                | ✓                     | Execution at specified price or better.                     |
| `STOP_MARKET`          | ✓                     | Conditional market order through OKX algo orders.           |
| `STOP_LIMIT`           | ✓                     | Conditional limit order through OKX algo orders.            |
| `MARKET_IF_TOUCHED`    | ✓                     | Conditional market order through OKX algo orders.           |
| `LIMIT_IF_TOUCHED`     | ✓                     | Conditional limit order through OKX algo orders.            |
| `TRAILING_STOP_MARKET` | ✓                     | Trailing stop market order through OKX advance algo orders. |

:::info
**Conditional orders**: `STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`,
`LIMIT_IF_TOUCHED`, and `TRAILING_STOP_MARKET` use OKX algo orders. The
`TRAILING_STOP_MARKET` path uses OKX's advance algo order API (`move_order_stop`) and
requires the `cancel-advance-algos` endpoint for cancellation.
:::

### Spread orders

OKX spread instruments use a separate spread trading order book and API family. The
execution client routes spread orders by spread instrument ID, for example
`ETH-USD-SWAP_ETH-USD-261225.OKX`, through the HTTP `/api/v5/sprd/*` endpoints.

The adapter uses OKX's spread REST endpoints for submit, cancel, mass cancel, order
status, and trade reports. It subscribes to the OKX business WebSocket
[`sprd-orders` channel](https://www.okx.com/docs-v5/en/#spread-trading-websocket-private-channel-order-channel)
for live spread order updates.

OKX `sprd-orders` WebSocket updates do not include fee fields. The adapter fails closed and discards
the whole update, so it emits neither a fill event nor an order-state update. Startup reconciliation
recovers the order from REST; set `open_check_interval_secs` to poll open orders continuously.
Historical and reconciliation fill reports from the REST
[`sprd/trades` endpoint](https://www.okx.com/docs-v5/en/#spread-trading-rest-api-get-trades)
include OKX fee data.

Supported spread order instructions:

- `LIMIT` with GTC time-in-force.
- `LIMIT` with IOC time-in-force.
- `LIMIT` with post-only execution.

Spread order lists, conditional orders, FOK time-in-force, and modify requests are not
supported by the OKX spread trading API path.

Relevant OKX docs:

- [Spread order placement](https://www.okx.com/docs-v5/en/#spread-trading-rest-api-place-order).
- [Spread order details](https://www.okx.com/docs-v5/en/#spread-trading-rest-api-get-order-details).
- [Spread order channel](https://www.okx.com/docs-v5/en/#spread-trading-websocket-private-channel-order-channel).

### Execution instructions

| Instruction   | Linear perpetual swap | Notes                                                                             |
| ------------- | --------------------- | --------------------------------------------------------------------------------- |
| `post_only`   | ✓                     | Only for limit orders.                                                            |
| `reduce_only` | ✓                     | Futures and swaps need `net` mode; margin needs `isolated` or `cross` trade mode. |

### Time in force

| Time in force | Linear perpetual swap | Notes                                |
| ------------- | --------------------- | ------------------------------------ |
| `GTC`         | ✓                     | Good Till Canceled.                  |
| `FOK`         | ✓                     | Fill or Kill.                        |
| `IOC`         | ✓                     | Immediate or Cancel.                 |
| `GTD`         | -                     | *No native OKX order time-in-force.* |

:::note
**GTD (Good Till Date) time in force**: OKX supports request expiry through `expTime`,
but that is a request timeout rather than a native order expiry instruction.

If you need GTD functionality, use Nautilus's strategy-managed GTD feature. It handles
order expiration by canceling the order at the specified expiry time.
:::

### Batch operations

| Operation    | Linear perpetual swap | Notes                                     |
| ------------ | --------------------- | ----------------------------------------- |
| Batch Submit | ✓                     | Submit multiple orders in single request. |
| Batch Modify | ✓                     | Modify multiple orders in single request. |
| Batch Cancel | ✓                     | Cancel multiple orders in single request. |

### Position management

| Feature          | Linear perpetual swap | Notes                                |
| ---------------- | --------------------- | ------------------------------------ |
| Query positions  | ✓                     | Real-time position updates.          |
| Position mode    | ✓                     | Net vs Long/Short mode (see below).  |
| Leverage control | -                     | Not exposed by the execution client. |
| Margin mode      | ✓                     | Supports isolated and cross modes.   |

#### Position modes

OKX supports two position modes for derivatives trading:

- **Net mode** (netting): One position per instrument. Buy and sell orders net against
  each other. This is the default and recommended mode for most traders.
- **Long/Short mode** (hedging): Separate long and short positions for the same
  instrument. This mode supports simultaneous long and short exposure.

:::note
Position mode applies account-wide. Set it through the OKX web or app interface, or with
`OKXHttpClient.set_position_mode`; the client configs do not set it. The adapter handles both
modes when reporting positions: in net mode it derives the position side from the signed
quantity, and in long/short mode it uses the `posSide` reported by OKX.
:::

### Trade modes and margin configuration

OKX's unified account system supports different trade modes for spot and derivatives. Configure
the account mode first through the OKX web or app interface; the API cannot set it for the first
time.

For account mode details, see the
[OKX Account Mode documentation](https://www.okx.com/docs-v5/en/#overview-account-mode).

#### Trade modes overview

The Python execution config selects trade modes as follows:

| Instrument | Trade mode | Configuration                                     |
| ---------- | ---------- | ------------------------------------------------- |
| Spot       | `cash`     | Automatic.                                        |
| Derivative | `isolated` | Default, or `margin_mode=OKXMarginMode.ISOLATED`. |
| Derivative | `cross`    | `margin_mode=OKXMarginMode.CROSS`.                |

```python
from nautilus_trader.adapters.okx import OKXExecutionClientConfig
from nautilus_trader.adapters.okx import OKXInstrumentType
from nautilus_trader.adapters.okx import OKXMarginMode
from nautilus_trader.model import AccountId


exec_config = OKXExecutionClientConfig(
    account_id=AccountId.from_str("OKX-001"),
    instrument_types=[OKXInstrumentType.SWAP],
    margin_mode=OKXMarginMode.CROSS,
)
```

The public Python config does not expose spot margin selection, so spot orders use cash
mode. In a mixed spot and derivatives client, `margin_mode` applies to derivatives only.

:::warning
**Manual trade mode override**: You can override the trade mode per order with
`params={"td_mode": "..."}`. This bypasses adapter selection and can lead to order
rejection when the value does not match the instrument type, such as `isolated` for
spot instruments.

Only use manual override for requirements that cannot be met through configuration.
:::

### Order querying

| Feature              | Linear perpetual swap | Notes                          |
| -------------------- | --------------------- | ------------------------------ |
| Query open orders    | ✓                     | List all active orders.        |
| Query order history  | ✓                     | Historical order data.         |
| Order status updates | ✓                     | Real-time order state changes. |
| Trade history        | ✓                     | Execution and fill reports.    |

### Contingent orders

| Feature            | Linear perpetual swap | Notes                                  |
| ------------------ | --------------------- | -------------------------------------- |
| Order lists        | ✓                     | Batch via WS; regular orders only.     |
| OCO orders         | -                     | Not submitted by `OKXExecutionClient`. |
| Bracket orders     | -                     | Not submitted by `OKXExecutionClient`. |
| Conditional orders | ✓                     | Stop and limit-if-touched orders.      |

The low-level HTTP client models OKX attached TP/SL and OCO payloads, but
`OKXExecutionClient` does not translate Nautilus OCO or bracket order lists into those payloads.

#### Conditional order architecture

Conditional orders (OKX algo orders) use a hybrid architecture:

- **Submission**: HTTP REST API (`/api/v5/trade/order-algo`).
- **Status updates**: WebSocket business endpoint (`/ws/v5/business`) on the
  `orders-algo` channel.
- **Cancellation**: HTTP REST API with algo order ID tracking.

This design ensures:

- Immediate submission acknowledgment through HTTP.
- Real-time status updates through WebSocket.
- Proper order lifecycle management with algo order ID mapping.

#### Supported conditional order types

| Order type             | Trigger types     | Notes                                                |
| ---------------------- | ----------------- | ---------------------------------------------------- |
| `STOP_MARKET`          | Last, Mark, Index | Market execution when triggered.                     |
| `STOP_LIMIT`           | Last, Mark, Index | Limit order placement when triggered.                |
| `MARKET_IF_TOUCHED`    | Last, Mark, Index | Market execution when price touched.                 |
| `LIMIT_IF_TOUCHED`     | Last, Mark, Index | Limit order placement when price touched.            |
| `TRAILING_STOP_MARKET` | -                 | Callback ratio or spread; optional activation price. |

:::warning
OKX's `close_fraction` conditional-order parameter is not normalized to the generic
`close_position` risk contract. Do not add `OKX` to `full_position_exit_venues` based on
`close_fraction`; leave the venue unlisted so ordinary quantity and notional checks apply.
:::

#### Trigger price types

Stop and touched orders support different trigger price sources:

- **Last price** (`TriggerType.LAST_PRICE`): Uses the last traded price (default).
- **Mark price** (`TriggerType.MARK_PRICE`): Uses the mark price.
- **Index price** (`TriggerType.INDEX_PRICE`): Uses the underlying index price.

```python
# Example: Stop loss using mark price trigger
stop_order = order_factory.stop_market(
    instrument_id=instrument_id,
    order_side=OrderSide.SELL,
    quantity=Quantity.from_str("0.1"),
    trigger_price=Price.from_str("45000.0"),
    trigger_type=TriggerType.MARK_PRICE,  # Use mark price for trigger
)
strategy.submit_order(stop_order)
```

## Risk management

### Liquidation and ADL event handling

The OKX adapter detects exchange-initiated risk management events:

- **Liquidation warnings**: When `instrument_types` includes `MARGIN`, `SWAP`, `FUTURES`, or
  `OPTION`, the execution client subscribes to the `liquidation-warning` channel with
  `instType=ANY` and logs a warning when OKX reports a position nearing liquidation. This is an
  early warning only: the position may already be liquidated by the time the message arrives, and
  the adapter surfaces it as a log message rather than a strategy-facing event.
- **Liquidation orders**: When the exchange liquidates a position, the adapter detects
  the liquidation category and logs warnings with order details. These orders continue
  through the normal order and fill pipeline.
- **Auto-deleveraging (ADL)**: When OKX closes your position to offset a counterparty's
  liquidation, the adapter detects and logs the ADL event with position details.

Liquidation-order and ADL detection is driven by the `category` field on the order record. The
recognized values are:

| `category`              | Meaning                       |
| ----------------------- | ----------------------------- |
| `full_liquidation`      | Full position liquidation.    |
| `partial_liquidation`   | Partial position liquidation. |
| `adl`                   | Auto-deleveraging close.      |
| `delivery`              | Contract delivery at expiry.  |
| `normal` / other values | Regular order flow.           |

Category detection runs on both paths:

- WebSocket `orders` channel (live order and fill updates).
- HTTP `GET /api/v5/trade/orders-history` (used during reconciliation and cold-start mass status).

:::info
**Liquidation and ADL events are logged at WARNING level** with details including order
ID, instrument, and state. Liquidation warnings instead log position side, size, margin ratio,
mark price, and margin mode. Monitor these logs as part of your risk management process.

The adapter forwards these exchange-generated orders as `OrderStatusReport` and `FillReport`
messages and sends position updates as `PositionStatusReport` messages. Because the orders are
untracked at dispatch time, this path does not emit strategy-owned order events directly.
:::

Upstream references:

- [Order channel and `category` field](https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-order-channel)
- [Liquidation warning channel](https://www.okx.com/docs-v5/en/#trading-account-websocket-liquidation-warning-channel)
- [Auto-Deleveraging mechanism](https://www.okx.com/help/okx-contract-auto-deleveraging-adl)
- [Liquidation mechanism](https://www.okx.com/help/introduction-to-liquidation)

## Options trading

The OKX adapter supports trading options (`OPTION` instrument type) with some differences
from other derivatives. OKX options are inverse contracts settled in the underlying
cryptocurrency.
For full API details see the
[OKX Options Trading documentation](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order).

### Supported order types

Only limit-style orders are supported. OKX does not allow market orders for options.

| Order type        | Supported | Notes                                            |
| ----------------- | --------- | ------------------------------------------------ |
| `LIMIT`           | ✓         | Standard limit order.                            |
| `MARKET`          | -         | Rejected by the adapter before reaching the API. |
| `MARKET_TO_LIMIT` | -         | Rejected by the adapter before reaching the API. |

Options support FOK and IOC time-in-force. OKX uses a dedicated `op_fok` order type for
options FOK orders; the adapter handles this mapping automatically.

Conditional/algo orders (`STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`,
`LIMIT_IF_TOUCHED`, `TRAILING_STOP_MARKET`) are not supported for options and are denied.

### Pricing modes

Options orders can be priced in three mutually exclusive ways. Pass the pricing mode via
order `params`:

| Mode  | Parameter | Description                                      |
| ----- | --------- | ------------------------------------------------ |
| Price | (default) | Standard limit price in the contract's currency. |
| USD   | `px_usd`  | Price in USD terms.                              |
| IV    | `px_vol`  | Price in implied volatility (1.0 = 100%).        |

```python
# Price in USD
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-USD-261225-50000-C.OKX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("0"),  # Placeholder; px_usd takes precedence
    params={"px_usd": "100.5"},
)

# Price in implied volatility
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-USD-261225-50000-C.OKX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("0"),  # Placeholder; px_vol takes precedence
    params={"px_vol": "0.55"},
)
```

When modifying an order, the same `px_usd` or `px_vol` params can be passed to the modify
command to amend the price in the original pricing mode.

### Option Greeks

OKX publishes two parallel greek sets on the `opt-summary` channel:

- **Black-Scholes (`BLACK_SCHOLES`)**: Greeks denominated in USD. Matches the convention
  used by the Deribit and Bybit adapters.
- **Price-adjusted (`PRICE_ADJUSTED`)**: Greeks denominated in the underlying coin
  units. Matches OKX's native contract convention.

By default the adapter emits both on every `opt-summary` tick. Each emitted `OptionGreeks`
carries a `convention` field set to `GreeksConvention.BLACK_SCHOLES` or
`GreeksConvention.PRICE_ADJUSTED`, so receivers can branch per message.

To narrow the stream, pass `params["greeks_convention"]` on subscribe:

- Single string: `"BLACK_SCHOLES"` or `"PRICE_ADJUSTED"` (case-insensitive).
- List of strings: `["BLACK_SCHOLES", "PRICE_ADJUSTED"]`.
- Omitted: adapter emits both.

Unknown entries log a warning and are skipped. If every requested entry is unknown, the
adapter falls back to emitting both.

```python
# Default (both conventions, receiver branches)
self.subscribe_option_greeks(instrument_id)


def on_option_greeks(self, greeks: OptionGreeks) -> None:
    if greeks.convention == GreeksConvention.BLACK_SCHOLES:
        self._handle_bs(greeks)
    else:
        self._handle_pa(greeks)
```

```python
# Single-convention narrowing
self.subscribe_option_greeks(
    instrument_id,
    params={"greeks_convention": "PRICE_ADJUSTED"},
)
```

```python
# Explicit list (equivalent to the default when both are listed)
self.subscribe_option_greeks(
    instrument_id,
    params={"greeks_convention": ["BLACK_SCHOLES", "PRICE_ADJUSTED"]},
)
```

:::note
The data engine deduplicates option-greeks subscriptions by `instrument_id`, so if two actors
on one node subscribe to the same instrument with different single conventions only the first
one reaches the adapter. The second actor gets the first actor's convention set. Workaround:
either actor can subscribe without `params` (or with the full list) to receive both streams
and filter locally on `greeks.convention`.
:::

### Position Greeks

OKX position payloads include position-level Black-Scholes Greeks (`delta_bs`, `gamma_bs`,
`theta_bs`, and `vega_bs`). The adapter's standard `PositionStatusReport` does not expose these
fields. The `opt-summary` stream described above provides the adapter's exposed per-instrument
Greeks.

### Restrictions

- `reduce_only` is not applicable to options and is automatically stripped.
- Position side defaults to `Net`.

### Configuration

:::warning
Option discovery requires at least one `instrument_families` value, for example `BTC-USD`.
Pass it to `OKXDataClientConfig` when loading options from Python. The public Python execution
config constructor does not expose this field, so selecting `OKXInstrumentType.OPTION` only on
`OKXExecutionClientConfig` skips option loading and logs a warning.
:::

## Event contracts

OKX exposes prediction market contracts through `instType=EVENTS`. The adapter loads
these instruments as Nautilus `BinaryOption` instruments and preserves OKX metadata
in the instrument `info` field under the keys `series_id`, `inst_category`,
`inst_id_code`, `state`, and `rule_type`.

### Loading event contract instruments

Use `OKXInstrumentType.EVENTS` in the data or execution client config. The adapter requests the
event contract series list, then requests instruments for each series.

```python
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXInstrumentType


data_config = OKXDataClientConfig(instrument_types=[OKXInstrumentType.EVENTS])
```

### Event contract market data

The low-level HTTP client exposes OKX's public event contract discovery endpoints:

- `request_event_contract_series`.
- `request_event_contract_events`.
- `request_event_contract_markets`.

The low-level WebSocket client supports the `event-contract-markets` channel through
`subscribe_event_contract_markets` and `unsubscribe_event_contract_markets`. This
channel publishes market status and floor-strike generation updates, has no initial
snapshot, and does not include `instId`, so the adapter forwards it as raw venue JSON.

:::note
OKX's standard market data endpoints return YES-side data for `EVENTS`. Derive NO-side
prices from YES-side prices when a strategy needs both outcomes.
:::

### Event contract trading

Pass the OKX event outcome through order `params` when submitting event contract orders:

```python
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-ABOVE-DAILY-261224-1600-65000.OKX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("0.42"),
    params={"outcome": "yes"},
)
strategy.submit_order(order)
```

OKX requires `outcome` for `EVENTS` orders. It also requires `speedBump=1` for
non-post-only event contract orders and amendments. The adapter validates `outcome`
before sending the order and defaults `speedBump` to `1` for non-post-only event
orders when it is not supplied.

Settlement fills arrive with OKX order category `delivery`. The adapter parses this
category during live order updates and reconciliation.

Upstream references:

- [Event contract REST endpoints](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-series).
- [WS channel](https://www.okx.com/docs-v5/en/#public-data-websocket-event-contract-markets-channel).
- [Place order request fields](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order).

## Authentication

To use the OKX adapter, create API credentials in your OKX account:

1. Log into your OKX account and navigate to the API management page.
2. Create a new API key with the required permissions for trading and data access.
3. Record your API key, secret key, and passphrase.

You can provide these credentials through environment variables:

```bash
export OKX_API_KEY="your_api_key"
export OKX_API_SECRET="your_api_secret"
export OKX_API_PASSPHRASE="your_passphrase"
```

Or pass them directly in the configuration (not recommended for production).

## Demo trading

OKX provides a demo trading environment for testing strategies without real funds.

### Setting up a demo account

1. Log into your OKX account at [okx.com](https://www.okx.com).
2. Navigate to **Trade** > **Demo Trading**.
3. Go to **Personal Center** within Demo Trading.
4. Select **Demo Trading API** and create a new API key.
5. Record your demo API key, secret key, and passphrase.

You can provide demo credentials through environment variables:

```bash
export OKX_API_KEY="your_demo_api_key"
export OKX_API_SECRET="your_demo_api_secret"
export OKX_API_PASSPHRASE="your_demo_passphrase"
```

### Configuration

Set `environment=OKXEnvironment.DEMO` in your client configuration:

```python
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXEnvironment


data_config = OKXDataClientConfig(environment=OKXEnvironment.DEMO)
```

When demo mode is enabled:

- REST API requests reuse the region's live host with the `x-simulated-trading: 1` header.
- WebSocket connections use demo endpoints (`wspap.okx.com` for the global region).

:::note
Demo API keys are separate from production keys. Create API keys for demo trading
through the Demo Trading interface. Production API keys do not work in demo mode.
:::

## Regional endpoints

OKX serves distinct endpoints per region, and an API key is only valid against the region
where it was registered (using a key against another region's endpoints returns
`API key doesn't exist`). Set `region` to select the correct endpoint set:

| Region   | Registered on | REST          | WebSocket host  |
| -------- | ------------- | ------------- | --------------- |
| `GLOBAL` | `www.okx.com` | `www.okx.com` | `ws.okx.com`    |
| `EEA`    | `my.okx.com`  | `eea.okx.com` | `wseea.okx.com` |
| `US`     | `app.okx.com` | `us.okx.com`  | `wsus.okx.com`  |

Despite its enum name, `US` also selects the endpoints for Australian accounts registered on
`app.okx.com`.

`region` defaults to `GLOBAL`. For example, an EEA account:

```python
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXRegion


data_config = OKXDataClientConfig(region=OKXRegion.EEA)
```

`region` selects the regional defaults, and combines with `environment` to pick the demo
hosts (for example `wseeapap.okx.com` for EEA demo). Explicit `base_url_http` and
`base_url_ws` overrides always take precedence over the region defaults.

## Funding rates

The adapter receives funding rate data from the
[Funding Rate Channel](https://www.okx.com/docs-v5/en/#public-data-websocket-funding-rate-channel)
WebSocket stream. OKX provides both `fundingTime` and `nextFundingTime` in each message,
and the adapter computes `interval` as the difference between these two values.

For historical funding rate requests, the adapter computes the interval from consecutive
funding timestamps returned by the
[Get Funding Rate History](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-funding-rate-history)
endpoint.

## Rate limiting

The adapter enforces OKX's per-endpoint quotas while keeping sensible defaults for REST
and WebSocket calls.

:::warning
OKX enforces per-endpoint and per-account quotas. A rate-limited request returns OKX error code
`50011`; throttle requests on the affected key before retrying.
:::

### REST limits

Every request passes through an internal global bucket of 250 requests per second, plus the
endpoint-specific bucket below. The endpoint quotas mirror OKX's published limits where
available.

| Key / endpoint                          | Limit (req/sec) | Notes                                     |
| --------------------------------------- | --------------- | ----------------------------------------- |
| `okx:global`                            | 250             | Adapter-level shared bucket.              |
| `/api/v5/account/set-position-mode`     | 2               | OKX 5 requests / 2 seconds, rounded down. |
| `/api/v5/account/balance`               | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/account/trade-fee`             | 2               | OKX 5 requests / 2 seconds, rounded down. |
| `/api/v5/account/instruments`           | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/account/positions`             | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/account/positions-history`     | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/instruments`            | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/public/position-tiers`         | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/event-contract/series`  | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/event-contract/events`  | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/event-contract/markets` | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/opt-summary`            | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/public/price-limit`            | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/public/time`                   | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/mark-price`             | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/public/funding-rate-history`   | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/market/index-tickers`          | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/market/books`                  | 20              | OKX 40 requests / 2 seconds.              |
| `/api/v5/market/books-rpi`              | 20              | Adapter bucket; OKX publishes 20 / 2 sec. |
| `/api/v5/market/candles`                | 20              | OKX 40 requests / 2 seconds.              |
| `/api/v5/market/history-candles`        | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/market/history-trades`         | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/sprd/spreads`                  | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/sprd/order`                    | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/sprd/cancel-order`             | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/sprd/mass-cancel`              | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/sprd/orders-pending`           | 5               | OKX 10 requests / 2 seconds.              |
| `/api/v5/sprd/orders-history`           | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/sprd/trades`                   | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/trade/order`                   | 30              | OKX 60 requests / 2 seconds.              |
| `/api/v5/trade/batch-orders`            | 7               | OKX 300 orders / 2 seconds, rounded down. |
| `/api/v5/trade/amend-order`             | 30              | OKX 60 requests / 2 seconds.              |
| `/api/v5/trade/amend-batch-orders`      | 7               | OKX 300 orders / 2 seconds, rounded down. |
| `/api/v5/trade/cancel-batch-orders`     | 7               | OKX 300 orders / 2 seconds, rounded down. |
| `/api/v5/trade/orders-pending`          | 30              | OKX 60 requests / 2 seconds.              |
| `/api/v5/trade/orders-history`          | 20              | OKX 40 requests / 2 seconds.              |
| `/api/v5/trade/fills`                   | 30              | OKX 60 requests / 2 seconds.              |
| `/api/v5/trade/order-algo`              | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/trade/cancel-algos`            | 1               | OKX 20 orders / 2 seconds.                |
| `/api/v5/trade/cancel-advance-algos`    | 1               | Conservative bucket, see below.           |
| `/api/v5/trade/amend-algos`             | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/trade/orders-algo-pending`     | 10              | OKX 20 requests / 2 seconds.              |
| `/api/v5/trade/orders-algo-history`     | 10              | OKX 20 requests / 2 seconds.              |

All keys include the `okx:global` bucket. URLs are normalized with query strings removed
before rate limiting, so requests with different filters share the same quota.

The adapter's `/api/v5/market/books-rpi` bucket is 20 requests per second, while OKX publishes
20 requests per 2 seconds. The venue limit remains authoritative, so callers should keep RPI book
snapshot traffic within the published quota.

For order-based batch quotas, the adapter uses request-level buckets that assume full
batch sizes: 20 orders per request for regular batch operations and 10 orders per
request for algo cancels. OKX's public docs do not list a rate limit for
`/api/v5/trade/cancel-advance-algos`, so the adapter applies a conservative bucket; the HTTP
client calls that endpoint to cancel advance algo orders such as trailing stops.

### WebSocket limits

- Connection establishment: 3 requests per second (per IP).
- Subscription operations (subscribe/unsubscribe/login): 480 requests per hour per connection.

Order operation buckets mirror OKX's published limits where available.

| Operation key  | Limit (req/sec) | Notes                                                      |
| -------------- | --------------- | ---------------------------------------------------------- |
| `order`        | 30              | OKX 60 requests / 2 seconds.                               |
| `cancel`       | 30              | OKX 60 requests / 2 seconds.                               |
| `amend`        | 30              | OKX 60 requests / 2 seconds.                               |
| `batch-order`  | 7               | OKX 300 orders / 2 seconds, rounded down for full batches. |
| `batch-cancel` | 7               | OKX 300 orders / 2 seconds, rounded down for full batches. |
| `batch-amend`  | 7               | OKX 300 orders / 2 seconds, rounded down for full batches. |
| `mass-cancel`  | 2               | OKX 5 requests / 2 seconds, rounded down.                  |
| `algo-order`   | 10              | OKX 20 requests / 2 seconds.                               |
| `algo-cancel`  | 1               | OKX 20 orders / 2 seconds, rounded down for full batches.  |

:::info
See the [OKX rate limit documentation](https://www.okx.com/docs-v5/en/#rest-api-rate-limit).
:::

## Reconciliation

The OKX adapter applies separate reconciliation policies to current venue state and terminal
history:

| Data                      | Unset lookback        | Explicit lookback     | OKX source                    |
| ------------------------- | --------------------- | --------------------- | ----------------------------- |
| Pending regular orders    | All current orders    | All current orders    | Regular pending orders        |
| Live algo orders          | All current orders    | All current orders    | Algo pending orders           |
| Current positions         | All current positions | All current positions | Account positions             |
| Terminal orders and fills | 3 days                | Up to 7 days          | Order and trade history       |
| Fill lookback <= 3 days   | Recent fills          | Recent fills          | `/api/v5/trade/fills`         |
| Fill lookback > 3 days    | Not requested         | Extended fills        | `/api/v5/trade/fills-history` |

Values above 7 days are clamped to the longest complete window across the regular order history
and spread trade history endpoints used for reconciliation. This is not a limit on all archived data
available from OKX.

## Configuration

### Data client

The OKX data client provides the following Python configuration options.

| Option                             | Default                    | Description                                                                    |
| ---------------------------------- | -------------------------- | ------------------------------------------------------------------------------ |
| `instrument_types`                 | `[OKXInstrumentType.SPOT]` | OKX instrument types to load.                                                  |
| `instrument_families`              | `None`                     | Required for options (`BTC-USD`); filters futures, swaps, and events when set. |
| `load_spreads`                     | `False`                    | Loads live spread instruments.                                                 |
| `base_url_http`                    | `None`                     | Override for the OKX REST endpoint.                                            |
| `base_url_ws_public`               | `None`                     | Override for the public WebSocket URL.                                         |
| `base_url_ws_business`             | `None`                     | Override for the business WebSocket URL.                                       |
| `api_key`                          | `None`                     | Falls back to `OKX_API_KEY` when unset.                                        |
| `api_secret`                       | `None`                     | Falls back to `OKX_API_SECRET` when unset.                                     |
| `api_passphrase`                   | `None`                     | Falls back to `OKX_API_PASSPHRASE`.                                            |
| `environment`                      | `LIVE`                     | Environment enum (`LIVE` or `DEMO`).                                           |
| `region`                           | `GLOBAL`                   | Region enum (`GLOBAL`, `EEA`, or `US`).                                        |
| `http_timeout_secs`                | `60`                       | REST market data request timeout.                                              |
| `max_retries`                      | `3`                        | Retry attempts for recoverable REST errors.                                    |
| `retry_delay_initial_ms`           | `1,000`                    | Initial delay before retrying.                                                 |
| `retry_delay_max_ms`               | `10,000`                   | Maximum exponential backoff delay.                                             |
| `update_instruments_interval_mins` | `60`                       | REST instrument cache reconciliation interval in minutes; `0` disables.        |
| `book_stale_check_interval_secs`   | `5`                        | Stale book check interval.                                                     |
| `book_stale_threshold_secs`        | `30`                       | Idle time before a stale book warning.                                         |
| `book_snapshot_timeout_secs`       | `3`                        | Post-reconnect snapshot wait.                                                  |
| `vip_level`                        | `None`                     | Enables higher-depth books by VIP tier.                                        |
| `proxy_url`                        | `None`                     | Optional HTTP and WebSocket proxy URL.                                         |
| `transport_backend`                | `Sockudo`                  | WebSocket transport backend.                                                   |

Set `book_stale_check_interval_secs`, `book_stale_threshold_secs`, or
`book_snapshot_timeout_secs` to `0` to disable that health monitor. Quiet markets can idle
without book updates; increase `book_stale_threshold_secs` for sparse instruments.

Supported data client `instrument_types` values are `SPOT`, `MARGIN`, `SWAP`,
`FUTURES`, `OPTION`, and `EVENTS`. See [Options trading](#options-trading) before selecting
`OPTION` from Python.

Spread instruments use `load_spreads` instead of `instrument_types` because OKX serves them from
`/api/v5/sprd/spreads`.

### Execution client

The OKX execution client provides the following Python configuration options.

| Option                   | Default                    | Description                                                                                             |
| ------------------------ | -------------------------- | ------------------------------------------------------------------------------------------------------- |
| `instrument_types`       | `[OKXInstrumentType.SPOT]` | Tradable OKX instrument types.                                                                          |
| `load_spreads`           | `False`                    | Loads live spread instruments.                                                                          |
| `account_id`             | Required                   | Nautilus account ID for the client.                                                                     |
| `base_url_http`          | `None`                     | Override for the OKX trading REST endpoint.                                                             |
| `base_url_ws_private`    | `None`                     | Override for the private WebSocket URL.                                                                 |
| `base_url_ws_business`   | `None`                     | Override for the business WebSocket URL.                                                                |
| `api_key`                | `None`                     | Falls back to `OKX_API_KEY` when unset.                                                                 |
| `api_secret`             | `None`                     | Falls back to `OKX_API_SECRET` when unset.                                                              |
| `api_passphrase`         | `None`                     | Falls back to `OKX_API_PASSPHRASE`.                                                                     |
| `environment`            | `LIVE`                     | Environment enum (`LIVE` or `DEMO`).                                                                    |
| `region`                 | `GLOBAL`                   | Region enum (`GLOBAL`, `EEA`, or `US`).                                                                 |
| `margin_mode`            | `None`                     | Margin mode (`ISOLATED` or `CROSS`).                                                                    |
| `http_timeout_secs`      | `60`                       | REST trading request timeout.                                                                           |
| `max_retries`            | `3`                        | Retry attempts for recoverable REST errors. Order submission endpoints are exempt and always send once. |
| `retry_delay_initial_ms` | `1,000`                    | Initial delay before retrying.                                                                          |
| `retry_delay_max_ms`     | `10,000`                   | Maximum exponential backoff delay.                                                                      |
| `auth_timeout_secs`      | `None`                     | Override WebSocket authentication timeout.                                                              |
| `proxy_url`              | `None`                     | Optional HTTP and WebSocket proxy URL.                                                                  |
| `transport_backend`      | `Sockudo`                  | WebSocket transport backend.                                                                            |

Supported execution client `instrument_types` values are `SPOT`, `MARGIN`, `SWAP`,
`FUTURES`, `OPTION`, and `EVENTS`. See [Options trading](#options-trading) before selecting
`OPTION` from Python.

Spread instruments use OKX spread IDs instead of `instrument_types`; load them with
`load_spreads=True` on the data and execution clients before trading them.

### Manual endpoint overrides

Setting `region` (see [Regional endpoints](#regional-endpoints)) selects the correct EEA or
US endpoints automatically, which is the recommended approach. The explicit `base_url_*`
overrides below remain available for proxies, custom routing, or endpoints not covered by a
region; they take precedence over the `region` default. The EEA bases are shown as an example.

| Config field           | Live base                  | Demo base                     | WebSocket path    |
| ---------------------- | -------------------------- | ----------------------------- | ----------------- |
| `base_url_http`        | `https://eea.okx.com`      | `https://eea.okx.com`         |                   |
| `base_url_ws_public`   | `wss://wseea.okx.com:8443` | `wss://wseeapap.okx.com:8443` | `/ws/v5/public`   |
| `base_url_ws_private`  | `wss://wseea.okx.com:8443` | `wss://wseeapap.okx.com:8443` | `/ws/v5/private`  |
| `base_url_ws_business` | `wss://wseea.okx.com:8443` | `wss://wseeapap.okx.com:8443` | `/ws/v5/business` |

For WebSocket fields, join the base and path in the same row.

Use `base_url_ws_public` with data client configs and `base_url_ws_private` with execution client
configs. When overriding either WebSocket URL, also set `base_url_ws_business` because the adapter
does not derive a custom business WebSocket URL from the other override.

See the [OKX EEA API documentation](https://my.okx.com/docs-v5/en/) for the current
official endpoint list.

Use `OKXDataClientConfig` with `OKXDataClientFactory` and `OKXExecutionClientConfig` with
`OKXExecutionClientFactory`. The Python examples show a complete
`LiveNode.builder(...)` configuration for data and execution clients.

## Contributing

:::info
For additional features or to contribute to the OKX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
