# Derive

Derive (formerly Lyra) is a decentralized derivatives venue offering European-style options
and cash-settled perpetual swaps, and one of the largest on-chain options markets. Trading
runs against subaccounts owned by the user's EOA or multisig, so collateral stays in the
user's custody while orders match through the venue's orderbook.

The v3 API batches operations for settlement on Ethereum. Orders match off chain and
settle on chain, pairing orderbook execution with self-custody. Orders are authorized with
EIP-712 typed-data signatures from a session key scoped to a subaccount, which keeps the
signing key separate from the wallet owner and lets users rotate or revoke access without
moving funds.

## Overview

The Derive adapter is implemented in Rust under `crates/adapters/derive`. It exposes:

- `DeriveHttpClient`: Low-level REST connectivity to `api.derive.xyz/v3` (mainnet) or
  `testnet.api.derive.xyz/v3` (testnet).
- `DeriveWebSocketClient`: JSON-RPC WebSocket transport with subscription tracking, reconnect, and signed order entry.
- `DeriveInstrumentProvider`: Per-currency instrument fetch and caching.
- `DeriveDataClient`: Live market data client.
- `DeriveDataClientFactory`: Data client factory for the live node builder.
- `DeriveExecutionClient`: Live execution client for signed order, cancel, query, and report flows.
- `DeriveExecutionClientFactory`: Execution client factory for the live node builder.

Execution flows use EIP-712 typed-data signing against the venue's per-action module contracts.

Python surface available from `nautilus_trader.adapters.derive`:

- `DeriveDataClientConfig`, `DeriveExecutionClientConfig`
- `DeriveDataClientFactory`, `DeriveExecutionClientFactory`
- `DeriveEnvironment`
- `DERIVE`, `DERIVE_CLIENT_ID`, and `DERIVE_VENUE`

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/derive/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/derive/examples/)

## Derive documentation

