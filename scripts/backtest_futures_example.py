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
from nautilus_trader.trading.strategy import Strategy, StrategyConfig

# Catalog path
CATALOG_PATH = Path("C:/projects/nautilus_trader/catalog")

# Instrument ID (must match what was written to catalog)
ETHUSDT_PERP_ID = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")


class SimpleStrategyConfig(StrategyConfig):
    """Configuration for simple strategy."""
    instrument_id: str = "ETHUSDT-PERP.BINANCE"


class SimpleStrategy(Strategy):
    """
    A simple example strategy that just logs trades.
    Replace this with your actual trading logic.
    """
    
    def __init__(self, config: SimpleStrategyConfig):
        super().__init__(config)
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.trade_count = 0
    
    def on_start(self):
        self.subscribe_trade_ticks(self.instrument_id)
        self.log.info(f"Strategy started, subscribed to {self.instrument_id}")
    
    def on_trade_tick(self, tick):
        self.trade_count += 1
        if self.trade_count <= 5:
            self.log.info(f"Trade #{self.trade_count}: {tick.price} @ {tick.size}")
        elif self.trade_count == 6:
            self.log.info("... (suppressing further trade logs)")
    
    def on_stop(self):
        self.log.info(f"Strategy stopped. Total trades received: {self.trade_count:,}")


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
    
    # Load trade ticks from catalog
    print("\n📊 Loading trade ticks from catalog...")
    ticks = catalog.trade_ticks(instrument_ids=[str(instrument.id)])
    print(f"✓ Loaded {len(ticks):,} trade ticks")
    
    if not ticks:
        print("❌ No trade ticks found! Run convert_to_parquet.py first.")
        return
    
    # Configure backtest engine
    config = BacktestEngineConfig(
        logging=LoggingConfig(log_level="INFO"),
    )
    engine = BacktestEngine(config=config)
    
    # Add venue for Binance USDT-M Futures
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,  # Futures use netting
        account_type=AccountType.MARGIN,  # Margin account for futures
        base_currency=USDT,
        starting_balances=[Money(100_000, USDT)],  # 100k USDT starting balance
        leverage=Decimal("20"),  # 20x leverage
    )
    print("✓ Added BINANCE venue (Futures: Netting, Margin, 20x leverage)")
    
    # Add instrument and data
    engine.add_instrument(instrument)
    engine.add_data(ticks)
    print("✓ Added instrument and trade tick data")
    
    # Add strategy
    strategy = SimpleStrategy(SimpleStrategyConfig())
    engine.add_strategy(strategy)
    print("✓ Added strategy")
    
    # Run backtest
    print("\n🚀 Running backtest...")
    print("-" * 60)
    engine.run()
    print("-" * 60)
    
    # Print results
    print("\n📈 Backtest Results:")
    print(f"  - Total trades processed: {strategy.trade_count:,}")
    
    # Cleanup
    engine.dispose()
    print("\n✅ Backtest complete!")


if __name__ == "__main__":
    main()

