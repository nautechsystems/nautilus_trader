# Bybit

:::info
We are currently working on this integration guide.
:::

Founded in 2018, Bybit is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products. This integration supports live market data ingest and order
execution with Bybit.

## Installation

To install NautilusTrader with Bybit support:

```bash
pip install --upgrade "nautilus_trader[bybit]"
```

To build from source with all extras (including Bybit):

```bash
uv sync --all-extras
```

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bybit/).

## Overview

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Bybit adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BybitHttpClient`: Low-level HTTP API connectivity.
- `BybitWebSocketClient`: Low-level WebSocket API connectivity.
- `BybitInstrumentProvider`: Instrument parsing and loading functionality.
- `BybitDataClient`: A market data feed manager.
- `BybitExecutionClient`: An account management and trade execution gateway.
- `BybitLiveDataClientFactory`: Factory for Bybit data clients (used by the trading node builder).
- `BybitLiveExecClientFactory`: Factory for Bybit execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## Bybit documentation

Bybit provides extensive documentation for users which can be found in the [Bybit help center](https://www.bybit.com/en/help-center).
It’s recommended you also refer to the Bybit documentation in conjunction with this NautilusTrader integration guide.

## Products

A product is an umbrella term for a group of related instrument types.

:::note
Product is also referred to as `category` in the Bybit v5 API.
:::

The following product types are supported on Bybit:

- Spot cryptocurrencies
- Perpetual contracts
- Perpetual inverse contracts
- Futures contracts
- Futures inverse contracts
- Option contracts

## Symbology

To distinguish between different product types on Bybit, Nautilus uses specific product category suffixes for symbols:

- `-SPOT`: Spot cryptocurrencies
- `-LINEAR`: Perpetual and futures contracts
- `-INVERSE`: Inverse perpetual and inverse futures contracts
- `-OPTION`: Option contracts

These suffixes must be appended to the Bybit raw symbol string to identify the specific product type
for the instrument ID. For example:

- The Ether/Tether spot currency pair is identified with `-SPOT`, such as `ETHUSDT-SPOT`.
- The BTCUSDT perpetual futures contract is identified with `-LINEAR`, such as `BTCUSDT-LINEAR`.
- The BTCUSD inverse perpetual futures contract is identified with `-INVERSE`, such as `BTCUSD-INVERSE`.

## Order capability

Bybit offers a flexible combination of trigger types, enabling a broader range of Nautilus orders.
All the order types listed below can be used as *either* entries or exits, except for trailing stops
(which utilize a position-related API).

### Order types

| Order Type             | Spot | Linear | Inverse | Notes                    |
|------------------------|------|--------|---------|--------------------------|
| `MARKET`               | ✓    | ✓      | ✓       |                          |
| `LIMIT`                | ✓    | ✓      | ✓       |                          |
| `STOP_MARKET`          | ✓    | ✓      | ✓       |                          |
| `STOP_LIMIT`           | ✓    | ✓      | ✓       |                          |
| `MARKET_IF_TOUCHED`    | ✓    | ✓      | ✓       |                          |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓      | ✓       |                          |
| `TRAILING_STOP_MARKET` | -    | ✓      | ✓       | Not supported for Spot.  |

### Execution instructions

| Instruction   | Spot | Linear | Inverse | Notes                             |
|---------------|------|--------|---------|-----------------------------------|
| `post_only`   | ✓    | ✓      | ✓       | Only supported on `LIMIT` orders. |
| `reduce_only` | -    | ✓      | ✓       | Not supported for Spot products.  |

### Time in force

| Time in force | Spot | Linear | Inverse | Notes                        |
|---------------|------|--------|---------|------------------------------|
| `GTC`         | ✓    | ✓      | ✓       | Good Till Canceled.          |
| `GTD`         | -    | -      | -       | *Not supported*.             |
| `FOK`         | ✓    | ✓      | ✓       | Fill or Kill.                |
| `IOC`         | ✓    | ✓      | ✓       | Immediate or Cancel.         |

### Advanced order features

| Feature            | Spot | Linear | Inverse | Notes                                  |
|--------------------|------|--------|---------|----------------------------------------|
| Order Modification | ✓    | ✓      | ✓       | Price and quantity modification.       |
| Bracket/OCO Orders | ✓    | ✓      | ✓       | UI only; API users implement manually. |
| Iceberg Orders     | ✓    | ✓      | ✓       | Max 10 per account, 1 per symbol.      |

### Batch operations

| Operation          | Spot | Linear | Inverse | Notes                                  |
|--------------------|------|--------|---------|----------------------------------------|
| Batch Submit       | ✓    | ✓      | ✓       | Submit multiple orders in single request. |
| Batch Modify       | ✓    | ✓      | ✓       | Modify multiple orders in single request. |
| Batch Cancel       | ✓    | ✓      | ✓       | Cancel multiple orders in single request. |

### Position management

| Feature              | Spot | Linear | Inverse | Notes                                    |
|---------------------|------|--------|---------|------------------------------------------|
| Query positions     | -    | ✓      | ✓       | Real-time position updates.              |
| Position mode       | -    | ✓      | ✓       | One-Way vs Hedge mode.                   |
| Leverage control    | -    | ✓      | ✓       | Dynamic leverage adjustment per symbol.  |
| Margin mode         | -    | ✓      | ✓       | Cross vs Isolated margin.                |

### Order querying

| Feature              | Spot | Linear | Inverse | Notes                                    |
|---------------------|------|--------|---------|------------------------------------------|
| Query open orders   | ✓    | ✓      | ✓       | List all active orders.                  |
| Query order history | ✓    | ✓      | ✓       | Historical order data.                   |
| Order status updates| ✓    | ✓      | ✓       | Real-time order state changes.          |
| Trade history       | ✓    | ✓      | ✓       | Execution and fill reports.             |

### Contingent orders

| Feature              | Spot | Linear | Inverse | Notes                                    |
|---------------------|------|--------|---------|------------------------------------------|
| Order lists         | -    | -      | -       | *Not supported*.                         |
| OCO orders          | ✓    | ✓      | ✓       | UI only; API users implement manually.  |
| Bracket orders      | ✓    | ✓      | ✓       | UI only; API users implement manually.  |
| Conditional orders  | ✓    | ✓      | ✓       | Stop and limit-if-touched orders.       |

### Configuration options

The following execution client configuration options affect order behavior:

| Option                       | Default | Description                                          |
|------------------------------|---------|------------------------------------------------------|
| `use_gtd`                    | `False` | GTD is not supported; orders are remapped to GTC for local management. |
| `use_ws_trade_api`           | `False` | If `True`, uses WebSocket for order requests instead of HTTP. |
| `use_http_batch_api`         | `False` | If `True`, uses HTTP batch API when WebSocket trading is enabled. |
| `futures_leverages`          | `None`  | Dict to set leverage for futures symbols. |
| `position_mode`              | `None`  | Dict to set position mode for USDT perpetual and inverse futures. |
| `margin_mode`                | `None`  | Sets margin mode for the account. |

### Order parameters

Individual orders can be customized using the `params` dictionary when submitting orders:

| Parameter     | Type   | Description                                                                    |
|---------------|--------|--------------------------------------------------------------------------------|
| `is_leverage` | `bool` | For SPOT products only. If `True`, enables margin trading (borrowing) for the order. Default: `False`. See [Bybit's isLeverage documentation](https://bybit-exchange.github.io/docs/v5/order/create-order#request-parameters). |

#### Example: SPOT margin trading

```python
# Submit a SPOT order with margin enabled
order = strategy.order_factory.market(
    instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.1"),
    params={"is_leverage": True}  # Enable margin for this order
)
strategy.submit_order(order)
```

:::note
Without `is_leverage=True` in the params, SPOT orders will only use your available balance
and won't borrow funds, even if you have auto-borrow enabled on your Bybit account.
:::

For a complete example of using order parameters including `is_leverage`, see the
[bybit_exec_tester.py](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bybit/bybit_exec_tester.py) example.

### Product-specific limitations

The following limitations apply to SPOT products, as positions are not tracked on the venue side:

- `reduce_only` orders are *not supported*.
- Trailing stop orders are *not supported*.

### Trailing stops

Trailing stops on Bybit do not have a client order ID on the venue side (though there is a `venue_order_id`).
This is because trailing stops are associated with a netted position for an instrument.
Consider the following points when using trailing stops on Bybit:

- `reduce_only` instruction is available
- When the position associated with a trailing stop is closed, the trailing stop is automatically "deactivated" (closed) on the venue side.
- You cannot query trailing stop orders that are not already open (the `venue_order_id` is unknown until then).
- You can manually adjust the trigger price in the GUI, which will update the Nautilus order.

## Configuration

The product types for each client must be specified in the configurations.

### Data clients

If no product types are specified then all product types will be loaded and available.

### Execution clients

The adapter automatically determines the account type based on configured product types:

- **SPOT only**: Uses `CASH` account type with borrowing support enabled
- **Derivatives or mixed products**: Uses `MARGIN` account type (UTA - Unified Trading Account)

This allows you to trade SPOT alongside derivatives in a single Unified Trading Account, which is the standard account type for most Bybit users.

:::info
**Unified Trading Accounts (UTA) and SPOT margin trading**

Most Bybit users now have Unified Trading Accounts (UTA) as Bybit steers new users to this account type.
Classic accounts are considered legacy.

For SPOT margin trading on UTA accounts:

- Borrowing is **NOT automatically enabled** - it requires explicit API configuration
- To use SPOT margin via API, you must submit orders with `is_leverage=True` in the parameters (see [Bybit docs](https://bybit-exchange.github.io/docs/v5/order/create-order#request-parameters))
- If auto-borrow/auto-repay is enabled on your Bybit account, the venue will automatically borrow/repay funds for those margin orders
- Without auto-borrow enabled, you'll need to manually manage borrowing through Bybit's interface

**Important**: The Nautilus Bybit adapter defaults to `is_leverage=False` for SPOT orders,
meaning they won't use margin unless you explicitly enable it.
:::

## Fee currency logic

Understanding how Bybit determines the currency for trading fees is important for accurate accounting and position tracking. The fee currency rules vary between SPOT and derivatives products.

### SPOT trading fees

For SPOT trading, the fee currency depends on the order side and whether the fee is a rebate (negative fee for maker orders):

#### Normal fees (positive)

- **BUY orders**: Fee is charged in the **base currency** (e.g., BTC for BTCUSDT)
- **SELL orders**: Fee is charged in the **quote currency** (e.g., USDT for BTCUSDT)

#### Maker rebates (negative fees)
When maker fees are negative (rebates), the currency logic is **inverted**:

- **BUY orders with maker rebate**: Rebate is paid in the **quote currency** (e.g., USDT for BTCUSDT)
- **SELL orders with maker rebate**: Rebate is paid in the **base currency** (e.g., BTC for BTCUSDT)

:::note
**Taker orders never have inverted logic**, even if the maker fee rate is negative. Taker fees always follow the normal fee currency rules.
:::

#### Example: BTCUSDT SPOT

- **Buy 1 BTC as taker (0.1% fee)**: Pay 0.001 BTC in fees
- **Sell 1 BTC as taker (0.1% fee)**: Pay equivalent USDT in fees
- **Buy 1 BTC as maker (-0.01% rebate)**: Receive USDT rebate (inverted)
- **Sell 1 BTC as maker (-0.01% rebate)**: Receive BTC rebate (inverted)

### Derivatives trading fees

For all derivatives products (LINEAR, INVERSE, OPTION), fees are always charged in the **settlement currency**:

| Product Type | Settlement Currency                   | Fee Currency |
|--------------|---------------------------------------|--------------|
| LINEAR       | USDT (typically)                      | USDT         |
| INVERSE      | Base coin (e.g., BTC for BTCUSD)      | Base coin    |
| OPTION       | USDC (legacy) or USDT (post Feb 2025) | USDC/USDT    |

### Fee calculation

When the WebSocket execution message doesn't provide the exact fee amount (`execFee`), the adapter calculates fees as follows:

#### SPOT products

- **BUY orders**: `fee = base_quantity × fee_rate`
- **SELL orders**: `fee = notional_value × fee_rate` (where `notional_value = quantity × price`)

#### Derivatives

- All derivatives: `fee = notional_value × fee_rate`

### Official documentation

For complete details on Bybit's fee structure and currency rules, refer to:

- [Bybit WebSocket Private Execution](https://bybit-exchange.github.io/docs/v5/websocket/private/execution)
- [Bybit Spot Fee Currency Instruction](https://bybit-exchange.github.io/docs/v5/enum#spot-fee-currency-instruction)

The most common use case is to configure a live `TradingNode` to include Bybit
data and execution clients. To achieve this, add a `BYBIT` section to your client
configuration(s):

```python
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        BYBIT: {
            "api_key": "YOUR_BYBIT_API_KEY",
            "api_secret": "YOUR_BYBIT_API_SECRET",
            "base_url_http": None,  # Override with custom endpoint
            "product_types": [BybitProductType.LINEAR]
            "testnet": False,
        },
    },
    exec_clients={
        BYBIT: {
            "api_key": "YOUR_BYBIT_API_KEY",
            "api_secret": "YOUR_BYBIT_API_SECRET",
            "base_url_http": None,  # Override with custom endpoint
            "product_types": [BybitProductType.LINEAR]
            "testnet": False,
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
node.add_exec_client_factory(BYBIT, BybitLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials

There are two options for supplying your credentials to the Bybit clients.
Either pass the corresponding `api_key` and `api_secret` values to the configuration objects, or
set the following environment variables:

For Bybit live clients, you can set:

- `BYBIT_API_KEY`
- `BYBIT_API_SECRET`

For Bybit demo clients, you can set:

- `BYBIT_DEMO_API_KEY`
- `BYBIT_DEMO_API_SECRET`

For Bybit testnet clients, you can set:

- `BYBIT_TESTNET_API_KEY`
- `BYBIT_TESTNET_API_SECRET`

:::tip
We recommend using environment variables to manage your credentials.
:::

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.
