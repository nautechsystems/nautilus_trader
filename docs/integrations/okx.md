# OKX

Founded in 2017, OKX is a cryptocurrency exchange that offers spot, margin, perpetual
swap, futures, options, and event contract trading. This integration supports live
market data ingest and order execution on OKX.

## Overview

This adapter is written in Rust, with optional Python bindings for Python workflows.
It does not require external OKX client libraries. The core components are compiled as
a static library and linked automatically during the build.

## Examples

Live example scripts are available in
[examples/live/okx](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/okx/).

### Product support

| Product type      | Data feed | Trading | Notes                                               |
|-------------------|-----------|---------|-----------------------------------------------------|
| Spot              | ✓         | ✓       | Spot trading pairs.                                 |
| Margin            | ✓         | ✓       | Spot trading with margin or leverage.               |
| Perpetual swaps   | ✓         | ✓       | Linear and inverse contracts.                       |
| Futures           | ✓         | ✓       | Dated futures contracts.                            |
| Options           | ✓         | ✓       | Limit‑style orders only; no market or conditional.  |
| Event contracts   | ✓         | ✓       | Parsed as Nautilus `BinaryOption` instruments.      |

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

The OKX adapter includes multiple components, which can be used separately or together:

- `OKXHttpClient`: Low-level HTTP API connectivity.
- `OKXWebSocketClient`: Low-level WebSocket API connectivity.
- `OKXInstrumentProvider`: Instrument parsing and loading functionality.
- `OKXDataClient`: Market data feed manager.
- `OKXExecutionClient`: Account management and trade execution gateway.
- `OKXLiveDataClientFactory`: Factory for OKX data clients (used by the trading node builder).
- `OKXLiveExecClientFactory`: Factory for OKX execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as shown below),
and won't need to work directly with these lower-level components.
:::

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

- `BTC-USD-251226` - Bitcoin futures expiring December 26, 2025
- `ETH-USD-251226` - Ethereum futures expiring December 26, 2025
- `BTC-USD-250328` - Bitcoin futures expiring March 28, 2025

Note: Futures are typically inverse contracts (coin-margined).

