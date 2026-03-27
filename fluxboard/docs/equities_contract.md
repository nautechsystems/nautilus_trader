<!-- DOCID: apps/fluxboard/docs/equities_contract@v1 -->

# Equities HTTP Contract (`equities:v1`)

This document freezes the operator-facing HTTP contract for the dedicated equities surface.
It is implementation-facing and keeps `/equities`, `profile=equities`, and `portfolio=equities` stable.

## Scope and Route Surface

Required routes:

| Route | Status | Notes |
| --- | --- | --- |
| `/equities` | required | Dashboard landing |
| `/equities/dashboard` | required | Explicit dashboard route |
| `/equities/signal` | required | Signals page |
| `/equities/params` | required | Params page |
| `/equities/balances` | required | Balances page |
| `/equities/trades` | required | Trades page |
| `/equities/alerts` | required | Alerts page |

The equities rollout keeps trade[XYZ] execution on `HYPERLIQUID` plus `dex = "xyz"` and also supports enrolled `BINANCE_PERP` multivenue routes on the shared equities surface.
The reference venue for FV inputs is `IBKR`.
The enrolled split rollout currently serves `AAPL`, `AMD`, `AMZN`, `COIN`, `CRCL`, `EWY`, `GOOGL`, `HOOD`, `INTC`, `META`, `MSFT`, `MSTR`, `NVDA`, `ORCL`, `PLTR`, and `TSLA` through the shared equities control plane.
Representative canonical routes include `xyz:AAPL-USD-PERP.HYPERLIQUID`, `PLTRUSDT-PERP.BINANCE_PERP`, and `AAPL.NASDAQ`.

## Frozen Deploy Identity

1. The intended active equities deploy contract is the split `equities_maker` plus `equities_taker` family pair via the enrolled stock allowlist in `api.equities_strategy_ids`.
2. The enrolled strategy ids and service names use the `*_maker` and `*_taker` suffixes across both tradexyz and Binance multivenue routes.
3. Representative split strategy ids include `aapl_tradexyz_maker`, `aapl_tradexyz_taker`, `pltr_binance_perp_maker`, and `pltr_binance_perp_taker`.
4. `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled` is rollback material, not the active contract.
5. On the shared `tokenmm-api` host, `/equities` is a proxied route, not the asset prefix. That public HTML shell must load Fluxboard assets from `/static/fluxboard/assets/*`.
6. `/equities` stays a SPA route, not the asset prefix. Shared Fluxboard files still publish from `/static/fluxboard/*`.
7. Task 2 of the March 11 live review locked that build/static-serving contract to the shared `/static/fluxboard/` base.
8. If public `/equities` emits `/tokenmm/assets/*`, treat that as live host drift from a stale/shared bundle, not as a supported contract.
9. `SMSN` and `SKHX` are not part of the current enrolled set because exact IBKR qualification is still unresolved.

## Profile Contract

Read endpoints use `profile=equities`.
The shared portfolio identity remains `portfolio=equities`.
Mixed-family params requests must add an explicit `strategy=<strategy_id>` selector for schema lookup and single-target writes.
Bulk params writes remain valid through `updates[].strategy_id`.

Primary requests:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/params?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/param-schema?profile=equities&strategy=aapl_tradexyz_maker'
curl -fsS -X PATCH 'http://127.0.0.1:5022/api/v1/params?profile=equities&strategy=aapl_tradexyz_maker'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/trades?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/alerts?profile=equities'
```

## Strategy and Deploy Identity

1. One stock can run both `*_maker` and `*_taker`; grouped nodes are an internal deploy detail and do not change the outer strategy-level operator surface.
2. The live allowlist is `api.equities_strategy_ids`.
3. Required portfolio readiness is `api.equities_required_strategy_ids`.
4. `deploy/equities/equities.live.toml` exposes one `[[strategy_contracts]]` row per enrolled strategy variant as the canonical identity registry for `strategy_id`, `portfolio_asset_id`, venue instrument ids, and shared account scopes.
5. `portfolio_asset_id` is the canonical equities inventory identity. Do not infer portfolio identity from venue-specific base strings such as `XYZ:AAPL`.
6. Shared account scopes are explicit: `execution_account_scope_id`, `reference_account_scope_id`, and optional `hedge_account_scope_id`.
7. `strategy_id` remains strategy-local. Shared-account ownership is modeled through provenance fields, not by rewriting shared rows to look strategy-owned.
8. The systemd install flow uses `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS` from `/etc/flux/common.env`.
9. Future strategy changes must preserve the outer equities surface even if the inner strategy implementation changes.
10. realtime behavior remains part of the external contract even if node topology changes behind `/equities`.

## Response Expectations

1. `signals.strategies[].meta.strategy_groups` is `equities`.
2. `balances` represents the shared `equities` portfolio view.
3. The current live balances contract may still use the legacy shared-row marker `scope = "shared_account"` until the later balance-model tasks land.
4. Later balance-model tasks will add explicit shared-account provenance fields:
   - `source_scope`: `strategy`, `shared_account`, or `portfolio`
   - `account_scope_id`: stable account identity such as `ibkr.reference.main`
   - `source_strategy_ids`: enrolled strategies that consume or publish against that shared row
5. Strategy-local rows may still expose `strategy_id`, but future shared-account and portfolio rows must not rely on `strategy_id` as their ownership identity.
6. `balances` may include both Hyperliquid execution rows and IBKR reference-account rows when the IBKR reference monitor is connected.
7. Future shared IBKR cash rows may carry `source_scope = "shared_account"` and a shared `account_scope_id` when multiple equities strategies project the same IBKR account.
8. `signals` should show an IBKR reference market identity even when the IBKR gateway is unavailable; in that state, the reference prices may be empty or stale, but they must not mirror the Hyperliquid maker leg.
9. Clients should ignore unknown fields and tolerate additional metadata fields.

## Shared Quote Health Semantics

These semantics are shared across Flux strategy families and operator surfaces.

1. `age_ms` is informational only: it is time since the last observed quote update at serialization time.
2. High `age_ms` does not by itself mean feed failure; quiet overnight books can legitimately hold an unchanged quote.
3. `feed_state`, when present, reports transport/subscription health:
   - `ok`
   - `degraded`
   - `down`
   - `unknown`
4. `quote_state`, when present, reports quote freshness/presence:
   - `fresh`
   - `old`
   - `missing`
5. Old-but-connected quotes must be represented as `feed_state = ok` and `quote_state = old`; they are not feed-down by default.
6. `tradeable` and any hedge-eligibility flags are backend policy outputs, not UI heuristics inferred from `age_ms`.
7. Operator UX should show quote age separately from feed health so “quiet market”, “old quote”, and “broken feed” remain distinct.
