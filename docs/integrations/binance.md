# Binance

Founded in 2017, Binance is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products.

This integration supports live market data ingest and order execution for:

- **Binance Spot** (including Binance US)
- **Binance USDT-Margined Futures** (perpetuals and delivery contracts)
- **Binance Coin-Margined Futures**

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/binance/).

## Overview

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Binance adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BinanceHttpClient`: Low-level HTTP API connectivity.
- `BinanceWebSocketClient`: Low-level WebSocket API connectivity.
- `BinanceInstrumentProvider`: Instrument parsing and loading functionality.
- `BinanceSpotDataClient`/`BinanceFuturesDataClient`: A market data feed manager.
- `BinanceSpotExecutionClient`/`BinanceFuturesExecutionClient`: An account management and trade execution gateway.
- `BinanceLiveDataClientFactory`: Factory for Binance data clients (used by the trading node builder).
- `BinanceLiveExecClientFactory`: Factory for Binance execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

### Product support

| Product Type                             | Supported | Notes                               |
|------------------------------------------|-----------|-------------------------------------|
| Spot Markets (incl. Binance US)          | ✓         |                                     |
| Margin Accounts (Cross & Isolated)       | -         | Margin trading not implemented.     |
| USDT-Margined Futures (PERP & Delivery)  | ✓         |                                     |
| Coin-Margined Futures                    | ✓         |                                     |

:::note
Margin trading (cross & isolated) is not implemented at this time.
Contributions via [GitHub issue #2631](https://github.com/nautechsystems/nautilus_trader/issues/#2631)
or pull requests to add margin trading functionality are welcome.
:::

## Data types

To provide complete API functionality to traders, the integration includes several
custom data types:

- `BinanceTicker`: Represents data returned for Binance 24-hour ticker subscriptions, including comprehensive price and statistical information.
- `BinanceBar`: Represents data for historical requests or real-time subscriptions to Binance bars, with additional volume metrics.
- `BinanceFuturesMarkPriceUpdate`: Represents mark price updates for Binance Futures subscriptions.

See the Binance [API Reference](../api_reference/adapters/binance.md) for full definitions.

## Symbology

As per the Nautilus unification policy for symbols, the native Binance symbols are used where possible including for
spot assets and futures contracts. Because NautilusTrader is capable of multi-venue + multi-account
trading, it's necessary to explicitly clarify the difference between `BTCUSDT` as the spot and margin traded
pair, and the `BTCUSDT` perpetual futures contract (this symbol is used for *both* natively by Binance).

Therefore, Nautilus appends the suffix `-PERP` to all perpetual symbols.
E.g. for Binance Futures, the `BTCUSDT` perpetual futures contract symbol would be `BTCUSDT-PERP` within the Nautilus system boundary.

## Order capability

The following tables detail the order types, execution instructions, and time-in-force options supported across different Binance account types:

### Order types

| Order Type             | Spot | Margin | USDT Futures | Coin Futures | Notes                   |
|------------------------|------|--------|--------------|--------------|-------------------------|
| `MARKET`               | ✓    | ✓      | ✓            | ✓            | Quote quantity support: Spot/Margin only. |
| `LIMIT`                | ✓    | ✓      | ✓            | ✓            |                         |
| `STOP_MARKET`          | -    | ✓      | ✓            | ✓            | Not supported for Spot. |
| `STOP_LIMIT`           | ✓    | ✓      | ✓            | ✓            |                         |
| `MARKET_IF_TOUCHED`    | -    | -      | ✓            | ✓            | Futures only.           |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓      | ✓            | ✓            |                         |
| `TRAILING_STOP_MARKET` | -    | -      | ✓            | ✓            | Futures only.           |

### Execution instructions

| Instruction   | Spot | Margin | USDT Futures | Coin Futures | Notes                                 |
|---------------|------|--------|--------------|--------------|---------------------------------------|
| `post_only`   | ✓    | ✓      | ✓            | ✓            | See restrictions below.               |
| `reduce_only` | -    | -      | ✓            | ✓            | Futures only; disabled in Hedge Mode. |

#### Post-only restrictions

Only *limit* order types support `post_only`.

| Order Type               | Spot | Margin | USDT Futures | Coin Futures | Notes                                                      |
|--------------------------|------|--------|--------------|--------------|------------------------------------------------------------|
| `LIMIT`                  | ✓    | ✓      | ✓            | ✓            | Uses `LIMIT_MAKER` for Spot/Margin, `GTX` TIF for Futures. |
| `STOP_LIMIT`             | -    | -      | ✓            | ✓            | Not supported for Spot/Margin.                             |

### Time in force

| Time in force | Spot | Margin | USDT Futures | Coin Futures | Notes                                           |
|---------------|------|--------|--------------|--------------|-------------------------------------------------|
| `GTC`         | ✓    | ✓      | ✓            | ✓            | Good Till Canceled.                             |
| `GTD`         | ✓*   | ✓*     | ✓            | ✓            | *Converted to GTC for Spot/Margin with warning. |
| `FOK`         | ✓    | ✓      | ✓            | ✓            | Fill or Kill.                                   |
| `IOC`         | ✓    | ✓      | ✓            | ✓            | Immediate or Cancel.                            |

### Advanced order features

| Feature            | Spot | Margin | USDT Futures | Coin Futures | Notes                                        |
|--------------------|------|--------|--------------|--------------|----------------------------------------------|
| Order Modification | ✓    | ✓      | ✓            | ✓            | Price and quantity for `LIMIT` orders only.  |
| Bracket/OCO Orders | ✓    | ✓      | ✓            | ✓            | One-Cancels-Other for stop loss/take profit. |
| Iceberg Orders     | ✓    | ✓      | ✓            | ✓            | Large orders split into visible portions.    |

### Batch operations

| Operation          | Spot | Margin | USDT Futures | Coin Futures | Notes                                        |
|--------------------|------|--------|--------------|--------------|----------------------------------------------|
| Batch Submit       | ✓    | ✓      | ✓            | ✓            | Submit multiple orders in single request.    |
| Batch Modify       | -    | -      | ✓            | ✓            | Modify multiple orders in single request. Futures only. |
| Batch Cancel       | ✓    | ✓      | ✓            | ✓            | Cancel multiple orders in single request.    |

### Position management

| Feature              | Spot | Margin | USDT Futures | Coin Futures | Notes                                      |
|---------------------|------|--------|--------------|--------------|---------------------------------------------|
| Query positions     | -    | ✓      | ✓            | ✓            | Real-time position updates.                 |
| Position mode       | -    | -      | ✓            | ✓            | One-Way vs Hedge mode (position IDs).       |
| Leverage control    | -    | ✓      | ✓            | ✓            | Dynamic leverage adjustment per symbol.     |
| Margin mode         | -    | ✓      | ✓            | ✓            | Cross vs Isolated margin per symbol.        |

### Risk events

| Feature              | Spot | Margin | USDT Futures | Coin Futures | Notes                                       |
|----------------------|------|--------|--------------|--------------|---------------------------------------------|
| Liquidation handling | -    | -      | ✓            | ✓            | Exchange-forced position closures.          |
| ADL handling         | -    | -      | ✓            | ✓            | Auto-Deleveraging events.                   |

Binance Futures can trigger exchange-generated orders in response to risk events:

- **Liquidations**: When insufficient margin exists to maintain a position, Binance forcibly closes it at the bankruptcy price. These orders have client IDs starting with `autoclose-`.
- **ADL (Auto-Deleveraging)**: When the insurance fund is depleted, Binance closes profitable positions to cover losses. These orders use client ID `adl_autoclose`.
- **Settlements**: Quarterly contract deliveries use client IDs starting with `settlement_autoclose-`.

The adapter detects these special order types via their client ID patterns and execution type (`CALCULATED`), then:

1. Logs a warning with order details for monitoring.
2. Generates an `OrderStatusReport` to seed the cache.
3. Generates a `FillReport` with correct fill details and TAKER liquidity side.

This ensures liquidation and ADL events are properly reflected in portfolio state and PnL calculations.

### Order querying

| Feature              | Spot | Margin | USDT Futures | Coin Futures | Notes                                      |
|---------------------|------|--------|--------------|--------------|---------------------------------------------|
| Query open orders   | ✓    | ✓      | ✓            | ✓            | List all active orders.                     |
| Query order history | ✓    | ✓      | ✓            | ✓            | Historical order data.                      |
| Order status updates| ✓    | ✓      | ✓            | ✓            | Real-time order state changes.              |
| Trade history       | ✓    | ✓      | ✓            | ✓            | Execution and fill reports.                 |

### Contingent orders

| Feature              | Spot | Margin | USDT Futures | Coin Futures | Notes                                       |
|---------------------|------|--------|--------------|--------------|----------------------------------------------|
| Order lists         | -    | -      | -            | -            | *Not supported*.                             |
| OCO orders          | ✓    | ✓      | ✓            | ✓            | One-Cancels-Other for stop loss/take profit. |
| Bracket orders      | ✓    | ✓      | ✓            | ✓            | Stop loss + take profit combinations.        |
| Conditional orders  | ✓    | ✓      | ✓            | ✓            | Stop and market-if-touched orders.           |

### Order parameters

Customize individual orders by supplying a `params` dictionary when calling `Strategy.submit_order`. The Binance execution clients currently recognise:

| Parameter       | Type   | Account types     | Description |
|-----------------|--------|-------------------|-------------|
| `price_match`   | `str`  | USDT/COIN Futures | Set one of Binance's `priceMatch` modes (see Price match section below) to delegate price selection to the exchange. Cannot be combined with `post_only` or iceberg (`display_qty`) instructions. |

### Price match

Binance Futures supports BBO (Best Bid/Offer) price matching via the `priceMatch` parameter, which delegates price selection to the exchange. This feature allows limit orders to dynamically join the order book at optimal prices without manually specifying the exact price level.

When using `price_match`, you submit a limit order with a reference price (for local risk checks), but Binance determines the actual working price based on the current market state and the selected price match mode.

#### Valid price match values

Valid `priceMatch` values for Binance Futures:

| Value         | Behaviour                                                      |
|---------------|----------------------------------------------------------------|
| `OPPONENT`    | Join the best price on the opposing side of the book.          |
| `OPPONENT_5`  | Join the opposing side price but allow up to a 5-tick offset.  |
| `OPPONENT_10` | Join the opposing side price but allow up to a 10-tick offset. |
| `OPPONENT_20` | Join the opposing side price but allow up to a 20-tick offset. |
| `QUEUE`       | Join the best price on the same side (stay maker).             |
| `QUEUE_5`     | Join the same-side queue but offset up to 5 ticks.             |
| `QUEUE_10`    | Join the same-side queue but offset up to 10 ticks.            |
| `QUEUE_20`    | Join the same-side queue but offset up to 20 ticks.            |

:::info
For more details, see the [official documentation](https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api).
:::

#### Event sequence

When an order is submitted with `price_match`, the following sequence of events occurs:

1. **Order submission**: Nautilus sends the order to Binance with the `priceMatch` parameter but omits the limit price in the API request.
2. **Order acceptance**: Binance accepts the order and determines the actual working price based on the current market and the specified price match mode.
3. **OrderAccepted event**: Nautilus generates an `OrderAccepted` event when the order is confirmed.
4. **OrderUpdated event**: If the Binance-accepted price differs from the original reference price, Nautilus immediately generates an `OrderUpdated` event with the actual working price.
5. **Price synchronization**: The order's limit price in the Nautilus cache is now synchronized with the actual price accepted by Binance.

This ensures that the order price in your system accurately reflects what Binance has accepted, which is critical for position management, risk calculations, and strategy logic.

#### Example

```python
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("65000"),  # Reference price for local risk checks
)