#### OPTIONS

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}-{Strike}-{Type}`

Examples:

- `BTC-USD-251226-100000-C` - Bitcoin call option, $100,000 strike, expiring December 26, 2025
- `BTC-USD-251226-100000-P` - Bitcoin put option, $100,000 strike, expiring December 26, 2025
- `ETH-USD-251226-4000-C` - Ethereum call option, $4,000 strike, expiring December 26, 2025

Where:

- `C` = Call option
- `P` = Put option

#### EVENTS

OKX event contract instrument IDs use the market ID returned by the OKX instruments API.
The adapter represents these markets as Nautilus `BinaryOption` instruments.

Example:

- `BTC-ABOVE-DAILY-260224-1600-65000` - Event contract market in the
  `BTC-ABOVE-DAILY` series.

### Common questions

**Q: How do I subscribe to spot Bitcoin USD?**
A: Use `BTC-USDT.OKX` for USDT-margined spot or `BTC-USDC.OKX` for USDC-margined spot.

**Q: What's the difference between BTC-USDT-SWAP and BTC-USD-SWAP?**
A: `BTC-USDT-SWAP` is a linear perpetual (USDT-margined), while `BTC-USD-SWAP` is an inverse perpetual (BTC-margined).

**Q: How do I know which contract type to use?**
A: Check the `contract_types` parameter in the configuration:

- For linear contracts: `OKXContractType.LINEAR`.
- For inverse contracts: `OKXContractType.INVERSE`.

**Q: How do I load event contracts?**
A: Use `OKXInstrumentType.EVENTS`. To scope loading, pass OKX `seriesId` values such as
`BTC-ABOVE-DAILY` through `instrument_families`.

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

:::note
OKX has specific requirements for client order IDs:

- **No hyphens allowed**: OKX does not accept hyphens (`-`) in client order IDs.
- Maximum length: 32 characters.
- Allowed characters: letters and numbers only.

When configuring your strategy, ensure you set:

```python
use_hyphens_in_client_order_ids=False
```

:::

### Order types

| Order type             | Linear perpetual swap | Notes                                                         |
|------------------------|-----------------------|---------------------------------------------------------------|
| `MARKET`               | ✓                     | Immediate execution at market price. Supports quote quantity. |
| `MARKET_TO_LIMIT`      | ✓                     | Market order converted to IOC limit.                          |
| `LIMIT`                | ✓                     | Execution at specified price or better.                       |
| `STOP_MARKET`          | ✓                     | Conditional market order through OKX algo orders.             |
| `STOP_LIMIT`           | ✓                     | Conditional limit order through OKX algo orders.              |
| `MARKET_IF_TOUCHED`    | ✓                     | Conditional market order through OKX algo orders.             |
| `LIMIT_IF_TOUCHED`     | ✓                     | Conditional limit order through OKX algo orders.              |
| `TRAILING_STOP_MARKET` | ✓                     | Trailing stop market order through OKX advance algo orders.   |

:::info
**Conditional orders**: `STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`,
`LIMIT_IF_TOUCHED`, and `TRAILING_STOP_MARKET` use OKX algo orders. The
`TRAILING_STOP_MARKET` path uses OKX's advance algo order API (`move_order_stop`) and
requires the `cancel-advance-algos` endpoint for cancellation.
:::

### Quantity semantics for spot margin trading

When using spot margin trading (`use_spot_margin=True`), OKX interprets order
quantities differently depending on the order side:

- **Limit** orders interpret `quantity` as the number of base currency units.
- **Market SELL** orders also use base-unit quantities.
- **Market BUY** orders interpret `quantity` as quote notional (e.g., USDT).

:::warning
**When submitting spot margin market BUY orders**, set `quote_quantity=True` on the order (or
pre-compute the quote-denominated amount). The OKX execution client denies base-denominated
market buy orders for spot margin to prevent unintended fills.

**On the first fill**, the order quantity is automatically updated from the quote quantity to the
actual base quantity received, reflecting the executed trade.
:::

```python
# Spot margin market BUY with quote quantity (spend $100 USDT)
order = strategy.order_factory.market(
    instrument_id=instrument_id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(100.0),
    quote_quantity=True,  # Interpret as USDT notional
)
strategy.submit_order(order)
```

### Execution instructions

| Instruction   | Linear perpetual swap | Notes                  |
|---------------|-----------------------|------------------------|
| `post_only`   | ✓                     | Only for limit orders. |
| `reduce_only` | ✓                     | Only for derivatives.  |

### Time in force

| Time in force | Linear perpetual swap | Notes                                             |
|---------------|-----------------------|---------------------------------------------------|
| `GTC`         | ✓                     | Good Till Canceled.                               |
| `FOK`         | ✓                     | Fill or Kill.                                     |
| `IOC`         | ✓                     | Immediate or Cancel.                              |
| `GTD`         | -                     | *No native OKX order time‑in‑force.*              |

:::note
**GTD (Good Till Date) time in force**: OKX supports request expiry through `expTime`,
but that is a request timeout rather than a native order expiry instruction.

If you need GTD functionality, use Nautilus's strategy-managed GTD feature. It handles
order expiration by canceling the order at the specified expiry time.
:::

### Batch operations

| Operation          | Linear perpetual swap | Notes                                     |
|--------------------|-----------------------|-------------------------------------------|
| Batch Submit       | ✓                     | Submit multiple orders in single request. |
| Batch Modify       | ✓                     | Modify multiple orders in single request. |
| Batch Cancel       | ✓                     | Cancel multiple orders in single request. |

### Position management

| Feature           | Linear perpetual swap | Notes                                                |
|-------------------|-----------------------|------------------------------------------------------|
| Query positions   | ✓                     | Real‑time position updates.                          |
| Position mode     | ✓                     | Net vs Long/Short mode (see below).                  |
| Leverage control  | ✓                     | Dynamic leverage adjustment per instrument.          |
| Margin mode       | ✓                     | Supports cash, isolated, and cross modes.            |

#### Position modes

OKX supports two position modes for derivatives trading:

- **Net mode** (netting): One position per instrument. Buy and sell orders net against
  each other. This is the default and recommended mode for most traders.
- **Long/Short mode** (hedging): Separate long and short positions for the same
  instrument. This mode supports simultaneous long and short exposure.

:::note
Position mode must be configured through the OKX web or app interface and applies
account-wide. The adapter detects the current position mode and handles position
reporting accordingly.
:::

### Trade modes and margin configuration

OKX's unified account system supports different trade modes for spot and derivatives
trading. The adapter determines the correct trade mode from your configuration and
instrument type.

:::note
**Important**: Configure the account mode first through the OKX Web or app interface.
The API cannot set the account mode for the first time.
:::

For more details on OKX account modes and margin, see the
[OKX Account Mode documentation](https://www.okx.com/docs-v5/en/#overview-account-mode).

#### Trade modes overview

OKX supports multiple account modes. For orders, the adapter selects one of the `cash`,
`isolated`, or `cross` trade modes from your configuration:

| Mode           | Used for                       | Leverage | Borrowing | Configuration                         |
|----------------|--------------------------------|----------|-----------|---------------------------------------|
| **`cash`**     | Spot trading without leverage. | -        | -         | Default when `use_spot_margin=False`. |
| **`isolated`** | Spot margin or derivatives.    | ✓        | ✓         | `margin_mode=ISOLATED`.               |
| **`cross`**    | Spot margin or derivatives.    | ✓        | ✓         | `margin_mode=CROSS`.                  |

#### Configuration-based trade mode selection

The adapter selects the trade mode from:

1. **Instrument type** (`SPOT` vs other OKX instrument types).
2. **Configuration settings** (`use_spot_margin` for `SPOT`, `margin_mode` otherwise).

##### For SPOT trading

```python
# Simple SPOT trading without leverage (uses 'cash' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SPOT,),
        use_spot_margin=False,  # Default - simple SPOT
        # ... other config
    ),
}

