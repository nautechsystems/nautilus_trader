<!-- DOCID: docs/architecture/equities_hl_ibkr_prod_model@v1 -->

# Equities HL vs IBKR Production Identity Model

Task 1 defines the canonical shared contract for the dedicated equities profile without changing the outer equities surface.

## Goals

- Keep `/equities`, `profile=equities`, and `portfolio=equities` stable.
- Make `deploy/equities/equities.live.toml` the single source of truth for canonical stock identity.
- Separate strategy-local identity from shared-account provenance before later portfolio and API work lands.

## Canonical Strategy Contract

Each enrolled equities strategy now has one `[[strategy_contracts]]` entry in the shared deploy manifest.

Required fields:

- `strategy_id`: strategy-local process identity.
- `portfolio_asset_id`: canonical stock identity used by the shared equities portfolio.
- `maker_instrument_id`: Hyperliquid execution instrument identity.
- `reference_instrument_id`: IBKR listing-venue instrument identity.
- `execution_account_scope_id`: stable scope id for the shared Hyperliquid execution account.
- `reference_account_scope_id`: stable scope id for the shared IBKR reference account.
- `hedge_account_scope_id`: optional stable scope id reserved for future hedge-account projection.

`strategy_id` is not the shared-account identity. It names one strategy node. Shared balances and shared portfolio rows must use the canonical asset and scope fields above instead of inferring ownership from venue strings or strategy ids.

## Shared-Account Provenance

Shared-account rows must carry explicit provenance:

- `source_scope`: one of `strategy`, `shared_account`, or `portfolio`.
- `account_scope_id`: the stable account-scope id that produced the row, such as `ibkr.reference.main`.
- `source_strategy_ids`: enrolled strategies that depend on or contribute to the shared row.

These fields let the API and Fluxboard represent pre-existing IBKR holdings and other portfolio-scoped rows without pretending they belong to a single strategy process.

## Non-Goals

- Do not change the public equities route surface.
- Do not move balance polling or portfolio aggregation in this task.
- Do not introduce MakerV4 canary behavior yet; this task only defines the contract later tasks will consume.
