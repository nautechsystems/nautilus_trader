# Accounting

The accounting subsystem tracks balances, margins, and PnL for each account. This guide covers the data model,
the query API that strategies use, and the conventions adapter authors must follow to stay consistent across
venues.

It applies equally to backtest and live trading. For backtest-specific
configuration (starting balances, margin-model selection per venue), see
[Backtesting](backtesting/).

## Account types

When configuring a venue for live trading or backtests, use `account_type` to select Cash, Margin, or Betting.

Wallet models on-chain wallet state. The Blockchain adapter selects it. Its execution client supports locally
signed Uniswap V3 market swaps but is not production-ready.

| Account type | Typical use case                                | Reservations                                                              |
| ------------ | ----------------------------------------------- | ------------------------------------------------------------------------- |
| Cash         | Spot trading (for example, BTC/USDT, stocks)    | Notional for pending buy orders. Base-asset quantity for pending sells.   |
| Margin       | Derivatives or any product that allows leverage | Initial margin for each order plus maintenance margin for open positions. |
| Betting      | Sports betting, bookmaking                      | Stake required by the venue. No leverage.                                 |
| Wallet       | Blockchain wallets (DeFi)                       | Amounts reserved locally for pending orders. No leverage or borrowing.    |

### Cash accounts

Cash accounts **settle trades in full**. They have no leverage and thus no concept of margin. Locked balances
reflect the value reserved for pending orders: the notional value of each pending buy and the quantity each
pending sell would deliver.

#### Precision and rounding

Cash flows and reservations use checked domain or `Decimal` arithmetic and project to native `Money` precision
with midpoint-to-even rounding. Balance updates add and subtract `Money` amounts without rounding to currency
display precision. Currency conversions preserve native `Money` precision too.