strategy.submit_order(
    order,
    params={"price_match": "QUEUE"},
)
```

:::note
After submission, if Binance accepts the order at a different price (e.g., 64,995.50), you will receive both an `OrderAccepted` event followed by an `OrderUpdated` event with the new price.
:::

### Trailing stops

For trailing stop market orders on Binance:

- Use `activation_price` (optional) to specify when the trailing mechanism activates
- When omitted, Binance uses the current market price at submission time
- Use `trailing_offset` for the callback rate (in basis points)

:::warning
Do not use `trigger_price` for trailing stop orders - it will fail with an error. Use `activation_price` instead.
:::

## Order books

Order books can be maintained at full or partial depths depending on the
subscription. WebSocket stream throttling is different between Spot and Futures exchanges,
Nautilus will use the highest streaming rate possible:

Order books can be maintained at full or partial depths based on the subscription settings.
WebSocket stream update rates differ between Spot and Futures exchanges, with Nautilus using the
highest available streaming rate:

- **Spot**: 100ms
- **Futures**: 0ms (*unthrottled*)

There is a limitation of one order book per instrument per trader instance.
As stream subscriptions may vary, the latest order book data (deltas or snapshots)
subscription will be used by the Binance data client.

Order book snapshot rebuilds will be triggered on:

- Initial subscription of the order book data.
- Data websocket reconnects.

The sequence of events is as follows:

- Deltas will start buffered.
- Snapshot is requested and awaited.
- Snapshot response is parsed to `OrderBookDeltas`.
- Snapshot deltas are sent to the `DataEngine`.
- Buffered deltas are iterated, dropping those where the sequence number is not greater than the last delta in the snapshot.
- Deltas will stop buffering.
- Remaining deltas are sent to the `DataEngine`.

## Binance data differences

The `ts_event` field value for `QuoteTick` objects will differ between Spot and Futures exchanges,
where the former does not provide an event timestamp, so the `ts_init` is used (which means `ts_event` and `ts_init` are identical).

## Binance specific data

It's possible to subscribe to Binance specific data streams as they become available to the
adapter over time.

:::note
Bars are not considered 'Binance specific' and can be subscribed to in the normal way.
As more adapters are built out which need for example mark price and funding rate updates, then these
methods may eventually become first-class (not requiring custom/generic subscriptions as below).
:::

### `BinanceFuturesMarkPriceUpdate`

You can subscribe to `BinanceFuturesMarkPriceUpdate` (including funding rating info)
data streams by subscribing in the following way from your actor or strategy:

```python
from nautilus_trader.adapters.binance import BinanceFuturesMarkPriceUpdate
from nautilus_trader.model import DataType
from nautilus_trader.model import ClientId

