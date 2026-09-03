# Accounting

The accounting subsystem tracks balances, margins, and PnL for every account the
platform interacts with. This guide covers the data model, the query API that
strategies use, and the conventions adapter authors must follow to stay
consistent across venues.

It applies equally to backtest and live trading. For backtest-specific
configuration (starting balances, margin-model selection per venue), see
[Backtesting](backtesting/).

## Account types

When you attach a venue to the engine for either live trading or a backtest, you
pick one of three accounting modes via `account_type`: Cash, Margin, or Betting.
A fourth account type, Wallet, models on-chain wallet state. The Blockchain
adapter selects it, and its execution client is still in development.

| Account type | Typical use case                                | What the engine locks                                                     |
| ------------ | ----------------------------------------------- | ------------------------------------------------------------------------- |
| Cash         | Spot trading (e.g., BTC/USDT, stocks)           | Notional for pending buy orders; quantity for pending sell orders.        |
| Margin       | Derivatives or any product that allows leverage | Initial margin for each order plus maintenance margin for open positions. |
| Betting      | Sports betting, bookmaking                      | Stake required by the venue; no leverage.                                 |
| Wallet       | Blockchain wallets (DeFi)                       | Amounts reserved locally for pending orders; no leverage or borrowing.    |

### Cash accounts

Cash accounts settle trades in full; there is no leverage and therefore no
concept of margin. Locked balances reflect the value reserved for pending
orders: the notional value of each pending buy and the quantity each pending
sell would deliver.

### Margin accounts

