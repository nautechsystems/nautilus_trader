# Coinbase

Founded in 2012, Coinbase is one of the largest US-regulated cryptocurrency
exchanges, offering trading across spot, perpetual swaps, and dated futures via
the Advanced Trade API. This adapter supports live market data ingest and
order execution on both spot (Cash) and CFM derivatives (Margin) accounts
through a shared execution client, with the account type selected by the
factory (see [Execution scope](#execution-scope)).

## Overview

The Coinbase adapter is implemented in Rust and exposed to Python through configurations,
factories, enums, and constants.

Components:

- `CoinbaseRawHttpClient`: Low-level REST client owning transport, JWT signing, and rate limits.
- `CoinbaseHttpClient`: Domain REST client parsing venue responses into Nautilus types.
- `CoinbaseWebSocketClient`: Low-level WebSocket connectivity with JWT subscribe auth.
- `CoinbaseInstrumentProvider`: Instrument parsing and loading.
- `CoinbaseDataClient`: Market data feed manager.
- `CoinbaseDataClientFactory`: Data client factory.
- `CoinbaseExecutionClient`: Execution client (spot or CFM derivatives; REST orders + WS streams).
- `CoinbaseExecutionClientFactory`: Execution client factory; spot vs CFM derivatives is selected by `account_type` on the config.

Python surface available from `nautilus_trader.adapters.coinbase`:

- `CoinbaseDataClientConfig`, `CoinbaseExecutionClientConfig`
- `CoinbaseDataClientFactory`, `CoinbaseExecutionClientFactory`
- `CoinbaseEnvironment`, `CoinbaseMarginType`
- `COINBASE`, `COINBASE_CLIENT_ID`, and `COINBASE_VENUE`

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/coinbase/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/coinbase/examples/)

## Coinbase documentation

Coinbase provides documentation for the Advanced Trade API:

- [REST API reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/introduction)
- [Order management guide](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/orders)
- [WebSocket channels](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-channels)
- [API key authentication](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication)
- [Rate limiting](https://docs.cdp.coinbase.com/coinbase-app/api-architecture/rate-limiting)

It's recommended you also refer to the Coinbase documentation in conjunction
with this NautilusTrader integration guide.

:::info
This adapter targets the Coinbase Advanced Trade API. The separate
[Coinbase International Exchange (INTX)](https://international.coinbase.com)
venue is not covered; its adapter was removed in NautilusTrader 1.224.0.
:::

## Products

A product is an umbrella term for a group of related instrument types.

The following product types are supported:

| Product Type        | Supported | Notes                                             |
| ------------------- | --------- | ------------------------------------------------- |
| Spot                | ✓         | USD, USDC, and USDT-quoted spot pairs.            |
| Perpetual contracts | ✓         | USD-margined perpetual swaps on the FCM venue.    |
| Futures contracts   | ✓         | Dated delivery futures (nano BTC, nano ETH, etc). |

## Symbology

Coinbase uses the venue's native `product_id` field directly as the Nautilus
symbol. The instrument ID is `{product_id}.COINBASE`.

| Product      | Format                          | Examples                           |
| ------------ | ------------------------------- | ---------------------------------- |
| Spot         | `{base}-{quote}`                | `BTC-USD`, `ETH-USDC`, `SOL-USDT`. |
| Perpetual    | `{contract_code}-{ddMMMyy}-CDE` | `BIP-20DEC30-CDE` (BTC PERP).      |
| Dated future | `{contract_code}-{ddMMMyy}-CDE` | `BIT-24APR26-CDE` (BTC Apr 2026).  |

The `-CDE` suffix denotes the Coinbase Derivatives Exchange (FCM venue).
Perpetuals carry an exchange-assigned far-future expiry (e.g. `20DEC30`) but
are classified as `CryptoPerpetual` based on the presence of an ongoing
funding rate. Dated futures are classified as `CryptoFuture`.

The adapter resolves the product type structurally from API metadata
(`future_product_details.contract_expiry_type` and, when that is
`EXPIRING`, the presence of a non-empty `future_product_details.funding_rate`
as a perpetual-only structural signal); the fallback heuristic checks
`display_name` for `PERP` or `Perpetual` substrings.

Examples of full Nautilus instrument IDs:

- `BTC-USD.COINBASE` (spot Bitcoin/USD).
- `ETH-USDC.COINBASE` (spot Ether/USDC).
- `BIP-20DEC30-CDE.COINBASE` (BTC perpetual swap).
- `BIT-24APR26-CDE.COINBASE` (BTC dated future, Apr 2026).

### Aliased products (USDC and USD)

Coinbase consolidates USDC- and USD-quoted versions of the same pair into a
single matching-engine book and exposes the relationship in `GET /products`
via the `alias` and `alias_to` fields:

```text
BTC-USD :  alias=""        alias_to=["BTC-USDC"]   # canonical
BTC-USDC:  alias="BTC-USD" alias_to=[]             # alias of BTC-USD
```

When a caller subscribes or submits using the alias side, the venue rewrites
the request to the canonical id on the wire. The adapter records the
`product_id -> alias` map at instrument bootstrap and handles the rewrite
transparently on both sides:

- Data subscriptions go out on the canonical id. The data WebSocket client
  holds the reverse mapping and re-keys inbound messages back to the
  caller-supplied id before parsing.
- Orders are submitted on the caller's `product_id`. The execution client
  records that id under the `client_order_id` and re-keys the canonical id
  the user channel echoes back, so an alias-side order is never reported
  against the canonical instrument.

A strategy holding only USDC can therefore trade `BTC-USDC.COINBASE` end to
end without referencing the canonical `BTC-USD`. Settlement currency is
determined by the submitted `product_id`, so an order placed on
`BTC-USDC.COINBASE` always debits or credits the USDC wallet.

## Environments

Coinbase provides two trading environments. Configure the appropriate
environment using the `environment` field in your client configuration.

| Environment | `environment` value           | REST base URL                      |
| ----------- | ----------------------------- | ---------------------------------- |
| Live        | `CoinbaseEnvironment.LIVE`    | `https://api.coinbase.com`         |
| Sandbox     | `CoinbaseEnvironment.SANDBOX` | `https://api-sandbox.coinbase.com` |

### Live (production)

The default environment for live trading with real funds.

```python
config = CoinbaseExecutionClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    # environment=CoinbaseEnvironment.LIVE (default)
)
```

Environment variables: `COINBASE_API_KEY`, `COINBASE_API_SECRET`.

### Sandbox

A static-mock test environment for integration plumbing, per the
[Sandbox docs](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/sandbox).

```python
config = CoinbaseExecutionClientConfig(
    api_key="ANY_NON_EMPTY_STRING",  # required by the adapter constructor
    api_secret="ANY_NON_EMPTY_STRING",
    environment=CoinbaseEnvironment.SANDBOX,
)
```

The sandbox venue does not enforce authentication, but
`CoinbaseExecutionClient::new` still requires both fields (or the matching
environment variables) to be present in order to construct.

:::warning
**Sandbox is not a parallel trading venue:**

- All responses are static and pre-defined; there is no live market or
  dynamic pricing.
- Only Accounts and Orders endpoints are available; other resources are not.
- Authentication is not required (and not enforced).
- A custom `X-Sandbox` request header can trigger predefined error scenarios.

Use sandbox to wire up your client and verify request/response shape; use
production (with real funds and care) for any realistic behaviour testing.
:::

## Authentication

Coinbase Advanced Trade uses ES256 JWT authentication. Each REST request and
each WebSocket subscription generates a short-lived JWT signed with your EC
private key. The adapter resolves credentials from environment variables or
from the config fields.

### Creating an API key

Coinbase has several key types. The adapter requires a **Coinbase App Secret
API key** with the **ECDSA** signature algorithm (not Ed25519).

<Steps>
<Step>
Go to the CDP portal API keys page:
[portal.cdp.coinbase.com/projects/api-keys](https://portal.cdp.coinbase.com/projects/api-keys).
</Step>
<Step>
Select the **Secret API Keys** tab and click **Create API key**.
</Step>
<Step>
Enter a nickname (e.g. `nautilus-trading`).
</Step>
<Step>
Expand **API restrictions** and set permissions to **View** and **Trade**.
</Step>
<Step>
Expand **Advanced Settings** and change the signature algorithm from Ed25519
to **ECDSA**. This step is required: Ed25519 keys do not work with the
Advanced Trade API.
</Step>
<Step>
Click **Create API key**. Save the key name and private key from the modal.
The key name looks like `organizations/{org_id}/apiKeys/{key_id}`. The
private key is a PEM-encoded EC key (SEC1 format).
</Step>
</Steps>

:::warning
Coinbase no longer auto-downloads the key file. Copy the values from the
creation modal or click the download button before closing it. You cannot
retrieve the private key afterward.
:::

:::info
Do not use expired legacy Coinbase App API keys. Create a CDP API key and select the ECDSA
algorithm; the adapter signs requests with ES256. See Coinbase's
[legacy key migration guide](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/legacy-keys).
:::

For full details see the Coinbase
[API key authentication guide](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication).

### Environment variables

| Variable              | Description                                           |
| --------------------- | ----------------------------------------------------- |
| `COINBASE_API_KEY`    | Key name (`organizations/{org_id}/apiKeys/{key_id}`). |
| `COINBASE_API_SECRET` | PEM-encoded EC private key (full multi-line string).  |

Example:

```bash
export COINBASE_API_KEY="organizations/abc-123/apiKeys/def-456"
export COINBASE_API_SECRET="$(cat ~/path/to/cdp_api_key.pem)"
```

:::tip
We recommend using environment variables to manage your credentials.
:::

### JWT lifetime

Coinbase JWTs expire after 120 seconds. Per the
[WebSocket overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-overview),
a different JWT must be generated for each authenticated WebSocket message
(i.e. for each subscribe). The adapter regenerates a fresh JWT for every
signed REST request and for every authenticated subscribe message; no
manual rotation is required.

## Portfolios

A Coinbase account holds one or more **portfolios**. Each portfolio has its
own wallets (USD, USDC, BTC, etc.), balances, and order scope. Every account
has a `DEFAULT` portfolio; users can create additional `CONSUMER` portfolios
to segregate strategies, risk, or tax lots.

A CDP API key is **bound to a single portfolio at creation time**. Every
authenticated request (account lookup, order submission, cancel) operates
against that portfolio unless a different one is explicitly specified.

### Finding your portfolio UUIDs

Run the adapter's authenticated probe binary; it prints the portfolios
visible to your CDP key, the account balances in the bound portfolio, and
a few reference REST calls:

```bash
cargo run --bin coinbase-http-private --package nautilus-coinbase
```

Sample output:

```text
Found 1 portfolio(s)
  name=Default type=DEFAULT uuid=ca7244bc-21d1-5e4c-bfe5-80f208ac5723 deleted=false
Account has 3 balance(s)
  USDC total=100.00000000 USDC free=100.00000000 USDC locked=0.00000000 USDC
  AUD total=0.00 AUD free=0.00 AUD locked=0.00 AUD
  BTC total=0.00000000 BTC free=0.00000000 BTC locked=0.00000000 BTC
```

Equivalent curl (you have to sign your own ES256 JWT with your CDP PEM
key first):

```bash
curl -H "Authorization: Bearer $JWT" \
  https://api.coinbase.com/api/v3/brokerage/portfolios
```

### When `retail_portfolio_id` is required

Coinbase's `POST /orders` endpoint routes to the key's bound portfolio by
default, so a single-portfolio account does not need to set this field.
Set it on [`CoinbaseExecutionClientConfig`](#execution-client-configuration-options)
when either is true:

- The account holds multiple portfolios and you want to trade against one
  that is not the key's default.
- The venue rejects orders with `account is not available` and the wallet
  diagnosis below has been ruled out.

:::note
Coinbase's [Create Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order)
marks `retail_portfolio_id` as deprecated and applicable only to legacy keys,
stating that CDP keys default to the key's permissioned portfolio. The adapter
still sends the field when configured, so it remains available if the venue's
default routing does not match your account layout.
:::

### Creating a new portfolio

Most users will not need to create a new portfolio; the account's default
works out of the box. Create one on
[coinbase.com/portfolios](https://www.coinbase.com/portfolios) only if you
want to:

- Segregate API-driven trading from manual retail activity.
- Isolate risk or P&L between strategies.
- Work around a restricted default (e.g. a Vault).

After creating a portfolio, fund it (transfer from the default portfolio's
wallet on coinbase.com) before sending any orders, otherwise the venue
returns `account is not available` for the quote currency.

### Troubleshooting `account is not available`

The venue returns this error for several distinct reasons; diagnose by
running the probe binary above and inspecting the portfolio wallet list.

| Symptom                                                              | Likely cause                                                                                                                                                                                  | Fix                                                                                                                                                                                                                                                                                      |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rejected only for a specific product (e.g. `BTC-USD` with only USDC) | Portfolio is missing a wallet for the product's quote currency. USD and USDC are separate on Coinbase, and the venue routes orders by the submitted `product_id`, not by the canonical alias. | Submit against the product whose quote currency you hold (e.g. `BTC-USDC` for USDC wallets). The adapter resolves the data-side alias internally; no config change needed. Funding the missing wallet via coinbase.com is also an option but unnecessary when only one currency is held. |
| Every order rejected across all products                             | Key is bound to a non-default portfolio and `retail_portfolio_id` is unset.                                                                                                                   | Set `retail_portfolio_id` on `CoinbaseExecutionClientConfig` to the target portfolio UUID.                                                                                                                                                                                               |
| Rejected for `*-USD` products on a non-US account                    | Jurisdictional restriction (e.g. AU accounts cannot trade USD-quoted pairs).                                                                                                                  | Use locally-available quotes (USDC, AUD, EUR, etc.) instead of USD.                                                                                                                                                                                                                      |
| Rejected right after key rotation                                    | New key was created in a different portfolio than the previous one.                                                                                                                           | Update `retail_portfolio_id` to match the new key's portfolio, or move funds.                                                                                                                                                                                                            |

## Market data

The data client serves everything except derivatives index and funding data
from WebSocket channels. Coinbase Advanced Trade does not publish index
prices or funding rates on any WebSocket channel, so those two streams are
sourced from REST polling instead.

| Nautilus subscription | Source                      | Notes                                                                       |
| --------------------- | --------------------------- | --------------------------------------------------------------------------- |
| Book deltas           | `level2` channel            | `L2_MBP` only; other book types are rejected.                               |
| Quotes                | `ticker` channel            | Top-of-book from the venue's ticker payload.                                |
| Trades                | `market_trades` channel     | Also available as a REST request.                                           |
| Bars                  | `candles` channel           | Fixed five-minute buckets; the venue accepts no granularity parameter.      |
| Instrument status     | `status` channel            | See [Instrument status](#instrument-status).                                |
| Index prices          | REST `/products/{id}` poll  | Derivatives only, at `derivatives_poll_interval_secs`.                      |
| Funding rates         | REST `/products/{id}` poll  | Perpetuals only; see [Funding rates](#funding-rates).                       |
| Mark prices           | *Not supported by Coinbase* | The subscription is rejected rather than synthesized from settlement price. |

A `heartbeats` subscription is always sent on connect and replayed on every
reconnect. It satisfies the venue's five-second subscribe deadline and keeps
the connection alive when the subscribed product topics are quiet.

### Bars

Historical bar requests accept EXTERNAL aggregation at the granularities the
adapter maps: 1m, 5m, 15m, 30m, 1h, 2h, 6h, and 1d. Any other step or
aggregation is rejected. Coinbase's `/products/{id}/candles` endpoint also
accepts `FOUR_HOUR`, which the adapter does not currently map.

Live bar subscriptions are different: the WebSocket `candles` channel takes
no granularity parameter and publishes five-minute buckets only. The adapter
stamps each received candle with the `BarType` registered for that product,
so subscribing at any other bar specification yields five-minute bars labelled
with the requested type. Request a `5-MINUTE-LAST-EXTERNAL` bar type for live
subscriptions, and use historical requests for the other granularities.

### Funding rates

The adapter polls the REST `/products/{id}` endpoint at
`derivatives_poll_interval_secs` (default 15 s) and emits a
`FundingRateUpdate` from the FCM `future_product_details` payload when
`funding_rate` is present. The funding interval is parsed from the
`funding_interval` field (typically `"3600s"`, hourly funding) and the next
funding timestamp from `funding_time`. Coinbase Advanced Trade does not
publish `funding_rate` on the WebSocket `ticker` channel, so REST polling
is the only live source.

Historical funding rate requests are not implemented.

### Instrument status

`subscribe_instrument_status` joins the Coinbase WebSocket `status` channel
on first subscription (the venue publishes one status feed for all
products), filters incoming events to the subscribed instruments, and emits
`InstrumentStatus` events with `MarketStatusAction::Trading` for `online`,
`Halt` for `offline`, and `Close` for `delisted`. Products reporting an
unset status (futures) or a status the adapter does not model carry no
information for the data engine and are skipped. The channel subscription is
dropped when the last instrument unsubscribes.

## Orders capability

The tables below describe the Coinbase **venue** order surface. The shipped
[`CoinbaseExecutionClient`](#execution-scope) handles spot or CFM derivatives
based on the configured `account_type`. Coinbase order capabilities differ
between Spot and Derivatives (perpetuals and dated futures share the same
FCM order surface).

### Execution scope

`CoinbaseExecutionClientFactory` produces a single `CoinbaseExecutionClient`
type. The product family is selected by the `account_type` field on
`CoinbaseExecutionClientConfig`:

| `account_type`        | Bootstrap instruments                         | Account state source                                                                                   |
| --------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `AccountType::Cash`   | `CoinbaseProductType::Spot` only.             | `/accounts` REST endpoint.                                                                             |
| `AccountType::Margin` | `CoinbaseProductType::Future` (perp + dated). | CFM `balance_summary` REST + `futures_balance_summary` WS, plus position reports from `cfm/positions`. |

Other account types are rejected at factory creation. OMS is always
`Netting` because the venue does not expose hedge mode.

To prevent cross-account bleed-through:

1. Connect-time instrument bootstrap is limited to the configured product
   family; the other family's products never enter the in-process cache.
2. `submit_order` denies any order whose instrument is outside that cache.
3. `generate_order_status_report(s)` and `generate_fill_reports` post-filter
   their output through the same cache, so a Coinbase account that holds
   both spot and derivative activity will not surface the other scope's
   reports through a single client.

Run one execution client per scope; if you need both spot and CFM activity
on the same trader, instantiate two clients with distinct `account_type`
values (and distinct `account_id`s).

### Order types

The matrix lists order types as exposed through the Nautilus model. The
right column shows the corresponding `order_configuration` keys the adapter
emits. Coinbase order types not in this table (TWAP, Bracket, Scaled, SOR
LIMIT IOC) are documented under [Advanced order features](#advanced-order-features)
and noted there as *Not currently implemented* by the adapter.

| Order Type             | Spot | Perpetual | Future | Wire shape                                                      |
| ---------------------- | ---- | --------- | ------ | --------------------------------------------------------------- |
| `MARKET`               | ✓    | ✓         | ✓      | `market_market_ioc` (all products); `market_market_fok` (perps) |
| `LIMIT`                | ✓    | ✓         | ✓      | `limit_limit_gtc` / `limit_limit_gtd` / `limit_limit_fok`       |
| `STOP_LIMIT`           | ✓    | ✓         | ✓      | `stop_limit_stop_limit_gtc` / `stop_limit_stop_limit_gtd`       |
| `STOP_MARKET`          | -    | -         | -      | *Not supported by Coinbase*.                                    |
| `MARKET_IF_TOUCHED`    | -    | -         | -      | *Not supported by Coinbase*.                                    |
| `LIMIT_IF_TOUCHED`     | -    | -         | -      | *Not supported by Coinbase*.                                    |
| `TRAILING_STOP_MARKET` | -    | -         | -      | *Not supported by Coinbase*.                                    |

### Execution instructions

| Instruction   | Spot | Perpetual | Future | Notes                                                                         |
| ------------- | ---- | --------- | ------ | ----------------------------------------------------------------------------- |
| `post_only`   | ✓    | ✓         | ✓      | LIMIT GTC and LIMIT GTD only.                                                 |
| `reduce_only` | -    | -         | -      | *Not supported by Coinbase*; see [Derivatives trading](#derivatives-trading). |

### Time in force

The adapter accepts the values in this matrix; combinations not listed are
rejected at submit time with `"Unsupported TIF {tif} for {order_type}"`.

| Order type   | GTC | GTD | IOC | FOK | Notes                                                                                                                                            |
| ------------ | --- | --- | --- | --- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `MARKET`     | ✓   | -   | ✓   | (✓) | GTC is mapped to IOC; explicit IOC is honoured. FOK builds `market_market_fok`, which Coinbase documents as perpetuals-only and rejects on spot. |
| `LIMIT`      | ✓   | ✓   | -   | ✓   | GTD requires `expire_time`. LIMIT IOC *not currently implemented* (see [SOR LIMIT IOC](#advanced-order-features)).                               |
| `STOP_LIMIT` | ✓   | ✓   | -   | -   | Requires `trigger_price`.                                                                                                                        |

### Advanced order features

| Feature            | Spot | Perpetual | Future | Notes                                                                                          |
| ------------------ | ---- | --------- | ------ | ---------------------------------------------------------------------------------------------- |
| Order Modification | ✓    | -         | -      | Open GTC variants only; Coinbase rejects futures-venue edits with `CANNOT_EDIT_FUTURES_ORDER`. |
| Bracket Orders     | -    | -         | -      | *Not currently implemented*. Venue exposes `trigger_bracket_gtc` / `trigger_bracket_gtd`.      |
| OCO Orders         | -    | -         | -      | *Not supported by Coinbase* as a distinct order type.                                          |
| Iceberg Orders     | -    | -         | -      | *Not supported by Coinbase*.                                                                   |
| TWAP Orders        | -    | -         | -      | *Not currently implemented*. Venue exposes `twap_limit_gtd`.                                   |
| Scaled Orders      | -    | -         | -      | *Not currently implemented*. Venue exposes `scaled_limit_gtc`.                                 |
| SOR LIMIT IOC      | -    | -         | -      | *Not currently implemented*. Venue exposes `sor_limit_ioc` for smart-order-routed LIMIT IOC.   |

See the [Create Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order)
and [Edit Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/edit-order)
for the underlying venue specification.

### Position controls (derivatives)

| Control       | Notes                                       |
| ------------- | ------------------------------------------- |
| Leverage      | Set per order; default `1.0`.               |
| Margin type   | Set per order: cross (default) or isolated. |
| Position mode | One-way only; hedge mode is not exposed.    |

### Batch operations

| Operation    | Notes                                                                                                                                         |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Batch Submit | Not supported. Each order is one `Create Order` request.                                                                                      |
| Batch Modify | Not supported. Each edit is one `Edit Order` request.                                                                                         |
| Batch Cancel | `POST /api/v3/brokerage/orders/batch_cancel` accepts an `order_ids` array. No documented max size; per-order success/failure in the response. |

### Order querying

| Feature              | Spot | Perpetual | Future | Notes                                       |
| -------------------- | ---- | --------- | ------ | ------------------------------------------- |
| Query open orders    | ✓    | ✓         | ✓      | List all active orders.                     |
| Query order history  | ✓    | ✓         | ✓      | Historical order data with cursor paging.   |
| Order status updates | ✓    | ✓         | ✓      | Real-time state changes via `user` channel. |
| Trade history        | ✓    | ✓         | ✓      | Execution and fill reports.                 |

### Spot trading limitations

- MARKET FOK is not accepted on spot; Coinbase documents `market_market_fok`
  as perpetuals-only and rejects it with `UNSUPPORTED_ORDER_CONFIGURATION`.
- Quote-denominated MARKET orders are supported; LIMIT orders are sized in
  base units.

### Derivatives trading

Coinbase derivatives trade through the FCM (Futures Commission Merchant)
venue. The exec client submits orders through the same `POST /orders`
endpoint used for spot; per-order `leverage` and `margin_type` (`CROSS` or
`ISOLATED`) defaults come from `CoinbaseExecutionClientConfig.default_leverage`
and `default_margin_type`. Margin balances update from both the REST
`cfm/balance_summary` endpoint (connect-time snapshot, `query_account`,
and on WebSocket reconnect) and the authenticated `futures_balance_summary`
WebSocket channel. Position reports come from the REST `cfm/positions`
endpoints.

Coinbase's Advanced Trade API does not document a `reduce_only` field on the create-order schema.
The execution client rejects reduce-only orders before transport instead of submitting them without
the instruction.

The adapter logs a warning when a REST order status report describes a
forced-close order, and when the CFM balance summary reports a liquidation
buffer below 20% of the liquidation threshold. Coinbase does not flag
auto-deleveraging separately from liquidation, so both surface through the
same warning. The user channel carries no equivalent warning, so forced
closes are visible from reconciliation rather than from the live stream.

## Execution client behaviour

This section documents how `CoinbaseExecutionClient` translates Nautilus
order commands and Coinbase venue events into Nautilus execution events.

### Order submission

`submit_order` builds the Coinbase `order_configuration` shape directly from
Nautilus order fields:

- `MARKET` IOC and GTC (the Nautilus default) -> `market_market_ioc`; FOK ->
  `market_market_fok`. `Day` and `Gtd` are rejected before the HTTP call so
  callers do not silently receive IOC semantics. A `MARKET` order built with
  `Gtc` executes as IOC at the venue; strategies that require strict
  backtest/live parity should construct `MarketOrder` with `Ioc` explicitly.
- `LIMIT` GTC -> `limit_limit_gtc`, GTD -> `limit_limit_gtd` (requires
  `expire_time`), FOK -> `limit_limit_fok`.
- `STOP_LIMIT` GTC -> `stop_limit_stop_limit_gtc`, GTD ->
  `stop_limit_stop_limit_gtd`. Stop direction is derived from the order
  side (`Buy` -> `STOP_DIRECTION_STOP_UP`, `Sell` -> `STOP_DIRECTION_STOP_DOWN`).
- `STOP_MARKET`, `MARKET_IF_TOUCHED`, `LIMIT_IF_TOUCHED`, and trailing-stop
  variants are not supported by Coinbase. They surface as `OrderRejected`
  carrying the `build_order_configuration` error from the spawned submit
  task (the order is emitted as `OrderSubmitted` first).

On a successful HTTP create, an `OrderAccepted` is emitted carrying the
venue order ID returned in `success_response.order_id`. On a `success=false`
response, `OrderRejected` is emitted with the formatted venue failure reason.
Because any submit attempt may have reached Coinbase, a transport error,
timeout, rate-limit response, decode failure, or HTTP 5xx does not prove
rejection. The adapter leaves the order in flight and retains its submit
metadata until the user channel or reconciliation resolves it.

### Order modification

`modify_order` posts to `/orders/edit` with the typed `EditOrderRequest`.
Coinbase supports edits on open GTC variants only, and rejects edits on
futures-venue orders with `CANNOT_EDIT_FUTURES_ORDER`, so modification is
effectively spot-only. Other order types must use cancel-replace.

Coinbase's `/orders/edit` requires both `price` and `size` even when only one
is changing; an omitted `size` is read as 0 and rejected with
`INVALID_EDITED_SIZE` or `CANNOT_EDIT_TO_BELOW_FILLED_SIZE`. The exec client
auto-fills missing fields from the cached order, so strategies can call
`modify_order(price=X)` without repeating the current quantity. Values from
the `ModifyOrder` command win; otherwise the cached order's current `price`
and `quantity` are used.

Venue edit failures emit `OrderModifyRejected` with the typed `EditOrderResponse`
reason (preferring `edit_failure_reason`, falling back to `preview_failure_reason`).
HTTP failures with unknown venue outcome leave the order in `PENDING_UPDATE` until
an update, query result, or reconciliation resolves it.

### Cancellation

- `cancel_order` posts a single-id `batch_cancel`. An explicit per-order venue
  failure surfaces as `OrderCancelRejected`; a whole-request transport failure
  with unknown venue outcome leaves the order in `PENDING_CANCEL` for
  reconciliation.
- `cancel_all_orders` lists open orders via REST without the `OPEN`-only
  filter (because Coinbase's `OPEN` filter excludes `PENDING` and `QUEUED`
  orders that are still cancelable), filters locally to
  `{Accepted, Triggered, PendingUpdate, PartiallyFilled}` and the requested
  side, then chunks `batch_cancel` calls in groups of 100.
  Per-order venue failures emit `OrderCancelRejected`; whole-request failures
  with unknown venue outcome leave affected orders pending reconciliation.
- `batch_cancel_orders` chunks the same way and surfaces explicit per-order
  venue failures as `OrderCancelRejected`. Transport failures with unknown
  venue outcome leave affected orders pending reconciliation.

### User WebSocket channel

`CoinbaseExecutionClient` subscribes to the `user` channel with no
`product_ids` filter and a fresh JWT, parses each event into an
`OrderStatusReport`, and feeds it to the execution event stream. Coinbase
reports cumulative state per order rather than per-trade fills, so the exec
client synthesizes a `FillReport` from the cumulative delta. The per-fill
price is derived as `(avg_now * qty_now - avg_prev * qty_prev) / delta_qty`
so multi-fill orders carry the correct trade price, not the cumulative
weighted average. The original quantity is restored on terminal updates
(`CANCELLED`, `EXPIRED`, `FAILED`) where the venue zeroes `leaves_quantity`.

The user channel does not echo `price`, `stop_price`, `trigger_type`, or
maker/taker classification. The exec client caches these at submit time
under the `client_order_id` and patches reports before emit, so the
reconciler does not observe a `Some(price) -> None` divergence and
`post_only` fills are correctly stamped `liquidity_side = Maker`. Order
status `PENDING`, `QUEUED`, and `OPEN` all map to `OrderStatus::Accepted` to
avoid spurious backwards-transition warnings when user-channel updates
race the REST `OrderAccepted` event.

A `submit_order` rejection carrying `INVALID_LIMIT_PRICE_POST_ONLY` (or the
preview/new-order equivalent) is emitted with `due_post_only = true` so
strategies can react to post-only crossings (typically by re-quoting against
the new TOB).

On reconnect, account state is re-fetched via REST so balance changes during
the disconnect window are recovered. Cumulative per-order tracking persists
across reconnects so synthesized fill deltas remain correct.

### Fill deduplication

The user-channel WebSocket can replay events on reconnect. The execution
client maintains a 10,000-entry FIFO dedup keyed on
`(venue_order_id, trade_id)` and drops any fill whose synthesized trade ID
matches a recently-seen one. The cumulative-state map is bounded with the
same capacity to protect against orders that never receive a terminal
event in this client's lifetime. After very long disconnections (beyond
the in-memory dedup window) replayed fills may emit duplicate
`FillReport` values; strategies should rely on REST reconciliation to
recover canonical state in that case.

### Position reconciliation

For Cash (spot) accounts the client returns no position reports because
Coinbase spot has no positions. For Margin accounts position reports come
from the REST `cfm/positions` (list) and `cfm/positions/{product_id}`
(single) endpoints and are post-filtered to the bootstrap instrument cache.
Open orders and historical fills are reconciled from REST via
`generate_order_status_report(s)` and `generate_fill_reports` on connect
and on the standard reconciliation interval set by `LiveExecutionEngineConfig`.

## Rate limiting

Coinbase publishes the following limits for the Advanced Trade APIs:

| Surface                        | Limit                                                                               | Source                               |
| ------------------------------ | ----------------------------------------------------------------------------------- | ------------------------------------ |
| WebSocket connections          | 8 per second per IP address                                                         | Advanced Trade WebSocket Rate Limits |
| WebSocket unauthenticated msgs | 8 per second per IP address                                                         | Advanced Trade WebSocket Rate Limits |
| WebSocket subscribe deadline   | First subscribe message must arrive within 5 s of connect or the server disconnects | Advanced Trade WebSocket Overview    |
| Authenticated WebSocket JWT    | 120 s; a fresh JWT must be generated for every authenticated subscribe message      | Advanced Trade WebSocket Overview    |
| REST per-key quota             | 10,000 requests per hour per API key (Coinbase App general policy)                  | Coinbase App Rate Limiting           |

When the REST limit is exceeded, Coinbase returns HTTP `429` with this body:

```json
{
  "errors": [
    {
      "id": "rate_limit_exceeded",
      "message": "Too many requests"
    }
  ]
}
```

The adapter additionally throttles all REST traffic client-side at 30 requests
per second, and subscribe and unsubscribe messages at 8 per second, so bursts
are shaped before they reach the venue.

:::info
Coinbase's current Advanced Trade documentation publishes WebSocket limits but
no Advanced Trade-specific REST quota (per-second ceilings, per-portfolio
limits), so the Coinbase App per-hour quota above is the most specific
documented value. References:
[WebSocket rate limits](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-rate-limits),
[WebSocket overview](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/websocket/websocket-overview),
[Coinbase App rate limiting](https://docs.cdp.coinbase.com/coinbase-app/api-architecture/rate-limiting).
:::

## Reconnect and resubscribe

The WebSocket client uses exponential backoff with a base of 250ms and a cap
of 30s on reconnect. Retained subscriptions are replayed automatically after
the handshake completes. Coinbase disconnects a client that has not sent a
subscribe message within 5 seconds of connecting, so the replay always
includes the `heartbeats` topic, which is marked before the first replay and
kept for the client's lifetime.

For authenticated channels (`user`, and `futures_balance_summary` on
Margin clients), the adapter generates a fresh JWT for every subscribe
message, as Coinbase requires a different JWT for each authenticated
WebSocket message. A topic that requires authentication is skipped rather
than sent unsigned when no credentials are configured or the JWT cannot be
built, and the failure surfaces as an error on the client's message stream.
Once a subscription is accepted the data flow continues for the lifetime of
the WebSocket connection without further authentication.

If the execution client is connected again while a prior user WebSocket is
still active or reconnecting, it tears that connection down and rebuilds the
inner client rather than reusing the existing state machine. This guarantees
a fresh command channel, output channel, and shutdown signal even when the
previous session's `Disconnect` command lost a race with the shutdown signal.

## Configuration

### Data client configuration options

| Option                             | Default   | Description                                                                       |
| ---------------------------------- | --------- | --------------------------------------------------------------------------------- |
| `api_key`                          | `None`    | Falls back to `COINBASE_API_KEY` env var.                                         |
| `api_secret`                       | `None`    | Falls back to `COINBASE_API_SECRET` env var.                                      |
| `base_url_rest`                    | `None`    | Override for the REST base URL.                                                   |
| `base_url_ws`                      | `None`    | Override for the WebSocket market data URL.                                       |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports.                             |
| `environment`                      | `Live`    | `Live` or `Sandbox`.                                                              |
| `http_timeout_secs`                | `10`      | HTTP request timeout (seconds).                                                   |
| `ws_timeout_secs`                  | `30`      | WebSocket timeout (seconds).                                                      |
| `update_instruments_interval_mins` | `60`      | Interval between instrument catalogue refreshes.                                  |
| `derivatives_poll_interval_secs`   | `15`      | Interval between REST polls that emit `IndexPriceUpdate` and `FundingRateUpdate`. |
| `transport_backend`                | `Sockudo` | WebSocket transport backend.                                                      |

### Execution client configuration options

| Option                   | Default   | Description                                                                                                                                |
| ------------------------ | --------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `account_id`             | `Venue`   | Nautilus account identifier; defaults to `COINBASE-001`.                                                                                   |
| `api_key`                | `None`    | Falls back to `COINBASE_API_KEY` env var.                                                                                                  |
| `api_secret`             | `None`    | Falls back to `COINBASE_API_SECRET` env var.                                                                                               |
| `base_url_rest`          | `None`    | Override for the REST base URL.                                                                                                            |
| `base_url_ws`            | `None`    | Override for the user data WebSocket URL.                                                                                                  |
| `proxy_url`              | `None`    | Optional proxy URL for HTTP and WebSocket transports.                                                                                      |
| `environment`            | `Live`    | `Live` or `Sandbox`.                                                                                                                       |
| `http_timeout_secs`      | `10`      | HTTP request timeout (seconds).                                                                                                            |
| `max_retries`            | `3`       | Maximum retry attempts for HTTP requests.                                                                                                  |
| `retry_delay_initial_ms` | `100`     | Initial retry delay (milliseconds).                                                                                                        |
| `retry_delay_max_ms`     | `5,000`   | Maximum retry delay (milliseconds).                                                                                                        |
| `account_type`           | `Cash`    | `Cash` for spot or `Margin` for CFM derivatives. See [Execution scope](#execution-scope).                                                  |
| `default_margin_type`    | `None`    | Default `CoinbaseMarginType` (`Cross` or `Isolated`) applied to derivatives orders. Ignored on Cash.                                       |
| `default_leverage`       | `None`    | Default leverage applied to derivatives orders. Ignored on Cash.                                                                           |
| `retail_portfolio_id`    | `None`    | CDP retail portfolio UUID, sent on create-order when set. Coinbase marks the field deprecated for CDP keys. See [Portfolios](#portfolios). |
| `transport_backend`      | `Sockudo` | WebSocket transport backend.                                                                                                               |

Configurations are constructed from the adapter's public Python module:

```python
from nautilus_trader.adapters.coinbase import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase import CoinbaseEnvironment
from nautilus_trader.adapters.coinbase import CoinbaseExecutionClientConfig
from nautilus_trader.model import AccountId

data_config = CoinbaseDataClientConfig(
    api_key="YOUR_COINBASE_API_KEY",
    api_secret="YOUR_COINBASE_API_SECRET",
    environment=CoinbaseEnvironment.LIVE,
)

exec_config = CoinbaseExecutionClientConfig(
    account_id=AccountId("COINBASE-001"),
    api_key="YOUR_COINBASE_API_KEY",
    api_secret="YOUR_COINBASE_API_SECRET",
    environment=CoinbaseEnvironment.LIVE,
)
```

The current Python examples show how to pair these configs with
`CoinbaseDataClientFactory` and `CoinbaseExecutionClientFactory` in `LiveNode.builder(...)`.

## Known limitations

### Venue-side

- Order modification is restricted to open GTC orders and is rejected on
  futures-venue orders with `CANNOT_EDIT_FUTURES_ORDER`; everything else must
  use cancel-replace.
- OCO orders are not exposed as a distinct order type.
- Trailing stop, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED, and iceberg orders are
  not supported by Coinbase.
- Mark prices are not published on REST or WebSocket, so mark price
  subscriptions are rejected.
- Batch submit and batch modify are not available; only batch cancel is.
- Sandbox is a static-mock environment (Accounts and Orders endpoints only,
  pre-defined responses, no real market data).
- The user-channel WebSocket reports cumulative per-order state, not
  per-trade fills. The exec client derives per-fill quantity, price, and
  commission from the cumulative delta; per-trade `trade_id`s are
  synthesized from `(venue_order_id, cumulative_quantity)`.

### Adapter-side

- **Stable fill identity differs across live and REST paths.** The user
  channel does not provide Coinbase's per-fill `trade_id`, so live
  `FillReport` values use IDs synthesized from the venue order ID and
  cumulative quantity. REST reconciliation uses the venue `trade_id`, so the identifiers can
  differ across live processing and reconciliation.
- **One product family per client.** Submission, modification, cancellation,
  and report generation are filtered to the configured product family (spot
  under `AccountType::Cash`; perp + dated futures under `AccountType::Margin`).
  Orders whose instrument falls outside the bootstrapped cache are denied.
  See [Execution scope](#execution-scope).
- **Position reports are always empty for Cash accounts.** Coinbase spot has
  no positions. Derivatives (CFM) position reports come from `cfm/positions`
  and appear only on Margin clients.
- **User-channel updates omit `price`, `stop_price`, and `trigger_type`.**
  For orders this client submitted, the missing fields are patched from a
  cache populated at `submit_order` time. For external orders (submitted by
  another process or via the Coinbase UI), the user-channel handler
  enriches the report on first sight by fetching
  `/orders/historical/{venue_order_id}` and caching the result. The REST
  call adds latency to the first user-channel update for an external
  order; subsequent updates use the cached enrichment.
- **Cancel-all and batch-cancel REST list failures are logged only.** If the
  list-open-orders REST call fails, no per-order `OrderCancelRejected` is
  emitted; orders remain in `PendingCancel` until the next reconciliation
  recovers them. Mirrors the Bybit adapter pattern.
- **Newly listed products require a reconnect to be tradeable.** The
  instrument cache is populated on connect; products listed after that
  are not in the cache and `submit_order` will deny them.
- **MARKET orders default to IOC.** A `MarketOrder` constructed with the
  Nautilus default `TimeInForce::Gtc` is mapped to `market_market_ioc` at
  the venue. Explicit `TimeInForce::Ioc` is honoured; `TimeInForce::Fok`
  routes to `market_market_fok`, which Coinbase documents as perpetuals-only
  and rejects at runtime on spot with `UNSUPPORTED_ORDER_CONFIGURATION`.
  `Day` and `Gtd` are rejected at submit time.
- **Historical funding rate requests are not implemented.** Funding rates
  are available only as live updates from the derivatives REST poll.
- **Live bar subscriptions ignore the requested granularity.** The venue's
  `candles` channel publishes five-minute buckets and accepts no granularity
  parameter, and `subscribe_bars` does not reject other bar specifications.
  A subscription at any other step receives five-minute bars stamped with the
  requested `BarType`. See [Bars](#bars).

## Diagnostic binaries

Three binaries assist with connectivity checks, live verification, and
account hygiene:

- `coinbase-http-public` requests spot instruments, a product book, and
  recent trades without credentials. Use it to confirm connectivity before
  configuring an API key.
- `coinbase-http-private` lists portfolios, prints wallet balances, runs
  `/orders/preview` for `BTC-USD` and `BTC-USDC`, and surfaces per-product
  gating flags. Recommended first stop when bringing a new account online.
- `coinbase-cancel-all-open` cancels every open order on the authenticated
  CDP key. Useful between test runs to clear resting orders.

The two authenticated binaries read `COINBASE_API_KEY` and
`COINBASE_API_SECRET` from the environment.

## Contributing

:::info
For additional features or to contribute to the Coinbase adapter, please see
our [contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