# In your `on_start` method
self.subscribe_data(
    data_type=DataType(BinanceFuturesMarkPriceUpdate, metadata={"instrument_id": self.instrument.id}),
    client_id=ClientId("BINANCE"),
)
```

This will result in your actor/strategy passing these received `BinanceFuturesMarkPriceUpdate`
objects to your `on_data` method. You will need to check the type, as this
method acts as a flexible handler for all custom/generic data.

```python
from nautilus_trader.core import Data

def on_data(self, data: Data):
    # First check the type of data
    if isinstance(data, BinanceFuturesMarkPriceUpdate):
        # Do something with the data
```

## Rate limiting

Binance uses an interval-based rate limiting system where request weight is tracked per fixed time window (e.g., every minute resets at :00 seconds). The adapter uses token bucket rate limiters to approximate this behavior, helping to reduce the risk of quota violations while maintaining high throughput for normal trading operations.

| Key / Endpoint        | Limit (weight/min) | Notes                                                 |
|-----------------------|--------------------|-------------------------------------------------------|
| `binance:global`      | Spot: 6,000<br>Futures: 2,400 | Default bucket applied to every request.   |
| `/api/v3/order`       | 3,000              | Spot order placement.                                 |
| `/api/v3/allOrders`   | 150                | Spot all-orders endpoint (20× weight multiplier).     |
| `/api/v3/klines`      | 600                | Spot historical klines.                               |
| `/fapi/v1/order`      | 1,200              | Futures order placement.                              |
| `/fapi/v1/allOrders`  | 60                 | Futures historical orders (20× multiplier).           |
| `/fapi/v1/klines`     | 600                | Futures historical klines.                            |

Binance assigns request weight dynamically (e.g. `/klines` scales with `limit`). The quotas above mirror the static limits but the client still draws a single token per call, so long history pulls may need manual pacing to respect the live `X-MBX-USED-WEIGHT-*` headers.

:::warning
Binance returns HTTP 429 when you exceed the allowed weight and repeated bursts can trigger temporary IP bans, so leave enough headroom between batches.
:::

:::info
For more details on rate limiting, see the official documentation: <https://binance-docs.github.io/apidocs/futures/en/#limits>.
:::

## Configuration

### Data client configuration options

| Option                             | Default | Description |
|------------------------------------|---------|-------------|
| `venue`                            | `BINANCE` | Venue identifier used when registering the client. |
| `api_key`                          | `None`  | Binance API key; loaded from environment variables when omitted. |
| `api_secret`                       | `None`  | Binance API secret; loaded from environment variables when omitted. |
| `key_type`                         | `HMAC`  | Cryptographic key type (`HMAC`, `RSA`, or `ED25519`). |
| `account_type`                     | `SPOT`  | Account type for data endpoints (spot, margin, USDT futures, coin futures). |
| `base_url_http`                    | `None`  | Override for the HTTP REST base URL. |
| `base_url_ws`                      | `None`  | Override for the WebSocket base URL. |
| `us`                               | `False` | Route requests to Binance US endpoints when `True`. |
| `testnet`                          | `False` | Use Binance testnet endpoints when `True`. |
| `update_instruments_interval_mins` | `60`    | Interval (minutes) between instrument catalogue refreshes. |
| `use_agg_trade_ticks`              | `False` | When `True`, subscribe to aggregated trade ticks instead of raw trades. |

### Execution client configuration options

| Option                               | Default | Description |
|--------------------------------------|---------|-------------|
| `venue`                              | `BINANCE` | Venue identifier used when registering the client. |
| `api_key`                            | `None`  | Binance API key; loaded from environment variables when omitted. |
| `api_secret`                         | `None`  | Binance API secret; loaded from environment variables when omitted. |
| `key_type`                           | `HMAC`  | Cryptographic key type (`HMAC`, `RSA`, or `ED25519`). |
| `account_type`                       | `SPOT`  | Account type for order placement (spot, margin, USDT futures, coin futures). |
| `base_url_http`                      | `None`  | Override for the HTTP REST base URL. |
| `base_url_ws`                        | `None`  | Override for the WebSocket base URL. |
| `us`                                 | `False` | Route requests to Binance US endpoints when `True`. |
| `testnet`                            | `False` | Use Binance testnet endpoints when `True`. |
| `use_gtd`                            | `True`  | When `False`, remaps GTD orders to GTC for local expiry management. |
| `use_reduce_only`                    | `True`  | When `True`, passes through `reduce_only` instructions to Binance. |
| `use_position_ids`                   | `True`  | Enable Binance hedging position IDs; set `False` for virtual hedging. |
| `use_trade_lite`                     | `False` | Use TRADE_LITE execution events that include derived fees. |
| `treat_expired_as_canceled`          | `False` | Treat `EXPIRED` execution types as `CANCELED` when `True`. |
| `recv_window_ms`                     | `5,000` | Receive window (milliseconds) for signed REST requests. |
| `max_retries`                        | `None`  | Maximum retry attempts for order submission/cancel/modify calls. |
| `retry_delay_initial_ms`             | `None`  | Initial delay (milliseconds) between retry attempts. |
| `retry_delay_max_ms`                 | `None`  | Maximum delay (milliseconds) between retry attempts. |
| `futures_leverages`                  | `None`  | Mapping of `BinanceSymbol` to initial leverage for futures accounts. |
| `futures_margin_types`               | `None`  | Mapping of `BinanceSymbol` to futures margin type (isolated/cross). |
| `listen_key_ping_max_failures`       | `3`     | Consecutive listen key ping failures allowed before recovery triggers. |
| `log_rejected_due_post_only_as_warning` | `True` | Log post-only rejections as warnings when `True`; otherwise as errors. |

The most common use case is to configure a live `TradingNode` to include Binance
data and execution clients. To achieve this, add a `BINANCE` section to your client
configuration(s):

```python
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future, coin_future}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
    exec_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future, coin_future}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.add_exec_client_factory(BINANCE, BinanceLiveExecClientFactory)

