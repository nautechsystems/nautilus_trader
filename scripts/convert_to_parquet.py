"""
Convert ETHUSDT trades CSV to Parquet format for NautilusTrader.

This script:
1. Reads the large CSV in chunks to avoid memory issues
2. Creates a ParquetDataCatalog in nautilus_trader directory
3. Writes TradeTick data with CryptoPerpetual instrument (futures, not spot)
"""
import sys
from decimal import Decimal
from pathlib import Path

import pandas as pd

# Add nautilus_trader to path
NAUTILUS_PATH = Path("C:/projects/nautilus_trader")
sys.path.insert(0, str(NAUTILUS_PATH))

from nautilus_trader.model.currencies import ETH, USDT
from nautilus_trader.model.identifiers import InstrumentId, Symbol, Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Money, Price, Quantity
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler

# Paths
CSV_PATH = Path("C:/projects/binance-bulk-downloader/data/nautilus/ethusdt-trades.csv")
CATALOG_PATH = Path("C:/projects/nautilus_trader/catalog")

# Chunk size for reading CSV (adjust based on available RAM)
CHUNK_SIZE = 5_000_000  # 5 million rows per chunk


def create_ethusdt_perp_instrument() -> CryptoPerpetual:
    """Create ETHUSDT perpetual futures instrument for Binance."""
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("ETHUSDT-PERP"),
            venue=Venue("BINANCE"),
        ),
        raw_symbol=Symbol("ETHUSDT"),
        base_currency=ETH,
        quote_currency=USDT,
        settlement_currency=USDT,
        is_inverse=False,
        price_precision=2,
        size_precision=3,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.001"),
        max_quantity=Quantity.from_str("10000.000"),
        min_quantity=Quantity.from_str("0.001"),
        max_notional=None,
        min_notional=Money(10.00, USDT),
        max_price=Price.from_str("152588.43"),
        min_price=Price.from_str("29.91"),
        margin_init=Decimal("0.05"),  # 5% = 20x leverage
        margin_maint=Decimal("0.025"),  # 2.5% maintenance
        maker_fee=Decimal("0.0002"),  # 0.02%
        taker_fee=Decimal("0.0004"),  # 0.04%
        ts_event=0,
        ts_init=0,
    )


def main():
    print("=" * 60)
    print("CSV to Parquet Converter for NautilusTrader")
    print("=" * 60)
    
    # Create catalog directory
    CATALOG_PATH.mkdir(parents=True, exist_ok=True)
    print(f"\n✓ Catalog directory: {CATALOG_PATH}")
    
    # Create instrument
    instrument = create_ethusdt_perp_instrument()
    print(f"✓ Created instrument: {instrument.id}")
    
    # Initialize catalog
    catalog = ParquetDataCatalog(str(CATALOG_PATH))
    print(f"✓ Initialized ParquetDataCatalog")
    
    # Write instrument to catalog
    catalog.write_data([instrument])
    print(f"✓ Written instrument to catalog")
    
    # Create wrangler
    wrangler = TradeTickDataWrangler(instrument=instrument)
    
    # Count total rows for progress
    print(f"\n📊 Counting rows in CSV (this may take a moment)...")
    total_rows = sum(1 for _ in open(CSV_PATH, 'r')) - 1  # -1 for header
    print(f"✓ Total rows: {total_rows:,}")
    
    # Process in chunks
    print(f"\n🔄 Processing CSV in chunks of {CHUNK_SIZE:,} rows...")
    chunk_num = 0
    processed_rows = 0

    for chunk in pd.read_csv(
        CSV_PATH,
        chunksize=CHUNK_SIZE,
        parse_dates=['timestamp'],
        index_col='timestamp',
    ):
        chunk_num += 1
        chunk_size = len(chunk)
        processed_rows += chunk_size

        # Wrangle to TradeTick objects
        ticks = wrangler.process(chunk)

        # Write to catalog (skip disjoint check since we're writing sequential chunks)
        catalog.write_data(ticks, skip_disjoint_check=True)

        progress = (processed_rows / total_rows) * 100
        print(f"  Chunk {chunk_num}: {chunk_size:,} rows | Total: {processed_rows:,}/{total_rows:,} ({progress:.1f}%)")
    
    print(f"\n{'=' * 60}")
    print(f"✅ COMPLETE!")
    print(f"  - Processed: {processed_rows:,} trade ticks")
    print(f"  - Catalog location: {CATALOG_PATH}")
    print(f"  - Instrument: ETHUSDT-PERP.BINANCE (CryptoPerpetual)")
    print(f"{'=' * 60}")


if __name__ == "__main__":
    main()

