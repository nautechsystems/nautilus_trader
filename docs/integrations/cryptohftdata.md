# CryptoHFTData

[CryptoHFTData](https://www.cryptohftdata.com/) provides historical high-frequency crypto market data as hourly Parquet files. NautilusTrader integrates with CHD through a Rust-native adapter that downloads `.parquet.zst` files, decodes them through Arrow, converts rows into Nautilus model objects, and can write directly to a Nautilus Parquet catalog.

The adapter is historical-only. It does not depend on the CHD Python SDK and does not provide live trading or live market data streams.

## Access and pricing

CHD is free to use. After signing up for a CHD account, users receive an API key that can be used with this adapter for unrestricted historical data downloads under CHD's current free access policy.

Create an account at the [CHD signup page](https://www.cryptohftdata.com/signup), then provide the key through the `CRYPTOHFTDATA_API_KEY` environment variable or a local, untracked ingest config.

## Capabilities

- `CryptoHFTDataClient`: authenticated CHD file download client.
- `CryptoHFTDataDataLoader`: local `.parquet` / `.parquet.zst` loader for CHD files.
- `run_cryptohftdata_ingest_from_config`: direct CHD-to-catalog ingestion from JSON config.
- Custom catalog data types for CHD open interest and liquidation events.

## Data mapping

| CHD data type | Nautilus output |
| --- | --- |
| `trades` | `TradeTick` |
| `orderbook` | `OrderBookDelta` |
| `klines` | 1-minute external `Bar` |
| `mark_price` | `MarkPriceUpdate`, `IndexPriceUpdate`, `FundingRateUpdate` |
| `open_interest` | `CryptoHFTDataOpenInterest` custom data |
| `liquidations` | `CryptoHFTDataLiquidation` custom data |

`ticker` files are intentionally not converted in the first adapter version. Prefer trades, bars, order book, and mark/index/funding datasets for Nautilus workflows.

## Environment variables

- `CRYPTOHFTDATA_API_KEY`: CHD API key for dataset downloads.
- `NAUTILUS_PATH`: optional parent directory containing `catalog/`; used when an ingest config does not set `output_path`.

Never store API keys in repository configs. Use environment variables or local untracked files.

By default the Rust client mirrors the CHD SDK auth flow: it requests a short-lived JWT from `/jwt-token` with `X-API-Key`, uses that bearer token for downloads, refreshes it before expiry, and falls back to `X-API-Key` if token generation fails. Set `use_jwt` to `false` only when debugging auth issues with CHD support.

## Python loading

```python
from nautilus_trader.adapters.cryptohftdata import CryptoHFTDataDataLoader

loader = CryptoHFTDataDataLoader()

trades = loader.load_trades(
    path="/data/chd/binance_futures/2025-07-16/00/AAVEUSDT_trades.parquet.zst",
    exchange="binance_futures",
    symbol="AAVEUSDT",
)

deltas = loader.load_order_book_deltas(
    path="/data/chd/binance_futures/2025-07-16/00/AAVEUSDT_orderbook.parquet.zst",
    exchange="binance_futures",
    symbol="AAVEUSDT",
)
```

## Catalog ingest

Create a JSON config:

```json
{
  "exchange": "binance_futures",
  "symbols": ["AAVEUSDT"],
  "data_types": ["trades", "orderbook", "mark_price", "open_interest"],
  "from": "2025-07-16",
  "to": "2025-07-16",
  "output_path": "/tmp/nautilus/catalog",
  "cache_dir": "/tmp/chd-cache",
  "max_concurrent_downloads": 4,
  "use_jwt": true,
  "compression": "zstd",
  "gap_policy": "error"
}
```

Run from Rust:

```bash
export CRYPTOHFTDATA_API_KEY="..."
cargo run -p nautilus-cryptohftdata --bin cryptohftdata-ingest -- chd_config.json
```

Or from Python:

```python
from nautilus_trader.adapters.cryptohftdata import run_cryptohftdata_ingest_from_config

await run_cryptohftdata_ingest_from_config("chd_config.json")
```

`output_path` is the catalog root. The catalog writer creates files under `output_path/data/...`.

## Example notebook

See `examples/backtest/notebooks/cryptohftdata_ethusdt_orderbook_imbalance.py` for a Jupytext getting-started notebook that downloads all 24 CHD Binance Futures `ETHUSDT` L2 order book files for May 1, 2026, samples top-of-book quotes from the maintained L2 book, runs the built-in `OrderBookImbalance` example strategy, and plots cumulative plus hourly intraday PnL.

The repository also includes the executed `.ipynb` output so the demo results can be viewed without rerunning the CHD download.

## Supported CHD exchanges

The adapter currently recognizes:

- `binance_spot`, `binance_futures`
- `bybit_spot`, `bybit`
- `kraken_spot`, `kraken_derivatives`
- `okx_spot`, `okx_futures`
- `bitget_spot`, `bitget_futures`
- `hyperliquid_spot`, `hyperliquid_futures`
- `lighter`, `aster_futures`, `bitmex`

These exchange IDs map to existing Nautilus venue IDs where possible. New CHD venues such as Lighter and Aster use their own venue IDs.
