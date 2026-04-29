# Coinbase

Founded in 2012, Coinbase is one of the largest US-regulated cryptocurrency
exchanges, offering trading across spot, perpetual swaps, and dated futures via
the Advanced Trade API. This adapter supports live market data ingest and
order execution on both spot (Cash) and CFM derivatives (Margin) accounts
through a shared execution client, with the account type selected by the
factory (see [Execution scope](#execution-scope)).

:::note
This adapter is Rust-only and is consumed by the v2 system (and the Rust
`LiveNode`). It does not ship a legacy Python `TradingNode` integration;
only configuration and enum types are exported through PyO3 so v2 Python
entry points can construct them.
:::

## Overview

The Coinbase adapter is implemented in Rust and consumed by the v2 system.
The adapter does not ship a legacy Python `TradingNode` integration; only
configuration and enum types are exported through PyO3 so v2 entry points can
construct them from Python.

Current components:

| Component                          | Status | Notes                                                                      |
|------------------------------------|--------|----------------------------------------------------------------------------|
| `CoinbaseHttpClient`               | Built  | Two‑layer REST client: raw endpoint methods + domain wrapper.              |
| `CoinbaseWebSocketClient`          | Built  | Low‑level WebSocket connectivity with JWT subscribe auth.                  |
| `CoinbaseInstrumentProvider`       | Built  | Instrument parsing and loading.                                            |
| `CoinbaseDataClient`               | Built  | Rust market data feed manager.                                             |
| `CoinbaseDataClientFactory`        | Built  | Rust data client factory.                                                  |
| `CoinbaseExecutionClient`          | Built  | Rust execution client (spot or CFM derivatives; REST orders + WS streams). |
| `CoinbaseExecutionClientFactory`   | Built  | Execution client factory; spot vs CFM derivatives is selected by `account_type` on the config. |

PyO3 surface available from `nautilus_trader.core.nautilus_pyo3.coinbase`:

- `CoinbaseDataClientConfig`, `CoinbaseExecClientConfig`
- `CoinbaseEnvironment`, `CoinbaseMarginType`
- `COINBASE` venue constant

## Coinbase documentation

Coinbase provides documentation for the Advanced Trade API:

- [REST API reference](https://docs.cdp.coinbase.com/advanced-trade/reference)
- [WebSocket channels](https://docs.cdp.coinbase.com/advanced-trade/docs/ws-channels)
- [API key authentication](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication)
- [Rate limits](https://docs.cdp.coinbase.com/advanced-trade/docs/rate-limits)

It's recommended you also refer to the Coinbase documentation in conjunction
with this NautilusTrader integration guide.

:::info
This adapter targets the Coinbase Advanced Trade API. The separate
[Coinbase International Exchange (INTX)](https://international.coinbase.com)
venue is supported by the dedicated `coinbase_intx` adapter.
:::

## Products

A product is an umbrella term for a group of related instrument types.

The following product types are supported:

| Product Type        | Supported | Notes                                                    |
|---------------------|-----------|----------------------------------------------------------|
| Spot                | ✓         | USD, USDC, and USDT-quoted spot pairs.                   |
| Perpetual contracts | ✓         | USD-margined perpetual swaps on the FCM venue.           |
| Futures contracts   | ✓         | Dated delivery futures (nano BTC, nano ETH, etc).        |

## Symbology

Coinbase uses the venue's native `product_id` field directly as the Nautilus
symbol. The instrument ID is `{product_id}.COINBASE`.

| Product          | Format                             | Examples                           |
|------------------|------------------------------------|------------------------------------|
| Spot             | `{base}-{quote}`                   | `BTC-USD`, `ETH-USDC`, `SOL-USDT`. |
| Perpetual        | `{contract_code}-{ddMMMyy}-CDE`    | `BIP-20DEC30-CDE` (BTC PERP).      |
| Dated future     | `{contract_code}-{ddMMMyy}-CDE`    | `BIT-24APR26-CDE` (BTC Apr 2026).  |

The `-CDE` suffix denotes the Coinbase Derivatives Exchange (FCM venue).
Perpetuals carry an exchange-assigned far-future expiry (e.g. `20DEC30`) but
are classified as `CryptoPerpetual` based on the presence of an ongoing
funding rate. Dated futures are classified as `CryptoFuture`.

The adapter resolves the product type structurally from API metadata
(`future_product_details.perpetual_details.funding_rate` plus
`contract_expiry_type`); the fallback heuristic checks `display_name` for
`PERP` or `Perpetual` substrings.

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
the request to the canonical id on the wire. The adapter handles this
transparently: it records the `product_id -> alias` map at bootstrap, sends
the canonical id on subscribe and order submit, registers a reverse mapping
on the WebSocket clients, and re-keys inbound messages back to the
caller-supplied id before parsing.

A strategy holding only USDC can therefore trade `BTC-USDC.COINBASE` end to
end without referencing the canonical `BTC-USD`. Settlement currency is
determined by the submitted `product_id`, so an order placed on
`BTC-USDC.COINBASE` always debits or credits the USDC wallet.

## Environments

Coinbase provides two trading environments. Configure the appropriate
environment using the `environment` field in your client configuration.

| Environment | `environment` value             | REST base URL                      |
|-------------|---------------------------------|------------------------------------|
| Live        | `CoinbaseEnvironment.LIVE`      | `https://api.coinbase.com`         |
| Sandbox     | `CoinbaseEnvironment.SANDBOX`   | `https://api-sandbox.coinbase.com` |

### Live (production)

The default environment for live trading with real funds.

```python
config = CoinbaseExecClientConfig(
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
config = CoinbaseExecClientConfig(
    environment=CoinbaseEnvironment.SANDBOX,
    # API credentials are not required by sandbox.
)
```

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
Do not use legacy API keys from coinbase.com/settings/api (UUID format with
HMAC-SHA256 signing). Those use a different auth scheme (`CB-ACCESS-*`
headers) that the adapter does not support.
:::

For full details see the Coinbase
[API key authentication guide](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication).

### Environment variables

| Variable              | Description                                               |
|-----------------------|-----------------------------------------------------------|
| `COINBASE_API_KEY`    | Key name (`organizations/{org_id}/apiKeys/{key_id}`).     |
| `COINBASE_API_SECRET` | PEM‑encoded EC private key (full multi‑line string).      |

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

```
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
Set it on [`CoinbaseExecClientConfig`](#execution-client-configuration-options)
when either is true:

- The account holds multiple portfolios and you want to trade against one
  that is not the key's default.
- The venue rejects orders with `account is not available` and the wallet
  diagnosis below has been ruled out.

### Creating a new portfolio

Most users will not need to create a new portfolio; the account's default
works out of the box. Create one on
[coinbase.com/portfolios](https://www.coinbase.com/portfolios) only if you
want to:

- Segregate API‑driven trading from manual retail activity.
- Isolate risk or P&L between strategies.
- Work around a restricted default (e.g. a Vault).

After creating a portfolio, fund it (transfer from the default portfolio's
wallet on coinbase.com) before sending any orders, otherwise the venue
returns `account is not available` for the quote currency.

### Troubleshooting `account is not available`

The venue returns this error for several distinct reasons; diagnose by
running the probe binary above and inspecting the portfolio wallet list.

| Symptom                                                              | Likely cause                                                                                          | Fix                                                                                       |
|----------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------|
| Rejected only for a specific product (e.g. `BTC-USD` with only USDC) | Portfolio is missing a wallet for the product's quote currency. USD and USDC are separate on Coinbase, and the venue routes orders by the submitted `product_id`, not by the canonical alias. | Submit against the product whose quote currency you hold (e.g. `BTC-USDC` for USDC wallets). The adapter resolves the data‑side alias internally; no config change needed. Funding the missing wallet via coinbase.com is also an option but unnecessary when only one currency is held. |
| Every order rejected across all products                             | Key is bound to a non‑default portfolio and `retail_portfolio_id` is unset.                           | Set `retail_portfolio_id` on `CoinbaseExecClientConfig` to the target portfolio UUID.     |
| Rejected for `*-USD` products on a non‑US account                    | Jurisdictional restriction (e.g. AU accounts cannot trade USD‑quoted pairs).                          | Use locally‑available quotes (USDC, AUD, EUR, etc.) instead of USD.                       |
| Rejected right after key rotation                                    | New key was created in a different portfolio than the previous one.                                   | Update `retail_portfolio_id` to match the new key's portfolio, or move funds.             |

## Orders capability

The tables below describe the Coinbase **venue** order surface. The shipped
[`CoinbaseExecutionClient`](#execution-scope) handles spot or CFM derivatives
based on the configured `account_type`. Coinbase order capabilities differ
between Spot and Derivatives (perpetuals and dated futures share the same
FCM order surface).

### Execution scope

`CoinbaseExecutionClientFactory` produces a single `CoinbaseExecutionClient`
type. The product family is selected by the `account_type` field on
`CoinbaseExecClientConfig`:

| `account_type`        | Bootstrap instruments                         | Account state source                                      |
|-----------------------|-----------------------------------------------|-----------------------------------------------------------|
| `AccountType::Cash`   | `CoinbaseProductType::Spot` only.             | `/accounts` REST endpoint.                                |
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
and noted there as *Not yet supported* by the adapter.

| Order Type             | Spot | Perpetual | Future | Wire shape                                                  |
|------------------------|------|-----------|--------|-------------------------------------------------------------|
| `MARKET`               | ✓    | ✓         | ✓      | `market_market_ioc` (spot + CFM); `market_market_fok` (CFM only) |
| `LIMIT`                | ✓    | ✓         | ✓      | `limit_limit_gtc` / `limit_limit_gtd` / `limit_limit_fok`   |
| `STOP_LIMIT`           | -    | ✓         | ✓      | `stop_limit_stop_limit_gtc` / `stop_limit_stop_limit_gtd`   |
| `STOP_MARKET`          | -    | -         | -      | *Not exposed by the venue.*                                 |
| `MARKET_IF_TOUCHED`    | -    | -         | -      | *Not exposed by the venue.*                                 |
| `LIMIT_IF_TOUCHED`     | -    | -         | -      | *Not exposed by the venue.*                                 |
| `TRAILING_STOP_MARKET` | -    | -         | -      | *Not exposed by the venue.*                                 |

### Execution instructions

| Instruction   | Spot | Perpetual | Future | Notes                                                              |
|---------------|------|-----------|--------|--------------------------------------------------------------------|
| `post_only`   | ✓    | ✓         | ✓      | LIMIT GTC and LIMIT GTD only.                                      |
| `reduce_only` | -    | ✓         | ✓      | Derivatives only.                                                  |

### Time in force

The adapter accepts the values in this matrix; combinations not listed are
rejected at submit time with `"Unsupported TIF {tif} for {order_type}"`.

| Order type   | GTC | GTD | IOC | FOK | Notes                                                          |
|--------------|-----|-----|-----|-----|----------------------------------------------------------------|
| `MARKET`     | ✓   | -   | ✓   | (✓) | GTC is mapped to IOC; explicit IOC is honoured. FOK builds the venue's `market_market_fok` shape, but the matching engine currently rejects it on spot with `UNSUPPORTED_ORDER_CONFIGURATION`; usable on CFM derivatives only. |
| `LIMIT`      | ✓   | ✓   | -   | ✓   | GTD requires `expire_time`. LIMIT IOC *not yet supported* (see [SOR LIMIT IOC](#advanced-order-features)). |
| `STOP_LIMIT` | ✓   | ✓   | -   | -   | Requires `trigger_price`. Derivatives only.                    |

### Advanced order features

| Feature            | Spot | Perpetual | Future | Notes                                                                              |
|--------------------|------|-----------|--------|------------------------------------------------------------------------------------|
| Order Modification | ✓    | ✓         | ✓      | GTC variants only (LIMIT, STOP_LIMIT, Bracket); other types use cancel‑replace.    |
| Bracket Orders     | -    | -         | -      | *Not yet supported.* Venue exposes `trigger_bracket_gtc` / `trigger_bracket_gtd`.  |
| OCO Orders         | -    | -         | -      | *Not exposed by the venue* as a distinct order type.                               |
| Iceberg Orders     | -    | -         | -      | *Not exposed by the venue.*                                                        |
| TWAP Orders        | -    | -         | -      | *Not yet supported.* Venue exposes `twap_limit_gtd`.                               |
| Scaled Orders      | -    | -         | -      | *Not yet supported.* Venue exposes `scaled_limit_gtc`.                             |
| SOR LIMIT IOC      | -    | -         | -      | *Not yet supported.* Venue exposes `sor_limit_ioc` for smart‑order‑routed LIMIT IOC. |

See the [Create Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order)
and [Edit Order reference](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/edit-order)
for the underlying venue specification.

### Position controls (derivatives)

| Control       | Notes                                                                |
|---------------|----------------------------------------------------------------------|
| Leverage      | Set per order; default `1.0`.                                        |
| Margin type   | Set per order: cross (default) or isolated.                          |
| Position mode | One‑way only; hedge mode is not exposed.                             |

### Batch operations

| Operation     | Notes                                                                                              |
|---------------|----------------------------------------------------------------------------------------------------|
| Batch Submit  | Not supported. Each order is one `Create Order` request.                                           |
| Batch Modify  | Not supported. Each edit is one `Edit Order` request.                                              |
| Batch Cancel  | `POST /api/v3/brokerage/orders/batch_cancel` accepts an `order_ids` array. No documented max size; per‑order success/failure in the response. |

### Order querying

| Feature              | Spot | Perpetual | Future | Notes                                       |
|----------------------|------|-----------|--------|---------------------------------------------|
| Query open orders    | ✓    | ✓         | ✓      | List all active orders.                     |
| Query order history  | ✓    | ✓         | ✓      | Historical order data with cursor paging.   |
| Order status updates | ✓    | ✓         | ✓      | Real‑time state changes via `user` channel. |
| Trade history        | ✓    | ✓         | ✓      | Execution and fill reports.                 |

### Spot trading limitations

- `reduce_only` is not supported on spot orders (the instruction applies to
  derivatives).
- Trailing stop orders are not supported.
- Native stop‑limit and bracket orders are not available on Spot.
- Quote‑denominated MARKET orders are supported; LIMIT orders are sized in
  base units.

### Derivatives trading

Coinbase derivatives trade through the FCM (Futures Commission Merchant)
venue. The exec client submits orders through the same `POST /orders`
endpoint used for spot; per-order `leverage` and `margin_type` (`CROSS` or
`ISOLATED`) defaults come from `CoinbaseExecClientConfig.default_leverage`
and `default_margin_type`. Margin balances update from both the REST
`cfm/balance_summary` endpoint (connect-time snapshot, `query_account`,
and on WebSocket reconnect) and the authenticated `futures_balance_summary`
WebSocket channel. Position reports come from the REST `cfm/positions`
endpoints.

Coinbase's Advanced Trade API does not document a `reduce_only` field on
the create-order schema, even though the venue's failure-reason enum
acknowledges the concept. The client threads `reduce_only` through its
`submit_order` signature for API parity and includes the flag on the wire
only when set to `true`; if the venue later accepts it, no client changes
are required.

#### Funding rates

The adapter polls the REST `/products/{id}` endpoint at
`derivatives_poll_interval_secs` (default 15 s) and emits a
`FundingRateUpdate` from the FCM `future_product_details` payload when
`funding_rate` is present. The funding interval is parsed from the
`funding_interval` field (typically `"3600s"`, hourly funding) and the next
funding timestamp from `funding_time`. Coinbase Advanced Trade does not
publish `funding_rate` on the WebSocket `ticker` channel, so REST polling
is the only live source.

Historical funding rate requests are served by reading the same REST
products endpoint and deriving the interval from consecutive funding
timestamps.

#### Position reconciliation

For Cash (spot) accounts the client returns no position reports because
Coinbase spot has no positions. For Margin accounts position reports come
from the REST `cfm/positions` (list) and `cfm/positions/{product_id}`
(single) endpoints and are post-filtered to the bootstrap instrument cache.
Open orders and historical fills are reconciled from REST via
`generate_order_status_report(s)` and `generate_fill_reports` on connect
and on the standard reconciliation interval set by `LiveExecEngineConfig`.

#### Fill deduplication

The user-channel WebSocket can replay events on reconnect. The execution
client maintains a 10,000-entry FIFO dedup keyed on
`(venue_order_id, trade_id)` and drops any fill whose synthesized trade ID
matches a recently-seen one. The cumulative-state map is bounded with the
same capacity to protect against orders that never receive a terminal
event in this client's lifetime. After very long disconnections (beyond
the in-memory dedup window) replayed fills may emit duplicate
`OrderFilled` events; strategies should rely on REST reconciliation to
recover canonical state in that case.

## Execution client behaviour

This section documents how `CoinbaseExecutionClient` translates Nautilus
order commands and Coinbase venue events into Nautilus execution events.

### Order submission

`submit_order` builds the Coinbase `order_configuration` shape directly from
Nautilus order fields:

- `MARKET` -> `market_market_ioc`. Only `TimeInForce::Ioc` and `Gtc` (the
  Nautilus default) are accepted; any explicit `Fok`, `Day`, or `Gtd` on a
  market order is rejected before the HTTP call so callers do not silently
  receive IOC semantics. A `MARKET` order built with `Gtc` executes as IOC
  at the venue; strategies that require strict backtest/live parity should
  construct `MarketOrder` with `Ioc` explicitly.
- `LIMIT` GTC -> `limit_limit_gtc`, GTD -> `limit_limit_gtd` (requires
  `expire_time`), FOK -> `limit_limit_fok`.
- `STOP_LIMIT` GTC -> `stop_limit_stop_limit_gtc`, GTD ->
  `stop_limit_stop_limit_gtd`. Stop direction is derived from the order
  side (`Buy` -> `STOP_DIRECTION_STOP_UP`, `Sell` -> `STOP_DIRECTION_STOP_DOWN`).
- `STOP_MARKET`, `MARKET_IF_TOUCHED`, `LIMIT_IF_TOUCHED`, and trailing-stop
  variants are rejected with `OrderDenied` (not exposed by the venue).

On a successful HTTP create, an `OrderAccepted` is emitted carrying the
venue order ID returned in `success_response.order_id`. On a `success=false`
response or HTTP error, `OrderRejected` is emitted with the formatted
failure reason.

### Order modification

`modify_order` posts to `/orders/edit` with the typed `EditOrderRequest`.
Coinbase restricts edits to GTC variants (LIMIT, STOP_LIMIT, Bracket); other
order types must use cancel-replace.

Coinbase's `/orders/edit` requires both `price` and `size` even when only one
is changing; an omitted `size` is read as 0 and rejected with
`INVALID_EDITED_SIZE` or `CANNOT_EDIT_TO_BELOW_FILLED_SIZE`. The exec client
auto-fills missing fields from the cached order, so strategies can call
`modify_order(price=X)` without repeating the current quantity. Values from
the `ModifyOrder` command win; otherwise the cached order's current `price`
and `quantity` are used.

Failures emit `OrderModifyRejected` with the typed `EditOrderResponse` reason
(preferring `edit_failure_reason`, falling back to `preview_failure_reason`).

### Cancellation

- `cancel_order` posts a single-id `batch_cancel`. Per-order failure surfaces
  as `OrderCancelRejected`.
- `cancel_all_orders` lists open orders via REST without the `OPEN`-only
  filter (because Coinbase's `OPEN` filter excludes `PENDING` and `QUEUED`
  orders that are still cancelable), filters locally to
  `{Submitted, Accepted, Triggered, PendingUpdate, PartiallyFilled}` and
  the requested side, then chunks `batch_cancel` calls in groups of 100.
  Per-order and transport failures emit `OrderCancelRejected` for every
  affected order.
- `batch_cancel_orders` chunks the same way and surfaces both per-order
  failures and transport errors as `OrderCancelRejected`.

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

## Rate limiting

Coinbase publishes the following limits for the Advanced Trade APIs:

| Surface                           | Limit                                                | Source                                                |
|-----------------------------------|------------------------------------------------------|-------------------------------------------------------|
| WebSocket connections             | 8 per second per IP address                          | Advanced Trade WebSocket Rate Limits                  |
| WebSocket unauthenticated msgs    | 8 per second per IP address                          | Advanced Trade WebSocket Rate Limits                  |
| WebSocket subscribe deadline      | First subscribe message must arrive within 5 s of connect or the server disconnects | Advanced Trade WebSocket Overview |
| Authenticated WebSocket JWT       | 120 s; a fresh JWT must be generated for every authenticated subscribe message | Advanced Trade WebSocket Overview |
| REST per‑key quota                | 10,000 requests per hour per API key (Coinbase App general policy) | Coinbase App Rate Limiting       |

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

:::info
The Advanced Trade-specific REST quota (per-second ceilings, per-portfolio
limits) is not separately published in the Advanced Trade docs at the time of
writing; the Coinbase App per-hour quota above is the most specific
documented value. References:
[REST rate limits](https://docs.cdp.coinbase.com/advanced-trade/docs/rest-api-rate-limits/),
[WebSocket rate limits](https://docs.cdp.coinbase.com/advanced-trade/docs/ws-rate-limits),
[Coinbase App rate limiting](https://docs.cdp.coinbase.com/coinbase-app/api-architecture/rate-limiting).
:::

## Reconnect and resubscribe

The WebSocket client uses exponential backoff with a base of 250ms and a cap
of 30s on reconnect. After reconnect, subscriptions are restored automatically
in the order they were created. Coinbase requires a subscribe message within
5 seconds of connection or the server disconnects; the adapter sends queued
subscriptions immediately after the WebSocket handshake completes.

For authenticated channels (`user`, and `futures_balance_summary` on
Margin clients), the adapter generates a fresh JWT for every
subscribe message; per the Coinbase docs, "you must generate a different
JWT for each websocket message sent, since the JWTs will expire after 120
seconds." Once a subscription is accepted the data flow continues for
the lifetime of the WebSocket connection without further authentication.

When the exec client's WebSocket reconnects, the inner client is rebuilt
from scratch (rather than relying on the existing connection's state
machine) to guarantee a fresh `cmd_tx`/`out_rx`/signal trio even if the
prior session's `Disconnect` command lost a race with the shutdown signal.
Cumulative per-order tracking persists across reconnects so synthesized
fill deltas remain correct.

## Configuration

### Data client configuration options

| Option                             | Default | Description                                                                       |
|------------------------------------|---------|-----------------------------------------------------------------------------------|
| `api_key`                          | `None`  | Falls back to `COINBASE_API_KEY` env var.                                         |
| `api_secret`                       | `None`  | Falls back to `COINBASE_API_SECRET` env var.                                      |
| `base_url_rest`                    | `None`  | Override for the REST base URL.                                                   |
| `base_url_ws`                      | `None`  | Override for the WebSocket market data URL.                                       |
| `proxy_url`                        | `None`  | Optional proxy URL for HTTP and WebSocket transports.                             |
| `environment`                      | `Live`  | `Live` or `Sandbox`.                                                              |
| `http_timeout_secs`                | `10`    | HTTP request timeout (seconds).                                                   |
| `ws_timeout_secs`                  | `30`    | WebSocket timeout (seconds).                                                      |
| `update_instruments_interval_mins` | `60`    | Interval between instrument catalogue refreshes.                                  |
| `derivatives_poll_interval_secs`   | `15`    | Interval between REST polls that emit `IndexPriceUpdate` and `FundingRateUpdate`. |

### Execution client configuration options

| Option                   | Default | Description                                                                                              |
|--------------------------|---------|----------------------------------------------------------------------------------------------------------|
| `api_key`                | `None`  | Falls back to `COINBASE_API_KEY` env var.                                                                |
| `api_secret`             | `None`  | Falls back to `COINBASE_API_SECRET` env var.                                                             |
| `base_url_rest`          | `None`  | Override for the REST base URL.                                                                          |
| `base_url_ws`            | `None`  | Override for the user data WebSocket URL.                                                                |
| `proxy_url`              | `None`  | Optional proxy URL for HTTP and WebSocket transports.                                                    |
| `environment`            | `Live`  | `Live` or `Sandbox`.                                                                                     |
| `http_timeout_secs`      | `10`    | HTTP request timeout (seconds).                                                                          |
| `max_retries`            | `3`     | Maximum retry attempts for HTTP requests.                                                                |
| `retry_delay_initial_ms` | `100`   | Initial retry delay (milliseconds).                                                                      |
| `retry_delay_max_ms`     | `5000`  | Maximum retry delay (milliseconds).                                                                      |
| `account_type`           | `Cash`  | `Cash` for spot or `Margin` for CFM derivatives. See [Execution scope](#execution-scope).                |
| `default_margin_type`    | `None`  | Default `CoinbaseMarginType` (`Cross` or `Isolated`) applied to derivatives orders. Ignored on Cash.     |
| `default_leverage`       | `None`  | Default leverage applied to derivatives orders. Ignored on Cash.                                         |
| `retail_portfolio_id`    | `None`  | CDP retail portfolio UUID. Required when the API key is bound to a non‑default portfolio (the venue rejects orders with `account is not available` otherwise). See [Portfolios](#portfolios). |

Configurations are constructed from Python via the PyO3-exported types:

```python
from nautilus_trader.core.nautilus_pyo3 import CoinbaseDataClientConfig
from nautilus_trader.core.nautilus_pyo3 import CoinbaseExecClientConfig
from nautilus_trader.core.nautilus_pyo3 import CoinbaseEnvironment

data_config = CoinbaseDataClientConfig(
    api_key="YOUR_COINBASE_API_KEY",
    api_secret="YOUR_COINBASE_API_SECRET",
    environment=CoinbaseEnvironment.LIVE,
)

exec_config = CoinbaseExecClientConfig(
    api_key="YOUR_COINBASE_API_KEY",
    api_secret="YOUR_COINBASE_API_SECRET",
    environment=CoinbaseEnvironment.LIVE,
)
```

The v2 system instantiates the Rust factories directly from these configs;
no Python factory wiring is required.

## Known limitations

### Venue-side

- Order modification is restricted to GTC orders (LIMIT, STOP_LIMIT, Bracket);
  other types must use cancel-replace.
- OCO orders are not exposed as a distinct order type.
- Trailing stop, MARKET_IF_TOUCHED, LIMIT_IF_TOUCHED, and iceberg orders are
  not exposed by the venue.
- Batch submit and batch modify are not available; only batch cancel is.
- Sandbox is a static-mock environment (Accounts and Orders endpoints only,
  pre-defined responses, no real market data).
- The user-channel WebSocket reports cumulative per-order state, not
  per-trade fills. The exec client derives per-fill quantity, price, and
  commission from the cumulative delta; per-trade `trade_id`s are
  synthesized from `(venue_order_id, cumulative_quantity)`.

### Adapter-side

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
  routes to `market_market_fok` but is rejected at runtime by the matching
  engine on spot with `UNSUPPORTED_ORDER_CONFIGURATION` (the wire shape is
  documented in the API spec but only accepted on CFM derivatives). `Day`
  and `Gtd` are rejected at submit time.

## Authenticated binaries

Two binaries assist with live verification and account hygiene:

- `coinbase-http-private` lists portfolios, prints wallet balances, runs
  `/orders/preview` for `BTC-USD` and `BTC-USDC`, and surfaces per-product
  gating flags. Recommended first stop when bringing a new account online.
- `coinbase-cancel-all-open` cancels every open order on the authenticated
  CDP key. Useful between test runs to clear resting orders.

Both read `COINBASE_API_KEY` and `COINBASE_API_SECRET` from the environment.

## Contributing

:::info
For additional features or to contribute to the Coinbase adapter, please see
our [contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
