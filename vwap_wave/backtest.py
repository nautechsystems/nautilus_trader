# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Backtest Configuration
# -------------------------------------------------------------------------------------------------
"""
Backtesting configuration and runner for VWAP Wave strategy.

Provides utilities for running backtests with various data sources and
generating performance reports.
"""

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from pathlib import Path
from typing import Optional

from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money

from vwap_wave.config.settings import VWAPWaveConfig
from vwap_wave.strategy import VWAPWaveStrategy
from vwap_wave.strategy import VWAPWaveStrategyConfig


def create_backtest_engine(
    trader_id: str = "VWAP-WAVE-001",
    log_level: str = "INFO",
) -> BacktestEngine:
    """
    Create a configured backtest engine.

    Parameters
    ----------
    trader_id : str
        The trader ID for the backtest.
    log_level : str
        The logging level.

    Returns
    -------
    BacktestEngine
        The configured backtest engine.

    """
    config = BacktestEngineConfig(
        trader_id=TraderId(trader_id),
        logging=LoggingConfig(log_level=log_level),
    )

    return BacktestEngine(config=config)


def add_forex_venue(
    engine: BacktestEngine,
    venue_name: str = "OANDA",
    starting_balance: Decimal = Decimal("100000"),
) -> Venue:
    """
    Add a forex venue to the backtest engine.

    Parameters
    ----------
    engine : BacktestEngine
        The backtest engine.
    venue_name : str
        The venue name.
    starting_balance : Decimal
        The starting account balance in USD.

    Returns
    -------
    Venue
        The added venue.

    """
    venue = Venue(venue_name)

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(starting_balance, USD)],
    )

    return venue


def add_crypto_venue(
    engine: BacktestEngine,
    venue_name: str = "BINANCE",
    starting_balance: Decimal = Decimal("100000"),
) -> Venue:
    """
    Add a crypto venue to the backtest engine.

    Parameters
    ----------
    engine : BacktestEngine
        The backtest engine.
    venue_name : str
        The venue name.
    starting_balance : Decimal
        The starting account balance in USDT.

    Returns
    -------
    Venue
        The added venue.

    """
    venue = Venue(venue_name)

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[Money(starting_balance, USDT)],
    )

    return venue


def create_bar_type(
    instrument_id: InstrumentId,
    aggregation_minutes: int = 15,
) -> BarType:
    """
    Create a bar type for the given instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    aggregation_minutes : int
        The bar aggregation in minutes.

    Returns
    -------
    BarType
        The bar type.

    """
    bar_spec = BarSpecification(
        step=aggregation_minutes,
        aggregation=BarAggregation.MINUTE,
        price_type=PriceType.MID,
    )

    return BarType(instrument_id, bar_spec)


def run_vwap_wave_backtest(
    engine: BacktestEngine,
    instrument_id: InstrumentId,
    bar_type: BarType,
    config: Optional[VWAPWaveConfig] = None,
) -> dict:
    """
    Run a VWAP Wave strategy backtest.

    Parameters
    ----------
    engine : BacktestEngine
        The configured backtest engine with venue and data.
    instrument_id : InstrumentId
        The instrument to trade.
    bar_type : BarType
        The bar type to use.
    config : VWAPWaveConfig, optional
        The strategy configuration.

    Returns
    -------
    dict
        Backtest results including account report, positions, and orders.

    """
    if config is None:
        config = VWAPWaveConfig()

    # Create strategy config
    strategy_config = VWAPWaveStrategyConfig(
        instrument_id=instrument_id,
        bar_type=bar_type,
        vwap_wave_config=config,
    )

    # Create strategy instance
    strategy = VWAPWaveStrategy(strategy_config)

    # Add strategy to engine
    engine.add_strategy(strategy)

    # Run backtest
    engine.run()

    # Get venue for reports
    venue = instrument_id.venue

    # Generate reports
    results = {
        "account_report": engine.trader.generate_account_report(venue),
        "positions_report": engine.trader.generate_positions_report(),
        "order_fills_report": engine.trader.generate_order_fills_report(),
    }

    return results


def print_backtest_summary(results: dict) -> None:
    """
    Print a summary of backtest results.

    Parameters
    ----------
    results : dict
        The backtest results from run_vwap_wave_backtest.

    """
    print("\n" + "=" * 60)
    print("VWAP WAVE BACKTEST SUMMARY")
    print("=" * 60)

    if "account_report" in results:
        print("\nAccount Report:")
        print(results["account_report"])

    if "positions_report" in results:
        print("\nPositions Report:")
        print(results["positions_report"])

    if "order_fills_report" in results:
        print("\nOrder Fills Report:")
        print(results["order_fills_report"])

    print("\n" + "=" * 60)


# Example usage functions

def example_forex_backtest():
    """
    Example: Run a forex backtest with EUR/USD.

    Note: This requires bar data to be loaded into the engine.
    See NautilusTrader documentation for data loading patterns.
    """
    # Create engine
    engine = create_backtest_engine()

    # Add venue
    venue = add_forex_venue(engine, "OANDA", Decimal("100000"))

    # Create instrument ID
    instrument_id = InstrumentId.from_str("EUR/USD.OANDA")

    # Create bar type
    bar_type = create_bar_type(instrument_id, aggregation_minutes=15)

    # Note: You need to add instrument and data to the engine
    # engine.add_instrument(instrument)
    # engine.add_data(bar_data)

    # Custom configuration for forex
    config = VWAPWaveConfig()

    # Run backtest
    # results = run_vwap_wave_backtest(engine, instrument_id, bar_type, config)
    # print_backtest_summary(results)

    # Clean up
    # engine.reset()
    # engine.dispose()

    print("Forex backtest example - add data loading to run")


def example_crypto_backtest():
    """
    Example: Run a crypto backtest with BTC/USDT.

    Note: This requires bar data to be loaded into the engine.
    See NautilusTrader documentation for data loading patterns.
    """
    # Create engine
    engine = create_backtest_engine()

    # Add venue
    venue = add_crypto_venue(engine, "BINANCE", Decimal("100000"))

    # Create instrument ID
    instrument_id = InstrumentId.from_str("BTCUSDT.BINANCE")

    # Create bar type
    bar_type = create_bar_type(instrument_id, aggregation_minutes=15)

    # Note: You need to add instrument and data to the engine
    # engine.add_instrument(instrument)
    # engine.add_data(bar_data)

    # Custom configuration for crypto (more conservative)
    from vwap_wave.config.instruments import get_instrument_config
    config = get_instrument_config("BTC/USDT", VWAPWaveConfig())

    # Run backtest
    # results = run_vwap_wave_backtest(engine, instrument_id, bar_type, config)
    # print_backtest_summary(results)

    # Clean up
    # engine.reset()
    # engine.dispose()

    print("Crypto backtest example - add data loading to run")


if __name__ == "__main__":
    print("VWAP Wave Backtest Module")
    print("-" * 40)
    print("\nExample functions available:")
    print("  - example_forex_backtest()")
    print("  - example_crypto_backtest()")
    print("\nSee function docstrings for usage instructions.")