# SPOT trading WITH margin/leverage (uses 'isolated' or 'cross' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SPOT,),
        use_spot_margin=True,  # Enable margin trading for SPOT
        margin_mode=OKXMarginMode.ISOLATED,  # Or CROSS for shared margin
        # ... other config
    ),
}
```

##### For non-spot trading

```python
# Derivatives with isolated margin (default - uses 'isolated' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SWAP,),
        margin_mode=OKXMarginMode.ISOLATED,  # Or omit - ISOLATED is default
        # ... other config
    ),
}

# Derivatives with cross margin (uses 'cross' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SWAP,),
        margin_mode=OKXMarginMode.CROSS,  # Share margin across all positions
        # ... other config
    ),
}
```

##### For mixed SPOT and derivatives trading

When trading both SPOT and derivatives instruments, the adapter determines the trade
mode per order based on the instrument being traded:

```python
# Mixed SPOT + SWAP configuration
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SPOT, OKXInstrumentType.SWAP),
        use_spot_margin=True,           # Applies to SPOT orders only
        margin_mode=OKXMarginMode.CROSS,  # Applies to SWAP orders only
        # ... other config
    ),
}
```

**How it works:**

- **SPOT orders** use `cross` mode because `use_spot_margin=True` and
  `margin_mode=CROSS`.
- **SWAP orders** use `cross` mode because `margin_mode=CROSS`.
- Each order gets the correct `tdMode` based on its instrument type.
- No manual intervention is required.

This supports strategies that trade across instrument types with different margin
configuration, such as:

- Spot-futures arbitrage strategies.
- Delta-neutral strategies combining spot and perpetual swaps.
- Market making across spot and derivatives markets.

:::warning
**Manual trade mode override**: You can override the trade mode per order with
`params={"td_mode": "..."}`. This bypasses adapter selection and can lead to order
rejection when the value does not match the instrument type, such as `isolated` for
spot instruments.

Only use manual override for requirements that cannot be met through configuration.
:::

#### Benefits of configuration-based approach

- **Type-safe**: Configuration is validated at startup before placing any orders.
- **Automatic**: The adapter chooses the mode based on instrument type and intent.
- **Clear**: Field names explain intent, such as `use_spot_margin` vs `td_mode`.
- **Safe**: Incompatible combinations are rejected before they reach OKX.
- **Backwards compatible**: Default values preserve existing behavior.

### Order querying

| Feature              | Linear perpetual swap | Notes                                     |
|----------------------|-----------------------|-------------------------------------------|
| Query open orders    | ✓                     | List all active orders.                   |
| Query order history  | ✓                     | Historical order data.                    |
| Order status updates | ✓                     | Real‑time order state changes.            |
| Trade history        | ✓                     | Execution and fill reports.               |

### Contingent orders

| Feature            | Linear perpetual swap | Notes                                 |
|--------------------|-----------------------|---------------------------------------|
| Order lists        | ✓                     | Batch via WS; regular orders only.    |
| OCO orders         | ✓                     | One‑Cancels‑Other orders.             |
| Bracket orders     | ✓                     | Stop loss + take profit combinations. |
| Conditional orders | ✓                     | Stop and limit‑if‑touched orders.     |

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

| Order type             | Trigger types     | Notes                                     |
|------------------------|-------------------|-------------------------------------------|
| `STOP_MARKET`          | Last, Mark, Index | Market execution when triggered.          |
| `STOP_LIMIT`           | Last, Mark, Index | Limit order placement when triggered.     |
| `MARKET_IF_TOUCHED`    | Last, Mark, Index | Market execution when price touched.      |
| `LIMIT_IF_TOUCHED`     | Last, Mark, Index | Limit order placement when price touched. |
| `TRAILING_STOP_MARKET` | Last, Mark, Index | Trailing stop with callback ratio.        |

#### Trigger price types

Conditional orders support different trigger price sources:

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

- **Liquidation orders**: When the exchange liquidates a position, the adapter detects
  the liquidation category and logs warnings with order details. These orders continue
  through the normal order and fill pipeline.
- **Auto-deleveraging (ADL)**: When OKX closes your position to offset a counterparty's
  liquidation, the adapter detects and logs the ADL event with position details.

Detection is driven by the `category` field on the order record. The
recognized values are:

| `category`              | Meaning                                              |
|-------------------------|------------------------------------------------------|
| `full_liquidation`      | Full position liquidation.                           |
| `partial_liquidation`   | Partial position liquidation.                        |
| `adl`                   | Auto‑deleveraging close.                             |
| `delivery`              | Contract delivery at expiry.                         |
| `normal` / other values | Regular order flow.                                  |

Detection runs on both paths:

- WebSocket `orders` channel (live order/fill updates).
- HTTP `GET /api/v5/trade/orders-history` and `orders-history-archive`
  (used during reconciliation and cold-start mass status).

:::info
**Liquidation and ADL events are logged at WARNING level** with details including order
ID, instrument, and state. Monitor these logs as part of your risk management process.

The adapter handles these exchange-generated orders, emits the relevant `OrderFilled`
events, and updates positions. Your strategy code does not need a separate path.
:::

Upstream references:

- [Order channel and `category` field](https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-order-channel)
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

| Order type | Supported | Notes                                             |
|------------|-----------|---------------------------------------------------|
| `LIMIT`    | ✓         | Standard limit order.                             |
| `MARKET`   | -         | Rejected by the adapter before reaching the API.  |

Options support FOK and IOC time-in-force. OKX uses a dedicated `op_fok` order type for
options FOK orders; the adapter handles this mapping automatically.

Conditional/algo orders (`STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`,
`LIMIT_IF_TOUCHED`, `TRAILING_STOP_MARKET`) are not supported for options and are denied.

### Pricing modes

Options orders can be priced in three mutually exclusive ways. Pass the pricing mode via
order `params`:

| Mode  | Parameter | Description                                             |
|-------|-----------|---------------------------------------------------------|
| Price | (default) | Standard limit price in the contract's currency.        |
| USD   | `px_usd`  | Price in USD terms.                                     |
| IV    | `px_vol`  | Price in implied volatility (1.0 = 100%).               |

```python
# Price in USD
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-USD-250328-50000-C.OKX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("0"),  # Placeholder; px_usd takes precedence
    params={"px_usd": "100.5"},
)