See [Precision and settlement](#precision-and-settlement) for the accounting rationale and worked examples.

#### Sell reservations

A sell delivers the instrument's base asset, so the reservation belongs in that asset. Instruments whose
`base_currency()` is `None`, such as binary options and equities, model no such asset. A pending sell on those
instruments **locks nothing**, and its proceeds are not compared against the free quote balance.

Reserving the order quantity in the quote currency would label a share or contract count as money and lock
collateral the sale never consumes. Comparing sale proceeds against free collateral would deny a sale for lack
of the very balance it credits.

In live trading, inventory availability for those instruments is not tracked or
checked locally, including across concurrent sells. The venue enforces it, and any
rejection arrives through the normal order lifecycle. This boundary is
deliberate: reproducing the venue's inventory check locally would require
per-asset balances the adapter does not receive. Buy reservations and sell
checks for instruments that do model a base asset are unchanged.

#### Simulated short-selling checks

Backtests and sandbox check each equity or binary-option sell on a cash account at submission. The order's
remaining quantity must not exceed the long inventory in its selected position, including executed fills
awaiting position updates. Sells that reduce or close that position stay valid.

The check uses the order price or current bid to calculate contract units for
quote-sized sells. Trigger-style market and trailing orders can convert at a
different price when they fill, changing the contract quantity.

An entry contributes inventory to its contingent exits only as fills execute. By default, `oto_full_trigger`
is `false`, so a full-size exit can be rejected after a partial entry fill. Set `oto_full_trigger` to `true`
to release the exit only after the full entry fill. This also applies when position updates are deferred.

This submission check does not reserve inventory for other working sells or
revalidate later quantity changes.

### Margin accounts

Margin accounts support instruments that require collateral, such as futures or leveraged crypto perps. They
track account balances, reserve margin for open orders and positions, and apply a configurable leverage per
instrument. Margin balances have two scopes. See [Margin scopes](#margin-scopes) below.

**Terms**:

- **Leverage**: amplifies exposure relative to account equity. Higher leverage
  raises both potential returns and risk.
- **Initial margin**: collateral reserved when an order is submitted.
- **Maintenance margin**: minimum collateral required to keep an open position.
- **Locked balance**: funds reserved as collateral, not available for new orders.

:::note
Reduce-only orders can only decrease exposure. Thus, they do not contribute to `balance_locked` on cash
accounts or add to initial margin on margin accounts. Wallet orders still reserve the input asset because the
on-chain transaction spends that asset even when the order reduces a position.
:::

### Betting accounts

Betting accounts are specialized for venues where you stake an amount to win or lose a fixed payout
(prediction markets, sports books). They lock only the stake required by the venue. Leverage and margin do not
apply.

### Wallet accounts

Wallet accounts represent blockchain wallets: unleveraged, multi-currency holdings of native and ERC-20 token
balances with no margin and no borrowing.

For reported states:

- `total` is the observed on-chain balance.
- `locked` tracks local pending-order reservations.
- `free = total - locked`.

Account state events **contribute totals only**: the account ignores incoming `locked` and `free` values,
keeps its local reservations, and rederives `free`. It rebuilds transient reservations from submitted and open
orders during live startup.

While an amendment is pending, the account reserves the full observed balance of the debit currency. The
pending event does not carry the requested terms. If the reserved amount exceeds the latest observed total,
`locked` is capped at `total` and `free` stays zero until the balance or reservation changes.

A balance with a negative total is rejected rather than applied. ERC-20
allowances are spender authorizations and are never represented as balances or
locked funds.

## Balance model

An `AccountBalance` holds three values in the same currency:

- `total`: the venue-reported total balance figure (wallet, net liquidation,
  or margin balance, depending on the venue).
- `locked`: amount reserved against open orders and positions.
- `free`: amount available for new orders (`total - locked`).

The **balance invariant** `total == locked + free` must always hold exactly in the stored Money representation.

### Constructing balances

The Python `AccountBalance(total, locked, free)` constructor requires all three fields up front. Adapter code
written in Rust has derived constructors that enforce the invariant centrally. Prefer them over
`AccountBalance::new` whenever the venue reports only two of the three values:

| Rust constructor                              | When to use                                                                      |
| --------------------------------------------- | -------------------------------------------------------------------------------- |
| `AccountBalance::from_total_and_locked`       | Venue reports total and locked. `free` is derived from the two.                  |
| `AccountBalance::from_total_and_free`         | Venue reports total and free. `locked` is derived from the two.                  |
| `AccountBalance::from_money_total_and_locked` | Calculated total and locked are already Money amounts. Retains native precision. |
| `AccountBalance::new`                         | All three values are already known and consistent (tests, pass-through).         |

Each derived constructor clamps the supplied locked or free amount into `[0, total]` when `total >= 0`,
so transient overshoots from venue rounding never leave the account in a broken
state.

## Currency and valuation contracts

Accounting values keep their source currency until an explicit conversion succeeds. This prevents a valid
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
  currency. `mark_values()` stays a gross position-value query and includes that asset.
- MTM snapshots distinguish carried stale inputs from positions that have never had complete valuation data.
  Stale-price metadata covers only open instrument and position-side pairs.

See [Portfolio](portfolio.md#equity-and-mark-to-market) for equity formulas, price and xrate selection, snapshot
metadata, and missing-price query scope.

## Precision and settlement

Position PnL and calculated account balances retain amounts below currency display precision, so small profits
and fees remain available to later calculations.

### Cost basis and residuals

Financial calculations use checked domain types and `Decimal`:

- Positions derive average prices from retained opening and closing costs. A rounded average never feeds the
  cost basis.
- Partial closes allocate opening cost proportionally and retain the unallocated cost. The final close consumes
  the remaining cost.
- Positions carry a Decimal PnL remainder below the native `Money` scale into later updates. Reversal fee
  allocations preserve the original fee total.

Derived `Money` rounds midpoint to even at nine decimal places in standard mode and 16 in high-precision mode,
or at a finer supported currency precision. Display rounding does not change the stored amount.

Decimal has a 96-bit coefficient and a maximum scale of 28. Inputs must fit exactly; derived arithmetic rounds
within that bound and rejects overflow or a nonzero value disappearing. PnL sums may absorb a residual smaller
than one native settlement `Money` unit if Decimal cannot retain it. A disappearing operand of at least one
native unit still fails. See the
[full arithmetic contract](positions.md#numerical-precision).

### Worked examples

Both examples use a linear instrument with a multiplier of one, USD prices, and no fees.

#### Profit below a cent

Buy `0.01` units at `0.1234 USD`, then sell them at `0.1235 USD`:

- Purchase cost: `0.01 * 0.1234 = 0.001234 USD`.
- Sale proceeds: `0.01 * 0.1235 = 0.001235 USD`.
- Realized PnL and calculated cash-balance increase: `0.000001 USD`.

A two-decimal display shows `0.00 USD`, but the stored profit remains available to later calculations.
Rounding each cash flow to cents would erase it.

#### Partial closes with a repeating average

Buy three units at `0.10 USD` and four at `0.20 USD`. Opening cost is `1.10 USD`; the average price, `1.10 / 7`,
repeats. Close at `0.30 USD` in two fills. At standard native precision:

| Action               | Quantity held | Cumulative realized PnL (USD) |
| -------------------- | ------------- | ----------------------------- |
| After both buys      | 7             | 0.000000000                   |
| Sell one at 0.30 USD | 6             | 0.142857143                   |
| Sell six at 0.30 USD | 0             | 1.000000000                   |

The first sale retains unallocated cost and the difference between Decimal PnL and the rounded `Money` result.
The final close consumes both remainders: total PnL is `2.10 - 1.10 = 1.00 USD`. Discarding fractional PnL or
reusing a rounded average can introduce drift.

### Venue cash settlement

Venue settlement rules specify where and how to round booked cash, which can differ from internal PnL.
With `calculate_account_state=False`, cash and margin accounts skip fill-driven balance calculations and use
venue-reported balances. FX rollover calculates in Decimal and rounds each posting to currency precision.

CME's [normal futures method (2015)](https://www.cmegroup.com/clearing/files/CME-Money-Calculations-Futures-and-Options.pdf)
rounds `price * contract value factor` to currency precision at both prices, subtracts those values, then
multiplies by quantity. Splitting trades at the same price or adding intermediate marks for an unchanged
holding preserves the total. Midpoints round away from zero. Notional products round after multiplying the
price change by quantity and the contract value factor.

[Eurex changed variation margin and other cash flows in 2023](https://www.eurex.com/ec-en/find/circulars/clearing-circular-3572446)
from per-contract rounding to transaction or position rounding, following client demand. Matching a clearing
statement requires the product's settlement rules; native-precision PnL alone does not reproduce every convention.

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

Both scopes coexist on the same `MarginAccount` in separate internal stores. An `AccountState` event can carry
entries in either or both scopes. `MarginAccount.apply()` routes each entry to the correct store based on
whether `instrument_id` is set.

:::warning
`MarginAccount.apply()` **replaces** both margin stores from the incoming event. It does not merge with prior
state. An event carrying neither balances nor margins leaves the prior stores in place.

Adapters that emit partial snapshots must include each live margin entry on each update or those entries will
be dropped until the next full snapshot.

Balances the event carries replace the stored entry for their currency. Currencies the event omits are
retained.
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

The signatures below describe the Python bindings. Point queries return `None` when the entry is absent. Total
queries always return a `Money` (zero for the currency if nothing matches).

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

#### Margin queries

- `portfolio.instrument_initial_margins(venue=..., account_id=...) -> dict[InstrumentId, Money] | None`
- `portfolio.instrument_maintenance_margins(venue=..., account_id=...) -> dict[InstrumentId, Money] | None`

An `account_id` selects that account. A venue-only query resolves only when
exactly one account is issued under the venue, so pass `account_id` when several
accounts share a venue.

When a margin account resolves, these return the same per-instrument money views as
`MarginAccount.initial_margins` and `MarginAccount.maintenance_margins`. If no margin account resolves, they
return `None`.

For account-wide data on cross-margin venues, query the account directly through
`portfolio.account(venue=venue).account_initial_margin(ccy)`. The returned account is a **detached snapshot**
and cannot mutate Portfolio state.

#### PnL and valuation queries

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

- **Traditional brokers** (for example, Interactive Brokers): fixed margin percentages regardless of leverage.
- **Crypto exchanges** (Binance, others): leverage may reduce margin requirements.

Both built-in models compute margin as a percentage of notional using the
instrument's `margin_init` and `margin_maint` fields. They differ only in
whether leverage reduces the reservation. For venues with true per-contract
fixed margin (CME / ICE), set `instrument.margin_init` and `margin_maint` so
the percentage recovers the desired dollar amount.

### HEDGING-mode netting

Under `OmsType.HEDGING`, the first fill for a new position ID opens a `Position`. Later fills can update that
position. An account can thus hold many open sub-positions for the same instrument. The accounts manager nets
those sub-positions onto a hypothetical NETTING position in `ts_opened` order. It then runs the margin model
once on the resulting net signed quantity and average open price.

The replay follows the same rules as `Position.apply`:

- Same-side fills produce a quantity-weighted average open price.
- Opposite-side fills partial-close at the existing average.
- A fill that crosses zero makes the residual take the flipping fill's price.

Sub-positions sharing a `ts_opened` fold in `(ts_opened, position_id)` order so the result does not depend on
cache iteration order.

With the same margin model and leverage, HEDGING and NETTING maintenance margins match if folded net quantities
and average open prices match. The margin requirement scales with **net economic exposure**.

### Available models

#### `StandardMarginModel`

Uses **fixed percentages without leverage division**, matching traditional broker
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

Divides margin requirements **by leverage**.

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

On a $1,000 account: the standard model blocks the trade. The leveraged model allows it.

### Python model selection

Pass a built-in model, a subclass of `nautilus_trader.model.MarginModel`, or a compatible Python object
as `margin_model` to `BacktestVenueConfig`, `BacktestEngine.add_venue()`, or `SandboxExecutionClientConfig`.
Models apply only to margin accounts. No `MarginModelConfig` wrapper is needed.
See [Backtesting](backtesting/accounts-and-margin.md#margin-models).

#### Callback contract

Custom objects must expose callable `calculate_initial_margin` and `calculate_maintenance_margin` methods
or configuration raises `TypeError`. Inherited base methods raise `NotImplementedError`.

Both take `(instrument, quantity, price, leverage, use_quote_for_inverse)` and return `Money`:

- `instrument`: concrete Python type, such as `CurrencyPair`.
- `quantity`, `price`, and `leverage`: `Quantity`, `Price`, and `decimal.Decimal`.
- `use_quote_for_inverse`: `True` requests quote-currency margin for inverse instruments.
  Risk and portfolio calls pass `None`; direct account calls also accept `True` and `False`.

#### Failure handling

- Direct `MarginAccount` calls raise `ValueError` with callback context for exceptions or non-`Money` results.
- Pre-trade risk checks reject the order command if initial-margin calculation fails.
- Portfolio failures silently skip the instrument's margin update and retain its previous margin.
  One failing open order skips that instrument's entire initial-margin update.

#### State and runtime limits

Configuration and account clones share the Python instance and state. Canonical backtest results use
`margin_model: "python"` for all custom Python classes.

Keep callbacks independent of the cache and portfolio: portfolio updates can hold borrows and temporarily
remove the account from the cache.

Callbacks acquire the GIL and clone instruments into Python objects. Conversion, dispatch, and `Money`
extraction add overhead; no benchmarks quantify it.

## Adapter convention

Live adapters translate venue responses into `AccountBalance` and
`MarginBalance` instances. The convention that adapter authors must follow:

### Building `AccountBalance`

Prefer the derived constructors so that clamping and the `total == locked + free` invariant are enforced
centrally. Hand-computing three fields and passing them to `AccountBalance::new` is only appropriate for
pass-through paths where all three values are already authoritative (for example, tests).

### Building `MarginBalance`

Pick the scope that matches what the venue reports:

| Venue reports                                  | Scope          | Emit with                                                  |
| ---------------------------------------------- | -------------- | ---------------------------------------------------------- |
| Per-instrument (isolated positions)            | Per-instrument | `MarginBalance::new(initial, maint, Some(id))`             |
| Single aggregate per collateral (cross margin) | Account-wide   | `MarginBalance::new(initial, maint, None)`                 |
| Multiple aggregates, one per collateral        | Account-wide   | One `MarginBalance` per currency with `instrument_id=None` |

:::info
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
