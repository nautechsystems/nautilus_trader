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

The equities rollout keeps trade[XYZ] on `HYPERLIQUID` plus `dex = "xyz"`.
It does not define a separate venue family for the equities surface.

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
4. Clients should ignore unknown fields and tolerate additional metadata fields.
