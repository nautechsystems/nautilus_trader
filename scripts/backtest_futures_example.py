"""
Example backtest script for ETHUSDT Perpetual Futures using NautilusTrader.

This script demonstrates how to:
1. Load trade tick data from ParquetDataCatalog
2. Configure a venue for USDT-M Futures (margin trading)
3. Run a simple momentum strategy
"""
import sys
from decimal import Decimal
from pathlib import Path

# Add nautilus_trader to path
NAUTILUS_PATH = Path("C:/projects/nautilus_trader")
sys.path.insert(0, str(NAUTILUS_PATH))

from nautilus_trader.backtest.engine import BacktestEngine, BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType, OmsType
from nautilus_trader.model.identifiers import InstrumentId, Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog import ParquetDataCatalog

from nautilus_trader.examples.strategies.orderflow_strategy import (
    OrderFlowStrategy,
    OrderFlowStrategyConfig,
)

# Catalog path
CATALOG_PATH = Path("C:/projects/nautilus_trader/catalog")

# Instrument ID (must match what was written to catalog)
ETHUSDT_PERP_ID = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")





def main():
    print("=" * 60)
    print("NautilusTrader Futures Backtest Example")
    print("=" * 60)
    
    # Load catalog
    catalog = ParquetDataCatalog(str(CATALOG_PATH))
    print(f"✓ Loaded catalog from: {CATALOG_PATH}")
    
    # Get instrument from catalog
    instruments = catalog.instruments()
    if not instruments:
        print("❌ No instruments found in catalog! Run convert_to_parquet.py first.")
        return
    
    instrument = instruments[0]
    print(f"✓ Found instrument: {instrument.id}")
    
    # Load trade ticks from catalog (first week for testing)
    print("\n📊 Loading trade ticks from catalog...")
    import pandas as pd
    ticks = catalog.trade_ticks(
        instrument_ids=[str(instrument.id)],
        start=pd.Timestamp('2025-03-01', tz='UTC'),
        end=pd.Timestamp('2025-03-02', tz='UTC'),  # First week
    )
    print(f"✓ Loaded {len(ticks):,} trade ticks (March 1-7)")
    
    if not ticks:
        print("❌ No trade ticks found! Run convert_to_parquet.py first.")
        return
    
    # Configure backtest engine
    config = BacktestEngineConfig(
        logging=LoggingConfig(log_level="ERROR"),  # ERROR to avoid logging spam (millions of POI checks)
    )
    engine = BacktestEngine(config=config)
    
    # Add venue for Binance USDT-M Futures
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,  # Futures use netting
        account_type=AccountType.MARGIN,  # Margin account for futures
        base_currency=USDT,
        starting_balances=[Money(100_000, USDT)],  # 100k USDT starting balance
        default_leverage=Decimal("20"),  # 20x leverage
        trade_execution=True,  # CRITICAL: Fast tick processing for L1_MBP book
    )
    print("✓ Added BINANCE venue (Futures: Netting, Margin, 20x leverage)")
    
    # Add instrument and data
    engine.add_instrument(instrument)
    engine.add_data(ticks)
    print("✓ Added instrument and trade tick data")

    # Get tick size from instrument
    tick_size = float(instrument.price_increment)

    # Configure the Order Flow Strategy
    # POI-based trading with orderflow confirmation
    strategy_config = OrderFlowStrategyConfig(
        instrument_id=instrument.id,
        tick_size=tick_size,
        trade_size=Decimal("10.0"),        # 10 ETH position (~$22k notional)
        poi_tolerance=5.0,                 # Within 5 ticks of POI to trigger
        warmup_ticks=1000,                 # Wait 1000 ticks for indicator warmup
        # Risk Management
        tp_pct=0.30,                       # 0.3% take profit
        sl_pct=0.30,                       # 0.3% stop loss
        trailing_activation_pct=0.25,      # 0.25% to activate trailing stop
        trailing_offset_pct=0.10,          # 0.1% trailing offset
        use_emulated_orders=True,          # Required for backtest
    )

    # Instantiate and add the strategy
    strategy = OrderFlowStrategy(config=strategy_config)
    engine.add_strategy(strategy)
    print("✓ Added OrderFlow strategy")
    
    # Run backtest
    print("\n🚀 Running backtest...")
    print("-" * 60)
    engine.run()
    print("-" * 60)

    # Print results
    print("\n📈 Backtest Results:")
    print(f"  - Total ticks processed: {strategy._tick_count:,}")
    print(f"  - Total trades executed: {strategy._trade_count:,}")

    # Generate tearsheet
    try:
        from nautilus_trader.analysis import TearsheetConfig
        from nautilus_trader.analysis.tearsheet import create_tearsheet

        print("\n📊 Generating tearsheet...")

        tearsheet_config = TearsheetConfig(theme="plotly_white")

        create_tearsheet(
            engine=engine,
            output_path="ethusdt_backtest_tearsheet.html",
            config=tearsheet_config,
        )
        print("✓ Tearsheet saved to: ethusdt_backtest_tearsheet.html")
    except ImportError:
        print("\n⚠️ Plotly not installed. Install with: pip install plotly>=6.3.1")

    # Cleanup
    engine.reset()
    engine.dispose()
    print("\n✅ Backtest complete!")


if __name__ == "__main__":
    main()

