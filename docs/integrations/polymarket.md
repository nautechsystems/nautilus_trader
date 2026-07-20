# Polymarket

Founded in 2020, Polymarket is a decentralized prediction market platform that enables
traders to speculate on event outcomes by buying and selling outcome tokens.

NautilusTrader provides a venue integration for data and execution via Polymarket's Central Limit
Order Book (CLOB) API.

This page documents the V2 integration. The adapter is implemented in Rust and exposed to Python
through PyO3 at `nautilus_trader.adapters.polymarket`; data, execution, signing, and WebSocket
operations therefore have the same behavior from Rust and Python.

NautilusTrader supports multiple Polymarket signature types for order signing, which gives
flexibility for different wallet configurations while NautilusTrader handles signing and order
preparation.

## Installation

To install NautilusTrader with Polymarket support:

```bash
uv pip install "nautilus_trader[polymarket]"
```

To build from source with all extras (including Polymarket):

```bash
uv sync --all-extras
```

## Examples

The maintained V2 examples are available in
[`crates/adapters/polymarket/examples`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/polymarket/examples)
for Rust and
[`python/examples/polymarket`](https://github.com/nautechsystems/nautilus_trader/tree/develop/python/examples/polymarket)
for Python.

## Binary options

A [binary option](https://en.wikipedia.org/wiki/Binary_option) is a type of financial exotic
option contract in which traders bet on the outcome of a yes-or-no proposition. If the
prediction is correct, the trader receives a fixed payout; otherwise, they receive nothing.
NautilusTrader represents Polymarket outcome tokens as `BinaryOption` instruments.

Polymarket uses **pUSD** as the collateral token for trading, [see below](#pusd) for more
information.

## Polymarket documentation

Polymarket offers resources for different audiences:

- [Polymarket Learn](https://learn.polymarket.com/): Educational content and guides for users
  to understand the platform and how to engage with it.
- [Polymarket CLOB API](https://docs.polymarket.com/trading/orders/overview): Technical
  documentation for developers interacting with the Polymarket CLOB API.

## Overview

This guide assumes a trader is setting up for both live market data feeds and trade execution.
The Polymarket integration adapter includes multiple components, which can be used together or
separately depending on the use case.

- `PolymarketWebSocketClient`: Low-level WebSocket API connectivity (built on top of the Nautilus `WebSocketClient` written in Rust).
- `PolymarketInstrumentProvider`: Instrument parsing and loading functionality for `BinaryOption` instruments.
- `PolymarketDataClient`: A market data feed manager.
- `PolymarketExecutionClient`: A trade execution gateway.
- `PolymarketDataClientFactory`: Factory for Polymarket data clients (used by the live node builder).
- `PolymarketExecutionClientFactory`: Factory for Polymarket execution clients (used by the live node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to work with these lower-level components directly.
:::

## pUSD

**pUSD** is the collateral token used for trading on Polymarket. It is a standard ERC-20 token on
Polygon, backed by USDC.

The proxy contract address is
[0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB](https://polygonscan.com/address/0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB)
on Polygon. Direct on-chain funding wraps Polygon USDC.e (bridged USDC) into pUSD
through the [CollateralOnramp](https://docs.polymarket.com/resources/contracts).
The Bridge API can also deposit supported assets from other chains and credit pUSD
after conversion.

## Wallets and accounts

To interact with Polymarket via NautilusTrader, you'll need a **Polygon**-compatible wallet (such as MetaMask).

### Signature types

Polymarket supports multiple signature types for order signing and verification:

| Signature Type | Wallet Type                    | Description                                                              | Use Case                                                                                                   |
|----------------|--------------------------------|--------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------|
| `0`            | EOA (Externally Owned Account) | Standard EIP712 signatures from wallets with direct private key control. | **Default.** Direct wallet connections (MetaMask, hardware wallets, etc.).                                 |
| `1`            | Email/Magic Wallet Proxy       | Smart contract wallet for email‑based accounts (Magic Link).             | Polymarket Proxy associated with Email/Magic accounts. Requires `funder` address.                          |
| `2`            | Browser Wallet Proxy           | Modified Gnosis Safe (1-of-1 multisig) for browser wallets.              | Polymarket Proxy associated with browser wallets. Enables UI verification. Requires `funder` address.      |
| `3`            | Deposit Wallet                 | ERC-1271 deposit wallet flow for new API users.                          | Requires deposit wallet `funder`; API credentials stay bound to the signer.                               |

:::note
See also: [Proxy wallet](https://docs.polymarket.com/developers/proxy-wallet) in the Polymarket documentation for more details about signature types and proxy wallet infrastructure.
:::

NautilusTrader defaults to signature type 0 (EOA) but can be configured to use any of the supported signature types via the `signature_type` configuration parameter.

A single wallet address is supported per trader instance when using environment variables,
or multiple wallets could be configured with multiple `PolymarketExecutionClient` instances.

:::note
Ensure your wallet is funded with **pUSD**, otherwise you will encounter the "not enough balance
or allowance" API error when submitting orders.
:::

### Setting allowances for Polymarket contracts

Before you can start trading, you need to ensure that your wallet has allowances set for Polymarket's smart contracts.
You can do this by running the provided script located at `nautilus_trader/adapters/polymarket/scripts/set_allowances.py`.

This script is adapted from a [gist](https://gist.github.com/poly-rodr/44313920481de58d5a3f6d1f8226bd5e) created by @poly-rodr.

:::note
Run the relevant allowance command once per EOA wallet, then rerun it when Polymarket changes
the required contracts.
:::

:::warning
[Polymarket retires the CLOB v1 Neg Risk Adapter](https://docs.polymarket.com/changelog)
on July 17, 2026 at 00:00 UTC (10:00 AEST).
Existing wallets do not approve its v2 replacement automatically. For every existing wallet,
run the Python script or Rust binary used by your deployment before the deadline. These commands
do not revoke the old v1 approvals;
handle revocation as a separate on-chain operation after reviewing any remaining legacy flows.
Run only one allowance command at a time for a given wallet.
:::

This script automates the process of approving the necessary allowances for the Polymarket contracts.
It sets approvals for the pUSD collateral token and Conditional Token Framework (CTF) contract to allow the
Polymarket CLOB Exchange to interact with your funds.

Before running the script, ensure the following prerequisites are met:

- Install the web3 Python package: `uv pip install "web3==7.12.1"`.
- Have a **Polygon**-compatible wallet funded with some POL (used for gas fees).
- Set the following environment variables in your shell:
  - `POLYGON_PRIVATE_KEY`: Your private key for the **Polygon**-compatible wallet.
  - `POLYGON_PUBLIC_KEY`: Your public key for the **Polygon**-compatible wallet.

Once you have these in place, the script will:

- Approve the maximum possible amount of pUSD (using the `MAX_UINT256` value) for the Polymarket collateral token contract.
- Set the approval for the CTF contract, allowing it to interact with your account for trading purposes.

:::note
You can also adjust the approval amount in the script instead of using `MAX_UINT256`,
with the amount specified in *fractional units* of **pUSD**, though this has not been tested.
:::

Ensure that your private key and public key are correctly stored in the environment variables before running the script.
Here's an example of how to set the variables in your terminal session:

```bash
export POLYGON_PRIVATE_KEY="YOUR_PRIVATE_KEY"
export POLYGON_PUBLIC_KEY="YOUR_PUBLIC_KEY"
```

Run the script using:

```bash
python nautilus_trader/adapters/polymarket/scripts/set_allowances.py
```

For the Rust v2 adapter, set `POLYMARKET_PK` and run:

```bash
cargo run -p nautilus-polymarket --bin polymarket-set-allowances
```

Both commands approve the current Neg Risk Adapter at
`0xadA2005600Dec949baf300f4C6120000bDB6eAab`. Both commands use
`https://polygon.drpc.org` by default; set `POLYGON_RPC_URL` to use another Polygon RPC endpoint.

### Script breakdown

The script performs the following actions:

- Connects to the Polygon network via an RPC URL (<https://polygon.drpc.org>).
- Signs and sends a transaction to approve the maximum pUSD allowance for Polymarket contracts.
- Sets approval for the CTF contract to manage Conditional Tokens on your behalf.
- Repeats the approval process for the Polymarket CLOB Exchange, Neg Risk CTF Exchange, and current Neg Risk adapter.

This allows Polymarket to interact with your funds when executing trades and ensures smooth integration with the CLOB Exchange.

## API keys

To trade with Polymarket, you'll need to generate API credentials. Follow these steps:

1. Ensure the following environment variables are set:
   - `POLYMARKET_PK`: Your private key for signing transactions.
   - `POLYMARKET_FUNDER`: The wallet address (public key) on the **Polygon** network used for funding trades on Polymarket.

2. Run the script using:

   ```bash
   python nautilus_trader/adapters/polymarket/scripts/create_api_key.py
   ```

The script will generate and print API credentials, which you should save to the following environment variables:

- `POLYMARKET_API_KEY`
- `POLYMARKET_API_SECRET`
- `POLYMARKET_PASSPHRASE`

These can then be used for Polymarket client configurations:

- `PolymarketDataClientConfig`
- `PolymarketExecClientConfig`

## Configuration

When setting up NautilusTrader to work with Polymarket, it’s crucial to properly configure the necessary parameters, particularly the private key.

**Key parameters**:

- `private_key`: The private key for your wallet used to sign orders. The interpretation depends on your `signature_type` configuration. If not explicitly provided in the configuration, it will automatically source the `POLYMARKET_PK` environment variable.
- `funder`: The **pUSD** funding wallet address used for funding trades. If not provided,
  will source the `POLYMARKET_FUNDER` environment variable.
- API credentials: You will need to provide the following API credentials to interact with the Polymarket CLOB:
  - `api_key`: If not provided, will source the `POLYMARKET_API_KEY` environment variable.
  - `api_secret`: If not provided, will source the `POLYMARKET_API_SECRET` environment variable.
  - `passphrase`: If not provided, will source the `POLYMARKET_PASSPHRASE` environment variable.
  API credentials are created from the private-key signer for L2 authentication. For
  `POLY_1271`, the deposit wallet remains the `funder`, but it is not the L2 auth address.
- `auto_load_missing_instruments` (default `True`): Controls whether subscribe and
  request commands for an instrument that is not already in the cache trigger an
  ad-hoc load via the Gamma API. When disabled, subscribing to an uncached
  instrument returns an error. See [Runtime instrument loading](#runtime-instrument-loading).
- `auto_load_debounce_ms` (default `100`): The window (milliseconds) over which
  concurrent auto-load requests are coalesced into a single batched Gamma call.

:::tip
We recommend using environment variables to manage your credentials.
:::

## Orders capability

Polymarket operates as a prediction market with a more limited set of order types and instructions compared to traditional exchanges.

### Order types

| Order Type             | Binary Options | Notes                                                                     |
|------------------------|----------------|---------------------------------------------------------------------------|
| `MARKET`               | ✓              | **BUY orders require quote quantity**, SELL orders require base quantity. |
| `LIMIT`                | ✓              |                                                                           |
| `STOP_MARKET`          | -              | *Not supported by Polymarket*.                                            |
| `STOP_LIMIT`           | -              | *Not supported by Polymarket*.                                            |
| `MARKET_IF_TOUCHED`    | -              | *Not supported by Polymarket*.                                            |
| `LIMIT_IF_TOUCHED`     | -              | *Not supported by Polymarket*.                                            |
| `TRAILING_STOP_MARKET` | -              | *Not supported by Polymarket*.                                            |

### Quantity semantics

Polymarket interprets order quantities differently depending on the order type *and* side:

- **Limit** orders interpret `quantity` as the number of conditional tokens (base units).
- **Market SELL** orders also use base-unit quantities.
- **Market BUY** orders interpret `quantity` as quote notional in **pUSD**.

As a result, a market buy order submitted with a base-denominated quantity will execute far
more size than intended.

When submitting market BUY orders, set `quote_quantity=True` on the order. The adapter converts
the quote amount (pUSD) to the signed base-unit share amount before posting to the CLOB. The
Polymarket execution client denies base-denominated market buys to
prevent unintended fills.

```python
# Market BUY with quote quantity (spend $10 pUSD)
order = strategy.order_factory.market(
    instrument_id=instrument_id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(10.0),
    time_in_force=TimeInForce.IOC,  # Maps to Polymarket FAK
    quote_quantity=True,  # Interpret as pUSD notional
)
strategy.submit_order(order)
```

### Execution instructions

| Instruction   | Binary Options | Notes                                                |
|---------------|----------------|------------------------------------------------------|
| `post_only`   | ✓              | Supported for limit orders with `GTC` or `GTD` only. |
| `reduce_only` | -              | *Not supported by Polymarket*.                       |

### Time-in-force options

Polymarket calls the `POST /order` field `orderType`. In NautilusTrader, this maps to
`TimeInForce`. The valid combinations depend on the Nautilus order type:

| Nautilus TIF | Polymarket `orderType` | Nautilus order scope | Notes |
|--------------|------------------------|----------------------|-------|
| `GTC`        | `GTC`                  | `LIMIT` only         | Good‑Til‑Cancelled; rests on the book. |
| `GTD`        | `GTD`                  | `LIMIT` only         | Good‑Til‑Date; rests until expiration, fill, or cancel. |
| `FOK`        | `FOK`                  | `LIMIT` or `MARKET`  | Fill the full size immediately or cancel the whole order. |
| `IOC`        | `FAK`                  | `LIMIT` or `MARKET`  | Fill available size immediately and cancel the remainder. |

:::note
Polymarket uses `FAK` (Fill-And-Kill) for the semantics NautilusTrader calls
`IOC` (Immediate or Cancel). Polymarket docs classify `FOK` and `FAK` as market
order types, while `GTC` and `GTD` are limit order types. For Nautilus `MARKET`
orders, the adapter accepts only `IOC` and `FOK`; `GTC` and `GTD` are valid for
resting `LIMIT` orders only.
:::

:::note
A marketable order (any `FOK`/`FAK` order, or a `BUY` that crosses the book)
must be worth at least **1 pUSD** in notional value, otherwise the venue rejects
it with `invalid amount for a marketable BUY order … min size: $1`. Resting
`GTC`/`GTD` limit orders are bounded only by the 5‑share minimum.
:::

:::note
The venue reports `GTD` expiry as an `OrderCanceled` event (not `OrderExpired`),
and Polymarket applies an internal expiration buffer of roughly one minute, so a
`GTD` order rests for about a minute less than the requested duration before the
venue cancels it.
:::

### Advanced order features

| Feature            | Binary Options | Notes                              |
|--------------------|----------------|------------------------------------|
| Order modification | -              | Cancellation functionality only.   |
| Bracket/OCO orders | -              | *Not supported by Polymarket.*     |
| Iceberg orders     | -              | *Not supported by Polymarket.*     |

### Batch operations

| Operation    | Binary Options | Notes                                                                                                                           |
|--------------|----------------|---------------------------------------------------------------------------------------------------------------------------------|
| Batch Submit | ✓              | The adapter uses `POST /orders` for independent limit‑order batches (max 15 orders per request). See [Batch submit](#batch-submit). |
| Batch Modify | -              | *Not supported by Polymarket*.                                                                                                  |
| Batch Cancel | ✓              | The adapter uses `DELETE /orders`.                                                                                             |

#### Batch submit

`SubmitOrderList` commands are routed to Polymarket's `POST /orders` endpoint. The endpoint
accepts at most 15 orders per request (`BATCH_ORDER_LIMIT`); larger lists are split into
sequential 15‑order chunks.

- Only `LIMIT` orders are batched. `MARKET` orders inside the list are routed to the
  single-order path, which signs a marketable order and submits it with `FAK` or `FOK`
  based on Nautilus `time_in_force`.
- `reduce_only` orders, `quote_quantity` orders, and `post_only` with market TIF
  (`IOC` or `FOK`) are rejected before submission.
- A single eligible order falls through to `POST /order` so it keeps the single‑order retry
  semantics; the batch path deliberately disables retry because the venue does not expose an
  idempotency key.
- If the batch response omits a leg, that order stays submitted for reconciliation. The adapter
  registers the signed order's expected hash so later WebSocket events and cancels still resolve to
  the local order. An omitted response cannot prove that the venue rejected the order.
- `BatchCancelOrders` is dispatched to `DELETE /orders` in one shot.

### Submit error handling

Polymarket's public documentation describes successful
[`POST /order`](https://docs.polymarket.com/api-reference/trade/post-a-new-order) responses
with `success`, `orderID`, `status`, and `errorMsg`, and documents
[API errors](https://docs.polymarket.com/resources/error-codes) as structured error responses.
It does not document statusless client exceptions or transport failures as venue rejections.

The adapter rejects only when the response proves the order was not accepted, such as
`success=false`, a documented order processing error, or another non-retryable client/API
error. Transport failures, timeouts, ambiguous retry exhaustion, statusless `PolyApiException`,
malformed responses, and server-side failures keep the order submitted. The batch endpoint reports
a rejected leg as `success=true` with an empty `orderID` and the reason in `errorMsg` (for example a
naked sell the venue cannot accept): the adapter rejects that leg with the venue reason. A leg with
no `orderID` and no reason stays submitted for reconciliation.

Once any single-order submit attempt has an ambiguous outcome, a later retry error cannot prove
that the first attempt failed. The adapter therefore keeps the order submitted even if a later
attempt returns a client error such as an already-existing order.

Failures before the adapter sends `POST /order` emit `OrderDenied`, not `OrderRejected`. This
includes a failed pUSD balance lookup needed to adjust a market BUY for fees.

When a rejection reason reports a post-only order crossing the book, the `OrderRejected` event
sets `due_post_only=true` so strategies can distinguish it from other venue rejections.

For unknown outcomes, the adapter derives the expected Polymarket order hash from the signed
EIP-712 order when possible and caches it as the `VenueOrderId`. Later WebSocket order events
(or reconciliation reports) then attach to the local `ClientOrderId` instead of becoming external
orders.

Quote-quantity market BUY orders still apply the signed quote-to-base quantity update on the
unknown path. Cancels requested while submit outcome is unknown are deferred until the expected
venue order ID is known, and fill tracking is registered under that ID.

### Position management

| Feature          | Binary Options | Notes                             |
|------------------|----------------|-----------------------------------|
| Query positions  | ✓              | Current user positions from the Polymarket Data API. |
| Position mode    | -              | Binary outcome positions only.    |
| Leverage control | -              | No leverage available.            |
| Margin mode      | -              | No margin trading.                |

### Order querying

| Feature              | Binary Options | Notes                          |
|----------------------|----------------|--------------------------------|
| Query open orders    | ✓              | Active orders only.            |
| Query order history  | ✓              | Limited historical data.       |
| Order status updates | ✓              | Real‑time order state changes. |
| Trade history        | ✓              | Execution and fill reports.    |

### Contingent orders

| Feature            | Binary Options | Notes                               |
|--------------------|----------------|-------------------------------------|
| Order lists        | -              | Independent order batches exist, but linked contingency semantics do not. |
| OCO orders         | -              | *Not supported by Polymarket*.      |
| Bracket orders     | -              | *Not supported by Polymarket*.      |
| Conditional orders | -              | *Not supported by Polymarket*.      |

### Precision limits

Polymarket enforces different precision constraints based on tick size and `orderType`.

**Binary Option instruments** typically support up to 6 decimal places for amounts
(with 0.0001 tick size), but **market orders (`FAK` and `FOK`) have stricter
precision requirements**:

- **Market order types (`FAK` and `FOK`):**
  - The direct maker amount is limited to **2 decimal places**.
  - The computed taker amount uses the market tick precision plus two size decimals.
  - A limit order submitted with `FAK` or `FOK` must also satisfy the stricter market-order amount
    validation. The venue rejects values that are valid for a resting order but not for that
    market-order type.
  - For a limit BUY, `quantity` is the nominal share quantity at the limit price. With `FAK` or
    `FOK`, Polymarket spends the resulting pUSD maker budget, so price improvement can return more
    shares; the adapter updates the order quantity to the actual fill.
  - The adapter denies the order before signing when `quantity * price` is not an exact cent amount.
    It does not round and recompute the nominal share quantity because that would change the signed
    price/amount ratio.

- **Resting limit order types (`GTC` and `GTD`):** More flexible precision based on
  market tick size.

### Tick size precision hierarchy

| Tick Size | Price Decimals | Size Decimals | Amount Decimals |
|-----------|----------------|---------------|-----------------|
| 0.1       | 1              | 2             | 3               |
| 0.01      | 2              | 2             | 4               |
| 0.001     | 3              | 2             | 5               |
| 0.0001    | 4              | 2             | 6               |

:::note

- The adapter validates tick size before signing. It also denies limit `FAK` or `FOK` BUYs whose
  maker amount has more than two decimal places. This applies to single and batch submissions.
- Resting `GTC` and `GTD` limit orders and all SELL orders keep their tick-derived amount precision.
- The adapter rejects limit prices outside the current market's `tick_size` to `1 - tick_size`
  range before signing.
- Market-order precision limits include two decimals for the sell size plus tick-derived bounds
  for the computed amount.
- Tick sizes can change dynamically during market conditions, particularly when markets become one-sided.

:::

### Tick size change handling

When a market's tick size changes (`tick_size_change` WebSocket event), old
book levels can be invalid on the new grid (for example `0.505` fits a `0.001`
tick but not a `0.01` tick). To keep old-grid prices out of the new epoch, the
adapter treats the change as a book epoch transition:

1. Publish the updated `BinaryOption` with the new `price_increment` and `price_precision`.
2. Drop the local order book for the instrument.
3. Mark the instrument as awaiting a fresh snapshot.
4. Drop incremental `price_change` book deltas until the snapshot arrives.
5. Reseed the book from the snapshot and resume normal processing.

Trade ticks and the instrument update flow through unchanged. Quote handling
follows `drop_quotes_missing_side`: when enabled, quote ticks require both bid
and ask prices; when disabled, missing sides use Polymarket boundary prices with
zero size. The adapter can keep quotes flowing during the gap by reading `best_bid`
and `best_ask` from each `price_change`.

## Trades

Trades on Polymarket can have the following statuses:

- `MATCHED`: Trade has been matched and sent to the executor service. The executor submits it as
  a transaction to the Exchange contract.
- `MINED`: Trade is observed to be mined into the chain, and no finality threshold is established.
- `CONFIRMED`: Trade has achieved strong probabilistic finality and was successful.
- `RETRYING`: Trade transaction has failed (revert or reorg) and is being retried/resubmitted by the operator.
- `FAILED`: Trade has failed and is not being retried.

Once a trade is initially matched, subsequent status updates arrive through the user WebSocket.
The execution adapter emits one `OrderFilled` at `MATCHED`. It treats `MINED` and `RETRYING` as
settlement updates without emitting another fill. `CONFIRMED` records finality and refreshes the
account. If the trade reaches `FAILED`, the adapter emits one `OrderFillVoided` for each locally
applied fill and refreshes the account. The correction does not relist the failed quantity, but it
preserves any maker-order remainder that was already working. An execution-complete order becomes
`VOIDED`. Matched WebSocket fills retain the raw trade fields in the `info` field of the
`OrderFilled` event.

### Trade ID derivation

Polymarket does not publish a trade ID on `last_trade_price` market-data events.
The adapter derives a deterministic `TradeId` from the asset ID, side, price,
size, and timestamp via the Rust `determine_trade_id` function using FNV-1a.
For execution fills, taker reports use the venue's trade `id` in both REST reconciliation and the
user WebSocket, so the same fill deduplicates across sources. A maker trade can fill more than one
of the user's resting orders, so maker reports combine the venue trade ID with the maker venue
order ID. The same venue event yields the same trade ID across replays.
For historical Data API trades, the loader uses
`{transactionHash[-24:]}-{asset[-4:]}-{seq:06d}` to distinguish fills in one transaction.

## Fees

Polymarket uses the formula `fee = C * feeRate * p * (1 - p)` where C is shares
traded and p is the share price. Fees peak at p = 0.50 and decrease symmetrically
toward the extremes. Only takers pay fees; makers pay zero.

| Category        | Taker `feeRate` | Maker `feeRate` | Maker rebate |
|-----------------|-----------------|-----------------|--------------|
| Crypto          | 0.072           | 0               | 20%          |
| Sports          | 0.03            | 0               | 25%          |
| Finance         | 0.04            | 0               | 25%          |
| Politics        | 0.04            | 0               | 25%          |
| Economics       | 0.05            | 0               | 25%          |
| Culture         | 0.05            | 0               | 25%          |
| Weather         | 0.05            | 0               | 25%          |
| Other / General | 0.05            | 0               | 25%          |
| Mentions        | 0.04            | 0               | 25%          |
| Tech            | 0.04            | 0               | 25%          |
| Geopolitics     | 0               | 0               | -            |

Fees are calculated in USDC, rounded to 5 decimal places, and applied at match time
by the protocol. The smallest fee charged is 0.00001 USDC; smaller fees round to zero.

:::note
For the latest rates, see Polymarket's [Fees](https://docs.polymarket.com/trading/fees) documentation.
:::

### Backtest fee model

For backtests, the adapter ships `PolymarketFeeModel` (a
`nautilus_trader.backtest.models.FeeModel` subclass) which applies the taker
fee formula above and credits passive maker fills with a rebate inferred from
the market category. Polymarket pays a 20% maker rebate on Crypto markets and
25% on other fee-enabled categories (Sports, Finance, Politics, Economics,
Culture, Weather, Tech, Mentions, Other), distributed daily from each market's
rebate pool. Geopolitics markets are fee-free with no rebates and the model
returns zero for them.

```python
from nautilus_trader.adapters.polymarket.fee_model import PolymarketFeeModel

# Default: maker rebates enabled
fee_model = PolymarketFeeModel()

# Or for taker-only strategies
fee_model = PolymarketFeeModel(maker_rebates_enabled=False)
```

The model can also be configured through `BacktestVenueConfig.fee_model` via
`ImportableFeeModelConfig` and `PolymarketFeeModelConfig`. Maker rebate share
inference uses the instrument's category labels first, then falls back to the
documented per-category fee rate when labels are absent.

## Reconciliation

The Polymarket API returns either all **active** (open) orders or specific orders when queried by the
Polymarket order ID (`venue_order_id`). The execution reconciliation procedure for Polymarket is as follows:

- Generate order reports for all instruments with active (open) orders, as reported by Polymarket.
- Generate position reports from current user positions reported by Polymarket's Data API.
- Compare these reports with Nautilus execution state.
- Generate missing orders to bring Nautilus execution state in line with positions reported by Polymarket.

Polymarket does not directly return orders that are no longer active. The V2 adapter recovers a
cached individual order from trade history when its terminal WebSocket update is missed.
Only `CONFIRMED` trades contribute to recovered fills; pending and failed settlement states do not.

Mass-status reconciliation pairs each order report with its venue fill reports. It applies the
real fills first to preserve trade IDs and commissions, then infers only any residual quantity
needed to reach the venue-reported status. REST order reports cap matched quantity to the greater
of locally applied fills and authenticated `CONFIRMED` trade history, so pending settlement cannot
create an inferred fill. Runtime order checks fetch confirmed trade history when the venue reports
more matched quantity than the local order and WebSocket fill tracker contain. Unpaired fill reports
retain the normal fill-only path.

### Single-order recovery from trades

`/data/order/{id}` only returns active orders, so a `Filled` or `Canceled` order
returns an empty response. To avoid the engine resolving a local `ACCEPTED`
order as `REJECTED` (which discards fills that already happened at the venue),
`generate_order_status_report` falls back to `/data/trades` filtered by the
venue order ID. The cached order is resolved via `client_order_id`, falling
back to the cache's `venue_order_id` index when only the venue ID is known.
Recovery is keyed on the cached order; without one the recovery defers to the
engine rather than synthesizing an external order from trade history alone:

- Cached order + recovered fills covering the cached quantity (within
  `DUST_SNAP_THRESHOLD` for CLOB cent-tick truncation): returns `Filled`. The
  engine reconciles any delta over the cached `filled_qty` via inferred fill.
- Cached order + recovered fills that fall short of the cached quantity by
  more than dust: returns `Canceled` with the recovered `filled_qty`. The
  engine's CANCELED branch transitions the order at the cached `filled_qty`,
  so any newly recovered fills that arrived only via REST (not WS) are not
  applied in this rare partial-cancel case. Closing the order is preferred
  over leaving it stuck open; if exact fill metadata matters in this scenario
  the venue trade history can be reviewed manually.
- Cached order, no trades: returns `Canceled` with
  `cancel_reason="ORDER_NOT_FOUND_AT_VENUE"`.
- Cached order with any `MATCHED`, `MINED`, or `RETRYING` trade: a singular order query preserves
  the locally applied matched quantity while terminal REST recovery waits for `CONFIRMED` or
  `FAILED`.
- No cached order (regardless of trades): returns `None`; the engine's
  not-found-at-venue path resolves the local entry.

The bulk open-order check cannot use this fallback for matched orders omitted by `GET /orders`.
With the default `open_check_open_only=true`, the engine leaves those cached orders open for later
reconciliation. With `open_check_open_only=false`, missing-order retries can mark an order rejected
before its pending settlement confirms. A singular order query or the next startup reconciliation
recovers the settled quantity from confirmed trade history.

## Fill quantity normalization

Polymarket wire amounts use six-decimal fixed-point mantissas. Market SELL signing truncates the
share-denominated `makerAmount` to two decimal places, while market BUY quote conversion can leave
a few microshares of drift between the registered and filled quantities. Both effects are fixed in
absolute share terms, so the adapter uses `DUST_SNAP_THRESHOLD = 0.01` shares. Anything at or above
that threshold remains a real partial fill or overfill.

| Direction | Source                                         | Adapter behavior                              |
|-----------|------------------------------------------------|-----------------------------------------------|
| Overfill  | Market BUY quote conversion (microshares)      | Snap fill down to `submitted_qty`              |
| Underfill | Signed or venue quantity truncation (`< 0.01`)  | Normalize atomic FOK; cancel a FAK remainder  |

Terminal quantity normalization triggers from the `MATCHED` order update for resting maker
orders, or directly on the confirming taker trade for atomic FOK orders. It emits a reconciliation
`OrderUpdated` which lowers the order quantity to the cumulative venue fill. It does not emit a
fill and does not change positions, balances, or commissions.

IOC maps to venue FAK. Once a taker trade confirms, every positive difference between
`original_size` and `size_matched` is an unfilled remainder which the venue has killed. The adapter
therefore emits `OrderCanceled` after the real fills instead of normalizing quantity or leaving the
order partially filled. REST reports apply the same rule when a `MATCHED` FAK has
`size_matched < original_size`. The same terminal handling runs after buffered fills drain when a
confirmed trade arrives before the submit response. A buffered `Canceled`, `Expired`, or
`Rejected` report takes precedence.

`FillReport.commission` always reflects the venue-reported size, not the
snapped quantity. The few-ulp difference is sub-microcent in pUSD.

The fill tracker is keyed by `venue_order_id` and registered on order
accept, so fill reports for orders placed in another session pass through
unchanged. `DUST_SNAP_THRESHOLD` is not configurable per-strategy; it lives
in `nautilus_polymarket::common::consts`.

## WebSockets

The `PolymarketWebSocketClient` is built on top of the high-performance Nautilus `WebSocketClient` base class, written in Rust.

### Data

The data adapter opens `market` subscriptions dynamically as instruments are requested. It currently
uses one market WebSocket connection. The `ws_max_subscriptions` configuration field is present,
but V2 does not yet enforce it or shard subscriptions across connections.

A single `price_change` payload can contain interleaved updates for several assets. The adapter
groups updates by instrument and publishes one atomic order book delta batch per instrument, while
quote processing remains in the venue payload order.

### Runtime instrument loading

Polymarket lists thousands of active markets and new markets appear throughout the day, so preloading
the full universe at startup is rarely practical. The data adapter auto-loads missing instruments on
demand so that strategies can subscribe to markets that are not in the cache:

- When a strategy issues `subscribe_quote_ticks`, `subscribe_trade_ticks`, `subscribe_order_book_deltas`,
  or `request_instrument` for an instrument that is not cached, the adapter registers the request and
  waits `auto_load_debounce_ms` (default 100 ms) so that concurrent requests coalesce.
- It then issues a single batched Gamma API call. Batches larger than the Gamma `condition_ids`
  query ceiling (about 100) are split across multiple calls and merged.
- Once the instruments are loaded, they are published to the data engine (populating the cache)
  and the deferred subscriptions open their WebSocket subscriptions atomically. A strategy that
  unsubscribes while the auto-load is in flight does not see a spurious subscription opened.

The feature is enabled by default. Disable it by setting `auto_load_missing_instruments=False` on
`PolymarketDataClientConfig`. To preload a known set of markets at startup instead, supply
`load_ids`, `event_slugs`, `market_slugs`, or `event_slug_builder` on
`PolymarketInstrumentProviderConfig`.

Newly-minted markets pass through a CLOB hydration window of several minutes during which Gamma
reports `active=true` but `GET /markets/{cid}` returns either a 404 or a 200 with empty
`token_id` strings. The adapter classifies these as transient and retries auto-load with
bounded exponential backoff plus jitter. Tune the cadence with `auto_load_max_retries`
(default 12), `auto_load_retry_delay_initial_secs` (default 5.0), and
`auto_load_retry_delay_max_secs` (default 15.0); the defaults cap the retry window near 3
minutes. Set `auto_load_max_retries=0` to disable retry. 5-minute markets (e.g. updown crypto)
can expire before the venue finishes hydrating, so budget for that or raise the cap. After the
retry budget is exhausted, a condition still missing on Gamma is logged as a terminal miss and the
caller must resubscribe after the market becomes available.

### Market resolution events

The Rust data client tracks Polymarket exposure at `condition_id` level so both YES and NO legs
close together when the venue resolves the market. Position events add open Polymarket binary
option instruments to an internal watchlist. Once a watched condition expires, the data client
waits `resolve_poll_grace_secs`, then polls Gamma every `resolve_poll_interval_secs` until the
condition resolves or `resolve_poll_max_wait_secs` elapses.

Resolution uses strict winner inference:

- Gamma must return a closed binary market with exactly two token IDs, two outcomes, and a binary
  `outcomePrices` shape.
- If Gamma does not provide a strict result for the condition, the client falls back to CLOB
  `GET /markets/{condition_id}` and uses `tokens[].winner`.
- Non-binary, ambiguous, malformed, or still-unresolved payloads are skipped. They remain on the
  watchlist until the poll window times out or a manual request resolves them.

When the client applies a resolution, it emits one `InstrumentStatus` close and one
`InstrumentClose` per tracked leg. The winner leg closes at `1`, and the losing leg closes at `0`.
The close type is `InstrumentCloseType.ContractExpired`. This event closes Nautilus exposure and
does not redeem tokens or claim funds on-chain.

The same apply path handles WebSocket `market_resolved` events, automatic polling, and manual
requests. After `resolve_poll_max_wait_secs`, automatic polling pauses the watched condition and
logs it for manual recovery. Manual requests can still retry the condition later.

#### Manual resolution requests

Use `request_data()` with data type `PolymarketResolveRequest` to force a resolution check. The
request accepts any of these params:

| Param            | Type                 | Description |
|------------------|----------------------|-------------|
| `condition_id`   | `str`                | Resolve one Polymarket condition. |
| `condition_ids`  | `str` or `list[str]` | Resolve one or more Polymarket conditions. |
| `instrument_ids` | `str` or `list[str]` | Resolve Polymarket instrument IDs; other venues are ignored. |

If a request omits all selectors, the client uses the watchlist. With automatic polling enabled,
the fallback selects paused or timed-out entries. With automatic polling disabled, it selects all
expired eligible entries, so operators can run the recovery flow manually.

The response payload is custom data with this dictionary shape:

| Key                          | Meaning |
|------------------------------|---------|
| `requested_condition_ids`    | Deduplicated condition IDs checked by the request. |
| `fetched_markets`            | Gamma markets returned across the batched lookup. |
| `resolved_markets`           | Conditions with a strict Gamma result or successful CLOB fallback result. |
| `skipped_non_binary_markets` | Gamma markets skipped for non‑binary or ambiguous resolution shape. |
| `clob_fallback_successes`    | Conditions resolved through the CLOB fallback path. |
| `emitted_condition_ids`      | Conditions that emitted at least one `InstrumentClose`. |
| `failed_condition_ids`       | Conditions where both Gamma and CLOB lookup failed. |
| `used_watchlist_fallback`    | Whether the request selected conditions from the watchlist. |
| `timed_out_watchlist`        | Timed‑out watchlist entries seen during fallback selection. |
| `error`                      | First summary error, if one occurred. |

Redemption is a separate account or execution workflow. Do not extend the data client resolution
path to claim funds; it only publishes market-outcome close events into Nautilus.

### Purging instruments at runtime

Polymarket auto-loads instruments on demand, so a long-running session keeps growing the cache as
markets resolve, new markets appear, and strategies cycle through events. Use `cache.purge_instrument`
to drop markets the strategy no longer tracks. The call removes the instrument record and every
cache-owned map keyed by it (order book, quotes, trades, bars).

```python
class PolymarketHousekeeping(Strategy):
    def on_position_closed(self, event: PositionClosed) -> None:
        # Drop the market once the position is closed and you have no further interest.
        instrument_id = event.instrument_id
        self.unsubscribe_quote_ticks(instrument_id)
        self.unsubscribe_order_book_deltas(instrument_id)
        self.cache.purge_instrument(instrument_id)
```

Common triggers on Polymarket:

- A market resolves and produces no further trades.
- An event ends and the strategy rotates off its markets.
- The strategy rotates a fixed-size watchlist and drops the oldest entry.

The purge skips any instrument that still has non-terminal orders (initialized, submitted,
accepted, emulated, released, or inflight) or non-closed positions, so it is safe to call without
coordinating with the execution client. Active WebSocket subscriptions belong to the data engine.
Unsubscribe before purging if you no longer want updates.

The cache also exposes `purge_order`, `purge_position`, `purge_closed_orders`,
`purge_closed_positions`, and `purge_account_events` for trimming closed execution state.
For long-running Polymarket nodes, schedule the bulk purges from `LiveExecEngineConfig`
(15 min interval, 60 min buffer is a sensible default). See
[Cache: purging cached data](../concepts/cache.md#purging-cached-data) for the full set.

:::warning
The caller decides when an instrument is no longer needed. Purging an instrument that another
actor, strategy, or engine still relies on causes missing instrument lookups and loses market-data
history.
:::

### Execution

The execution adapter keeps a `user` channel connection for order and trade events and manages market
subscriptions as needed for instruments seen during trading.

The adapter supports dynamic WebSocket subscribe and unsubscribe operations.
Matched WebSocket fills and their corrections are restored from cached order history and
deduplicated across reconnects. If a trade arrives before its instrument is available, the adapter
leaves it out of the dedup state. A redelivered event or later REST reconciliation can apply it after
instrument loading completes.
For a fully matched order, terminal quantity normalization waits for every trade ID in the order's
`associate_trades` list to confirm before lowering the order quantity to its actual fills. If a
confirmed trade is recovered through REST after a WebSocket gap, reconciliation applies the same
order-only normalization. If a `MATCHED` WebSocket update omits `associate_trades`, the adapter does
not infer that settlement is final; the next REST reconciliation recovers the residual after the
trade reaches `CONFIRMED`.

### Subscription limits

Polymarket does not publish a WebSocket subscription cap in its current rate-limit documentation.
The V2 configuration exposes `ws_max_subscriptions` with a default of 200, but the Rust client does
not currently enforce that value or create additional connections. It sends the supplied asset IDs
in one `subscribe` request on one market connection.

Do not rely on this setting for subscription sharding. Keep large-universe strategies below an
operationally verified venue limit until connection sharding is implemented.

## Rate limiting

Polymarket enforces rate limits via Cloudflare throttling.
When limits are exceeded, requests are throttled on sliding windows. Sustained
overshoot can still surface as HTTP 429 responses or temporary blocking.

### Selected REST limits

Polymarket changes these quotas over time. As of 2026-07-10, the official limits are:

| Endpoint                            | Burst (10s) | Sustained (10 min) | Notes                                      |
|-------------------------------------|-------------|--------------------|--------------------------------------------|
| General rate limiting               | 15,000      | -                  | Global documented rate limit.              |
| Health check (`/ok`)                | 100         | -                  | Health endpoint.                           |
| CLOB general                        | 9,000       | -                  | Aggregate across CLOB endpoints.           |
| CLOB `POST /order`                  | 5,000       | 120,000            | Single‑order submit.                       |
| CLOB `POST /orders`                 | 2,000       | 21,000             | Batch submit (up to 15 orders per request). |
| CLOB `DELETE /order`                | 5,000       | 120,000            | Single‑order cancel.                       |
| CLOB `DELETE /orders`               | 2,000       | 15,000             | Batch cancel.                              |
| CLOB `DELETE /cancel-all`           | 250         | 6,000              | Cancel all orders.                         |
| CLOB `DELETE /cancel-market-orders` | 1,500       | 21,000             | Cancel orders for one market.              |
| CLOB `GET /balance-allowance`       | 200         | -                  | Balance and allowance queries.             |
| CLOB API key endpoints              | 100         | -                  | Key management.                            |
| Gamma general                       | 4,000       | -                  | Aggregate across Gamma endpoints.          |
| Gamma `/markets`                    | 300         | -                  | Market metadata.                           |
| Gamma `/events`                     | 500         | -                  | Event metadata.                            |
| Data general                        | 1,000       | -                  | Aggregate across Data API endpoints.       |
| Data `/trades`                      | 200         | -                  | Trade history.                             |
| Data `/positions`                   | 150         | -                  | Current positions.                         |

### WebSocket limits

The WebSocket quotas are not part of the published REST rate-limits table. The V2 adapter exposes
`ws_max_subscriptions` (default 200), but it does not yet enforce that cap or shard connections.

:::warning
Exceeding Polymarket rate limits triggers Cloudflare throttling. Requests are queued
using sliding windows rather than rejected immediately, but sustained overshoot can
result in HTTP 429 responses or temporary blocking.
:::

:::info
For the latest rate limit details, see the official Polymarket documentation:
<https://docs.polymarket.com/api-reference/rate-limits>
:::

## Limitations and considerations

The following limitations are currently known:

- Reduce-only orders are not supported.
- Batch submit (`POST /orders`) accepts at most 15 orders per request; the adapter splits larger
  `SubmitOrderList` commands into sequential 15-order chunks.
- The adapter does not implement Polymarket's authenticated heartbeat auto-cancel endpoint.
- Position reports omit balances below 0.01 shares. Do not treat an omitted report as proof that a
  dust position is flat; a sub-minimum residual cannot be exited through the CLOB's five-share
  minimum order size. Position reconciliation therefore tolerates differences through 0.009999
  shares and reconciles differences of 0.01 shares or more.

## Configuration

Rust structs and PyO3 classes expose the same V2 client configuration. The only Rust-only fields
are the programmatic `filters` and `new_market_filter` trait objects on
`PolymarketDataClientConfig`.

### Data client options

Class/struct: `PolymarketDataClientConfig`.

| Option                                        | Default   | Description |
|-----------------------------------------------|-----------|-------------|
| `instrument_config`                           | `None`    | Bootstrap scope, passed as `PolymarketInstrumentProviderConfig`. |
| `base_url_http`, `base_url_ws`                | `None`    | Override the CLOB HTTP or WebSocket endpoint. |
| `base_url_gamma`, `base_url_data_api`         | `None`    | Override the Gamma or Data API endpoint. |
| `base_url_rtds`                               | `None`    | Override the RTDS endpoint. |
| `http_timeout_secs`, `ws_timeout_secs`        | `60`, `30` | HTTP and WebSocket timeout in seconds. |
| `ws_max_subscriptions`                        | `200`     | Configured cap; V2 does not currently enforce or shard it. |
| `update_instruments_interval_mins`            | `60`      | Instrument catalogue refresh interval; pass `None` to disable it. |
| `subscribe_new_markets`                       | `false`   | Subscribe to new‑market discovery events. |
| `drop_quotes_missing_side`                    | `true`    | Drop quotes that do not contain both a bid and an ask. |
| `new_market_fetch_max_concurrency`            | `8`       | Bound concurrent market fetches from discovery events. |
| `auto_load_missing_instruments`               | `true`    | Load unknown instruments for supported requests and subscriptions. |
| `auto_load_debounce_ms`                       | `100`     | Coalesce concurrent auto‑load requests. |
| `auto_load_max_retries`                       | `12`      | Retry transient CLOB hydration misses; `0` disables retry. |
| `auto_load_retry_delay_initial_secs`          | `5.0`     | Initial auto‑load retry delay. |
| `auto_load_retry_delay_max_secs`              | `15.0`    | Maximum auto‑load retry delay. |
| `resolve_poll_enabled`                        | `true`    | Poll expired watched conditions for resolution. |
| `resolve_poll_interval_secs`                  | `30`      | Resolution polling interval. |
| `resolve_poll_grace_secs`                     | `10`      | Delay after expiry before polling begins. |
| `resolve_poll_max_wait_secs`                  | `1800`    | Pause automatic polling after this wait. |
| `transport_backend`                           | `Sockudo` | WebSocket transport implementation. |

### Execution client options

Class/struct: `PolymarketExecClientConfig`.

| Option                                           | Default                 | Description |
|--------------------------------------------------|-------------------------|-------------|
| `trader_id`                                      | default `TraderId`      | Trader identifier registered by the client. |
| `account_id`                                     | `POLYMARKET-001`        | Account identifier for this execution client. |
| `private_key`                                    | `POLYMARKET_PK`         | EIP-712 signing key. |
| `api_key`, `api_secret`, `passphrase`            | environment variables   | CLOB L2 authentication credentials. |
| `funder`                                         | `POLYMARKET_FUNDER`     | Funding wallet; proxy and deposit‑wallet signatures require it to differ from the signing address. |
| `signature_type`                                 | `Eoa`                   | `Eoa`, `PolyProxy`, `PolyGnosisSafe`, or `Poly1271`. |
| `base_url_http`, `base_url_ws`, `base_url_data_api` | `None`                | Override the respective production endpoint. |
| `http_timeout_secs`                              | `60`                    | HTTP timeout in seconds. |
| `max_retries`                                    | `3`                     | Retries for single‑order submit and cancel requests. |
| `retry_delay_initial_ms`                         | `1000`                  | Initial retry delay. |
| `retry_delay_max_ms`                             | `10000`                 | Maximum retry delay. |
| `ack_timeout_secs`                               | `5`                     | Reserved for order/trade acknowledgment handling; not currently applied. |
| `transport_backend`                              | `Sockudo`               | WebSocket transport implementation. |

Batch submissions never retry because Polymarket does not expose an idempotency key.
Proxy signature clients fail during construction unless `funder` is present and differs from the
signing address.

### Instrument provider options

Pass `PolymarketInstrumentProviderConfig` as `instrument_config` on the data client config.

| Option               | Default | Description |
|----------------------|---------|-------------|
| `load_all`           | `false` | Load the full venue catalogue at startup. |
| `load_ids`           | `None`  | Load exact Nautilus instrument IDs. |
| `filters`            | `None`  | Validated Gamma market keyset filters. |
| `event_slugs`        | `None`  | Resolve all markets for the listed events at bootstrap. |
| `market_slugs`       | `None`  | Load the listed Gamma market slugs at bootstrap. |
| `event_slug_builder` | `None`  | Rust‑backed Up/Down event‑slug generator. |
| `log_warnings`       | `true`  | Emit provider warnings. |
| `use_gamma_markets`  | `false` | Compatibility field with no additional V2 behavior. |

#### Gamma query filters

The Rust v2 adapter uses the Gamma market and event keyset endpoints. It validates filters before
the first HTTP request, follows `next_cursor`, and applies the endpoint page ceilings of 100 markets
and 500 events.

Market keyset fields:

| Class         | Fields |
|---------------|--------|
| Scalar        | `limit`, `order`, `ascending`, `closed`, `decimalized`, `liquidity_num_min`, `liquidity_num_max`, `volume_num_min`, `volume_num_max`, `start_date_min`, `start_date_max`, `end_date_min`, `end_date_max`, `related_tags`, `tag_match`, `cyom`, `rfq_enabled`, `uma_resolution_status`, `game_id`, `include_tag`, `locale` |
| Repeated      | `id`, `slug`, `clob_token_ids`, `condition_ids`, `question_ids`, `market_maker_address`, `tag_id`, `sports_market_types` |
| Compatibility | `active`, `archived` |
| Alias         | `is_active` |
| Client only   | `offset`, `max_markets` |

The provider `filters` dictionary accepts only market fields. Rust callers configure event
discovery with `EventParamsFilter` and `GetGammaEventsParams`; event-only fields such as `live` or
`tag_slug` are not valid provider dictionary keys.

Event keyset fields:

| Class         | Fields |
|---------------|--------|
| Scalar        | `limit`, `order`, `ascending`, `closed`, `live`, `featured`, `cyom`, `title_search`, `liquidity_min`, `liquidity_max`, `volume_min`, `volume_max`, `start_date_min`, `start_date_max`, `end_date_min`, `end_date_max`, `start_time_min`, `start_time_max`, `tag_slug`, `related_tags`, `tag_match`, `event_date`, `event_week`, `featured_order`, `recurrence`, `parent_event_id`, `include_children`, `partner_slug`, `include_chat`, `include_template`, `include_best_lines`, `locale` |
| Repeated      | `id`, `slug`, `tag_id`, `exclude_tag_id`, `series_id`, `game_id`, `created_by` |
| Compatibility | `active`, `archived` |
| Client only   | `offset`, `max_events` |

Repeated fields are sent as repeated query keys. `offset` is applied across returned keyset pages
and is never sent to Gamma. `max_markets` caps markets locally, with each binary market normally
producing two instruments. `max_events` caps events locally; each event can contain many markets.
`condition_ids` accepts at most 100 values, and event `tag_id` values cannot overlap `exclude_tag_id`
values.

The provider `filters` dictionary accepts strings in the native Rust config and also accepts Python
`bool`, `int`, finite `float`, string, or lists of those scalar values when converting a legacy
Python-shaped config. The legacy-shaped conversion ignores `None` entries; native config entries
must be strings. `is_active=true` supplies `active=true`, `archived=false`, and `closed=false`;
explicit values override those defaults. Unknown keys, malformed values, empty lists, invalid date
or numeric bounds, and invalid combinations raise `ValueError` during Python config conversion.

See the official [market keyset](https://docs.polymarket.com/api-reference/markets/list-markets-keyset-pagination)
and [event keyset](https://docs.polymarket.com/api-reference/events/list-events-keyset-pagination)
references for the venue contract.

#### Event slug builder

The Rust Python v2 adapter treats Python as a configuration, factory, and user strategy boundary.
Provider, data, and execution operations run in Rust. `event_slug_builder` therefore accepts a
Rust-backed `PolymarketUpDownEventSlugConfig`; it does not accept Python callable paths.

Use this for predictable Polymarket Up/Down event slugs without downloading the full venue
catalogue. The builder emits slugs with the pattern
`{asset}-updown-{interval_mins}m-{unix_timestamp}` for the configured window of aligned periods.

```python
from nautilus_trader.adapters.polymarket import PolymarketInstrumentProviderConfig
from nautilus_trader.adapters.polymarket import PolymarketUpDownEventSlugConfig

instrument_config = PolymarketInstrumentProviderConfig(
    event_slug_builder=PolymarketUpDownEventSlugConfig(
        assets=["btc"],
        interval_mins=5,
        periods=3,
        start_offset_periods=0,
    ),
)
```

For custom event patterns, pass explicit `event_slugs`, pass direct `market_slugs`, or add a Rust
filter or builder. The Rust v2 adapter rejects Python callable `event_slug_builder` values so adapter
operations do not cross into Python during live trading.

## Python v2 discovery and historical data

The Python v2 package exports a Rust-backed `PolymarketDataLoader` for public discovery,
instrument construction, and historical trades. It uses the Rust Gamma, CLOB, and Data API clients,
so it does not require trading credentials or run networking in Python.

All network methods are asynchronous. Build a loader from a market slug and select its outcome token
by index:

```python
from nautilus_trader.adapters.polymarket import PolymarketDataLoader

loader = await PolymarketDataLoader.from_market_slug(
    "gta-vi-released-before-june-2026",
    token_index=0,
)

instrument = loader.instrument
token_id = loader.token_id
condition_id = loader.condition_id
```

`instrument` is a normalized `BinaryOption`. Resolution-bearing fields never enter
`instrument.info`. Read them separately after a backtest or simulation:

```python
metadata = loader.resolution_metadata
winner = next(
    (token["outcome"] for token in metadata["tokens"] if token["winner"]),
    None,
)
```

An event factory returns one loader for each market in the event:

```python
loaders = await PolymarketDataLoader.from_event_slug(
    "highest-temperature-in-nyc-on-january-26",
    token_index=1,
)
```

A negative token index or an index outside a market's token list raises `ValueError`. Construction
also fails clearly when Gamma has no matching slug or CLOB has not populated usable token IDs.

### Public discovery

Static query methods return stable Python mappings and lists while Rust owns validation and
pagination:

```python
market = await PolymarketDataLoader.query_market_by_slug("some-market")
details = await PolymarketDataLoader.query_market_details(market["conditionId"])
event = await PolymarketDataLoader.query_event_by_slug("some-event")

markets = await PolymarketDataLoader.query_markets(
    filters={
        "is_active": True,
        "tag_id": [21, 42],
        "order": "volume",
        "max_markets": 200,
    },
)
events = await PolymarketDataLoader.query_events(
    filters={
        "active": True,
        "closed": False,
        "max_events": 100,
    },
)
tags = await PolymarketDataLoader.query_tags()
results = await PolymarketDataLoader.query_search(
    "bitcoin",
    events_status="active",
    limit_per_type=20,
)
```

Market and event filter dictionaries use the fields listed under
[Gamma query filters](#gamma-query-filters). The provider config accepts only the market fields,
while `query_events` accepts the event fields. Unknown or malformed filters raise `ValueError`
before any request.

### Historical trades

`load_trades` returns normalized `TradeTick` objects in chronological order:

```python
from datetime import UTC, datetime, timedelta

end = datetime.now(UTC)
start = end - timedelta(days=1)

trades = await loader.load_trades(
    start=start,
    end=end,
    limit=1_000,
)
```

The window is inclusive. The Data API records trade timestamps in whole seconds, so Rust keeps all
trades in the `start` and `end` boundary seconds. With `start`, `limit` keeps the earliest matching
trades in the window. Without `start`, it keeps the most recent matching trades. The public API caps
offset-based pagination at 10,000; if that ceiling is reached, an unanchored request returns the
available partial result and logs a warning. A start-anchored request raises an error at the ceiling
because Rust cannot guarantee complete results from the requested start; narrow the time window and
retry.

The legacy v1 loader also exposes lower-level raw fetch and parse methods, Python HTTP injection,
and convenience scripts. Those v1-only APIs remain under the top-level legacy package and are not
part of the Python v2 facade.

## Contributing

:::info
For additional features or to contribute to the Polymarket adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
