# Equities trade[XYZ] Stock Universe Review

## Enrolled MakerV3 Stocks

The current checked-in `/equities` enroll list includes these exact-qualified stocks:

- `AAPL` -> `AAPL.NASDAQ`
- `AMD` -> `AMD.NASDAQ`
- `AMZN` -> `AMZN.NASDAQ`
- `BABA` -> `BABA.NYSE`
- `COIN` -> `COIN.NASDAQ`
- `CRCL` -> `CRCL.NYSE`
- `CRWV` -> `CRWV.NASDAQ`
- `GOOGL` -> `GOOGL.NASDAQ`
- `HOOD` -> `HOOD.NASDAQ`
- `HYUNDAI` -> `005380.KRX`
- `INTC` -> `INTC.NASDAQ`
- `META` -> `META.NASDAQ`
- `MSTR` -> `MSTR.NASDAQ`
- `MSFT` -> `MSFT.NASDAQ`
- `MU` -> `MU.NASDAQ`
- `NFLX` -> `NFLX.NASDAQ`
- `NVDA` -> `NVDA.NASDAQ`
- `ORCL` -> `ORCL.NYSE`
- `PLTR` -> `PLTR.NASDAQ`
- `RIVN` -> `RIVN.NASDAQ`
- `SNDK` -> `SNDK.NASDAQ`
- `TSM` -> `TSM.NYSE`
- `TSLA` -> `TSLA.NASDAQ`
- `USAR` -> `USAR.NASDAQ`

Each enrolled stock also carries a matching Hyperliquid `xyz:<SYMBOL>-USD-PERP.HYPERLIQUID` contract in `deploy/equities/equities.live.toml`.

## Excluded Non-Stock Products

These trade[XYZ] markets remain out of scope for the equities-stocks-only rollout:

- ETFs: `EWJ`, `EWY`, `URNM`
- FX: `EUR`, `USDJPY`
- Commodities: `BRENTOIL`, `COPPER`, `GOLD`, `NATGAS`, `PALLADIUM`, `PLATINUM`, `SILVER`
- Index / basket products: `XYZ100`

## Blocked / Unenrolled Stocks

These stock-like symbols are intentionally not enrolled yet:

- `SMSN`: likely Samsung Electronics, but exact IBKR qualification remains unresolved on this host
- `SKHX`: likely SK hynix, but exact IBKR qualification remains unresolved on this host

Do not guess these `instrument_id` values into the live allowlist until qualification is verified.