# Price in implied volatility
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-USD-250328-50000-C.OKX"),
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

The adapter exposes position-level Black-Scholes Greeks (`delta_bs`, `gamma_bs`, `theta_bs`,
`vega_bs`) from OKX position data. These are available through the standard position reporting
pipeline.

### Restrictions

- `reduce_only` is not applicable to options and is automatically stripped.
- Position side defaults to `Net`.

### Configuration

Options require the `instrument_families` config parameter to scope which underlyings to load:

```python
config = TradingNodeConfig(
    data_clients={
        OKX: OKXDataClientConfig(
            instrument_types=(OKXInstrumentType.OPTION,),
            instrument_families=("BTC-USD", "ETH-USD"),
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            instrument_types=(OKXInstrumentType.OPTION,),
            instrument_families=("BTC-USD", "ETH-USD"),
            margin_mode=OKXMarginMode.CROSS,
        ),
    },
)
```

## Event contracts

OKX exposes prediction market contracts through `instType=EVENTS`. The adapter loads
these instruments as Nautilus `BinaryOption` instruments and preserves OKX metadata
such as `seriesId`, `instCategory`, `instIdCode`, `state`, and `ruleType` in the
instrument `info` field.

### Loading event contract instruments

Use `OKXInstrumentType.EVENTS` in the data or execution client config. The
`instrument_families` setting maps to OKX `seriesId` values for event contracts. When
`instrument_families` is omitted, the adapter requests the event contract series list
first, then requests instruments for each series.

