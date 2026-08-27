# Bybit

Founded in 2018, Bybit is one of the largest cryptocurrency exchanges in terms
of daily trading volume and open interest of crypto assets and crypto
derivative products.

NautilusTrader provides Bybit integration for live market data and execution. The adapter is
implemented in Rust and exposed to Python through the same public configurations, factories, and
data types.

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bybit/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/bybit/examples/)

## Overview

This guide assumes a trader is setting up for both live market data feeds and trade execution.
The Bybit adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BybitDataClientConfig` and `BybitExecutionClientConfig`: Live client configuration.
- `BybitDataClientFactory` and `BybitExecutionClientFactory`: Trading node client factories.
- `BybitDataClient`: A market data feed manager, built by the data client factory.
- `BybitExecutionClient`: An account management and trade execution gateway, built by the execution
  client factory.
- `BybitHttpClient`: Low-level HTTP API connectivity.
- `BybitWebSocketClient`: Low-level WebSocket API connectivity.
- `BYBIT`, `BYBIT_CLIENT_ID`, `BYBIT_VENUE`: Public identifiers.
- `BybitEnvironment`, `BybitProductType`, `BybitMarginMode`, `BybitPositionIdx`,
  `BybitPositionMode`: Public enums used by the configurations and order params.

:::note
Most users define a configuration for a live trading node (as below),
and won't need to work with the lower-level components directly.
:::

## Bybit documentation

Bybit provides extensive documentation for users which can be found in the [Bybit help center](https://www.bybit.com/en/help-center).
It's recommended you also refer to the Bybit documentation in conjunction with this NautilusTrader integration guide.

## Products

A product is an umbrella term for a group of related instrument types.

:::note
Product is also referred to as `category` in the Bybit v5 API.
:::

The following product types are supported on Bybit:

| Product Type                | Supported | Notes                                     |
| --------------------------- | --------- | ----------------------------------------- |
| Spot cryptocurrencies       | ✓         | Native spot markets with margin support.  |
| Linear perpetual contracts  | ✓         | USDT/USDC margined perpetual swaps.       |
| Linear futures contracts    | ✓         | Delivery-settled linear futures.          |
| Inverse perpetual contracts | ✓         | Coin-margined perpetual swaps.            |
| Inverse futures contracts   | ✓         | Coin-margined delivery futures.           |
| Option contracts            | ✓         | European options settled in USDT or USDC. |

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
- A BTC USDT-settled put option: `BTC-27MAR26-70000-P-USDT-OPTION`.
- A ETH USDC-settled call option: `ETH-28FEB25-2800-C-OPTION`.

Bybit's option symbols include the settlement currency for USDT-settled
contracts (e.g. `BTC-27MAR26-70000-P-USDT`) but omit it for USDC-settled
contracts (e.g. `ETH-28FEB25-2800-C`). The adapter appends `-OPTION` to
whatever symbol the API returns.

## Instrument loading

The data and execution clients load all instruments for their configured `product_types` when they
connect. The default is `LINEAR`. Include each product type required by your subscriptions or
orders.

## Environments

Bybit provides three trading environments. Configure the appropriate
environment with the `environment` enum on your client configuration.

| Environment | Config                     | Description                                                      |
| ----------- | -------------------------- | ---------------------------------------------------------------- |
| **Mainnet** | `BybitEnvironment.MAINNET` | Production trading with real funds.                              |
| **Demo**    | `BybitEnvironment.DEMO`    | Practice trading with simulated funds on mainnet infrastructure. |
| **Testnet** | `BybitEnvironment.TESTNET` | Separate test network for development and integration testing.   |

### Mainnet (Production)

The default environment for live trading with real funds.

```python
from nautilus_trader.adapters.bybit import BybitEnvironment
from nautilus_trader.adapters.bybit import BybitExecutionClientConfig

config = BybitExecutionClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    environment=BybitEnvironment.MAINNET,
)
```

Environment variables: `BYBIT_API_KEY`, `BYBIT_API_SECRET`

### Demo trading

Demo trading uses Bybit's mainnet infrastructure with simulated funds.
Create demo API keys from the
[Bybit demo trading page](https://www.bybit.com/en/demo-trading).

```python
from nautilus_trader.adapters.bybit import BybitEnvironment
from nautilus_trader.adapters.bybit import BybitExecutionClientConfig

config = BybitExecutionClientConfig(
    api_key="YOUR_DEMO_API_KEY",
    api_secret="YOUR_DEMO_API_SECRET",
    environment=BybitEnvironment.DEMO,
)
```

Environment variables: `BYBIT_DEMO_API_KEY`, `BYBIT_DEMO_API_SECRET`

:::warning
**Demo environment limitations:**

- The WebSocket Trade API is **not supported** for demo trading. NautilusTrader automatically uses the HTTP REST API for order operations in demo mode, including order lists and batch cancels, which are sent as individual requests.
- Native TP/SL and option params (`order_iv`, `mmp`) on new orders work in demo via the HTTP create-order endpoint.
- The custom TP/SL trigger prices `tp_trigger_price` and `sl_trigger_price` are not supported in demo (orders setting them are denied); the create-order endpoint cannot carry them.
- Demo private streams use `wss://stream-demo.bybit.com`, but public market data uses Bybit's mainnet public stream `wss://stream.bybit.com`.