Derive publishes API documentation at [docs.derive.xyz](https://docs.derive.xyz). Refer to it
alongside this guide for additional details.

### V3 deployment and migration

Derive's [migration completion announcement](https://x.com/DeriveXYZ/status/2107605164131852434)
announces completion of the v3 migration on October 6, 2026. The
[migration proposal](https://forums.derive.xyz/t/dip-launch-derive-v3/322) describes winding
down Derive Chain afterward. These sources do not specify an exact v2 API retirement date
or establish that v2 remains available as a fallback.

Verify the migrated owner, funded subaccount, and session-key permissions before starting execution.
V3 uses the ultimate owner EOA or Ethereum multisig as `wallet_address`, replacing the intermediate
Derive Wallet. The migration proposal splits standard-margin subaccounts by risk universe, so
confirm the v3 `subaccount_id` instead of assuming the v2 ID selects the intended balances and
positions. Portfolio-margin subaccounts migrate one to one.

Migrated v2 admin keys receive trading, same-owner transfer, withdrawal, and liquidation scopes,
with expiry capped to 28 days after genesis, the v3 launch state. They do not receive `admin`,
`set_session_key`, or different-owner transfer permissions. V2 read-only and account keys migrate
without protocol scopes and cannot trade.
Check the key's current expiry, scopes, and permitted subaccounts through `private/session_keys`.
The owner must provision a scoped v3 key when the migrated key does not meet execution requirements:
an EOA calls `private/set_session_key`, and a contract owner submits the on-chain Set Session Key
action. See [Testnet onboarding](#testnet-onboarding) and [Mainnet onboarding](#mainnet-onboarding).

## Products

| Product type           | Supported | Notes                                                                                    |
| ---------------------- | --------- | ---------------------------------------------------------------------------------------- |
| ERC-20 spot            | ✓         | USDC-quoted `CurrencyPair`; see [Spot behavior](#spot-behavior) for live v3 limitations. |
| Perpetual swaps        | ✓         | Cash-settled in USDC, with per-currency listings such as `ETH-PERP`.                     |
| Options (calls / puts) | ✓         | European-style options using `{CURRENCY}-{EXPIRY}-{STRIKE}-{C\|P}`.                      |

## Symbology

Derive instruments use the native venue symbol with the venue suffix `.DERIVE`:

- Spot: `ETH-USDC.DERIVE` (base currency, quote currency).
- Perpetual: `ETH-PERP.DERIVE`, `BTC-PERP.DERIVE`.
- Option: `ETH-20260626-3000-C.DERIVE` (currency, expiry, strike, kind).

The first hyphen-separated segment of the symbol is the underlying currency. The provider
fetches every page of `public/get_all_instruments` for each product type and configured currency.
Subscribing to a new currency triggers a lazy REST fetch when `auto_load_missing_instruments`
is enabled (the default).

With `include_expired`, the provider merges live and expired option listings by instrument name.
It keeps the live definition when both listings contain the same option near expiry. Perpetual
and spot listings ignore this setting.

The adapter routes on the venue `instrument_type` (`perp`, `option`, `erc20`), not on the symbol
suffix, so spot pairs need no special symbology parsing. Spot reuses the same Trade-module
signing path as perps and options; the in-repo fixtures under
`crates/adapters/derive/test_data/spot/` capture the spot instrument, order book, ticker, and
trade field shapes the parser and execution paths are pinned to.

### Spot behavior

:::warning
Before the v3 migration, testnet accepted and canceled passive `ETH-USDC` limit orders, and mainnet
place/cancel was exercised manually. Live v3 spot trading, including place/cancel, trade frames,
tracked fill commissions, and restart reconciliation, remains unverified. The current `ETH-USDC`
minimum amount is `0.1 ETH`. An empty spot book does not provide a bounded way to close a
minimum-size fill.
:::

- **Sparse trades:** public spot trade channels (`trades.erc20.ETH`, `trades.ETH-USDC`) subscribe
  successfully but can be low-volume, so expect sparse trade frames.
- **Empty books:** the venue still broadcasts ticker and order book frames with a zeroed top of
  book. The adapter drops those partial quotes (logged at DEBUG), so the quote feed stays silent
  until a book forms. Trade frames are match-driven and independent of book state, so a trade
  that empties the book still emits an event.
- **Standard Margin:** spot orders are margined by initial margin fraction, not full notional.

## Environments

Configure the environment with the `DeriveEnvironment` enum on either client config.

| Environment | Config                       | REST                                | WebSocket                            |
| ----------- | ---------------------------- | ----------------------------------- | ------------------------------------ |
| Mainnet     | `DeriveEnvironment::Mainnet` | `https://api.derive.xyz/v3`         | `wss://api.derive.xyz/v3/ws`         |
| Testnet     | `DeriveEnvironment::Testnet` | `https://testnet.api.derive.xyz/v3` | `wss://testnet.api.derive.xyz/v3/ws` |

:::important
Testnet uses Ethereum Sepolia with its own session keys and balances. **Mainnet and testnet API
keys are not interchangeable.**
:::

Public market data (book, ticker, trades) does not require credentials.

When upgrading from v2, replace the old Derive Wallet address in `wallet_address` or the wallet
environment variable with the owner's EOA or multisig address. See
[V3 deployment and migration](#v3-deployment-and-migration).

The EIP-712 domain separators, Action typehash, and Trade module addresses for both networks
are shipped in `crates/adapters/derive/src/common/consts.rs` and tracked against Derive's
[v3 action-signing reference](https://docs.derive.xyz/authentication/action-signing).
Mainnet uses Ethereum chain 1, and testnet uses Sepolia chain 11155111. Both use the same Trade
module address. The owner is the EOA or multisig; the session key supplies the signer address.
`DeriveExecutionClientConfig::domain_separator`, `action_typehash`, and `trade_module_address`
accept per-instance overrides that take precedence over the shipped values.

Order, trigger-order, and replace requests sign the seven-word Trade ABI with 1e18 fixed-point scaling.
Financial inputs must fit 12 fractional digits, including trailing zeros; excess precision raises
an error instead of truncating the signed value. The request sends the same decimal values used to
build the signature.

Action nonces use UNIX nanoseconds and travel as decimal strings in requests and responses.
The allocator shares a monotonic stream per wallet and subaccount across client instances in the
same process, regardless of wallet lettercase. Clock rollback advances the last nonce only while
it remains within one hour of local time. The adapter applies the inclusive order window
[local time minus 90 days, local time plus 1 hour], a conservative lookback within the venue's
documented 120-day window. Local time
approximates the server clock, so clock skew can still cause venue rejection. Login timestamps use
milliseconds, and signature expiry uses seconds.

### Switch an existing deployment to v3

1. Stop submitting orders and reconcile existing orders, fills, balances, and positions.
1. Verify the v3 owner address, funded subaccount, session-key expiry and scopes, and environment.
1. Connect with the v3 defaults and confirm public market data and authenticated account reads.
1. Validate minimum-size execution with explicit price, fee, and exposure bounds on testnet and
   mainnet before enabling the strategy.

If account reads or signing fail, stop execution and check the owner, subaccount, key permissions,
and signing constants against the selected environment. Reconcile venue state after reconnecting
before resuming orders. Changing only the URL to v2 is not a recovery procedure: this adapter uses
v3 request schemas, nanosecond nonces, and v3 signatures. It does not support the v2 API.

V2 trigger and TWAP orders did not carry over to v3. Check venue state before recreating intended
trigger orders. This adapter does not submit TWAP orders.

## Testnet onboarding

Steps to reach a position where the execution client can submit a signed order:

1. **Sign in to the testnet dashboard.** Open
   [testnet.app.derive.xyz](https://testnet.app.derive.xyz/) and connect the owner EOA or multisig.
1. **Copy the owner wallet address.** Use your EOA or multisig address as `wallet_address`
   so the client sends it in the `X-DeriveWallet` header. V3 has no separate Derive Wallet address.
1. **Fund a subaccount.** Follow Derive's [deposit guide](https://docs.derive.xyz/getting-started/depositing)
   to mint test collateral and deposit on Sepolia. The first deposit creates the account and
   subaccounts. Copy the funded subaccount's integer ID as `subaccount_id`; orders require enough
   collateral to cover their margin.
1. **Generate a session key.** Follow Derive's [session-key guide](https://docs.derive.xyz/authentication/session-keys)
   to register a key with protocol scope `trade:orderbook:all`, or narrower product scopes,
   restricted to the funded subaccount. The separate off-chain `account_info` capability does
   not grant trading. Choose an expiry and verify the record through `private/session_keys`.
   Copy the raw secp256k1 private key as `session_key`; it stays local and is redacted from `Debug` output.
   To retire a key, re-register it with a past or zero expiry. Revocation has a minimum cooldown
   of [5, 15] minutes; it does not take effect immediately.
1. **Set the environment variables.** Export the three values the client reads in testnet
   mode (or pass them on `DeriveExecutionClientConfig`, where the config field wins):

   ```bash
   export DERIVE_TESTNET_WALLET_ADDRESS="0x..."  # owner EOA or multisig
   export DERIVE_TESTNET_SESSION_PRIVATE_KEY="0x..."  # secp256k1 session-key private key
   export DERIVE_TESTNET_SUBACCOUNT_ID="12345"  # integer subaccount id
   ```

EOA owners register keys through `private/set_session_key`. A multisig or contract owner must
first fund the account, then submit the Set Session Key action from the owner contract on Sepolia
or Ethereum. An on-chain registration is ignored when the account does not exist. Follow the
venue's [multisig guide](https://docs.derive.xyz/authentication/contract-owned-accounts).

### Minimum funding

Funding must cover the venue's initial-margin requirement and pending-order reservations.
Order acceptance also depends on the venue's price, quantity, and execution rules.
Use the venue's margin figures to size funding for the smallest viable test:

- **Smoke test (submit and cancel, no fills):** fund enough to cover the margin required
  for a passive order at the instrument's `minimum_amount`.
- **Options:** size the deposit to cover the venue's margin requirement for the passive order.
  `public/get_instrument` provides contract details but does not report initial-margin requirements.

For a fill round-trip, cover the venue's margin requirement, fees, and a bounded price move.
Use current executable quotes, the smallest viable quantity, `max_fee_per_contract`, and a limit
price or bounded `market_order_slippage_bps`. Close observed perpetual or option exposure with a
reduce-only market or `IOC`/`FOK` order; close spot exposure with an opposite-side order.
Confirm no open orders or positions remain.

For order-size limits and taker exceptions, see [Instrument loading](#instrument-loading).

### Check account health

Use `private/get_subaccount` after funding to confirm `initial_margin` stays positive once the
intended exposure's initial margin is applied. Both health fields include collateral credit
and position mark-to-market value after the venue's risk charges:

| Venue field          | Meaning                              | Consequence when negative                                                 |
| -------------------- | ------------------------------------ | ------------------------------------------------------------------------- |
| `initial_margin`     | Signed net initial-margin health     | Venue rejects risk-increasing orders that would make this value negative. |
| `maintenance_margin` | Signed net maintenance-margin health | Subaccount is exposed to liquidation.                                     |

The adapter's `query_account` command emits this snapshot as an `AccountState` event so the
strategy layer can inspect current health. These net-health values exclude pending open-order
reservations; a positive value alone does not prove that another order can be accepted.
The venue's `private/order_quote` returns pre/post initial-margin health for a proposed order;
actual order acceptance remains authoritative.
See [Account state](#account-state) for the mapping.

## Mainnet onboarding

Mainnet onboarding mirrors testnet against the production dashboard. Use real funds.

1. **Sign in to the mainnet dashboard.** Open [app.derive.xyz](https://app.derive.xyz/) and connect
   the owner's EOA or multisig wallet.
1. **Copy the owner wallet address.** Use the EOA or multisig address as `wallet_address`;
   the client uses it for authentication and as the signed action's owner.
1. **Fund or pick a subaccount.** Follow Derive's [deposit guide](https://docs.derive.xyz/getting-started/depositing)
   to deposit USDC or supported collateral on Ethereum. A first deposit creates the account
   and subaccounts; copy the funded subaccount's integer ID as `subaccount_id`.
   Confirm via `private/get_subaccount` that the deposit appears in the collateral balances;
   the adapter's `query_account` exposes modeled rows in `AccountState.info.collaterals`.
   Check `net_initial_margin` and `net_maintenance_margin` in the emitted metadata as described
   in [Check account health](#check-account-health). Positive net health alone does not prove
   that an order can be accepted.
1. **Generate a mainnet session key.** Follow Derive's [session-key guide](https://docs.derive.xyz/authentication/session-keys)
   to register a key with protocol scope `trade:orderbook:all`, or narrower product scopes,
   restricted to the subaccount. Verify its scopes and expiry through `private/session_keys`.
   Contract owners use the on-chain registration described in [Testnet onboarding](#testnet-onboarding),
   on Ethereum for mainnet. Copy the raw secp256k1 private key. Prefer short-lived keys
   for exploratory tester runs. To retire a key, re-register it with a past or zero expiry.
   Revocation has a minimum cooldown of [5, 15] minutes; it does not take effect immediately.
1. **Set the environment variables.** Export the three mainnet values (or pass them on
   `DeriveExecutionClientConfig`, where the config field wins):

   ```bash
   export DERIVE_WALLET_ADDRESS="0x..."  # owner EOA or multisig
   export DERIVE_SESSION_PRIVATE_KEY="0x..."  # secp256k1 session-key private key
   export DERIVE_SUBACCOUNT_ID="12345"  # integer subaccount id
   ```

### Select the example network

:::warning
Each Rust example (`node_data_tester`, `node_exec_tester`, `node_delta_neutral`) pins the network
with a `const DERIVE_ENVIRONMENT: DeriveEnvironment` literal near the top of the file.
**Check that constant before every run** and edit it to switch networks; the examples do not
read the network from the environment.
:::

Production deployments select the network via `DeriveDataClientConfig::environment` /
`DeriveExecutionClientConfig::environment`.

## Referral code attribution

Every signed order, replace, and trigger order carries the hard-coded NautilusTrader referral
code. Derive funds the referral program from its own revenue, so attribution adds no trading cost,
needs no approval, and is not configurable. This helps us gauge real usage of the integration
and prioritize ongoing maintenance.

## Capabilities

### Market data

| Capability                     | Supported | Notes                                                                   |
| ------------------------------ | --------- | ----------------------------------------------------------------------- |
| Request instrument (REST)      | ✓         | `public/get_instrument`; loads one instrument into the local cache.     |
| Request all instruments (REST) | ✓         | Paginated `public/get_all_instruments` for each currency.               |
| Instrument subscription        | -         | *Not supported.* Use the configured REST refresh interval.              |
| Order book deltas (L2_MBP)     | ✓         | Channel: `orderbook.{instrument}.{group}.{depth}`.                      |
| Order book depth (L2_MBP)      | ✓         | Same order book channel with `depth=10`.                                |
| Order book at interval         | -         | *Not supported.* Maintain interval books from deltas locally.           |
| Order book snapshot (REST)     | -         | *Not supported.* The venue has no book snapshot endpoint.               |
| Historical book deltas (REST)  | -         | *Not supported.* The venue has no historical book endpoint.             |
| Quotes (`ticker_slim`)         | ✓         | Channel: `ticker_slim.{instrument}.{interval}`.                         |
| Quote snapshot (REST)          | ✓         | One-shot `public/get_tickers`; emits a single `QuoteTick`.              |
| Historical quotes (REST)       | -         | *Not supported.* The venue exposes ticker snapshots only.               |
| Trades                         | ✓         | Channel: `trades.{instrument_type}.{currency}`.                         |
| Historical trades (REST)       | ✓         | Chronological and deduplicated; `limit` retains the newest trades.      |
| Bars / OHLC (REST)             | ✓         | Closed minute, hour, day, and week bars stamped at bucket close.        |
| Bars / OHLC (WS)               | -         | *Not supported.* The venue has no candle subscription channel.          |
| Mark price stream              | ✓         | Derived from `ticker_slim`; shares the quote subscription.              |
| Index price stream             | ✓         | Derived from `ticker_slim`; shares the quote subscription.              |
| Funding rate stream            | ✓         | Derived from the funding rate field on perp tickers.                    |
| Funding rate history (REST)    | ✓         | Chronological for perpetuals; `limit` retains the newest valid rows.    |
| Instrument status              | -         | *Not supported.* The instrument definition carries `is_active`.         |
| Instrument close               | -         | *Not supported.* The venue publishes option settlement over REST only.  |
| Option greeks                  | ✓         | Derived from `option_pricing` on option tickers.                        |
| Option chain                   | ✓         | Aggregated from quotes and greeks; `public/get_tickers` bootstraps ATM. |

#### Instrument loading

`request_instrument` calls `public/get_instrument` for the requested `InstrumentId` and
caches the returned definition before emitting the response. The cached instrument carries
the precision and increment fields used by later quote, trade, book, and bar parsing.

The quantity fields map as follows for perpetuals, options, and spot:

| Derive field     | Nautilus field           | Handling                                                |
| ---------------- | ------------------------ | ------------------------------------------------------- |
| `minimum_amount` | `info["minimum_amount"]` | Retained as venue metadata; `min_quantity` stays unset. |
| `amount_step`    | `size_increment`         | Order quantity increment.                               |
| `maximum_amount` | `max_quantity`           | Maximum order quantity.                                 |

:::important
Derive can fill taker orders below `minimum_amount`, while orders that would rest on the book
can be rejected with `11012: Invalid amount`. **Derive decides whether a sub-minimum order is valid.**
:::

Instrument loading treats venue error `12001` as an empty result for the affected product type,
so a currency without a perp, option, or spot listing does not block its other products. Rows that
fail to parse into domain instruments are logged and skipped while valid rows continue to load.
Malformed response fields fail that currency's fetch. A fetch error during initial loading
also fails the connection.

#### Historical data

| Data          | REST endpoint                       |
| ------------- | ----------------------------------- |
| Trades        | `public/get_trade_history`          |
| Bars          | `public/get_tradingview_chart_data` |
| Funding rates | `public/get_funding_rate_history`   |

##### Trade direction and deduplication

Public REST history and WebSocket trades can return maker and taker rows under the same
`trade_id`. Each row's `direction` is that participant's side. The adapter uses the taker's
side as the aggressor direction and inverts a maker row's direction, independent of row order.
Live v3 maker WebSocket row mapping remains unverified.
For rows with an absent or unrecognized `liquidity_role`, the adapter treats `direction` as
the taker's side.

Historical trade requests emit one `TradeTick` per trade ID within the request. The live feed
suppresses paired rows and reconnect replays while their IDs remain in a cache of the latest
4,096 trades. Automatic reconnect retains this cache; explicit disconnect or reset clears it.

##### Bar requirements and time bounds

- **Aggregation and price:** bars require `EXTERNAL` aggregation and `PriceType::Last`, since
  Derive candles are trade-based.
- **Periods:** the venue supports 1, 5, 15, and 30 minute, 1, 4, and 8 hour, 1 day, and 1 week
  steps. Any other bar specification is rejected before the request goes out.
- **Time bounds:** the bar `end` bound selects buckets by their start time at the venue. Responses
  omit any bucket whose close is after the request time, including the still-forming bucket
  returned by the venue.

#### Order book feeds

Derive exposes book deltas and depth snapshots through the same
`orderbook.{instrument}.{group}.{depth}` channel family. `subscribe_book_deltas` publishes
snapshot deltas as `OrderBookDeltas`, while `subscribe_book_depth` fixes `depth=10` and
publishes `OrderBookDepth` snapshots.

### Execution

Derive uses the configured session key for authenticated execution:

- Order submission and replacement requests carry locally generated EIP-712 signatures.
- The live execution client sends order writes over the authenticated WebSocket and receives
  account, order, trade, and balance updates through private channels.
- Report generation, account refreshes, and instrument lookups use REST.

#### Fills and settlement

The adapter ingests live and historical fills using `trade_id` and `order_id` as execution
identities. Quantity, price, and fee decode as `Decimal`; commission rounds to USDC's Nautilus
precision. Order status and private fill reports use the optional order label as `client_order_id`
when it is a valid Nautilus identifier. The adapter omits unrepresentable labels with a warning and retains venue identities
and financial values; reconciliation can resolve these fills by venue order ID.
Null, empty, or missing required financial values fail decoding. Private trade arrays skip
malformed rows with a warning. Invalid financial wire values reject a public WebSocket frame or
public REST page. Unrepresentable required trade or order IDs fail per-row parsing; the adapter
warns and continues with the remaining rows.

Settlement metadata does not gate a fill. `batch_status` and the Ethereum L1 batch `tx_hash`
can be null when execution first arrives. The adapter also accepts absent `op_uuid` metadata,
although the private-trade schema requires it. Published batch stages and their matching
`Error` states describe settlement progress or failure of a batch stage. The adapter does not
reverse or void a fill when a batch stage reports an error.
See the venue's [API changelog](https://docs.derive.xyz/changelog).

Private self-trade ID uniqueness and whether `trade_fee` includes a nonzero external order
`extra_fee` remain unverified. Private fills deduplicate by `trade_id`, so complete self-trade
accounting depends on the venue assigning distinct IDs to both legs. The adapter does not submit
`extra_fee`.

Trade channels have no settlement-status filter: the adapter subscribes to
`trades.{instrument_type}.{currency}` and `{subaccount_id}.trades`. Fill-report queries skip IDs
already in the live emitted-trade cache and deduplicate repeated IDs within a page and across
pages of the same query. Querying a trade does not record it as emitted. Core reconciliation uses
persisted order trade IDs to reject repeated fill application after replay or restart.

Live dispatch constructs the fill report and commission before changing order state. A commission
conversion failure leaves the trade unprocessed so a replay can retry it. The adapter records the
trade ID as emitted only after delivery succeeds; failed delivery retains the native trade record.
For a tracked order, a native fill can establish its first venue binding before an acknowledgement
arrives. The adapter emits `OrderAccepted` before `OrderFilled`. A fill whose side conflicts with
the tracked order remains deferred, including after that order reaches a terminal state.

Private trade updates can arrive before REST history or position reads reflect them.
Settlement progress does not trigger extra history polling in the adapter.

:::note
`DeriveHttpClient` also exposes HTTP order-entry methods for tooling and tests.
:::

Perpetuals, options, and ERC-20 spot pairs all use the Derive Trade module. Spot has no
separate signing path, and reconciliation treats spot instruments like other instrument
classes except for the reduce-only guard described below.

The adapter supports ordinary `private/order` requests: `LIMIT` and `MARKET` orders with
`GTC`, `IOC`, or `FOK` time-in-force values. It also supports Derive trigger orders for the
Nautilus-native stop and if-touched order types listed below. Unsupported Nautilus order
types are denied locally with `OrderDenied` before submission, so they cannot fill at the venue.

:::important
**Market orders require a cached quote before submission.** Without one, the adapter emits
`OrderDenied` and never signs.
:::

After the async submit task resolves the instrument, it refreshes the current ticker snapshot
and derives the signed slippage-bound `limit_price` from that refreshed quote.

#### Replacing regular orders

Derive implements regular order modification as cancel-replace. The adapter keeps one client order
identity across the native venue order IDs and emits `OrderUpdated` before routing child fills.
Commands use the current venue binding while delayed fills retain their original venue order IDs.

Replacement quantity is the requested gross quantity less all proven prior fills. For example,
replacing an order that has filled `0.3` with a gross target of `1.5` signs a child amount of `1.2`.
The request also sets `expected_filled_amount` to the observed current-leg fill ceiling. Derive's
[replace guard](https://docs.derive.xyz/trading/order-types#replace-atomic-cancel-+-replace) rejects
the replace when the target's filled amount moves past that value. Logical
order reports include cumulative fills, a quantity-weighted average when all required native
averages are available, and the earliest native creation timestamp as the acceptance time.

#### Startup reconciliation

Before private streaming starts, the execution client publishes account state, loads instrument
definitions required by the native portfolio and cached active orders, and waits for the engine
cache to observe them. It restores owned active order context from cached order events, including
known venue IDs and processed trade IDs, then verifies active orders against native open orders
and history for their instruments. Historical metadata gaps do not prevent this binding check.
After private subscription confirmation, it refreshes account state and required
instrument definitions again before processing stream updates. This closes the interval between
the initial portfolio snapshot and subscription confirmation.

When `private/get_order` returns `11006` for a known venue ID, the adapter searches paginated
`private/get_order_history` for that exact ID. It does not infer an unproved replacement binding
from an order label. An unresolved successor blocks startup or report generation. It also rejects
individual cancel and modify commands and returns an error for batch cancel and `cancel_all_orders`
before transmission. Ambiguous state-changing requests remain unreplayed.

While the process retains a replacement's signing nonce and target, a native REST row can resolve
it using the same parent-ID and nonce checks as a WebSocket row. Replacement history reads start
one hour before the retained nonce timestamp, allowing for the venue's accepted future nonce window.
An absent successor does not prove that the replacement failed. After a process restart, cached
`PendingUpdate` events do not retain enough evidence to resolve an ambiguous replacement automatically.

If an unresolved replacement prevents startup:

1. Stop the trading node and preserve its cache and order event history.
1. Inspect the subaccount's native orders, trades, and positions through the venue's account tools.
   Match owned orders by their native IDs and replacement ancestry; a matching label alone does
   not establish ownership of a successor.
1. Resolve the outstanding venue orders and reconcile the cached order state against confirmed
   native records before resuming trading. The adapter provides no automatic cache repair for
   this case; clearing `PendingUpdate` alone does not establish the replacement's outcome.

Mass status retains native order reports for historical replacement legs and processes them before
its current cumulative order report. If a reported fill belongs to a known leg missing from the
history response, the adapter fetches its companion order by exact venue ID. Open-only requests
still return active orders, and a targeted request for a known replaced leg returns the current
logical order.

Unbounded reconnect snapshots fail if a source fails, records are lost, or a fill has no matching
order report. Retained historical rows with inferred precision do not fail these snapshots. Bounded
snapshots retain valid reports and mark metadata gaps or lost records as incomplete coverage. If the
venue changes a private history's record or page count during pagination, the adapter discards the scan and restarts it
at most twice. Further count changes fail the request.

#### Margin model selection

Set `DeriveExecutionClientConfig.subaccount_id` to an existing Standard Margin (`SM`) or Portfolio
Margin (`PM2`) subaccount. Each subaccount is bound to the manager chosen at creation and its
risk universe; the adapter reads those identities from `private/get_subaccount`. The same account
query, order, position-report, and reconnect paths serve both models. Configure data-client
currencies for the products supported by that manager.

Create or fund subaccounts through the venue's account tools before connecting. The adapter does
not change their margin model or reproduce the venue's portfolio risk engine. See Derive's
[managers and risk universes](https://docs.derive.xyz/trading/managers-and-risk-universes)
for supported collateral and instruments.

The default native risk engine checks per-instrument initial margin against free balance in the
required currency. It does not use Derive's PM2 net-health metadata for order admission or portfolio
sizing. Positive venue PM2 health therefore does not establish native admission with collateral in
another currency. Live behavior with cross-currency collateral and nonzero maintenance-margin
credits (`mm_credits`) remains unverified.

#### Account state

Derive holds margin at the subaccount level. `AccountState` separates collateral balances,
margin requirements, and net account health.

##### Collateral balances

- `total` is the collateral `amount` (for example USDC or ETH), rounded to the registered currency precision.
- `locked` is zero; the adapter keeps pending vault-deposit holds in metadata.
- `free` equals the rounded `total`.
- `AccountState.info.collaterals` preserves a subset of each collateral row, with exact decimal strings.
- `collaterals[].initial_margin` is USD credit contributed by that collateral, not locked funds.

##### Margin requirements

The adapter emits one account-wide `MarginBalance`:

| Nautilus field | Derived USD requirement                                           |
| -------------- | ----------------------------------------------------------------- |
| `initial`      | `positions_value - positions_initial_margin - open_orders_margin` |
| `maintenance`  | `positions_value - positions_maintenance_margin`                  |

The source `positions_*_margin` fields are signed health contributions that include position
mark-to-market value. Subtracting them from `positions_value` extracts the corresponding risk
requirement. `open_orders_margin` is a signed deduction for pending order reservations, so it
increases the extracted initial requirement. The adapter uses subaccount aggregates, which
retain venue portfolio offsets, and emits no per-instrument margin balances.

The emitted requirements round to native USD precision (two decimal places, with ties rounded
to even). The subaccount's `currency` array does not determine their denomination. Modeled
source values remain in `AccountState.info`; the adapter does not calculate collateral haircuts
or apply another adjustment for `mm_credits`.

:::warning
`net_initial_margin` and `net_maintenance_margin` in `AccountState.info` report current net health,
including position mark-to-market value. They exclude pending open-order reservations. Use the
venue's order quote for pre/post initial-margin health; actual order acceptance determines
whether an additional order can execute.
:::

##### Account metadata

`AccountState.info` carries account identities, signed net health, and exact risk values:

- `net_initial_margin` and `net_maintenance_margin` carry net health.
- `positions_value` carries the aggregate position mark-to-market value.
- `positions_initial_margin` and `positions_maintenance_margin` carry signed position-health contributions.
- `open_orders_margin` carries the signed initial-health deduction for pending order reservations.
- `margin_type` and `manager_id` identify the margin model; `risk_universe_id` identifies its risk universe.
- `currency` preserves the venue's currency array.
- `collaterals` preserves a subset of each collateral row, with exact decimal strings.
- `mm_credits` carries maintenance-margin credits from risk-universe loss socialization.
- `projected_margin_change` preserves the projected change in `net_maintenance_margin` one minute
  after the next option expiry.
- `vault_deposit_holds` preserves pending reservations, including native amounts and target vault IDs.
- `is_under_liquidation` carries liquidation status.

Modeled decimals are JSON strings, exact within `Decimal` range and its maximum scale of 28;
source values beyond that scale round during decoding. Native balances can also round amounts
smaller than their currency's precision to zero; use the collateral metadata when that precision
matters. The venue does not specify whether pending vault holds are included in collateral
amounts, so the adapter neither subtracts them from nor adds them to balances.

A snapshot marked `failed_to_fetch` or missing required fields emits no `AccountState`, fails
initial connection, and skips post-reconnect mass status. Invalid nested rows, including
positions outside native `Quantity` precision or range, fail the whole account snapshot.
Position-report requests additionally require amounts to fit the cached instrument's size precision.
A position conversion failure rejects the whole mass status, including its order and fill reports,
preserving cached positions instead of treating a missing position report as flat.

Position reports preserve the venue's average entry price. Restart reconciliation can fail when
that price differs from the average reconstructed from fills; the adapter retains the position
report rather than omitting it to avoid the discrepancy.

#### Conditional orders

Derive trigger orders use `private/order` with trigger fields. The venue assigns the order id
and stores them with `order_status=untriggered` until its
trigger worker submits the signed child order. Reconciliation therefore reads both
`private/get_open_orders` and `private/get_trigger_orders`.

Live v3 trigger firing and missed-update recovery with delayed history or position views remain
unverified. A successful reconnect or restart does not establish those cases.

##### Signature expiry

API v3 accepts order signatures with lifetimes in [10 seconds, 120 days]; MMP orders have a
15-minute maximum. The adapter leaves MMP disabled and signs trigger orders with a fixed 31-day
expiry. `signature_expiry_secs` controls ordinary orders and replacements, and must exceed
10 seconds to allow signing latency, with a maximum of 120 days.
See [action signing](https://docs.derive.xyz/authentication/action-signing).

##### Supported types

| Nautilus order type | Supported | Derive `order_type` | Derive `trigger_type` | Notes                          |
| ------------------- | --------- | ------------------- | --------------------- | ------------------------------ |
| `StopMarket`        | ✓         | `market`            | `stoploss`            | Uses trigger price as bound.   |
| `StopLimit`         | ✓         | `limit`             | `stoploss`            | Sends limit and trigger price. |
| `MarketIfTouched`   | ✓         | `market`            | `takeprofit`          | Uses trigger price as bound.   |
| `LimitIfTouched`    | ✓         | `limit`             | `takeprofit`          | Sends limit and trigger price. |
| `MarketToLimit`     | -         | -                   | -                     | *Not supported by Derive*.     |
| Trailing stops      | -         | -                   | -                     | *Not supported by Derive*.     |
| TWAP / algo / RFQ   | -         | -                   | -                     | *Not exposed by this adapter*. |

The adapter maps Nautilus `TriggerType::Default` and `TriggerType::MarkPrice` to Derive
`trigger_price_type=mark`. Derive's current error-code reference states that index and
last-trade trigger price types are not supported yet, so `IndexPrice`, `LastPrice`, `BidAsk`,
and other trigger price types are denied locally with `OrderDenied` before submission.

##### Updating a trigger

Derive error `11054` states that trigger orders cannot replace or be replaced. The adapter
therefore rejects Nautilus modify requests for trigger orders with an `OrderModifyRejected`
event; cancel and resubmit for trigger updates.

##### Trigger price offsets

Derive validates the trigger price side and rejects a trigger that does not sit beyond the
current price in the expected direction with error `11051`. The trigger price is fixed when the
order is signed, so a tight offset on a fast-moving or high-priced instrument can drift onto the
wrong side before the venue receives the order.

:::warning
Size the trigger offset to comfortably exceed expected price movement during submission
(for `ETH-PERP`, tens of dollars rather than a few cents). A too-tight offset produces
spurious `11051` rejections.
:::

#### Bulk cancellation

##### Selection and routing

Derive supports both multi-order cancellation methods exposed by `Strategy`.

| Strategy method          | Supported | Parameters                                                            | Notes                                      |
| ------------------------ | --------- | --------------------------------------------------------------------- | ------------------------------------------ |
| `cancel_orders(...)`     | ✓         | `client_order_ids`, `client_id`, `params`                             | All orders must use the same instrument.   |
| `cancel_all_orders(...)` | ✓         | `instrument_id`, `order_side`, `client_id`, `strategy_only`, `params` | Defaults to the calling strategy's orders. |

The Derive execution client applies these methods as follows:

- **Explicit orders:** `cancel_orders` cancels each requested regular or trigger order individually.
- **Strategy-only:** `cancel_all_orders` with `strategy_only=True` expands cached matches into
  individual cancels.
- **Side-filtered:** `cancel_all_orders` with `strategy_only=False` and a Buy or Sell filter selects
  open regular and trigger orders from the cache for the configured execution client, account,
  exact instrument, and side, then cancels each match. It never widens to both sides.
- **Both sides:** `cancel_all_orders` with `strategy_only=False` and no side filter selects matching
  open triggers from the same execution client, account, and instrument scope and cancels them
  individually, then sends `private/cancel_by_instrument` for regular orders. It never sends
  `private/cancel_all`.

`cancel_all_orders` selects eligible orders from the cache without refreshing venue state.
For each selected order, the execution client's current venue binding takes precedence over the
cached ID. Pending triggers use `private/cancel_trigger_order`. Individual cancellation of an
activated trigger uses `private/cancel`; both-sides mode leaves already activated triggers to
`private/cancel_by_instrument`. The client checks activation again after pacing and routes a trigger
that activated during the wait through `private/cancel` before enqueueing the request.

##### Failure handling

If an order selected by `cancel_all_orders` with `strategy_only=False` has neither a current binding
nor a cached venue order ID, the whole command fails closed, logs a warning, sends no cancellation
request, and emits no order event.

`private/cancel_by_instrument` cancels regular open orders only. A successful request with
`cancelled_orders == 0` is an expected no-op and logs at debug level.

A failed trigger cancellation logs a warning but does not suppress the regular instrument
cancellation. A failed bulk request has no per-order outcome to emit; private channel updates and
later reconciliation remain responsible for observed order state.

#### Execution instructions

| Instruction   | Supported | Derive value  | Notes                                                       |
| ------------- | --------- | ------------- | ----------------------------------------------------------- |
| `post_only`   | ✓         | `post_only`   | Requires `GTC`; rejects if the order would take liquidity.  |
| `reduce_only` | ✓         | `reduce_only` | Perps and options, market or `IOC`/`FOK` only; spot denied. |

#### Time in force

Derive documents `gtc`, `post_only`, `fok`, and `ioc` as its `time_in_force` values. Nautilus
values with no Derive equivalent are denied locally with `OrderDenied` before submission. Derive
exposes post-only as a `time_in_force` value, so `post_only` cannot combine with `IOC` or `FOK`.

| Time in force  | Supported | Derive value | Notes                      |
| -------------- | --------- | ------------ | -------------------------- |
| `GTC`          | ✓         | `gtc`        | Good Till Canceled.        |
| `IOC`          | ✓         | `ioc`        | Immediate or Cancel.       |
| `FOK`          | ✓         | `fok`        | Fill or Kill.              |
| `GTD`          | -         | -            | *Not supported by Derive*. |
| `DAY`          | -         | -            | *Not supported by Derive*. |
| `AT_THE_OPEN`  | -         | -            | *Not supported by Derive*. |
| `AT_THE_CLOSE` | -         | -            | *Not supported by Derive*. |

#### Spot reduce-only orders

Derive spot has no position concept, so a reduce-only spot order can never reduce anything.
The venue always rejects it with error `11025`; the adapter avoids that round-trip when it
knows the instrument is spot. Cached spot instruments are denied with `OrderDenied`; lazily
resolved spot instruments are rejected with `OrderRejected` during submit.

Reduce-only orders for perpetuals and options still reach the venue, where the outcome
depends on the subaccount's position state. The `derive-flatten` bin closes derivative
positions only and never spot, since flattening a spot balance would dump the base asset
into a different quote.

##### Reduce-only time-in-force limits

Derive only honors `reduce_only` on market orders or non-resting limits (`IOC`/`FOK`). A
resting `GTC` or post-only limit with `reduce_only` is denied locally before submission.
Direct venue submissions fail with `11024 Reduce only not supported with this time in force`.

:::warning
A Nautilus bracket whose take-profit leg is a reduce-only `GTC` limit cannot rest on Derive:
the entry and stop-loss legs submit, but the adapter denies the take-profit locally. Use a reduce-only
`IOC`/`FOK` close or a non-reduce-only take-profit when targeting Derive.
:::

#### Order rejection semantics

State-changing writes (`submit_order`, `modify_order`, `cancel_order`) are sent once over the
WebSocket and **are not replayed**. The WebSocket request outcome determines whether the
adapter emits a terminal rejection or waits for reconciliation.

##### Definitive failures

The adapter emits a terminal rejection event (`OrderRejected`, `OrderModifyRejected`,
`OrderCancelRejected`) for definitive venue failures:

- Signed-action rejections such as invalid params, insufficient margin, or unknown orders.
- Venue business codes such as `11009 Zero liquidity`.
- Post-only crossing rejections (`11008 Post only order cannot cross the market`), reported
  as `OrderRejected` with `due_post_only=true`.
- Rate-limit responses (`-32000 Rate limit exceeded`), where the gateway rejects the request
  before the matching engine sees it.
- A successful `private/cancel_by_label` response with `cancelled_orders == 0`, which means
  no open order matched the client order ID label.

An unscoped `private/cancel_by_label` response with `cancelled_orders == -1` acknowledges the
request. Positive counts also wait for private order updates; neither response produces an
`OrderCanceled` event without confirmed order state. `query_order` uses the shared order-status
lookup for reconciliation.

##### Post-only classification

For post-only orders that reach the venue, Derive rejects a crossing order with JSON-RPC
`11008` and message `Post only order cannot cross the market`. The adapter marks that
terminal rejection with `due_post_only=true`; if a WebSocket/order-report rejection carries
the same reason, the tracked order path applies the same classification. Local denials
for unsupported post-only IOC/FOK combinations are `OrderDenied` events without
`due_post_only`, because they do not represent a venue crossing rejection.

Strategy-facing rejection reasons strip markup and control characters, normalize whitespace,
and contain at most 256 characters. JSON-RPC reasons retain the native error code and message
without the exception prefix or structured diagnostic payload. Logs retain the original errors.

##### Ambiguous outcomes

For ambiguous write outcomes, the adapter emits no terminal event and lets WebSocket
reconciliation or later status reports settle the state. The ambiguous set is deliberately
narrow:

- `-32603`, a generic JSON-RPC internal error.
- `9000` and `9001`, confirmation timeouts that require querying order state before resubmission.
- A response that cannot be decoded (the action may have been processed).
- A successful response whose venue order ID cannot be represented. The adapter retains the known
  command identity and pending replacement state for reconciliation. If that ID remains unrepresentable,
  status and fill reports cannot track the venue order; inspect venue state using the logged command identity.
- Request timeouts, dropped responses on reconnect, and transport errors.

This distinction protects both sides of the order lifecycle. A false terminal rejection can
make the engine treat a live order as rejected; a false ambiguous outcome can leave an
unplaced order hanging in `Submitted` forever because no WebSocket frame will arrive.

## Rate limiting

### Budget scopes and refill

Derive v3 applies [two tiers of rate limits](https://docs.derive.xyz/rate-limits). Request budgets
use fixed five-second windows. Instrument order budgets refill continuously, with capacity equal
to five seconds of their configured refill rate. An exhausted instrument bucket gains one token
after one second at the default rate; a window boundary does not replenish its entire capacity.

Authenticated HTTP and WebSocket clients on the same API host share wallet budgets across session
keys and subaccounts. Public requests, including WebSocket login, share a separate process-local
budget per API host. This conservatively assumes one source IP; clients behind separate proxies
still share that allowance. Clients in separate processes must coordinate their combined traffic.
Custom URLs with different API hosts use separate limiter state.

Request windows align to the creation of the shared limiter because the venue's window phase is
not observable. Bursts on either side of a local boundary can arrive within one venue window,
especially when transport queues delay departure. The venue rejects excess traffic with
`-32000 Rate limit exceeded`; the adapter surfaces this as a definitive rejection (see
[order rejection semantics](#order-rejection-semantics)).

### Request buckets

The table gives reference defaults and the adapter's label-cancel allowance. Deployment values
and negotiated allowances can differ. Read live request budgets with `public/getRateLimits`;
the endpoint does not report instrument budgets. On testnet, the HTTP endpoint reports the public
IP allowance even with authentication headers, while an authenticated WebSocket reports wallet allowances.

| Bucket                             | Scope                 | Reference allowance             |
| ---------------------------------- | --------------------- | ------------------------------- |
| Matching                           | Wallet                | 5 requests per 5-second window  |
| Non-matching, authenticated        | Wallet                | 25 requests per 5-second window |
| Public requests                    | IP                    | 25 requests per 5-second window |
| `private/cancel_all`               | Wallet and method     | 5 requests per 5-second window  |
| `private/cancel_by_label` endpoint | Wallet and method     | Adapter: 50 requests per window |
| Instrument orders, perpetuals/spot | Wallet and instrument | 1 token/second, capacity 5      |
| Instrument orders, options         | Wallet and instrument | 1 token/second, capacity 5      |

The venue also limits concurrent WebSocket connections per IP. Its published reference is four
connections, but deployment values differ.

The [integrator reference](https://docs.derive.xyz/integrators/trading/rate-limits) publishes 10 TPS
for unscoped label cancellation. The canonical reference does not give a numeric endpoint allowance;
the adapter retains 10 TPS for its label-cancel endpoint budget.

#### Matching write allocation

Order, replace, cancel, cancel-by-nonce, and cancel-by-instrument requests consume the matching
request budget and the supplied instrument's token bucket. The per-instrument configuration
applies to perpetuals, spot, and options and stays independent of the wallet matching allowance.

The adapter paces `cancel_by_label` against both matching and endpoint budgets, following the
canonical rate-limit reference. Raw HTTP requests whose body includes `instrument_name` also
consume instrument credit; the typed label-cancel parameters currently support unscoped calls only.
Testnet currently charges the matching budget for scoped label cancels only. The adapter retains
the stricter documented rule for unscoped calls. `cancel_all` uses its endpoint budget alone.

`cancel_trigger_order` and trigger-order reads use the non-matching budget. Trigger creation
uses `private/order` and reserves matching quota before signing.

The first explicit wallet and instrument settings apply to the shared limiter. A later constructor
with unset options preserves them. The limiter ignores conflicting explicit settings and logs a
warning. Shared budget debt survives reconnects and client recreation.

### Signing and venue responses

HTTP authentication headers and WebSocket login timestamps are signed after pacing waits.
The execution client reserves WebSocket order creation and replacement quota before generating
nonces and EIP-712 signatures. Held reservations occupy instrument capacity until dispatch, and
an abandoned reservation refunds its unused request and instrument credit.

Reserved WebSocket dispatch does not wait again after signing. If a reservation crosses a
request-window boundary, the adapter charges the current request window immediately. If that window is full, it returns a
definitive local throttling error before enqueueing the write. Instrument credit is charged once.

HTTP order methods and direct WebSocket signed-order methods accept already-signed bodies.
Callers must account for pacing delays within those signatures' validity periods.

Venue throttling remains a definitive rejection. Transport failures, timeouts, and ambiguous
write outcomes require reconciliation; the adapter does not blindly retry signed writes.

## WebSocket recovery

The data and execution clients reconnect automatically after a peer close, transport error, or
heartbeat timeout. The adapter sends protocol Ping frames every 30 seconds and treats 60 seconds
without any inbound frame as a dead connection; the selected transport can report a missed Pong
sooner. The transport retries connections with exponential backoff and jitter.

Recovery completes in this order:

1. The transport reconnects. If another disconnect occurs, recovery follows the latest connection.
1. Credentialed sessions log in again, then every session replays its confirmed and pending subscriptions
   in channel order. Pending and acknowledged unsubscriptions are not replayed.
1. The execution client captures current owned order context on the runner thread, refreshes account
   state, then generates mass status for orders, fills, and positions. A rejected account snapshot
   skips mass status; a position conversion failure rejects the whole mass status.

Custom hosts must process the runner's time-event messages to refresh cached order context on
reconnect. The refresh uses the configured `ws_timeout_secs` deadline. Without a time-event sender,
the client retains its initial context and refuses reconciliation after terminal binding eviction.

State-changing requests follow the [order rejection semantics](#order-rejection-semantics): the
adapter sends them once and never replays them. After three failed login or subscription recovery
attempts, the client logs the error, marks itself disconnected, and stops that WebSocket session.

`ws_timeout_secs` applies to individual WebSocket operations and reconnect cache refresh, not
heartbeat detection or reconnect backoff.

## Deterministic simulation testing

The Rust adapter supports `simulation` with `cfg(madsim)` through the shared clock, task, retry,
and transport seams. Configure controlled `http://` and `ws://` endpoints and select
`TransportBackend::Tungstenite`. Fixed synthetic credentials support private local-peer tests.

The audited runtime slices cover public single-instrument lookup (`public/get_instrument`) through
domain construction, public ticker subscriptions through complete quote construction, book and
trade subscription requests, private login and cancel requests, reconnect subscription replay,
command/frame scheduling, and shutdown. The integration tests in
`crates/adapters/derive/tests/integration/dst.rs` pin wire bytes,
login signatures, and domain fields, including the ticker interval. They emit `DST_TRACE` and
`DST_DOMAIN` records for downstream comparison across fresh processes with equal seeds,
configuration, and peer scripts. `cargo-test-sim` runs the in-repository assertions; fresh-process
comparison stays in the downstream DST harness, as specified by the
[adapter DST contract](../concepts/dst.md#adapter-dst-contract).

The static gate covers these production paths under `crates/adapters/derive/src/`:

- `common/parse.rs`, `common/rate_limit.rs`, `config.rs`, `data.rs`, `execution.rs`, and `providers.rs`.
- `http/client.rs`, `http/models.rs`, `http/query.rs`, and `http/parse.rs`.
- `signing/encoding.rs`, `signing/nonce.rs`, `signing/auth.rs`, `signing/eip712.rs`, and
  `signing/modules/trade.rs`.
- `websocket/client.rs`, `websocket/dispatch.rs`, `websocket/handler.rs`, `websocket/messages.rs`,
  and `websocket/parse.rs`.

Constants, enums, errors, endpoint defaults, module declarations, construction-only factories,
retry classifiers, credential and signing-context resolution, and the WebSocket context container
stay outside the static path list. These files introduce no independent async, clock, or randomness
boundary; credential resolution uses supplied configuration or declared environment inputs.
Python and FFI, TLS, live venue state, full data/execution-client lifecycles, signed order
submission and replacement, trigger firing, and other unaudited runtime slices remain outside this
simulation claim. Static coverage does not establish runtime proof for those slices.

## Subscription parameters

`subscribe_book_deltas` and `subscribe_book_depth` accept these `subscribe_params` keys:

| Key     | Type   | Default | Allowed                        |
| ------- | ------ | ------- | ------------------------------ |
| `group` | string | `"1"`   | `"1"`, `"10"`, `"100"`         |
| `depth` | string | `"10"`  | `"1"`, `"10"`, `"20"`, `"100"` |

`subscribe_quotes` accepts:

| Key        | Type   | Default  | Allowed           |
| ---------- | ------ | -------- | ----------------- |
| `interval` | string | `"1000"` | `"100"`, `"1000"` |

Unknown values are rejected at subscribe time.

### Shared ticker subscription

Quotes, mark prices, index prices, funding rates, and option greeks are all derived from the
same `ticker_slim.{instrument}.{interval}` WebSocket subscription. The adapter reference-counts
the underlying WS subscribe call:

- The first feed subscribed for an instrument opens the channel.
- The last unsubscribe closes it.

:::note
**The first subscription's `interval` wins.** Subsequent feeds subscribing with a different
interval share the existing channel.
:::

#### Ticker fields

Mark prices, index prices, funding rates, and option greeks read fields from the ticker payload.
Both the full ticker shape and the compact `ticker_slim` shape carry these fields, so derived
feeds work on either:

| Fields                      | Presence                  | Feed behavior                                                              |
| --------------------------- | ------------------------- | -------------------------------------------------------------------------- |
| `mark_price`, `index_price` | Required                  | A missing field fails deserialization and is logged, not silently dropped. |
| `funding_rate`              | Optional; perpetuals only | Supplies funding-rate events.                                              |
| `option_pricing`            | Optional; options only    | Supplies option greeks.                                                    |
| Bid/ask                     | Present in both shapes    | Quote feed works with either shape.                                        |

#### Instrument class restrictions

Funding rates are only meaningful for perpetuals, and option greeks only for options.
Subscribing the wrong feed for an instrument's class (e.g. funding rates for an option) is
accepted and the WebSocket subscription opens, but the parser returns no events for that feed
because the venue payload omits the funding rate field for non-perps and `option_pricing` for
non-options. Verify the instrument class before subscribing to derivative-specific feeds.

## Configuration

### Data client configuration options

Class/struct: `DeriveDataClientConfig`.

| Option                             | Default   | Description                                                                                 |
| ---------------------------------- | --------- | ------------------------------------------------------------------------------------------- |
| `base_url_rest`                    | `None`    | Override for the REST base URL.                                                             |
| `base_url_ws`                      | `None`    | Override for the WebSocket base URL.                                                        |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports.                                       |
| `environment`                      | `Mainnet` | Network selector (`MAINNET` or `TESTNET` in Python).                                        |
| `http_timeout_secs`                | `10`      | REST request timeout in seconds.                                                            |
| `ws_timeout_secs`                  | `None`    | Per-operation WebSocket timeout (login, subscribe, read, write) in seconds. Unset uses 10s. |
| `update_instruments_interval_mins` | `60`      | Interval in minutes between instrument refreshes.                                           |
| `currencies`                       | `[]`      | Currencies to bulk-load on connect. Empty means lazy-load on demand.                        |
| `include_expired`                  | `false`   | Merge live and expired option listings from `public/get_all_instruments`.                   |
| `auto_load_missing_instruments`    | `true`    | Lazy-load an unknown instrument before sending a subscribe request.                         |
| `transport_backend`                | `Sockudo` | WebSocket transport when `transport-sockudo` is enabled.                                    |

:::important
**`auto_load_missing_instruments` covers subscribe commands only.** Before requesting quotes,
trades, bars, or funding rates, bulk-load the instrument's currency or subscribe first.
:::

- `request_quotes`, `request_trades`, `request_bars`, and `request_funding_rates` fail when the
  instrument is not already cached.
- `request_instrument` is the exception: it always fetches `public/get_instrument`.
- Option-chain subscriptions require a cached option from the series to fetch the initial reference price.

### Execution client configuration options

Class/struct: `DeriveExecutionClientConfig`.

#### Account and credentials

| Option           | Default      | Description                                                      |
| ---------------- | ------------ | ---------------------------------------------------------------- |
| `account_id`     | `DERIVE-001` | Nautilus account identifier.                                     |
| `wallet_address` | `None`       | Owner EOA or multisig address. Falls back to env vars below.     |
| `session_key`    | `None`       | secp256k1 session-key private key. Falls back to env vars below. |
| `subaccount_id`  | `None`       | Derive subaccount id. Falls back to env vars below.              |

#### Connectivity and retries

| Option                   | Default   | Description                                                                                 |
| ------------------------ | --------- | ------------------------------------------------------------------------------------------- |
| `base_url_rest`          | `None`    | Override for the REST base URL.                                                             |
| `base_url_ws`            | `None`    | Override for the WebSocket base URL.                                                        |
| `proxy_url`              | `None`    | Optional proxy URL for HTTP and WebSocket transports.                                       |
| `environment`            | `Mainnet` | Network selector (`MAINNET` or `TESTNET` in Python).                                        |
| `http_timeout_secs`      | `10`      | REST request timeout in seconds.                                                            |
| `ws_timeout_secs`        | `None`    | Per-operation WebSocket timeout (login, subscribe, read, write) in seconds. Unset uses 10s. |
| `transport_backend`      | `Sockudo` | WebSocket transport when `transport-sockudo` is enabled.                                    |
| `max_retries`            | `3`       | Retry attempts for idempotent REST reads. Order writes are sent once and never replayed.    |
| `retry_delay_initial_ms` | `100`     | Initial retry delay in milliseconds.                                                        |
| `retry_delay_max_ms`     | `5,000`   | Maximum backoff delay in milliseconds.                                                      |

The default transport falls back to `Tungstenite` when the build disables the
`transport-sockudo` feature.

For HTTP reads, `Retry-After` sets a minimum delay that can exceed `retry_delay_max_ms`.
If that delay does not fit within the remaining 180-second retry budget, the client returns the
original error without retrying.

#### Order signing and pricing

:::important
`max_fee_per_contract` is required and must be greater than zero. Execution-client construction
fails before creating venue clients when the field is missing or non-positive.
:::

| Option                      | Default  | Description                                                                 |
| --------------------------- | -------- | --------------------------------------------------------------------------- |
| `max_fee_per_contract`      | Required | Positive per-contract USDC fee cap signed into each order.                  |
| `domain_separator`          | `None`   | Optional EIP-712 domain separator override.                                 |
| `action_typehash`           | `None`   | Optional EIP-712 action typehash override.                                  |
| `trade_module_address`      | `None`   | Optional Trade module contract address override.                            |
| `signature_expiry_secs`     | `600`    | Order/replace TTL; must be >10s and at most 120 days. Triggers use 31 days. |
| `market_order_slippage_bps` | `50`     | Slippage bound for market-order limit prices.                               |

#### Matching request limits

Both limits default to **1 request per second** when unset. Set negotiated values independently:
the wallet allowance uses fixed windows, and instrument credit refills continuously. Clients for
the same wallet on the same API host share the first explicit settings. Verify live request budgets
before raising the wallet allowance. Instrument budgets are not discoverable with `public/getRateLimits`.

| Option                                            | Default | Description                                         |
| ------------------------------------------------- | ------- | --------------------------------------------------- |
| `max_matching_requests_per_second`                | `None`  | Wallet matching requests per second.                |
| `max_per_instrument_matching_requests_per_second` | `None`  | Per-wallet/instrument token refill rate per second. |

#### Credential environment variables

The `wallet_address`, `session_key`, and `subaccount_id` fall back to environment variables when
unset:

| Field            | Mainnet variable             | Testnet variable                     |
| ---------------- | ---------------------------- | ------------------------------------ |
| `wallet_address` | `DERIVE_WALLET_ADDRESS`      | `DERIVE_TESTNET_WALLET_ADDRESS`      |
| `session_key`    | `DERIVE_SESSION_PRIVATE_KEY` | `DERIVE_TESTNET_SESSION_PRIVATE_KEY` |
| `subaccount_id`  | `DERIVE_SUBACCOUNT_ID`       | `DERIVE_TESTNET_SUBACCOUNT_ID`       |

The session key is the secp256k1 private key registered on the wallet for API signing. The
`session_key` field is redacted in `Debug` output and Python `repr`.

### Python live node

Python nodes use `LiveNode.builder(...)` and pass concrete factory instances. The node supplies the
trader identifier, while `DeriveExecutionClientConfig` supplies the account identifier.

```python
from decimal import Decimal

from nautilus_trader.adapters.derive import DeriveDataClientConfig
from nautilus_trader.adapters.derive import DeriveDataClientFactory
from nautilus_trader.adapters.derive import DeriveEnvironment
from nautilus_trader.adapters.derive import DeriveExecutionClientConfig
from nautilus_trader.adapters.derive import DeriveExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId

trader_id = TraderId("TESTER-001")

data_config = DeriveDataClientConfig(
    environment=DeriveEnvironment.TESTNET,
    currencies=["ETH", "BTC"],
)

exec_config = DeriveExecutionClientConfig(
    account_id=AccountId("DERIVE-001"),
    environment=DeriveEnvironment.TESTNET,
    max_fee_per_contract=Decimal("1000"),
)

node = (
    LiveNode.builder("DERIVE-001", trader_id, Environment.LIVE)
    .add_data_client(None, DeriveDataClientFactory(), data_config)
    .add_exec_client(None, DeriveExecutionClientFactory(), exec_config)
    .build()
)
```

### Rust data client

```rust
use nautilus_derive::{
    common::enums::DeriveEnvironment,
    config::DeriveDataClientConfig,
};

let config = DeriveDataClientConfig {
    environment: DeriveEnvironment::Testnet,
    currencies: vec!["ETH".to_string(), "BTC".to_string()],
    ..Default::default()
};
```

### Rust execution client

```rust
use nautilus_derive::{
    common::enums::DeriveEnvironment,
    config::DeriveExecutionClientConfig,
};
use rust_decimal::Decimal;

let config = DeriveExecutionClientConfig {
    wallet_address: Some("0x...".to_string()),
    session_key: Some("0x...".into()),
    subaccount_id: Some(1),
    environment: DeriveEnvironment::Testnet,
    max_fee_per_contract: Some(Decimal::from(1000)),
    ..Default::default()
};
```

## Known limitations

- `request_instruments` requires at least one configured currency in
  `DeriveDataClientConfig::currencies`; the adapter filters `public/get_all_instruments`
  by each configured currency and does not enumerate the currency universe.
- The venue does not push instrument status, instrument close, or candle subscriptions; the
  instrument definition carries `is_active` and the scheduled activation/deactivation
  timestamps, and bars are REST-only.
- The book snapshot REST endpoint and historical book deltas / historical quote endpoints
  are not exposed by the venue. See the capabilities table above.
- Derive's official REST docs mark `public/get_ticker` as deprecated in favor of
  `public/get_tickers` as of December 1, 2025. The adapter uses `public/get_tickers`
  for quote snapshots and option-chain reference-price bootstrap.
