#!/usr/bin/env python3

from decimal import Decimal

from strategy import DemoStrategy

from examples.utils.data_provider import prepare_demo_data_eurusd_futures_1min
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model import Bar
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money


if __name__ == "__main__":
    # ----------------------------------------------------------------------------------
    # 1. Configure and create backtest engine
    # ----------------------------------------------------------------------------------

    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST-INDICATOR-001"),  # Unique identifier for this backtest
        logging=LoggingConfig(
            log_level="INFO",  # Set to INFO to see indicator values
        ),
    )
    engine = BacktestEngine(config=engine_config)

    # ----------------------------------------------------------------------------------
    # 2. Prepare market data
    # ----------------------------------------------------------------------------------

    prepared_data: dict = prepare_demo_data_eurusd_futures_1min()
    venue_name: str = prepared_data["venue_name"]
    eurusd_instrument: Instrument = prepared_data["instrument"]
    eurusd_1min_bartype = prepared_data["bar_type"]
    eurusd_1min_bars: list[Bar] = prepared_data["bars_list"]

    # ----------------------------------------------------------------------------------
    # 3. Configure trading environment
    # ----------------------------------------------------------------------------------

    # Set up the trading venue with a margin account
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,  # Use a netting order management system
        account_type=AccountType.MARGIN,  # Use a margin trading account
        starting_balances=[Money(1_000_000, USD)],  # Set initial capital
        base_currency=USD,  # Account currency
        default_leverage=Decimal(1),  # No leverage (1:1)
    )

    # Register the trading instrument
    engine.add_instrument(eurusd_instrument)

    # Load historical market data
    engine.add_data(eurusd_1min_bars)

    # ----------------------------------------------------------------------------------
    # 4. Configure and run strategy
    # ----------------------------------------------------------------------------------

    # Create and register the strategy
    strategy = DemoStrategy(bar_type=eurusd_1min_bartype)
    engine.add_strategy(strategy)

    # Execute the backtest
    engine.run()

    # Clean up resources
    engine.dispose()