:::

### Testnet

A separate test network for development and integration testing.

```python
from nautilus_trader.adapters.bybit import BybitEnvironment
from nautilus_trader.adapters.bybit import BybitExecutionClientConfig

config = BybitExecutionClientConfig(
    api_key="YOUR_TESTNET_API_KEY",
    api_secret="YOUR_TESTNET_API_SECRET",
    environment=BybitEnvironment.TESTNET,
)
```

Environment variables: `BYBIT_TESTNET_API_KEY`, `BYBIT_TESTNET_API_SECRET`

:::note
Testnet supports all trading features including the WebSocket Trade API.
It uses completely separate infrastructure from mainnet, so market data
and liquidity differ significantly from production.
:::

When `environment=BybitEnvironment.TESTNET`, the adapter resolves Bybit's
documented testnet endpoints automatically:

- REST API: `https://api-testnet.bybit.com`
- Public WebSocket: `wss://stream-testnet.bybit.com/v5/public/{spot|linear|inverse|option}`
- Private WebSocket: `wss://stream-testnet.bybit.com/v5/private`
- Trade WebSocket: `wss://stream-testnet.bybit.com/v5/trade`

### Testnet setup

To set up a Bybit testnet account and credentials:

1. Open [testnet.bybit.com](https://testnet.bybit.com) in a desktop browser.
2. Create a separate testnet account or sign in to your existing testnet account.
3. Request test coins from **Assets -> Assets Overview -> Request Test Coins**
   so the account has balances for testing.
4. Open **API Management** at
   [testnet.bybit.com/app/user/api-management](https://testnet.bybit.com/app/user/api-management).
5. Click **Create New Key**.
6. Select the required permissions for your use case.
7. Complete the 2FA prompt and copy the API key and secret.
8. Export the credentials in your shell:

   ```bash
   export BYBIT_TESTNET_API_KEY="YOUR_TESTNET_API_KEY"
   export BYBIT_TESTNET_API_SECRET="YOUR_TESTNET_API_SECRET"
   ```

Bybit's current testnet guidance also notes:

- API keys are created on the website, not in the mobile app.
- New users may be unable to create API keys for the first 48 hours after
  registration.
- Testnet is separate from mainnet. Do not deposit real funds into a testnet
  account.
- Bybit currently documents testnet account setup through a desktop browser.

## Orders capability

Bybit offers a flexible combination of trigger types, enabling a broader range of Nautilus orders.
All the order types listed below can be used as *either* entries or exits.

### Order types

| Order Type             | Spot | Linear | Inverse | Option | Notes                                  |
| ---------------------- | ---- | ------ | ------- | ------ | -------------------------------------- |
| `MARKET`               | ✓    | ✓      | ✓       | ✓      | Quote quantity: Spot only.             |
| `LIMIT`                | ✓    | ✓      | ✓       | ✓      |                                        |
| `STOP_MARKET`          | ✓    | ✓      | ✓       | -      | *Not supported for Options*.           |
| `STOP_LIMIT`           | ✓    | ✓      | ✓       | -      | *Not supported for Options*.           |
| `MARKET_IF_TOUCHED`    | ✓    | ✓      | ✓       | -      | *Not supported for Options*.           |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓      | ✓       | -      | *Not supported for Options*.           |
| `TRAILING_STOP_MARKET` | -    | -      | -       | -      | See [Trailing stops](#trailing-stops). |

An order with a type the adapter does not support is denied locally at submission, with an
`UNSUPPORTED_ORDER_TYPE` reason, rather than being sent to the venue.

### Execution instructions

| Instruction   | Spot | Linear | Inverse | Option | Notes                                                             |
| ------------- | ---- | ------ | ------- | ------ | ----------------------------------------------------------------- |
| `post_only`   | ✓    | ✓      | ✓       | ✓      | Limit order types only; sent as Bybit's `PostOnly` time in force. |
| `reduce_only` | -    | ✓      | ✓       | ✓      | *Not supported for Spot*.                                         |

### Time in force

| Time in force | Spot | Linear | Inverse | Option | Notes                           |
| ------------- | ---- | ------ | ------- | ------ | ------------------------------- |
| `GTC`         | ✓    | ✓      | ✓       | ✓      | Good Till Canceled.             |
| `GTD`         | -    | -      | -       | -      | *Not supported*; sent as `GTC`. |
| `FOK`         | ✓    | ✓      | ✓       | ✓      | Fill or Kill.                   |
| `IOC`         | ✓    | ✓      | ✓       | ✓      | Immediate or Cancel.            |

### Advanced order features

| Feature            | Spot | Linear | Inverse | Option | Notes                                  |
| ------------------ | ---- | ------ | ------- | ------ | -------------------------------------- |
| Order Modification | ✓    | ✓      | ✓       | ✓      | Price and quantity modification.       |
| Bracket/OCO Orders | -    | -      | -       | -      | Not implemented; submit legs yourself. |
| Iceberg Orders     | -    | -      | -       | -      | Not implemented.                       |

### Batch operations

| Operation    | Spot | Linear | Inverse | Option | Notes                                     |
| ------------ | ---- | ------ | ------- | ------ | ----------------------------------------- |
| Batch Submit | ✓    | ✓      | ✓       | ✓      | Submit multiple orders in single request. |
| Batch Modify | -    | -      | -       | -      | Not wired into the execution client.      |
| Batch Cancel | ✓    | ✓      | ✓       | ✓      | Cancel multiple orders in single request. |

Batch submit and batch cancel use the trade WebSocket on mainnet and testnet. In demo mode the
adapter falls back to individual HTTP requests, because the demo environment has no trade
WebSocket.

Bybit accepts at most 10 Spot orders, 20 Linear or Inverse orders, or 5 Option orders in one batch
request. Linear, Inverse, and Spot batches consume UID quota per order, while an Option batch
consumes one request. The adapter splits Spot, Linear, and Inverse batches into groups of 10 by
default so one request cannot exceed the standard rolling UID allowance. It splits Option batches
into groups of five. The HTTP batch-cancel method accepts up to 20 Option operations in one call.

### Position management

| Feature          | Spot | Linear | Inverse | Option | Notes                                                       |
| ---------------- | ---- | ------ | ------- | ------ | ----------------------------------------------------------- |
| Query positions  | -    | ✓      | ✓       | ✓      | Real-time position updates.                                 |
| Position mode    | -    | ✓      | ✓       | -      | One-Way only for Options.                                   |
| Leverage control | -    | ✓      | ✓       | -      | Not applicable for Options.                                 |
| Margin mode      | -    | ✓      | ✓       | ✓      | `ISOLATED_MARGIN`, `REGULAR_MARGIN`, or `PORTFOLIO_MARGIN`. |

Set `margin_mode` on the execution client config to apply a `BybitMarginMode` to the account when
the client connects.

#### Hedge mode (BothSides)

Bybit only accepts Both Sides mode on USDT linear perpetuals. Configure the position mode at Bybit,
then pass `position_idx` through the order `params`: `1` for the long side or `2` for the short side.
Use `0` or omit the parameter for one-way mode.

Bybit documents these values in the V5 [switch position mode](https://bybit-exchange.github.io/docs/v5/position/position-mode)
and [place order](https://bybit-exchange.github.io/docs/v5/order/create-order#request-parameters)
APIs.

Orders and reports with `positionIdx=0` (one-way / Merged Single mode) carry no
venue position ID. For hedge-mode indexes `1` and `2`, the adapter maps reports
to venue position IDs ending in `-LONG` and `-SHORT`, and carries the same ID
onto fills when Bybit execution messages do not include `positionIdx`.

In hedge mode `positionIdx` identifies the position being affected, not the trade direction, so a
reduce-only sell resolves to the long index and a reduce-only buy resolves to the short index.

To override, pass `position_idx` via `params`:

```python
params = {"position_idx": 1}  # 0 one-way, 1 long, 2 short
```

### Risk events

| Feature              | Spot | Linear | Inverse | Option | Notes                                                 |
| -------------------- | ---- | ------ | ------- | ------ | ----------------------------------------------------- |
| Liquidation handling | -    | ✓      | ✓       | ✓      | Takeover fills flagged as exchange-generated.         |
| ADL handling         | -    | ✓      | ✓       | ✓      | Auto-deleveraging fills flagged and logged.           |
| ADL rank warnings    | -    | ✓      | ✓       | ✓      | Position reports logged when `adlRankIndicator >= 4`. |

Bybit emits venue-initiated fills with `execType` set to:

- `AdlTrade`: Auto-deleveraging execution. An opposing profitable position was
  selected to close the undercollateralised counterparty after the insurance
  fund could not cover the loss.
- `BustTrade`: Liquidation takeover. The liquidation engine seized the
  position after margin was exhausted.
- `Delivery`: USDC futures delivery.
- `Settle`: Inverse futures settlement.
- `CorporateAction`: Stock split or reverse stock split.

The adapter flags each as exchange-generated and logs a warning containing the
execution ID, symbol, side, quantity, and price. Fills flow through the normal
`FillReport` path; because these orders carry an empty `orderLinkId`, the
execution engine treats them as external and assigns them via
`external_order_claims` (or the `EXTERNAL` strategy by default).

Bybit also publishes an ADL ranking on position updates via the
`adlRankIndicator` field. The range is 0 (flat / no position) to 5 (next to
deleverage). The adapter logs a warning whenever an open position carries a
rank of 4 or higher so you can react before the venue force-closes.

Upstream references:

- [V5 `execType` values](https://bybit-exchange.github.io/docs/v5/enum#exectype)
- [V5 `createType` values](https://bybit-exchange.github.io/docs/v5/enum#createtype)
- [Liquidation mechanism](https://www.bybit.com/en/help-center/article/Liquidation-Process-Derivatives-Trading)
- [Auto-Deleveraging mechanism](https://www.bybit.com/en/help-center/article/Auto-Deleveraging-ADL-Derivatives-Trading)

### Order querying

| Feature              | Spot | Linear | Inverse | Option | Notes                          |
| -------------------- | ---- | ------ | ------- | ------ | ------------------------------ |
| Query open orders    | ✓    | ✓      | ✓       | ✓      | List all active orders.        |
| Query order history  | ✓    | ✓      | ✓       | ✓      | Historical order data.         |
| Order status updates | ✓    | ✓      | ✓       | ✓      | Real-time order state changes. |
| Trade history        | ✓    | ✓      | ✓       | ✓      | Execution and fill reports.    |

### Contingent orders

| Feature            | Spot | Linear | Inverse | Option | Notes                                  |
| ------------------ | ---- | ------ | ------- | ------ | -------------------------------------- |
| Order lists        | ✓    | ✓      | ✓       | ✓      | Submitted as a batch via WebSocket.    |
| OCO orders         | -    | -      | -       | -      | Not implemented; submit legs yourself. |
| Bracket orders     | -    | -      | -       | -      | Not implemented; submit legs yourself. |
| Conditional orders | ✓    | ✓      | ✓       | -      | Stop and limit-if-touched orders.      |

An order list is validated as a unit before any leg is sent. When one leg fails validation, that
leg is denied with its specific reason and the remaining legs are denied with `ORDER_LIST_DENIED`,
so a partially submitted list cannot reach the venue.

### Order parameters

Individual orders can be customized using the `params` dictionary when submitting orders:

| Parameter          | Type             | Description                                                         |
| ------------------ | ---------------- | ------------------------------------------------------------------- |
| `is_leverage`      | `bool`           | Spot only. Enables margin trading (borrowing). Default: `False`.    |
| `take_profit`      | `str` or `float` | TP trigger price. Attaches a native TP to the order.                |
| `stop_loss`        | `str` or `float` | SL trigger price. Attaches a native SL to the order.                |
| `tp_trigger_by`    | `str`            | TP trigger type: `"LastPrice"`, `"IndexPrice"`, or `"MarkPrice"`.   |
| `sl_trigger_by`    | `str`            | SL trigger type: `"LastPrice"`, `"IndexPrice"`, or `"MarkPrice"`.   |
| `tp_order_type`    | `str`            | TP execution type: `"Market"` or `"Limit"`.                         |
| `sl_order_type`    | `str`            | SL execution type: `"Market"` or `"Limit"`.                         |
| `tp_limit_price`   | `str` or `float` | Limit price for TP when `tp_order_type` is `"Limit"`.               |
| `sl_limit_price`   | `str` or `float` | Limit price for SL when `sl_order_type` is `"Limit"`.               |
| `tp_trigger_price` | `str` or `float` | Explicit TP trigger price sent alongside `take_profit`.             |
| `sl_trigger_price` | `str` or `float` | Explicit SL trigger price sent alongside `stop_loss`.               |
| `tpsl_mode`        | `str`            | TP/SL mode: `"Full"` or `"Partial"`.                                |
| `close_on_trigger` | `bool`           | Close the position when TP/SL triggers.                             |
| `position_idx`     | `int`            | Hedge-mode position index. See [Hedge mode](#hedge-mode-bothsides). |
| `bbo_side_type`    | `str`            | Linear/inverse BBO side: `"Queue"` or `"Counterparty"`.             |
| `bbo_level`        | `str` or `int`   | Linear/inverse BBO book level: `"1"` through `"5"`.                 |

Parameters left unset are omitted from the request, so Bybit's own defaults apply.

:::warning
Bybit's `close_on_trigger` parameter is not the generic `close_position` whole-position exit
contract used by the risk engine. The adapter sends the order quantity, and it ignores an unknown
`close_position` parameter. Do not add `BYBIT` to `full_position_exit_venues` based on
`close_on_trigger`; leave the venue unlisted so ordinary quantity and notional checks apply.
:::

The adapter validates these params before emitting `OrderSubmitted` and denies the order with a
`VALIDATION_FAILED` reason when a rule is broken:

- Every TP override field (`tp_trigger_by`, `tp_order_type`, `tp_limit_price`, `tp_trigger_price`)
  requires `take_profit`, and every SL override field likewise requires `stop_loss`.
- `tp_order_type="Limit"` requires `tp_limit_price`, and `tp_limit_price` requires
  `tp_order_type="Limit"`. The same pairing applies to `sl_order_type` and `sl_limit_price`.
- `bbo_side_type` and `bbo_level` must be provided together.

When `take_profit` or `stop_loss` is set without `tpsl_mode`, the adapter sends `Full`. When a TP or
SL price is set without its own `tp_trigger_by` or `sl_trigger_by`, the adapter derives the trigger
type from the order's trigger type.

:::note
On demo, native TP/SL params route through the HTTP create-order endpoint, with one exception:
the custom trigger prices `tp_trigger_price` and `sl_trigger_price` are not supported because that
endpoint cannot carry them, and orders that set either are denied. The `is_leverage` param applies
to Spot products only. See [Bybit's isLeverage documentation](https://bybit-exchange.github.io/docs/v5/order/create-order#request-parameters).
:::

When `bbo_side_type` and `bbo_level` are set, Nautilus sends Bybit's
`bboSideType` and `bboLevel` fields and omits the order price from the API
request. BBO orders are supported for linear and inverse limit, stop-limit, and
limit-if-touched orders.

#### Example: Order with native TP/SL

```python
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTCUSDT-LINEAR.BYBIT"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.01"),
    price=Price.from_str("60000.0"),
    params={
        "take_profit": "65000.0",
        "stop_loss": "58000.0",
        "tp_trigger_by": "LastPrice",
        "sl_trigger_by": "LastPrice",
    },
)
strategy.submit_order(order)
```

#### Example: BBO order

```python
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTCUSDT-LINEAR.BYBIT"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.01"),
    price=Price.from_str("60000.0"),
    params={"bbo_side_type": "Queue", "bbo_level": 1},
)
strategy.submit_order(order)
```

#### Example: Spot margin trading

```python
# Submit a Spot order with margin enabled
order = strategy.order_factory.market(
    instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.1"),
    params={"is_leverage": True},  # Enable margin for this order
)
strategy.submit_order(order)
```

:::note
Without `is_leverage=True` in the params, Spot orders use your available balance
and do not borrow funds, even if you have auto-borrow enabled on your Bybit account.
:::

For a complete example of using order parameters including `is_leverage`, see the
[Python execution tester](https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/live/bybit/exec_tester.py).

### Spot trading limitations

The following limitations apply to Spot products, as positions are not tracked on the venue side:

- `reduce_only` orders are *not supported*.
- Trailing stop orders are *not supported*.

### Options trading

Bybit lists European-style options on BTC and ETH, settled in USDT or USDC.
The adapter uses the `CryptoOption` instrument type and the `-OPTION` symbol
suffix. See the [symbology section](#symbology) for the full symbol format.

#### Options data

The adapter supports real-time options market data through the WebSocket ticker
channel:

| Data type                  | Description                                                              |
| -------------------------- | ------------------------------------------------------------------------ |
| Quotes (bid/ask)           | Top-of-book prices and sizes for each option contract.                   |
| Greeks                     | Delta, gamma, vega, theta, plus bid/ask/mark IV. Bybit publishes no rho. |
| Mark price                 | Exchange mark price for each option contract.                            |
| Index price                | Underlying index price.                                                  |
| Underlying (forward) price | Per-expiry forward price, used for ATM determination.                    |
| Open interest              | Per-contract open interest.                                              |
| Order book deltas          | L2 MBP updates from the option orderbook stream.                         |

Subscribe to per-instrument Greeks or aggregate them into option chain
snapshots with ATM-relative strike filtering. See the
[options concept guide](../concepts/options.md) for subscription patterns and
the [options data tutorial](../tutorials/options_data_bybit.md) for a
step-by-step walkthrough. NautilusTrader builds the option chain view locally
from Bybit's per-contract option market data.

Bar (kline) data is not available for options. Bybit does not provide kline
streams for this product type.

#### Options order parameters

In addition to the standard order parameters, option orders accept:

| Parameter  | Type             | Description                                                      |
| ---------- | ---------------- | ---------------------------------------------------------------- |
| `order_iv` | `str` or `float` | Place or amend the order by implied volatility instead of price. |
| `mmp`      | `bool`           | Enable Market Maker Protection for the order.                    |

These parameters are passed through `params` on `SubmitOrder`. On mainnet they flow through the
WebSocket trade channel; on demo they route through the HTTP create-order endpoint. Amending an
existing order by `order_iv` is not supported in demo mode.

#### Options trading limitations

- Amending an order by implied volatility (`order_iv`) and other WS-trade-only features are not supported in demo mode.
- Leverage is not configurable. Option buyers pay premium; sellers post margin.
- Position mode is one-way only. Hedge mode is not supported.
- Conditional order types (`STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`,
  `LIMIT_IF_TOUCHED`) are not supported.
- Trading stops (TP/SL on positions) are not supported.
- Funding rates do not apply to options.
- Options require a Unified Trading Account (UTA).

### Trailing stops

The adapter does not submit Nautilus `TRAILING_STOP_MARKET` orders to Bybit. Submitting one denies
the order locally with an `UNSUPPORTED_ORDER_TYPE` reason.

Bybit models trailing stops as an attribute of a netted position rather than as an order, so a
trailing stop has no client order ID on the venue side and cannot be queried until it is already
open. Attach a trailing stop through the Bybit interface if you need one, and manage the resulting
position exit outside Nautilus.

## Spot margin borrowing and repayment

NautilusTrader provides automated spot margin borrow repayment functionality to prevent interest accrual after closing short positions on Bybit.

### Background

When trading Spot with margin enabled (`is_leverage=True`), Bybit automatically borrows coins when you execute short positions.
However, after you close the short position (BUY order fills), the borrowed coins are **NOT automatically repaid** - they continue accruing hourly interest charges until manually repaid.
This can result in significant interest costs if left unattended.

### Automatic repayment (recommended)

The execution client can automatically repay spot margin borrows after BUY orders fully fill
on Spot instruments. This feature is disabled by default, so set
`auto_repay_spot_borrows=True` to opt in.

**How it works:**

1. When a Spot BUY order fully fills on the standard `execution` channel, the execution client
   attempts to repay the base coin borrow.
1. The repayment is capped at the lesser of the outstanding borrow and the base quantity acquired
   across the order's executions.
1. The execution client uses Bybit's converting repay endpoint to cover base-denominated trading
   fees. For MNT, which Bybit excludes from converting repayment, it uses no-convert repay and
   subtracts MNT-denominated fees from the amount.
1. A failed request or `FA` result status is logged without crashing the execution client. A `P`
   result status is logged as processing, not complete.
1. The execution client defers queued repayments during Bybit's UTC blackout window.

**Example:**

```python
from nautilus_trader.adapters.bybit import BybitExecutionClientConfig

config = BybitExecutionClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    product_types=[BybitProductType.SPOT],
    auto_repay_spot_borrows=True,  # Opt in; default is False
)
```

### UTC blackout window

Bybit blocks both repayment endpoints from **4 minutes through 5 minutes 30 seconds past every UTC
hour** for interest calculation. Auto-repayment keeps the request queued and attempts it at 5
minutes 31 seconds past the hour.

### Auto-repayment configuration

| Option                    | Type   | Default | Description                                                                                                                |
| ------------------------- | ------ | ------- | -------------------------------------------------------------------------------------------------------------------------- |
| `auto_repay_spot_borrows` | `bool` | `False` | If `True`, automatically repay Spot margin borrows after BUY orders fully fill. Repayment is deferred during the blackout. |

### Auto-repayment notes

- Auto-repayment only triggers on **Spot BUY orders**, not derivatives.
- Repayment uses converting repayment except for MNT, which uses no-convert repayment.
- Bybit documents the endpoint restrictions and result statuses in
  [Manual Repay](https://bybit-exchange.github.io/docs/v5/account/repay) and
  [Manual Repay Without Asset Conversion](https://bybit-exchange.github.io/docs/v5/account/no-convert-repay).
- Manual borrowing is still required before opening short positions unless auto-borrow is enabled
  on your Bybit account.

## Funding rates

The adapter receives funding rate data from the
[Linear Ticker](https://bybit-exchange.github.io/docs/v5/websocket/public/ticker#linear-inverse-perpetual-response)
WebSocket stream. Bybit provides the `fundingIntervalHour` field in ticker updates,
which the adapter uses to populate the `interval` field on `FundingRateUpdate`.

The adapter caches the last known `fundingIntervalHour` per symbol so that partial
ticker updates (which may omit the field) still carry the correct interval.

For historical funding rate requests, the adapter computes the interval from consecutive
funding timestamps. The oldest record in a response has no earlier timestamp to pair with,
so its interval is unset.

## Rate limiting

The adapter queues requests against exact rolling windows before it creates an authentication
timestamp or signature. HTTP clients and the trade WebSocket share UID state for the same API key
and environment. Data and execution clients also share IP state when they use the same origin and
proxy.

| Scope                         | Bybit limit                           | Adapter behavior                              |
| ----------------------------- | ------------------------------------- | --------------------------------------------- |
| HTTP IP                       | 600 requests per 5 seconds            | Shared by origin and proxy                    |
| HTTP and trade WebSocket UID  | Varies by endpoint and product        | Shared by API key and environment             |
| Trade WebSocket IP            | 3,000 requests per second             | Shared by WebSocket origin and proxy          |
| WebSocket connection attempts | 500 attempts per 5 minutes per domain | Shared across initial connects and reconnects |
| Option subscriptions          | 2,000 arguments per connection        | Rejected before subscription state changes    |

The UID limiter includes the documented lower-rate account and user routes, 50-request read
routes, product-specific order routes, cancel-all limits, and weighted batch operations. HTTP and
trade WebSocket responses update the configured UID limit from `X-Bapi-Limit`, track
`X-Bapi-Limit-Status`, and honor a future `X-Bapi-Limit-Reset-Timestamp` when the remaining count
reaches zero.

The execution client's `recv_window_ms` applies to signed REST requests and trade WebSocket order
commands. A queued WebSocket order gets its timestamp and receive-window header only after all
applicable quotas allow the send. A reconnect retry rebuilds the command with a fresh header and
uses a connection-bound write, so the transport cannot replay a stale order payload.

:::warning
Bybit returns `retCode` `10006` ("Too many visits") when the API rate limit is exceeded.
Exceeding the IP ceiling of 600 requests per 5 seconds returns HTTP 403 and bans the IP for at
least 10 minutes. A matching 403 discards the affected pooled HTTP session and starts a shared
10-minute cooldown. Other 403 responses do not activate the cooldown.
:::

:::warning
Coordination is process-local. Another process or host using the same API key or public IP can
consume venue quota that this adapter cannot reserve in advance. Response headers reduce this
gap for UID limits, but separate processes still require operational coordination.
:::

Explicit venue rate-limit responses are terminal rejections for the affected order operation.
Transport timeouts, service restarts, and duplicate request identifiers remain subject to order
reconciliation because they do not prove whether the venue accepted the order.

:::info
For more details on rate limiting, see the official documentation: <https://bybit-exchange.github.io/docs/v5/rate-limit>.
:::

## Account types

The execution client factory determines the account type and OMS type from the configured product
types:

- **Spot only**: `CASH` account type with a `HEDGING` OMS type.
- **Derivatives or mixed products**: `MARGIN` account type (UTA - Unified Trading Account) with a
  `NETTING` OMS type.

This allows you to trade Spot alongside derivatives in a single Unified Trading Account, which is the standard account type for most Bybit users.

:::info
**Unified Trading Accounts (UTA) and Spot margin trading**

Most Bybit users now have Unified Trading Accounts (UTA) as Bybit steers new users to this account type.
Classic accounts are considered legacy.

For Spot margin trading on UTA accounts:

- Borrowing is **NOT automatically enabled** - it requires explicit API configuration
- To use Spot margin via API, you must submit orders with `is_leverage=True` in the parameters (see [Bybit docs](https://bybit-exchange.github.io/docs/v5/order/create-order#request-parameters))
- If auto-borrow/auto-repay is enabled on your Bybit account, the venue will automatically borrow/repay funds for those margin orders
- Without auto-borrow enabled, you'll need to manually manage borrowing through Bybit's interface

**Important**: The Nautilus Bybit adapter defaults to `is_leverage=False` for Spot orders,
meaning they won't use margin unless you explicitly enable it.
:::

## Fee currency logic

Understanding how Bybit determines the currency for trading fees is important for accurate accounting and position tracking. The fee currency rules vary between Spot and derivatives products.

The adapter takes the commission amount and currency directly from the venue's `execFee` and
`feeCurrency` fields, so the rules below describe what Bybit reports rather than a local
calculation.

### Spot trading fees

For Spot trading, the fee currency depends on the order side and whether the fee is a rebate (negative fee for maker orders):

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

#### Example: BTCUSDT Spot

- **Buy 1 BTC as taker (0.1% fee)**: Pay 0.001 BTC in fees
- **Sell 1 BTC as taker (0.1% fee)**: Pay equivalent USDT in fees
- **Buy 1 BTC as maker (-0.01% rebate)**: Receive USDT rebate (inverted)
- **Sell 1 BTC as maker (-0.01% rebate)**: Receive BTC rebate (inverted)

### Derivatives trading fees

For all derivatives products (LINEAR, INVERSE, OPTION), fees are always charged in the **settlement currency**:

| Product Type | Settlement Currency              | Fee Currency |
| ------------ | -------------------------------- | ------------ |
| LINEAR       | USDT (typically)                 | USDT         |
| INVERSE      | Base coin (e.g., BTC for BTCUSD) | Base coin    |
| OPTION       | USDT or USDC                     | Settle coin  |

### Missing fee data

Bybit's `execution.fast` private channel omits the fee and execution type fields. Fill reports
parsed from that channel therefore carry zero commission. Subscribe to the standard `execution`
channel when exact fee data is required.

### Official documentation

For complete details on Bybit's fee structure and currency rules, refer to:

- [Bybit WebSocket Private Execution](https://bybit-exchange.github.io/docs/v5/websocket/private/execution)
- [Bybit Spot Fee Currency Instruction](https://bybit-exchange.github.io/docs/v5/enum#spot-fee-currency-instruction)

## Configuration

The product types for each client must be specified in the configurations.

### Data client configuration options

| Option                             | Default    | Description                                                                                                     |
| ---------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------- |
| `product_types`                    | `[LINEAR]` | Sequence of `BybitProductType` values to enable.                                                                |
| `environment`                      | `MAINNET`  | Bybit environment enum. Use `BybitEnvironment.MAINNET`, `BybitEnvironment.DEMO`, or `BybitEnvironment.TESTNET`. |
| `api_key`                          | `None`     | API key; loaded from the matching environment variable when omitted.                                            |
| `api_secret`                       | `None`     | API secret; loaded from the matching environment variable when omitted.                                         |
| `base_url_http`                    | `None`     | Override for the REST base URL.                                                                                 |
| `base_url_ws_public`               | `None`     | Override for the public WebSocket URL.                                                                          |
| `base_url_ws_private`              | `None`     | Override for the private WebSocket URL.                                                                         |
| `proxy_url`                        | `None`     | Optional proxy URL for HTTP and WebSocket transports.                                                           |
| `http_timeout_secs`                | `60`       | Timeout (seconds) for REST requests.                                                                            |
| `max_retries`                      | `3`        | Maximum retry attempts for REST requests.                                                                       |
| `retry_delay_initial_ms`           | `1,000`    | Initial retry delay (milliseconds).                                                                             |
| `retry_delay_max_ms`               | `10,000`   | Maximum retry delay (milliseconds).                                                                             |
| `heartbeat_interval_secs`          | `20`       | Heartbeat interval (seconds) for WebSocket clients.                                                             |
| `recv_window_ms`                   | `5,000`    | Receive window (milliseconds) for signed REST requests.                                                         |
| `update_instruments_interval_mins` | `60`       | Interval (minutes) between instrument catalog refreshes.                                                        |
| `instrument_status_poll_secs`      | `60`       | Interval (seconds) between instrument and status polls; `0` disables polling.                                   |
| `transport_backend`                | `Sockudo`  | WebSocket transport backend.                                                                                    |

### Execution client configuration options

| Option                      | Default    | Description                                                                                                     |
| --------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------- |
| `product_types`             | `[LINEAR]` | Sequence of `BybitProductType` values to enable.                                                                |
| `environment`               | `MAINNET`  | Bybit environment enum. Use `BybitEnvironment.MAINNET`, `BybitEnvironment.DEMO`, or `BybitEnvironment.TESTNET`. |
| `api_key`                   | `None`     | API key; loaded from the matching environment variable when omitted.                                            |
| `api_secret`                | `None`     | API secret; loaded from the matching environment variable when omitted.                                         |
| `base_url_http`             | `None`     | Override for the REST base URL.                                                                                 |
| `base_url_ws_private`       | `None`     | Override for the private WebSocket base URL.                                                                    |
| `base_url_ws_trade`         | `None`     | Override for the trade WebSocket base URL.                                                                      |
| `proxy_url`                 | `None`     | Optional proxy URL for HTTP and WebSocket transports.                                                           |
| `http_timeout_secs`         | `60`       | Timeout (seconds) for REST requests.                                                                            |
| `max_retries`               | `3`        | Maximum retry attempts for REST requests.                                                                       |
| `retry_delay_initial_ms`    | `1,000`    | Initial retry delay (milliseconds).                                                                             |
| `retry_delay_max_ms`        | `10,000`   | Maximum retry delay (milliseconds).                                                                             |
| `heartbeat_interval_secs`   | `5`        | Heartbeat interval (seconds) for WebSocket clients.                                                             |
| `auth_timeout_secs`         | `None`     | Optional WebSocket authentication timeout (seconds).                                                            |
| `recv_window_ms`            | `5,000`    | Receive window (milliseconds) for signed REST and trade WebSocket requests.                                     |
| `account_id`                | `None`     | Optional account ID associated with this client.                                                                |
| `use_spot_position_reports` | `False`    | Report Spot wallet balances as positions for scoped requests; bulk reports omit Spot (no pair attribution).     |
| `auto_repay_spot_borrows`   | `False`    | Automatically repay tracked Spot margin borrows after BUY orders fully fill.                                    |
| `margin_mode`               | `None`     | Unified margin mode setting for the account.                                                                    |
| `transport_backend`         | `Sockudo`  | WebSocket transport backend.                                                                                    |

The compiled default is Sockudo when the `transport-sockudo` Cargo feature is enabled and
Tungstenite otherwise.

Use `BybitDataClientConfig` with `BybitDataClientFactory` and `BybitExecutionClientConfig` with
`BybitExecutionClientFactory`. The current Python examples show the complete
`LiveNode.builder(...)` configuration for data and execution clients.

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

## Contributing

:::info
For additional features or to contribute to the Bybit adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