```python
config = TradingNodeConfig(
    data_clients={
        OKX: OKXDataClientConfig(
            instrument_types=(OKXInstrumentType.EVENTS,),
            instrument_families=("BTC-ABOVE-DAILY",),
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            instrument_types=(OKXInstrumentType.EVENTS,),
            instrument_families=("BTC-ABOVE-DAILY",),
            margin_mode=OKXMarginMode.CROSS,
        ),
    },
)
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
    instrument_id=InstrumentId.from_str("BTC-ABOVE-DAILY-260224-1600-65000.OKX"),
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

Settlement fills arrive with OKX order category `delivery`. The adapter recognizes this
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
from nautilus_trader.core.nautilus_pyo3 import OKXEnvironment

config = TradingNodeConfig(
    data_clients={
        OKX: OKXDataClientConfig(
            environment=OKXEnvironment.DEMO,
            # ... other config
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            environment=OKXEnvironment.DEMO,
            # ... other config
        ),
    },
)
```

When demo mode is enabled:

- REST API requests include the `x-simulated-trading: 1` header.
- WebSocket connections use demo endpoints (`wspap.okx.com`).

:::note
Demo API keys are separate from production keys. Create API keys for demo trading
through the Demo Trading interface. Production API keys do not work in demo mode.
:::

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

### REST limits

- Internal global bucket: 250 requests per second.
- Endpoint-specific quotas appear in the table below and mirror OKX's published limits
  where available.

### WebSocket limits

- Connection establishment: 3 requests per second (per IP).
- Subscription operations (subscribe/unsubscribe/login): 480 requests per hour per connection.
- Order actions (place/cancel/amend): 250 requests per second.

:::warning
OKX enforces per-endpoint and per-account quotas. Exceeding them leads to HTTP 429
responses and temporary throttling on that key.
:::

| Key / endpoint                          | Limit (req/sec) | Notes                                          |
|-----------------------------------------|-----------------|------------------------------------------------|
| `okx:global`                            | 250             | Adapter‑level shared bucket.                   |
| `/api/v5/public/instruments`            | 10              | OKX 20 requests / 2 seconds.                   |
| `/api/v5/public/event-contract/series`  | 5               | OKX 10 requests / 2 seconds.                   |
| `/api/v5/public/event-contract/events`  | 5               | OKX 10 requests / 2 seconds.                   |
| `/api/v5/public/event-contract/markets` | 5               | OKX 10 requests / 2 seconds.                   |
| `/api/v5/market/candles`                | 50              | Higher allowance for streaming candles.        |
| `/api/v5/market/history-candles`        | 20              | Conservative quota for large historical pulls. |
| `/api/v5/market/history-trades`         | 30              | Trade history pulls.                           |
| `/api/v5/account/balance`               | 5               | OKX 10 requests / 2 seconds.                   |
| `/api/v5/trade/order`                   | 30              | OKX 60 requests / 2 seconds.                   |
| `/api/v5/trade/orders-pending`          | 20              | Open order fetch.                              |
| `/api/v5/trade/orders-history`          | 20              | Historical orders.                             |
| `/api/v5/trade/fills`                   | 30              | Execution reports.                             |
| `/api/v5/trade/order-algo`              | 10              | Algo placements.                               |
| `/api/v5/trade/cancel-algos`            | 10              | Algo cancellation.                             |

All keys include the `okx:global` bucket. URLs are normalized with query strings removed
before rate limiting, so requests with different filters share the same quota.

