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
The canonical Hyperliquid execution instrument ID for the enrolled AAPL contract is `xyz:AAPL-USD-PERP.HYPERLIQUID`.
The canonical IBKR reference instrument ID for the enrolled AAPL contract is `AAPL.NASDAQ`.

## Frozen Deploy Identity

1. The active equities deploy contract is MakerV4 via `aapl_tradexyz_makerv4`.
2. `deploy/equities/strategies/aapl_tradexyz_makerv3.toml.disabled` is rollback-only material and must stay disabled during normal MakerV4 operations.
3. On the shared `tokenmm-api` host, `/equities` is a proxied route, not the asset prefix. That public HTML shell must load Fluxboard assets from `/static/fluxboard/assets/*`.
4. The standalone equities runner in repo still serves `/equities/assets/*` when hit directly.
5. If public `/equities` emits `/tokenmm/assets/*`, treat that as live host drift from a stale/shared bundle, not as a supported contract.

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

1. One stock uses one strategy file and one node process.
2. The live allowlist is `api.equities_strategy_ids`.
3. Required portfolio readiness is `api.equities_required_strategy_ids`.
4. The systemd install flow uses `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS` from `/etc/flux/common.env`.
5. Future strategy changes must preserve the outer equities surface even if the inner strategy implementation changes.

## Response Expectations

1. `signals.strategies[].meta.strategy_groups` is `equities`.
2. `balances` represents the shared `equities` portfolio view.
3. Per-row `strategy_id` values remain the enrolled equities strategy IDs.
4. `balances` may include both Hyperliquid execution rows and IBKR reference-account rows when the IBKR reference monitor is connected.
5. Shared IBKR cash rows may carry `scope = "shared_account"` when multiple equities strategies project the same IBKR account.
6. `signals` should show an IBKR reference market identity even when the IBKR gateway is unavailable; in that state, the reference prices may be empty or stale, but they must not mirror the Hyperliquid maker leg.
7. Clients should ignore unknown fields and tolerate additional metadata fields.
