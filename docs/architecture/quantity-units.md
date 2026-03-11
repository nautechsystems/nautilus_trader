# Quantity Units

This note defines the quantity semantics for NautilusTrader and Flux risk-facing
surfaces.

## Core Rule

NautilusTrader core domain quantities remain venue-native:

- `Order.quantity`
- `Fill.last_qty`
- `Position.quantity`
- `Position.signed_qty`

These values represent the exchange-native size submitted to, or reported by,
the venue. They are not implicitly normalized to base asset exposure.

## Risk Rule

Risk, balances, and portfolio inventory must use explicit base-exposure fields.

- `*_venue`: venue/native size used for execution and reconciliation
- `*_base`: normalized base-asset exposure used for strategy risk and balances
- `local_qty_base`: canonical maker-leg local exposure owned by each strategy
- `global_qty_base`: canonical shared portfolio exposure owned by `run_portfolio`

Risk-facing payloads must not rely on a bare `qty`, `local_qty`, `position_qty`,
or `order_qty` field when the unit matters.
`risk_delta` is diagnostic only and must not act as a hidden substitute for
`local_qty_base`.

## Required Risk-Facing Field Names

Use these names in external contracts and UI-facing payloads:

- `position_qty_venue`
- `position_qty_base`
- `local_qty_venue`
- `local_qty_base`
- `order_qty_venue`
- `order_qty_base`
- `qty_conversion_status`
- `qty_conversion_source`

## Conversion Semantics

The current `qty_conversion_status` space is:

- `identity`: venue quantity already equals base exposure
- `exact_multiplier`: base exposure derived exactly from venue size and contract multiplier
- `price_based`: base exposure derived from venue size plus price
- `unsupported`: instrument semantics do not support a safe conversion
- `missing_metadata`: required quantity metadata is unavailable
- `missing_price`: a price-based conversion was required but price was unavailable
- `non_integral_venue_qty`: base-to-venue conversion did not land on a valid venue increment

`qty_conversion_source` should identify the rule used, for example:

- `spot_identity`
- `linear_multiplier`
- `inverse_multiplier_last_price`

## Compatibility Guidance

If an existing payload still carries a bare `qty` field for compatibility, the
contract must say whether it is venue-native or base exposure. New risk-facing
surfaces should prefer the explicit `*_venue` and `*_base` names above.

## Reconciliation Rule

Risk correctness depends on fresh source ownership.

- strategy-local risk must come from venue-visible maker truth or explicitly
  reconciled cache truth
- shared portfolio risk must come only from the portfolio snapshot owned by
  `run_portfolio`
- missing or unreconciled truth must degrade explicitly rather than publish
  fabricated zeroes
- startup reconciliation failure means degraded or blocked trading, not
  best-effort stale-cache trading
