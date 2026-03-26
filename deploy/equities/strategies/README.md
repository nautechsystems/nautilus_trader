# Equities `strategies` contract

This directory holds one TOML file per equities node process enrolled into the Pulse-managed
`flux@equities-node-*` services.

## File naming

- Use the exact Flux strategy ID as the file name: `<flux_strategy_id>.toml`.
- Recommended naming patterns for enrolled Tier 1 names are now `<stock>_tradexyz_maker.toml` and `<stock>_tradexyz_taker.toml`.
- Each enrolled variant uses one strategy file and one node process; the same stock may run both variants concurrently.
- Keep the active enrolled set aligned with `deploy/equities/equities.live.toml`.
- Disabled configs should use the `.toml.disabled` suffix until they are re-enrolled.
- The intended active target after the March 13, 2026 admission freeze is the split `maker` plus `taker` rollout on the Tier 1 core basket below. Second-wave and decommissioned names should stay disabled until a later re-admission or removal task says otherwise.
- The rollback file is `aapl_tradexyz_makerv3.toml.disabled`.
- Treat `aapl_tradexyz_makerv3.toml.disabled` as rollback material, not the active control-plane contract.
- Strategy-file swaps must not change the public shared-host GUI contract: on `tokenmm-api`, `/equities` still serves the shared Fluxboard shell and that shell must resolve assets from `/static/fluxboard/assets/*`, not `/tokenmm/assets/*`.
- The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`.

## March 13, 2026 Prod Hardening Universe Policy

- The checked-in active file set is pruned to the Tier 1 production basket below.
- Treat the categories below as the source of truth for which strategy ids are active, disabled for second-wave validation, or decommissioned from the first-wave production set. Additional disabled `*.toml.disabled` files may remain in the directory as historical or rollback material until later cleanup removes them.

### Tier 1 Core Basket

- `aapl_tradexyz_maker`
- `aapl_tradexyz_taker`
- `amd_tradexyz_maker`
- `amd_tradexyz_taker`
- `amzn_tradexyz_maker`
- `amzn_tradexyz_taker`
- `googl_tradexyz_maker`
- `googl_tradexyz_taker`
- `meta_tradexyz_maker`
- `meta_tradexyz_taker`
- `msft_tradexyz_maker`
- `msft_tradexyz_taker`
- `nvda_tradexyz_maker`
- `nvda_tradexyz_taker`
- `orcl_tradexyz_maker`
- `orcl_tradexyz_taker`
- `pltr_tradexyz_maker`
- `pltr_tradexyz_taker`
- `tsla_tradexyz_maker`
- `tsla_tradexyz_taker`

### Second-Wave Disabled Basket

- `coin_tradexyz_makerv3`
- `hood_tradexyz_makerv3`
- `intc_tradexyz_makerv3`
- `mu_tradexyz_makerv3`
- `nflx_tradexyz_makerv3`
- `rivn_tradexyz_makerv3`

### Immediate Decommission / Out-of-Scope Basket

- `baba_tradexyz_makerv3`
- `crcl_tradexyz_makerv3`
- `crwv_tradexyz_makerv3`
- `mstr_tradexyz_makerv3`
- `sndk_tradexyz_makerv3`
- `tsm_tradexyz_makerv3`
- `usar_tradexyz_makerv3`

### Admission Policy for Any Future Re-Add

1. US-primary listed common stock only for Tier 1; no ADR / non-US-primary exposure in the first-wave prod basket.
2. Liquidity must be measured, not guessed: require a documented 30-day median daily dollar-volume floor before re-admission.
3. The name must have reliable reference data on IBKR and stable maker data on Hyperliquid for at least one full trading session in read-only mode.
4. The name must be free of recent launch / corporate-action / special-situation churn that would distort a first-wave canary.

## Required TOML keys per file

- `[identity].strategy_id` and `[identity].strategy_instance_id` stay aligned to the file name.
- `[strategy].strategy_id` stays descriptive and unique across node processes.
- `[strategy].strategy_groups` stays `equities`.
- `[strategy].param_set = "equities_maker"` or `"equities_taker"` stays explicit for the intended active equities rollout.
- `[strategy].manage_stop = false` stays explicit in the checked-in live equities configs; flatten-on-stop is opt-in only and must be set per strategy when explicitly desired.
- `[venues].execution_venue` stays `HYPERLIQUID` and `[venues].reference_venue` stays `IBKR`.
- `[node.venues.HYPERLIQUID].instrument_id` defines the trade[XYZ] builder-perp instrument.
- `[node.venues.IBKR].instrument_id` defines the IBKR reference instrument, for example `AAPL.NASDAQ` or `USAR.NASDAQ`.
- `[node.venues.IBKR].use_regular_trading_hours = false` keeps IBKR reference data available outside RTH on the split maker/taker contract.
- `[strategy].outside_rth_hedge_enabled = true` enables the session-aware overnight hedge policy.
- `[strategy].ibkr_primary_exchange` must match the listing venue used for the enrolled IBKR reference instrument.
- `[node.venues.IBKR.dockerized_gateway]` is now a non-owning client contract for enrolled nodes.
- `[node.venues.IBKR.dockerized_gateway].manage_container = false` keeps node processes from starting or restarting the shared IBKR gateway.
- The only equities gateway owner lives in shared config under `ibkr.reference.main`; nodes connect to that gateway but do not manage 2FA policy.
- `[node.venues.HYPERLIQUID].dex = "xyz"` stays explicit.
- `[node.venues.HYPERLIQUID].private_key_env` and `account_address_env` must reference env var names, not inline secrets.
- Keep the shared `[[contracts]]` IBKR entries aligned with the enrolled reference instruments, because the `/equities` API contract catalog is built from `deploy/equities/equities.live.toml`.
- In practice, that means each enrolled strategy file must keep the shared IBKR contract entry set in sync.
- Keep the shared `[[contracts]]` IBKR entry aligned with the active canary reference instrument before promoting that route into the enrolled stock set.
- Hyperliquid effective account identity resolves in this order: `vault_address_env`, then funded `account_address_env`, then the agent wallet's `userRole`-resolved master account.
- Do not duplicate `[redis]` in per-node deploy files; nodes inherit it from `deploy/equities/equities.live.toml`.
- Do not duplicate `[portfolio]` in per-node deploy files; nodes inherit the shared portfolio inventory feed from `deploy/equities/equities.live.toml`.
- `TRADE_XYZ_VAULT_ADDRESS` should be supplied in `/etc/flux/common.env` when vault routing is required.
- `SMSN` and `SKHX` remain intentionally unenrolled until exact IBKR qualification is verified.

## Inventory semantics

- `local_qty` is strategy-local inventory for that stock.
- `global_qty` is the shared `equities` portfolio aggregate for the stock portfolio.
- Each node publishes a strategy inventory component to Redis.
- `flux.runners.equities.run_portfolio` aggregates those components into the shared portfolio inventory feed consumed by all equities strategies.

## Env conventions

- Production node lifecycle is managed from Pulse via flux@ units.
- Production secrets should be provided through `/etc/flux/common.env` plus `/etc/flux/equities-node-*.env`.
- Required trade[XYZ] env vars are `TRADE_XYZ_AGENT_PK` and `TRADE_XYZ_ACCOUNT_ADDRESS`.
- Optional vault routing env var is `TRADE_XYZ_VAULT_ADDRESS`.
- Local smoke with the checked-in `node.venues.IBKR.dockerized_gateway` contract also requires `TWS_USERNAME` and `TWS_PASSWORD`.
- Set `TRADE_XYZ_ACCOUNT_ADDRESS` to the funded master account when the configured private key belongs to an agent wallet.
- If vault trading is enabled, provide `vault_address_env` and it will take precedence for account-state queries, fee lookup, and WS subscriptions.
- `deploy/equities/equities_stack.env` is for local paper/testnet smoke only.
- Use the same `[flux].namespace` and `[flux].schema_version` as the shared API/bridge config.
- Pulse-managed node services pass `--shared-config deploy/equities/equities.live.toml` so node runners inherit the shared `[redis]`, `[portfolio]`, `[[strategy_contracts]]`, and `[[account_scopes]]` contract tables.

Each file is a complete node config consumed directly by `python -m flux.runners.equities.run_node`.
Start from `equities.strategy.template.toml`.
