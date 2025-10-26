# Bybit Options Data Collector

This script discovers all available options for a given underlying asset (e.g., BTC) on Bybit,
subscribes to their quote and orderbook data, and stores the data in parquet files.

## Features

- **Automatic Discovery**: Connects to Bybit and discovers all available options for the
  specified underlying asset.
- **Complete Data Collection**: Fetches both quote ticks and orderbook deltas for all discovered options.
- **Spot Data**: Also collects data for the underlying spot instrument.
- **Organized Storage**: Saves data in a hierarchical directory structure with separate files
  for each instrument.
- **Real-time Processing**: Processes and stores data in real-time with configurable batch sizes.
- **Comprehensive Logging**: Provides detailed logging of discovered instruments and data
  collection statistics.

## Requirements

- Python 3.8+
- NautilusTrader
- Bybit API credentials (for live data access)

## Setup

1. Set your Bybit API credentials as environment variables:

```bash
export BYBIT_API_KEY="your_api_key"
export BYBIT_API_SECRET="your_api_secret"
```

2. Run the script:

```bash
python examples/live/bybit/bybit_options_data_collector.py
```

## Configuration

The script can be configured by modifying the `BybitOptionsDataCollectorConfig` in the script:

```python
strategy_config = BybitOptionsDataCollectorConfig(
    underlying_asset="BTC",  # The underlying asset to collect options for
    spot_instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
    depth=25,  # Orderbook depth (25 or 100 levels)
    batch_size=1000,  # Records to batch before writing to parquet
    data_dir="data",  # Directory to store parquet files
    log_interval=60.0,  # Log and save data every N seconds
)
```

## Data structure

The script creates the following directory structure:

```
data/
└── BTC/
    └── USDT/
        ├── spot/
        │   ├── BTCUSDT-SPOT_BYBIT_quote.parquet
        │   └── BTCUSDT-SPOT_BYBIT_orderbook.parquet
        └── options/
            ├── BTC-15SEP26-45000-C-USDT-OPTION_BYBIT_quote.parquet
            ├── BTC-15SEP26-45000-C-USDT-OPTION_BYBIT_orderbook.parquet
            ├── BTC-15SEP26-45000-P-USDT-OPTION_BYBIT_quote.parquet
            ├── BTC-15SEP26-45000-P-USDT-OPTION_BYBIT_orderbook.parquet
            └── ... (one file pair per option)
```

## Data formats

### Quote ticks data

Each quote tick record contains:

- `timestamp`: When the data was received
- `instrument_id`: The instrument identifier
- `bid_price`: Best bid price
- `ask_price`: Best ask price
- `bid_size`: Best bid size
- `ask_size`: Best ask size
- `ts_event`: Event timestamp
- `ts_init`: Initialization timestamp

### Orderbook deltas data

Each orderbook delta record contains:

- `timestamp`: When the data was received
- `instrument_id`: The instrument identifier
- `sequence`: Sequence number
- `delta_count`: Number of deltas in this update
- `best_bid`: Best bid price after applying deltas
- `best_ask`: Best ask price after applying deltas
- `bid_size`: Best bid size after applying deltas
- `ask_size`: Best ask size after applying deltas

## Usage examples

### Basic usage

```bash
# Collect data for all BTC options
python examples/live/bybit/bybit_options_data_collector.py
```

### Custom configuration

You can modify the script to:

- Change the underlying asset (e.g., ETH instead of BTC).
- Adjust the orderbook depth.
- Change the data storage directory.
- Modify the logging interval.

## Monitoring

The script provides real-time monitoring through logs:

- Discovery phase: Shows all found options grouped by expiry.
- Data collection: Shows statistics every 60 seconds (configurable).
- File operations: Logs when data is written to parquet files.

## Output example

```
2025-07-31T14:47:55.003958000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: Discovering all available BTC options...
2025-07-31T14:47:55.004032000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: Found 150 BTC options instruments
2025-07-31T14:47:55.004097000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: BTC options grouped by expiry (8 expiry dates):
2025-07-31T14:47:55.004159000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector:   15SEP26: 25 options
2025-07-31T14:47:55.004247000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector:   15OCT26: 25 options
2025-07-31T14:47:55.004300000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector:   15NOV26: 25 options
...
2025-07-31T14:47:55.004357000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: Total BTC options to monitor: 150
2025-07-31T14:47:55.004413000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: === 60.0 SECOND UPDATE ===
2025-07-31T14:47:55.004470000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: BTC-15SEP26-45000-C-USDT-OPTION.BYBIT: 150 quotes, 41 deltas
2025-07-31T14:47:55.004503000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: BTC-15SEP26-45000-P-USDT-OPTION.BYBIT: 148 quotes, 137 deltas
...
2025-07-31T14:47:55.004515000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: BTCUSDT-SPOT.BYBIT: 1076 quotes, 523 deltas
2025-07-31T14:47:55.006441000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: TOTAL: 15000 quotes, 8000 deltas
2025-07-31T14:47:55.006517000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: OPTIONS: 13924 quotes, 7477 deltas
2025-07-31T14:47:55.006522000Z [INFO] OPTIONS-COLLECTOR-001.BybitOptionsDataCollector: SPOT: 1076 quotes, 523 deltas
```

## Notes

- The script will automatically discover and subscribe to all available options for the
  specified underlying asset.
- Data is stored in parquet format for efficient storage and querying.
- The script handles connection issues and will attempt to reconnect.
- All data is timestamped and can be used for backtesting or analysis.
- The script can be stopped with Ctrl+C and will save any remaining data before exiting.

## Troubleshooting

1. **No options found**: Check that the underlying asset is correct and that Bybit has options
   available for that asset.
2. **Connection issues**: Verify your API credentials and internet connection.
3. **Memory issues**: Reduce the batch_size if collecting data for many options.
4. **Disk space**: Monitor disk usage as the script can generate large amounts of data.