# Finally build the node
node.build()
```

### Key types

Binance supports multiple cryptographic key types for API authentication:

- **HMAC** (default): Uses HMAC-SHA256 with your API secret
- **RSA**: Uses RSA signature with your private key
- **Ed25519**: Uses Ed25519 signature with your private key

You can specify the key type in your configuration:

```python
from nautilus_trader.adapters.binance import BinanceKeyType

config = TradingNodeConfig(
    data_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",  # For HMAC
            "key_type": BinanceKeyType.ED25519,  # or RSA, HMAC (default)
            "account_type": "spot",
        },
    },
)
```

:::note
Ed25519 keys must be provided in base64-encoded ASN.1/DER format. The implementation automatically extracts the 32-byte seed from the DER structure.
:::

### API credentials

There are multiple options for supplying your credentials to the Binance clients.
Either pass the corresponding values to the configuration objects, or
set the following environment variables:

For Binance live clients (shared between Spot/Margin and Futures), you can set:

- `BINANCE_API_KEY`
- `BINANCE_API_SECRET` (for all key types)

For Binance Spot/Margin testnet clients, you can set:

- `BINANCE_TESTNET_API_KEY`
- `BINANCE_TESTNET_API_SECRET` (for all key types)

For Binance Futures testnet clients, you can set:

- `BINANCE_FUTURES_TESTNET_API_KEY`
- `BINANCE_FUTURES_TESTNET_API_SECRET` (for all key types)

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

### Account type

All the Binance account types will be supported for live trading. Set the `account_type`
using the `BinanceAccountType` enum. The account type options are:

- `SPOT`
- `MARGIN` (Margin shared between open positions)
- `ISOLATED_MARGIN` (Margin assigned to a single position)
- `USDT_FUTURES` (USDT or BUSD stablecoins as collateral)
- `COIN_FUTURES` (other cryptocurrency as collateral)

:::tip
We recommend using environment variables to manage your credentials.
:::

### Base URL overrides

It's possible to override the default base URLs for both HTTP Rest and
WebSocket APIs. This is useful for configuring API clusters for performance reasons,
or when Binance has provided you with specialized endpoints.

### Binance US

There is support for Binance US accounts by setting the `us` option in the configs
to `True` (this is `False` by default). All functionality available to US accounts
should behave identically to standard Binance.

### Testnets

It's also possible to configure one or both clients to connect to the Binance testnet.
Set the `testnet` option to `True` (this is `False` by default):

```python
from nautilus_trader.adapters.binance import BINANCE

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_TESTNET_API_KEY",
            "api_secret": "YOUR_BINANCE_TESTNET_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future}
            "testnet": True,  # If client uses the testnet
        },
    },
    exec_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_TESTNET_API_KEY",
            "api_secret": "YOUR_BINANCE_TESTNET_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future}
            "testnet": True,  # If client uses the testnet
        },
    },
)
```

### Aggregated trades

Binance provides aggregated trade data endpoints as an alternative source of trades.
In comparison to the default trade endpoints, aggregated trade data endpoints can return all
ticks between a `start_time` and `end_time`.

To use aggregated trades and the endpoint features, set the `use_agg_trade_ticks` option
to `True` (this is `False` by default.)

### Parser warnings

Some Binance instruments are unable to be parsed into Nautilus objects if they
contain enormous field values beyond what can be handled by the platform.
In these cases, a *warn and continue* approach is taken (the instrument will not
be available).

These warnings may cause unnecessary log noise, and so it's possible to
configure the provider to not log the warnings, as per the client configuration
example below:

```python
from nautilus_trader.config import InstrumentProviderConfig

