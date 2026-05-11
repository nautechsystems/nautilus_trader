# CryptoHFTData

Rust-native adapter for ingesting CryptoHFTData (CHD) historical crypto market data into NautilusTrader data objects and Parquet catalogs.

The adapter intentionally avoids the CHD Python SDK in the hot path. It downloads CHD hourly `.parquet.zst` files, decodes them through Arrow/Parquet, converts rows to Nautilus model types, and can write directly to the Nautilus catalog layout.

## Quick Start

```bash
export CRYPTOHFTDATA_API_KEY="..."
cargo run -p nautilus-cryptohftdata --bin cryptohftdata-ingest -- ./chd_config.json
```

```json
{
  "exchange": "binance_futures",
  "symbols": ["AAVEUSDT"],
  "data_types": ["trades"],
  "from": "2025-07-16",
  "to": "2025-07-16",
  "output_path": "/tmp/nautilus/catalog",
  "cache_dir": "/tmp/chd-cache",
  "max_concurrent_downloads": 4,
  "use_jwt": true
}
```

`output_path` is the catalog root. Files are written under `output_path/data/...`.

The client requests CHD JWT bearer tokens by default and refreshes them before expiry. Set `use_jwt` to `false` only when explicitly testing API-key header fallback behavior.
