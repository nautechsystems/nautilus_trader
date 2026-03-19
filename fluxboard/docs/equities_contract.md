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

The equities rollout keeps trade[XYZ] execution on `HYPERLIQUID` plus `dex = "xyz"`.
The reference venue for FV inputs is `IBKR`.
The enrolled stock universe is now served through MakerV4 semantics on the shared equities control plane and currently includes `AAPL`, `AMD`, `AMZN`, `BABA`, `COIN`, `CRCL`, `CRWV`, `EWY`, `GOOGL`, `HOOD`, `INTC`, `META`, `MSTR`, `MSFT`, `MU`, `NFLX`, `NVDA`, `ORCL`, `PLTR`, `RIVN`, `SNDK`, `TSM`, `TSLA`, and `USAR`.
Representative canonical routes include `xyz:AAPL-USD-PERP.HYPERLIQUID`, `AAPL.NASDAQ`, `EWYUSDT-PERP.BINANCE_PERP`, and `USAR.NASDAQ`.

## Frozen Deploy Identity

1. The intended active equities deploy contract is MakerV4 via the enrolled stock allowlist in `api.equities_strategy_ids`.
2. The enrolled strategy ids and service names use the `*_makerv4` suffix.
3. Dead MakerV3 equities configs have been removed from the checked-in deploy surface.
4. On the shared `tokenmm-api` host, `/equities` is a proxied route, not the asset prefix. That public HTML shell must load Fluxboard assets from `/static/fluxboard/assets/*`.
5. `/equities` stays a SPA route, not the asset prefix. Shared Fluxboard files still publish from `/static/fluxboard/*`.
6. Task 2 of the March 11 live review locked that build/static-serving contract to the shared `/static/fluxboard/` base.
7. If public `/equities` emits `/tokenmm/assets/*`, treat that as live host drift from a stale/shared bundle, not as a supported contract.
8. `SMSN` and `SKHX` are not part of the current enrolled set because exact IBKR qualification is still unresolved.

## Profile Contract

All profile-scoped requests use `profile=equities`.
The shared portfolio identity remains `portfolio=equities`.

Primary requests:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/params?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/trades?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/alerts?profile=equities'
```

## Strategy and Deploy Identity

1. One strategy route uses one strategy file and one node process.
2. The live allowlist is `api.equities_strategy_ids`.
3. Required portfolio readiness is `api.equities_required_strategy_ids`.
4. `deploy/equities/equities.live.toml` exposes one `[[strategy_contracts]]` row per strategy route as the canonical identity registry for `strategy_id`, `portfolio_asset_id`, `maker_venue`, `maker_symbol`, `market_type`, venue instrument ids, and shared account scopes.
5. `portfolio_asset_id` is the canonical equities inventory identity. Do not infer portfolio identity from venue-specific base strings such as `XYZ:AAPL`.
6. `maker_venue`, `maker_symbol`, and `market_type` are mandatory per-route keys in the shared manifest.
7. Shared account scopes are explicit: `execution_account_scope_id`, `reference_account_scope_id`, and optional `hedge_account_scope_id`.
8. Multiple strategy routes may share the same `portfolio_asset_id` when one stock trades on multiple maker venues. Shared portfolio and future risk net at the stock bucket, while local maker inventory stays route-local.
9. `strategy_id` remains strategy-local. Shared-account ownership is modeled through provenance fields, not by rewriting shared rows to look strategy-owned.
10. The systemd install flow uses `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS` from `/etc/flux/common.env`.
11. Future strategy changes must preserve the outer equities surface even if the inner strategy implementation changes.

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