Margin accounts support instruments that require collateral, such as futures or
leveraged crypto perps. They track account balances, reserve margin for open
orders and positions, and apply a configurable leverage per instrument. Margin
is tracked in two scopes; see [Margin scopes](#margin-scopes) below.

**Terms**:

- **Leverage**: amplifies exposure relative to account equity. Higher leverage
  raises both potential returns and risk.
- **Initial margin**: collateral reserved when an order is submitted.
- **Maintenance margin**: minimum collateral required to keep an open position.
- **Locked balance**: funds reserved as collateral, not available for new orders.

:::note
Reduce-only orders do not contribute to `balance_locked` on cash accounts and
do not add to initial margin on margin accounts, since they can only decrease
exposure. Wallet orders still reserve the input asset because the on-chain
transaction spends that asset even when the order reduces a position.
:::

### Betting accounts

Betting accounts are specialised for venues where you stake an amount to win or
lose a fixed payout (prediction markets, sports books). The engine locks only
the stake required by the venue; leverage and margin do not apply.

### Wallet accounts

Wallet accounts represent blockchain wallets: unleveraged, multi-currency
holdings of native and ERC-20 token balances with no margin and no borrowing.
For reported states, `total` is the observed on-chain balance; `locked` tracks
local pending-order reservations, and `free = total - locked`. Account state
events contribute totals only: the account ignores incoming `locked` and `free`
values, retains its local reservations, and rederives `free`. It rebuilds
transient reservations from submitted and open orders during live startup.
While an amendment is pending, the account reserves the full observed balance
of the debit currency because the pending event does not carry the requested
terms. If the reserved amount exceeds the latest observed total, `locked` is
capped at `total` and `free` remains zero until the balance or reservation
changes.

A balance with a negative total is rejected rather than applied. ERC-20
allowances are spender authorizations and are never represented as balances or
locked funds.

## Balance model

An `AccountBalance` holds three values in the same currency:

- `total`: the venue-reported total balance figure (wallet, net liquidation,
  or margin balance, depending on the venue).
- `locked`: amount reserved against open orders and positions.
- `free`: amount available for new orders (`total - locked`).

The invariant `total == locked + free` must always hold at currency precision.

The Python `AccountBalance(total, locked, free)` constructor requires all three
fields up front. Adapter code written in Rust has two additional derived
constructors that enforce the invariant centrally; prefer them over
`AccountBalance::new` whenever the venue reports only two of the three values:

| Rust constructor                        | When to use                                                              |
| --------------------------------------- | ------------------------------------------------------------------------ |
| `AccountBalance::from_total_and_locked` | Venue reports total and locked; `free` is derived from the two.          |
| `AccountBalance::from_total_and_free`   | Venue reports total and free; `locked` is derived from the two.          |
| `AccountBalance::new`                   | All three values are already known and consistent (tests, pass-through). |

Each derived constructor clamps the venue-reported field into `[0, total]` when `total >= 0`,
so transient overshoots from venue rounding never leave the account in a broken
state.

## Currency and valuation contracts

Accounting values retain their source currency until an explicit conversion succeeds. This prevents a valid
number from being labeled with the wrong currency or an unavailable value from being treated as zero.

| Value                        | Currency contract                                              |
| ---------------------------- | -------------------------------------------------------------- |
| Instrument cost currency     | Base for inverse, settlement for quanto, and quote otherwise.  |
| Position PnL                 | Instrument cost currency captured when the position opens.     |
| Calculated locks and margins | Each calculated amount's currency, converted independently.    |
| Portfolio aggregates         | Native buckets, or the account base after conversion succeeds. |

Aggregations combine only compatible `Money` values. An account without a base currency keeps separate native
currency buckets. A single-instrument realized PnL query returns unavailable instead of combining mixed
currencies.

The accounting and valuation paths also follow these rules:

- Invalid or unrepresentable notional, PnL, fee, locked-balance, and margin results produce an error,
  unavailable value, or unpriced state. They do not substitute zero. A failed realized PnL recalculation also
  clears any earlier cached result.
- `equity()` counts a credited non-inverse base asset once for a multi-currency cash account without a base
  currency. `mark_values()` remains a gross position-value query and includes that asset.
- MTM snapshots distinguish carried stale inputs from positions that have never had complete valuation data.
  Stale-price metadata covers only open instrument and position-side pairs.

See [Portfolio](portfolio.md#equity-and-mark-to-market) for equity formulas, price and xrate selection, snapshot
metadata, and missing-price query scope.

## Margin scopes

A `MarginBalance` has four fields: `initial`, `maintenance`, `currency`, and an
`Optional[InstrumentId]` that selects one of two scopes.

### Per-instrument scope

`MarginBalance.instrument_id` is set to a concrete instrument. Use this for:

- Isolated margin (per-position collateral).
- Backtest or calculated margin, where the `AccountsManager` derives margin
  locally from open orders and positions per instrument.

### Account-wide scope

`MarginBalance.instrument_id` is `None`. The entry is keyed by its
`currency` (the collateral currency). Use this for cross-margin venues that
report a single aggregate per collateral currency. A venue may emit one
account-wide entry (single-collateral cross margin) or several (one per
collateral coin).

Both scopes coexist on the same `MarginAccount` in separate internal stores.
An `AccountState` event may carry entries in either or both scopes, and
`MarginAccount.apply()` routes each entry to the correct store based on whether
`instrument_id` is set.

:::note
`MarginAccount.apply()` **replaces** both stores from the incoming event. It does
not merge with prior state, and an event carrying neither balances nor margins
leaves the prior stores in place. Adapters that emit partial snapshots must
include every live margin entry on each update or those entries will be dropped
until the next full snapshot. Balances the event carries replace the stored
entry for their currency; currencies the event omits are retained.
:::

## Strategy query API

Use the query that matches the venue's reporting shape. If a venue reports
per-instrument margins, ask by `InstrumentId`. If it reports account-wide
margins, ask by `Currency`.

| Scope          | Queries                                                                      |
| -------------- | ---------------------------------------------------------------------------- |
| Per-instrument | `margin`, `initial_margin`, and `maintenance_margin`                         |
| Account-wide   | `account_margin`, `account_initial_margin`, and `account_maintenance_margin` |
| Both scopes    | `total_initial_margin` and `total_maintenance_margin`                        |

The signatures below describe the Python bindings. Point queries return `None`
when the entry is absent; total queries always return a `Money` (zero for the
currency if nothing matches).

### Per-instrument queries (`MarginAccount`)

- `margin(instrument_id) -> MarginBalance | None`
- `initial_margin(instrument_id) -> Money | None`
- `maintenance_margin(instrument_id) -> Money | None`
- `margins() -> dict[InstrumentId, MarginBalance]` (all per-instrument entries)
- `initial_margins() -> dict[InstrumentId, Money]`
- `maintenance_margins() -> dict[InstrumentId, Money]`

These methods only see the per-instrument store. On a cross-margin venue they
return empty dicts or `None`. Use the account-wide queries below.

### Account-wide queries (`MarginAccount`)

- `account_margin(currency) -> MarginBalance | None`
- `account_initial_margin(currency) -> Money | None`
- `account_maintenance_margin(currency) -> Money | None`
- `account_margins() -> dict[Currency, MarginBalance]` (all account-wide entries)
- `account_initial_margins() -> dict[Currency, Money]`
- `account_maintenance_margins() -> dict[Currency, Money]`

### Totals (`MarginAccount`)

These sum across per-instrument and account-wide entries for a given currency:

- `total_initial_margin(currency) -> Money`
- `total_maintenance_margin(currency) -> Money`

Useful when a strategy trades on a venue where both scopes may appear (for
example, isolated positions alongside cross-margin collateral).

### Python binding boundary

This query surface does not expose the internal Rust mutation methods
`update_margin`, `clear_margin`, `clear_account_margin`, `clear_initial_margin`,
`clear_maintenance_margin`, or `set_margin_model`. Python does expose other
mutation methods, including `update_initial_margin`, `update_maintenance_margin`,
`set_default_leverage`, and `set_leverage`.

### Portfolio-level queries

Margin queries:

- `portfolio.instrument_initial_margins(venue=..., account_id=...) -> dict[InstrumentId, Money] | None`
- `portfolio.instrument_maintenance_margins(venue=..., account_id=...) -> dict[InstrumentId, Money] | None`

When a margin account resolves, these return the same per-instrument money
views as `MarginAccount.initial_margins` and
`MarginAccount.maintenance_margins`; otherwise, they return `None`. For
account-wide data on cross-margin venues, query the account directly via
`portfolio.account(venue=venue).account_initial_margin(ccy)`. The returned account is
a detached snapshot and cannot mutate Portfolio state.

PnL, exposure, mark-to-market, and equity queries all accept `venue` and an
optional `account_id` to scope multi-account venues:

- `portfolio.unrealized_pnls(venue=..., account_id=..., target_currency=...) -> dict[Currency, Money]`
- `portfolio.realized_pnls(venue=..., account_id=..., target_currency=...) -> dict[Currency, Money]`
- `portfolio.total_pnls(venue=..., account_id=..., target_currency=...) -> dict[Currency, Money]`
- `portfolio.net_exposures(venue=..., account_id=..., target_currency=...) -> dict[Currency, Money] | None`
- `portfolio.mark_values(venue=..., account_id=...) -> dict[Currency, Money]`
- `portfolio.equity(venue=..., account_id=...) -> dict[Currency, Money]`
- `portfolio.missing_price_instruments(venue, account_id=...) -> list[InstrumentId]`

If both scope arguments are present, they must identify the same account. A missing price, failed
target-currency conversion, or arithmetic overflow invalidates the whole affected collection: a
query never returns partial or mixed-currency totals.

See the [Portfolio guide](portfolio.md#equity-and-mark-to-market) for the equity
formula, price fallback chain, base-currency conversion behavior, and the
warn-once missing-price tracker.

### Worked examples

Single-collateral cross margin (one account-wide entry):

```python
usdc_margin = margin_account.account_initial_margin(USDC)
usdc_total = margin_account.total_initial_margin(USDC)
```

Per-coin cross margin (one entry per collateral currency):

```python
for ccy, margin_balance in margin_account.account_margins().items():
    print(ccy, margin_balance.initial, margin_balance.maintenance)
```

## Margin models

NautilusTrader provides flexible margin calculation models for the calculated
path (backtests, and live strategies running with `calculate_account_state=True`
for reconciliation). Reported margins from a venue flow straight into the
account's `margins` or `account_margins` stores without going through a model.

### Overview

Different venues treat leverage differently:

- **Traditional brokers** (Interactive Brokers, TD Ameritrade): fixed margin percentages regardless of leverage.
- **Crypto exchanges** (Binance, others): leverage may reduce margin requirements.

Both built-in models compute margin as a percentage of notional using the
instrument's `margin_init` and `margin_maint` fields. They differ only in
whether leverage reduces the reservation. For venues with true per-contract
fixed margin (CME / ICE), set `instrument.margin_init` and `margin_maint` so
the percentage recovers the desired dollar amount.

### HEDGING-mode netting

Under `OmsType.HEDGING` each fill opens its own `Position`, so an account can
hold many open sub-positions for the same instrument. The accounts manager
nets those sub-positions onto a hypothetical NETTING position in `ts_opened`
order, then runs the margin model once on the resulting net signed quantity
and average open price.

The replay follows the same rules as `Position.apply`: same-side fills
produce a quantity-weighted average open price, opposite-side fills
partial-close at the existing average, and a fill that crosses zero makes
the residual take the flipping fill's price. Sub-positions sharing a
`ts_opened` fold in `(ts_opened, position_id)` order so the result does
not depend on cache iteration order.

HEDGING and NETTING accounts compute the same maintenance margin for the
same fill sequence; the requirement scales with net economic exposure.

### Available models

#### `StandardMarginModel`

Uses fixed percentages without leverage division, matching traditional broker
behavior.

```python
# Fixed percentages - leverage ignored
margin = notional * instrument.margin_init
```

- Initial margin: `notional_value * instrument.margin_init`
- Maintenance margin: `notional_value * instrument.margin_maint`

**Use cases**: traditional brokers (Interactive Brokers), forex brokers with
fixed margin requirements.

#### `LeveragedMarginModel`

Divides margin requirements by leverage.

```python
# Leverage reduces margin requirements
adjusted_notional = notional / leverage
margin = adjusted_notional * instrument.margin_init
```

- Initial margin: `(notional_value / leverage) * instrument.margin_init`
- Maintenance margin: `(notional_value / leverage) * instrument.margin_maint`

**Use cases**: crypto exchanges that reduce margin with leverage, venues where
leverage affects margin requirements.

### Default behavior

`MarginAccount` uses `LeveragedMarginModel` by default. Backtests select
`StandardMarginModel` by passing it directly to `BacktestVenueConfig.margin_model`.

### Worked example: EUR/USD

- **Instrument**: EUR/USD
- **Quantity**: 100,000 EUR
- **Price**: 1.10000
- **Notional**: $110,000
- **Leverage**: 50x
- **`instrument.margin_init`**: 3%

| Model     | Calculation            | Result | Percentage |
| --------- | ---------------------- | ------ | ---------- |
| Standard  | $110,000 × 0.03        | $3,300 | 3.00%      |
| Leveraged | ($110,000 ÷ 50) × 0.03 | $66    | 0.06%      |

On a $1,000 account: the standard model blocks the trade; the leveraged model
allows it.

### Python model selection

Pass `StandardMarginModel()` or `LeveragedMarginModel()` directly to the backtest venue. The
current Python binding does not accept custom margin model subclasses or a `MarginModelConfig`
wrapper. See [Backtesting](backtesting/accounts-and-margin.md#margin-models).

## Adapter convention

Live adapters translate venue responses into `AccountBalance` and
`MarginBalance` instances. The convention that adapter authors must follow:

### Building `AccountBalance`

Prefer the derived constructors so that clamping and the `total == locked + free`
invariant are enforced centrally. Hand-computing three fields and passing them
to `AccountBalance::new` is only appropriate for pass-through paths where all
three values are already authoritative (e.g., tests).

### Building `MarginBalance`

Pick the scope that matches what the venue reports:

| Venue reports                                  | Scope          | Emit with                                                  |
| ---------------------------------------------- | -------------- | ---------------------------------------------------------- |
| Per-instrument (isolated positions)            | Per-instrument | `MarginBalance::new(initial, maint, Some(id))`             |
| Single aggregate per collateral (cross margin) | Account-wide   | `MarginBalance::new(initial, maint, None)`                 |
| Multiple aggregates, one per collateral        | Account-wide   | One `MarginBalance` per currency with `instrument_id=None` |

:::note
Synthetic `ACCOUNT.{VENUE}` or `ACCOUNT-{COIN}.{VENUE}` `InstrumentId`
placeholders are not used. Account-wide entries carry `instrument_id=None` and
are keyed by `currency`.
:::

## Related guides

- [Backtesting](backtesting/): starting balances, margin models, and backtest-specific account
  setup.
- [Portfolio](portfolio.md): portfolio-level PnL, exposures, and currency
  conversion.
- [Positions](positions.md): position lifecycle, aggregation, and PnL.
- [Adapters](adapters.md): requirements and best practices for adapter authors.
- [Blockchain](../integrations/blockchain.md): the adapter that selects wallet accounts, and its
  execution status.