:::info
See the [OKX rate limit documentation](https://www.okx.com/docs-v5/en/#rest-api-rate-limit).
:::

## Configuration

### Configuration options

The OKX data client provides the following configuration options:

#### Data client

| Option                             | Default                     | Description                                  |
|------------------------------------|-----------------------------|----------------------------------------------|
| `instrument_types`                 | `(OKXInstrumentType.SPOT,)` | OKX instrument types to load.                |
| `contract_types`                   | `None`                      | Contract styles to load.                     |
| `instrument_families`              | `None`                      | Families or event `seriesId` values.         |
| `base_url_http`                    | `None`                      | Override for the OKX REST endpoint.          |
| `base_url_ws`                      | `None`                      | Override for the market data WebSocket URL.  |
| `api_key`                          | `None`                      | Falls back to `OKX_API_KEY` when unset.      |
| `api_secret`                       | `None`                      | Falls back to `OKX_API_SECRET` when unset.   |
| `api_passphrase`                   | `None`                      | Falls back to `OKX_API_PASSPHRASE`.          |
| `environment`                      | `None`                      | Environment enum (`LIVE` or `DEMO`).         |
| `http_timeout_secs`                | `60`                        | REST market data request timeout.            |
| `max_retries`                      | `3`                         | Retry attempts for recoverable REST errors.  |
| `retry_delay_initial_ms`           | `1,000`                     | Initial delay before retrying.               |
| `retry_delay_max_ms`               | `10,000`                    | Maximum exponential backoff delay.           |
| `update_instruments_interval_mins` | `60`                        | Background instrument refresh interval.      |
| `vip_level`                        | `None`                      | Enables higher‑depth books by VIP tier.      |
| `proxy_url`                        | `None`                      | Optional HTTP and WebSocket proxy URL.       |

`instrument_families` is required for `OPTION`, optional for `FUTURES`, `SWAP`, and
`EVENTS`, and ignored for `SPOT` and `MARGIN`. For `EVENTS`, pass OKX `seriesId`
values such as `BTC-ABOVE-DAILY`.

The OKX execution client provides the following configuration options:

#### Execution client

| Option                            | Default                     | Description                                 |
|-----------------------------------|-----------------------------|---------------------------------------------|
| `instrument_types`                | `(OKXInstrumentType.SPOT,)` | Tradable OKX instrument types.              |
| `contract_types`                  | `None`                      | Tradable contract styles to load.           |
| `instrument_families`             | `None`                      | Families or event `seriesId` values.        |
| `base_url_http`                   | `None`                      | Override for the OKX trading REST endpoint. |
| `base_url_ws`                     | `None`                      | Override for the private WebSocket URL.     |
| `api_key`                         | `None`                      | Falls back to `OKX_API_KEY` when unset.     |
| `api_secret`                      | `None`                      | Falls back to `OKX_API_SECRET` when unset.  |
| `api_passphrase`                  | `None`                      | Falls back to `OKX_API_PASSPHRASE`.         |
| `environment`                     | `None`                      | Environment enum (`LIVE` or `DEMO`).        |
| `margin_mode`                     | `None`                      | Margin mode (`ISOLATED` or `CROSS`).        |
| `use_spot_margin`                 | `False`                     | Enables spot‑style margin or leverage.      |
| `http_timeout_secs`               | `60`                        | REST trading request timeout.               |
| `use_fills_channel`               | `False`                     | Subscribes to fills channel (VIP5+).        |
| `use_mm_mass_cancel`              | `False`                     | Uses the market‑maker bulk cancel endpoint. |
| `max_retries`                     | `3`                         | Retry attempts for recoverable REST errors. |
| `retry_delay_initial_ms`          | `1,000`                     | Initial delay before retrying.              |
| `retry_delay_max_ms`              | `10,000`                    | Maximum exponential backoff delay.          |
| `use_spot_cash_position_reports`  | `False`                     | Generates SPOT cash positions from wallet.  |
| `proxy_url`                       | `None`                      | Optional HTTP and WebSocket proxy URL.      |

`instrument_families` has the same meaning for execution clients as it does for data
clients.

Below is an example configuration for a live trading node using OKX data and execution clients:

```python
from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig, OKXExecClientConfig
from nautilus_trader.adapters.okx.factories import OKXLiveDataClientFactory, OKXLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig, TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXEnvironment
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXMarginMode
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,           # Will use OKX_API_KEY env var
            api_secret=None,        # Will use OKX_API_SECRET env var
            api_passphrase=None,    # Will use OKX_API_PASSPHRASE env var
            base_url_http=None,
            environment=OKXEnvironment.LIVE,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR,),
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            api_key=None,
            api_secret=None,
            api_passphrase=None,
            base_url_http=None,
            base_url_ws=None,
            environment=OKXEnvironment.LIVE,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR,),
        ),
    },
)
node = TradingNode(config=config)
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.add_exec_client_factory(OKX, OKXLiveExecClientFactory)
node.build()
```

## Contributing

:::info
For additional features or to contribute to the OKX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