instrument_provider=InstrumentProviderConfig(
    load_all=True,
    log_warnings=False,
)
```

### Futures hedge mode

Binance Futures Hedge mode is a position mode where a trader opens positions in both long and short
directions to mitigate risk and potentially profit from market volatility.

To use Binance Future Hedge mode, you need to follow the three items below:

- 1. Before starting the strategy, ensure that hedge mode is configured on Binance.
- 2. Set the `use_reduce_only` option to `False` in BinanceExecClientConfig (this is `True` by default).

    ```python
    from nautilus_trader.adapters.binance import BINANCE

    config = TradingNodeConfig(
        ...,  # Omitted
        data_clients={
            BINANCE: BinanceDataClientConfig(
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURES,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
            ),
        },
        exec_clients={
            BINANCE: BinanceExecClientConfig(
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURES,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
                use_reduce_only=False,  # Must be disabled for Hedge mode
            ),
        }
    )
    ```

- 3. When submitting an order, use a suffix (`LONG` or `SHORT` ) in the `position_id` to indicate the position direction.

    ```python
    class EMACrossHedgeMode(Strategy):
        ...,  # Omitted
        def buy(self) -> None:
            """
            Users simple buy method (example).
            """
            order: MarketOrder = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.instrument.make_qty(self.trade_size),
                # time_in_force=TimeInForce.FOK,
            )

            # LONG suffix is recognized as a long position by Binance adapter.
            position_id = PositionId(f"{self.instrument_id}-LONG")
            self.submit_order(order, position_id)

        def sell(self) -> None:
            """
            Users simple sell method (example).
            """
            order: MarketOrder = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.instrument.make_qty(self.trade_size),
                # time_in_force=TimeInForce.FOK,
            )
            # SHORT suffix is recognized as a short position by Binance adapter.
            position_id = PositionId(f"{self.instrument_id}-SHORT")
            self.submit_order(order, position_id)
    ```

:::info
For additional features or to contribute to the Binance adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
