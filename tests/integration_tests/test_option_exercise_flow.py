# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Integration tests for option exercise and expiry in the backtest engine.

Option exercise logic was moved from the standalone OptionExerciseModule into the
OrderMatchingEngine. These tests verify option expiration behavior (ITM exercise, OTM
expiry) through full backtest runs.

"""

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


class OptionExerciseTestStrategy(Strategy):
    def __init__(self, config=None):
        super().__init__(config=config)
        self.option_id = InstrumentId.from_str("AAPL240315C00150000.NASDAQ")
        self.order_submitted = False

    def on_start(self):
        self.subscribe_quote_ticks(self.option_id)

    def on_quote_tick(self, tick):
        if tick.instrument_id == self.option_id and not self.order_submitted:
            order = self.order_factory.market(
                instrument_id=self.option_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
            )
            self.submit_order(order)
            self.order_submitted = True


def test_option_exercise_integration_flow():
    """
    Test the full option exercise flow in a backtest engine.

    Option expiration and exercise are handled by the matching engine at instrument
    expiration time.

    """
    venue = Venue("NASDAQ")
    engine = BacktestEngine(config=BacktestEngineConfig())

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
    )

    # Underlying
    underlying = Equity(
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        raw_symbol=Symbol("AAPL"),
        currency=USD,
        price_precision=2,
        price_increment=Price(0.01, 2),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(underlying)

    expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))

    # Option
    option = OptionContract(
        instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
        raw_symbol=Symbol("AAPL240315C00150000"),
        asset_class=AssetClass.EQUITY,
        underlying="AAPL",
        option_kind=OptionKind.CALL,
        strike_price=Price(150.0, 2),
        currency=USD,
        activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
        expiration_ns=expiry_ns,
        price_precision=2,
        price_increment=Price(0.01, 2),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(option)

    # Strategy
    strategy = OptionExerciseTestStrategy()
    engine.add_strategy(strategy)

    # Data:
    # 1. Quote for option to trigger strategy order (before expiration)
    # 2. Trade for underlying at expiration (ITM: 160 > 150)
    data = [
        QuoteTick(
            instrument_id=option.id,
            bid_price=Price(5.0, 2),
            ask_price=Price(5.1, 2),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=expiry_ns - 1000,
            ts_init=expiry_ns - 1000,
        ),
        TradeTick(
            instrument_id=underlying.id,
            price=Price(160.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=expiry_ns,
            ts_init=expiry_ns,
        ),
    ]
    engine.add_data(data)

    # Run backtest
    engine.run(start=pd.Timestamp(expiry_ns - 2000, unit="ns", tz="UTC"))

    # VERIFICATIONS

    # 1. Option position should be closed
    option_pos = engine.cache.positions_open(venue=venue, instrument_id=option.id)
    assert len(option_pos) == 0, "Option position should be closed at expiration"

    # 2. Underlying position should be created via physical settlement
    underlying_pos = engine.cache.positions_open(venue=venue, instrument_id=underlying.id)
    assert len(underlying_pos) == 1, "Underlying position should have been created by exercise"
    pos = underlying_pos[0]
    assert pos.quantity == Quantity.from_int(100), "Should have 100 shares from exercise"
    assert pos.avg_px_open == Price(150.0, 2), "Underlying should open at strike price"

    engine.dispose()


def test_otm_option_expiry():
    """
    Test OTM option expires worthless at expiration.

    OTM call (underlying < strike): no exercise, option position closed at zero value.

    """
    venue = Venue("NASDAQ")
    engine = BacktestEngine(config=BacktestEngineConfig())

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
    )

    underlying = Equity(
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        raw_symbol=Symbol("AAPL"),
        currency=USD,
        price_precision=2,
        price_increment=Price(0.01, 2),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(underlying)

    expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))

    option = OptionContract(
        instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
        raw_symbol=Symbol("AAPL240315C00150000"),
        asset_class=AssetClass.EQUITY,
        underlying="AAPL",
        option_kind=OptionKind.CALL,
        strike_price=Price(150.0, 2),
        currency=USD,
        activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
        expiration_ns=expiry_ns,
        price_precision=2,
        price_increment=Price(0.01, 2),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(option)

    strategy = OptionExerciseTestStrategy()
    engine.add_strategy(strategy)

    # Underlying at 140 < strike 150: OTM call, expires worthless
    data = [
        QuoteTick(
            instrument_id=option.id,
            bid_price=Price(5.0, 2),
            ask_price=Price(5.1, 2),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=expiry_ns - 1000,
            ts_init=expiry_ns - 1000,
        ),
        TradeTick(
            instrument_id=underlying.id,
            price=Price(140.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=expiry_ns,
            ts_init=expiry_ns,
        ),
    ]
    engine.add_data(data)

    engine.run(start=pd.Timestamp(expiry_ns - 2000, unit="ns", tz="UTC"))

    # Option position closed at zero value (OTM expiry)
    option_pos = engine.cache.positions_open(venue=venue, instrument_id=option.id)
    assert len(option_pos) == 0, "Option position should be closed at expiration"

    # No underlying position (OTM, no exercise)
    underlying_pos = engine.cache.positions_open(venue=venue, instrument_id=underlying.id)
    assert len(underlying_pos) == 0, "No underlying position for OTM expiry"

    engine.dispose()


def test_itm_option_expiry_with_custom_settlement():
    """
    Test ITM option exercise with custom settlement price for the option leg.

    With settlement_prices, the option leg closes at the specified price instead of
    avg_px_open. Underlying still settles at strike.

    """
    venue = Venue("NASDAQ")
    custom_option_price = 7.0

    engine = BacktestEngine(config=BacktestEngineConfig())
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        settlement_prices={
            InstrumentId.from_str("AAPL240315C00150000.NASDAQ"): custom_option_price,
        },
    )

    underlying = Equity(
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        raw_symbol=Symbol("AAPL"),
        currency=USD,
        price_precision=2,
        price_increment=Price(0.01, 2),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(underlying)

    expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))
    option = OptionContract(
        instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
        raw_symbol=Symbol("AAPL240315C00150000"),
        asset_class=AssetClass.EQUITY,
        underlying="AAPL",
        option_kind=OptionKind.CALL,
        strike_price=Price(150.0, 2),
        currency=USD,
        activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
        expiration_ns=expiry_ns,
        price_precision=2,
        price_increment=Price(0.01, 2),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(option)
    engine.add_strategy(OptionExerciseTestStrategy())

    data = [
        QuoteTick(
            instrument_id=option.id,
            bid_price=Price(5.0, 2),
            ask_price=Price(5.1, 2),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=expiry_ns - 1000,
            ts_init=expiry_ns - 1000,
        ),
        TradeTick(
            instrument_id=underlying.id,
            price=Price(160.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=expiry_ns,
            ts_init=expiry_ns,
        ),
    ]
    engine.add_data(data)
    engine.run(start=pd.Timestamp(expiry_ns - 2000, unit="ns", tz="UTC"))

    option_pos = engine.cache.positions_open(venue=venue, instrument_id=option.id)
    assert len(option_pos) == 0, "Option position should be closed"

    underlying_pos = engine.cache.positions_open(venue=venue, instrument_id=underlying.id)
    assert len(underlying_pos) == 1, "Underlying position should exist from exercise"
    assert underlying_pos[0].avg_px_open == Price(150.0, 2), "Underlying at strike"

    # Option bought at 5.0, closed at custom 7.0: +2 per share * 100 = +200 on option leg
    # (small variance from fees/rounding)
    account = engine.trader.generate_account_report(venue)
    total = float(account.iloc[-1]["total"])
    assert total > 1_000_000.0, f"Account should reflect custom settlement profit, was {total}"
    assert 1_000_180.0 <= total <= 1_000_220.0, (
        f"Expected ~1_000_200 from custom settlement, was {total}"
    )

    engine.dispose()


def test_otm_option_expiry_with_custom_settlement():
    """
    Test OTM option expiry with custom settlement price.

    With settlement_prices, the option leg closes at the specified price instead of
    zero. OTM normally expires worthless (0); custom allows non-zero.

    """
    venue = Venue("NASDAQ")
    custom_option_price = 0.5

    engine = BacktestEngine(config=BacktestEngineConfig())
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        settlement_prices={
            InstrumentId.from_str("AAPL240315C00150000.NASDAQ"): custom_option_price,
        },
    )

    underlying = Equity(
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        raw_symbol=Symbol("AAPL"),
        currency=USD,
        price_precision=2,
        price_increment=Price(0.01, 2),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(underlying)

    expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))
    option = OptionContract(
        instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
        raw_symbol=Symbol("AAPL240315C00150000"),
        asset_class=AssetClass.EQUITY,
        underlying="AAPL",
        option_kind=OptionKind.CALL,
        strike_price=Price(150.0, 2),
        currency=USD,
        activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
        expiration_ns=expiry_ns,
        price_precision=2,
        price_increment=Price(0.01, 2),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(option)
    engine.add_strategy(OptionExerciseTestStrategy())

    data = [
        QuoteTick(
            instrument_id=option.id,
            bid_price=Price(5.0, 2),
            ask_price=Price(5.1, 2),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=expiry_ns - 1000,
            ts_init=expiry_ns - 1000,
        ),
        TradeTick(
            instrument_id=underlying.id,
            price=Price(140.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=expiry_ns,
            ts_init=expiry_ns,
        ),
    ]
    engine.add_data(data)
    engine.run(start=pd.Timestamp(expiry_ns - 2000, unit="ns", tz="UTC"))

    option_pos = engine.cache.positions_open(venue=venue, instrument_id=option.id)
    assert len(option_pos) == 0, "Option position should be closed"

    underlying_pos = engine.cache.positions_open(venue=venue, instrument_id=underlying.id)
    assert len(underlying_pos) == 0, "No underlying for OTM"

    # Option bought at 5.0, OTM closed at custom 0.5: +50 vs 0 default
    # Start 1M, -500 buy, +50 close = 999550 (before any fees)
    account = engine.trader.generate_account_report(venue)
    total = float(account.iloc[-1]["total"])
    assert total > 999_500.0, f"Custom OTM settlement should recover some value, was {total}"
    assert 999_530.0 <= total <= 999_570.0, (
        f"Expected ~999550 from custom OTM settlement, was {total}"
    )

    engine.dispose()
