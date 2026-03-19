# Equities `strategies` contract

This directory holds one TOML file per equities node process enrolled into the Pulse-managed
`flux@equities-node-*` services.

## File naming

- Use the exact Flux strategy ID as the file name: `<flux_strategy_id>.toml`.
- Recommended naming pattern for Hyperliquid routes is `<stock>_tradexyz_makerv4.toml`.
- One strategy route uses one strategy file and one node process.
- Multiple strategy routes can share one canonical stock bucket in shared config.
- Keep the active enrolled set aligned with `deploy/equities/equities.live.toml`.
- Treat `deploy/equities/equities.live.toml` as the route registry and `deploy/equities/strategies/*.toml` as the active node set.
- The checked-in equities strategy set is fully `maker_v4`; dead `maker_v3` files have been removed from discovery.
- Strategy-file swaps must not change the public shared-host GUI contract: on `tokenmm-api`, `/equities` still serves the shared Fluxboard shell and that shell must resolve assets from `/static/fluxboard/assets/*`, not `/tokenmm/assets/*`.
- The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`.

## Enrolled MakerV4 Routes

These are the checked-in node TOMLs that Pulse-managed discovery should enroll.

### Enrolled Hyperliquid Routes

- `aapl_tradexyz_makerv4`
- `amd_tradexyz_makerv4`
- `amzn_tradexyz_makerv4`
- `baba_tradexyz_makerv4`
- `coin_tradexyz_makerv4`
- `crcl_tradexyz_makerv4`
- `crwv_tradexyz_makerv4`
- `googl_tradexyz_makerv4`
- `hood_tradexyz_makerv4`
- `intc_tradexyz_makerv4`
- `meta_tradexyz_makerv4`
- `msft_tradexyz_makerv4`
- `mstr_tradexyz_makerv4`
- `mu_tradexyz_makerv4`
- `nflx_tradexyz_makerv4`
- `nvda_tradexyz_makerv4`
- `orcl_tradexyz_makerv4`
- `pltr_tradexyz_makerv4`
- `rivn_tradexyz_makerv4`
- `sndk_tradexyz_makerv4`
- `tsla_tradexyz_makerv4`
- `tsm_tradexyz_makerv4`
- `usar_tradexyz_makerv4`

### Enrolled Binance Routes

- `amzn_binance_perp_makerv4`
- `coin_binance_perp_makerv4`
- `crcl_binance_perp_makerv4`
- `ewy_binance_perp_makerv4`
- `hood_binance_perp_makerv4`
- `intc_binance_perp_makerv4`
- `mstr_binance_perp_makerv4`
- `pltr_binance_perp_makerv4`
- `tsla_binance_perp_makerv4`

### Removed MakerV3 Files

- `aapl_tradexyz_makerv3`
- `baba_tradexyz_makerv3`
- `coin_tradexyz_makerv3`
- `crcl_tradexyz_makerv3`
- `crwv_tradexyz_makerv3`
- `hood_tradexyz_makerv3`
- `hyundai_tradexyz_makerv3`
- `intc_tradexyz_makerv3`
- `mstr_tradexyz_makerv3`
- `mu_tradexyz_makerv3`
- `nflx_tradexyz_makerv3`
- `rivn_tradexyz_makerv3`
- `sndk_tradexyz_makerv3`
- `tsm_tradexyz_makerv3`
- `usar_tradexyz_makerv3`

## Required TOML keys per file

- `[identity].strategy_id` and `[identity].strategy_instance_id` stay aligned to the file name.
- `[strategy].strategy_id` stays descriptive and unique across node processes.
- `[strategy].strategy_groups` stays `equities`.
- `[strategy].param_set = "makerv4"` stays explicit for the intended active equities rollout.
- `[strategy].manage_stop = false` stays explicit in the checked-in live equities configs; flatten-on-stop is opt-in only and must be set per strategy when explicitly desired.
- `[strategy].max_ibkr_quote_age_ms = 60000` is the checked-in equities default so Signal treats quiet-but-valid IBKR books as healthy while still failing closed on truly stale data.
- `[venues].execution_venue` must match the maker route (`HYPERLIQUID` or `BINANCE_PERP`) and `[venues].reference_venue` stays `IBKR`.
- `[node.venues.HYPERLIQUID].instrument_id` defines the trade[XYZ] builder-perp instrument for Hyperliquid routes.
- `[node.venues.BINANCE_PERP].instrument_id` defines the Binance USD-M equity-perp instrument for Binance routes.
- `[node.venues.BINANCE_PERP].api_key_env` and `api_secret_env` must reference `EQUITIES_BINANCE_*` env vars, not inline secrets.
- Portfolio Margin Binance accounts must set `[node.venues.BINANCE_PERP].private_api_family = "PORTFOLIO_MARGIN"` so private account/order/user-stream traffic routes to Binance `papi` while public market data remains on the normal futures market-data path.
- `[node.venues.IBKR].instrument_id` defines the IBKR reference instrument, for example `AAPL.NASDAQ` or `USAR.NASDAQ`.
- `[node.venues.IBKR].use_regular_trading_hours = false` keeps IBKR reference data available outside RTH on the MakerV4 contract.
- `[strategy].outside_rth_hedge_enabled = true` enables the session-aware overnight hedge policy.
- `[strategy].ibkr_primary_exchange` must match the listing venue used for the enrolled IBKR reference instrument.
- `[node.venues.IBKR.dockerized_gateway]` is now a non-owning client contract for enrolled nodes.
- `[node.venues.IBKR.dockerized_gateway].manage_container = false` keeps node processes from starting or restarting the shared IBKR gateway.
- The only equities gateway owner lives in shared config under `ibkr.reference.main`; nodes connect to that gateway but do not manage 2FA policy.
- `[node.venues.HYPERLIQUID].dex = "xyz"` stays explicit.
- `[node.venues.HYPERLIQUID].private_key_env` and `account_address_env` must reference env var names, not inline secrets.
- Keep the shared `[[contracts]]` IBKR entries aligned with the enrolled reference instruments, because the `/equities` API contract catalog is built from `deploy/equities/equities.live.toml`.
- In practice, that means each enrolled strategy file must keep the shared IBKR contract entry set in sync.
- Keep the shared `[[contracts]]` IBKR entry aligned with the active enrolled reference instrument set before restart.
- Hyperliquid effective account identity resolves in this order: `vault_address_env`, then funded `account_address_env`, then the agent wallet's `userRole`-resolved master account.
- Do not duplicate `[redis]` in per-node deploy files; nodes inherit it from `deploy/equities/equities.live.toml`.
- Do not duplicate `[portfolio]` in per-node deploy files; nodes inherit the shared portfolio inventory feed from `deploy/equities/equities.live.toml`.
- `TRADE_XYZ_VAULT_ADDRESS` should be supplied in `/etc/flux/common.env` when vault routing is required.
- `SMSN` and `SKHX` remain intentionally unenrolled until exact IBKR qualification is verified.

## Inventory semantics

- `local_qty` is strategy-local inventory for that stock.
- `global_qty` is the shared `equities` portfolio aggregate for the stock portfolio.
- Shared portfolio/risk nets by canonical `portfolio_asset_id`, not by maker venue or strategy file name.
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
