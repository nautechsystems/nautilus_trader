"""
Example of synthetic data pnl test.
"""

from datetime import UTC
from datetime import datetime
from decimal import Decimal
from typing import Any
from typing import Self

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.execution import PerContractFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import AssetClass
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import FuturesContract
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy


USD = Currency.from_str("USD")


def create_6E_instrument(venue: Venue) -> FuturesContract:
    """
    Create 6e instrument.
    """
    symbol = Symbol("6E")
    return FuturesContract(
        # Core identification parameters for the Euro FX futures contract
        instrument_id=InstrumentId(symbol, venue),  # 6E is CME's code for EUR/USD futures
        raw_symbol=symbol,  # Symbol as used on the exchange
        asset_class=AssetClass.FX,  # Indicates this is an FX futures contract
        currency=USD,  # Contract is denominated in USD
        # Price and size specifications from CME
        price_precision=5,  # 5 decimal places for EUR/USD pricing
        price_increment=Price.from_str("0.00005"),  # Minimum tick = 0.00005 ($6.25 value)
        multiplier=Quantity.from_int(125_000),  # Each contract = 125,000 EUR
        lot_size=Quantity.from_int(1),  # Minimum trading size is 1 contract
        # Contract specifications and expiration details
        underlying="EUR/USD",  # The underlying forex pair
        activation_ns=0,  # Contract start time (0 = active now)
        expiration_ns=int(datetime(2024, 12, 17, 14, 16, tzinfo=UTC).timestamp() * 1e9),
        # 3rd Wednesday at 9:16 AM CT
        # System timestamps for internal tracking
        ts_event=0,  # Event creation time
        ts_init=0,  # Initialization time
        margin_init=Decimal("0.21818181812"),
        # $3_000 per contract (at price 1.1000). This amount must be available to open new position.
        # It is not block, it is only entry requirement check.
        margin_maint=Decimal("0.18181818182"),
        # $2,500 per contract (at price 1.1000). This amount is really locked on account, while we have open position
        maker_fee=Decimal(
            0,
        ),  # CME Futures don't use maker/taker fee model. They have fixed fee per contract.
        taker_fee=Decimal(0),  # same as above
        # Additional contract specifications
        exchange="SIM",  # Chicago Mercantile Exchange rules
    )


class MinimalStrategyConfig(StrategyConfig):
    """
    Collect minimal strategy config tests.
    """

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        """
        Create a new instance.
        """
        kwargs.pop("instrument_id", None)
        kwargs.pop("bar_type", None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, instrument_id: InstrumentId, bar_type: BarType) -> None:
        """
        Initialize the instance.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type


class MinimalStrategy(Strategy):
    """
    Collect minimal strategy tests.
    """

    def __init__(self, config: MinimalStrategyConfig) -> None:
        """
        Initialize the instance.
        """
        super().__init__(config)
        self._config = config
        self.bars_processed = -1

        self.portfolio_realized_pnl_values: dict[datetime, Money | None] = {}
        self.portfolio_unrealized_pnl_values: dict[datetime, Money | None] = {}

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_bars(self._config.bar_type)

    def on_bar(self, bar: Bar) -> None:
        """
        On bar.
        """
        self.bars_processed += 1

        bar_dt = unix_nanos_to_dt(bar.ts_event)

        # Collect value of realized/unrealized pnl from Portfolio
        self.portfolio_realized_pnl_values[bar_dt] = self.portfolio.realized_pnl(
            self._config.instrument_id,
        )
        self.portfolio_unrealized_pnl_values[bar_dt] = self.portfolio.unrealized_pnl(
            self._config.instrument_id,
        )

        is_flat = self.portfolio.is_completely_net_flat()

        # Debug point 1: Open position
        # Problem is, that , but Portfolio return None: `self.portfolio.unrealized_pnl(self._config.instrument_id)`
        if self.bars_processed == 5:
            # See value of 2 variables:
            realized_pnl = self.portfolio.realized_pnl(
                self._config.instrument_id,
            )  # Has only commission -2.50. Is OK as no trade was closed yet.
            unrealized_pnl = self.portfolio.unrealized_pnl(self._config.instrument_id)
            log_msg = f"self.bars_processed={self.bars_processed!r}, realized_pnl={realized_pnl!r}, unrealized_pnl={unrealized_pnl!r}"
            self.log.info(log_msg)
            # <------------------- PUT DEBUG POINT HERE

        # Debug point 2: Closed position
        # Problem is, that , but Portfolio return None: `self.portfolio.unrealized_pnl(self._config.instrument_id)`
        if self.bars_processed == 10:
            # See value of 2 variables:
            realized_pnl = self.portfolio.realized_pnl(self._config.instrument_id)
            unrealized_pnl = self.portfolio.unrealized_pnl(
                self._config.instrument_id,
            )  # Returns 0, that is OK when closed position
            log_msg = f"self.bars_processed={self.bars_processed!r}, realized_pnl={realized_pnl!r}, unrealized_pnl={unrealized_pnl!r}"
            self.log.info(log_msg)
            # <------------------- PUT DEBUG POINT HERE

        # Open positions at bar(s): 1
        if is_flat and self.bars_processed in {1}:
            order = self.order_factory.market(
                instrument_id=self._config.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_str("1"),
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(order)
            self.order_placed = True
            log_msg = f"Market order placed at {bar.close}"
            self.log.info(log_msg)

        # Close positions at bar(s): 7
        if (not is_flat) and self.bars_processed in {7}:
            order = self.order_factory.market(
                instrument_id=self._config.instrument_id,
                order_side=OrderSide.SELL,
                quantity=Quantity.from_str("1"),
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(order)

    def on_stop(self) -> None:
        """
        On stop.
        """


if __name__ == "__main__":
    engine = BacktestEngine(
        config=BacktestEngineConfig(
            trader_id=TraderId.from_str("TESTER-001"),
        ),
    )

    # Venue
    venue = Venue("SIM")
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        fee_model=PerContractFeeModel(Money.from_str("2.50 USD")),
        starting_balances=[Money.from_str("1000000 USD")],
    )

    # Instrument
    instrument = create_6E_instrument(venue)
    engine.add_instrument(instrument)

    # Add 12 synthetic one-minute bars
    bar_type = BarType.from_str(f"{instrument.id}-1-MINUTE-LAST-EXTERNAL")

    timestamp_base = dt_to_unix_nanos(datetime(2024, 1, 1, tzinfo=UTC))
    bars = []

    for i in range(12):
        bar = Bar(
            bar_type=bar_type,
            open=instrument.make_price(1.10000 + i * 0.0001),
            high=instrument.make_price(1.20000 + i * 0.0001),
            low=instrument.make_price(1.10000 + i * 0.0001),
            close=instrument.make_price(1.10000 + i * 0.0001),
            volume=Quantity.from_str("100"),
            ts_event=timestamp_base + (i * 60_000_000_000),  # +1 minute
            ts_init=timestamp_base + (i * 60_000_000_000),
        )
        bars.append(bar)
    # Add all created bars
    engine.add_data(bars)

    # Create strategy
    config = MinimalStrategyConfig(
        instrument_id=instrument.id,
        bar_type=bar_type,
    )
    strategy = MinimalStrategy(config=config)
    engine.add_strategy(strategy)

    # Run backtest
    engine.run()

    # Results
    print(engine.generate_order_fills_report())
    print(engine.generate_positions_report())

    account_report = engine.generate_account_report(venue=venue)
    print(account_report)

    # Cleanup
    engine.dispose()
