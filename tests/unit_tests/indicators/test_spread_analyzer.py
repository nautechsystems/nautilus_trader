import pytest

from nautilus_trader.indicators import SpreadAnalyzer
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestSpreadAnalyzer:
    def test_instantiate(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1000)

        # Act, Assert
        assert analyzer.current == 0
        assert analyzer.current == 0
        assert analyzer.average == 0
        assert analyzer.initialized is False

    def test_handle_ticks_initializes_indicator(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1)  # Only one tick
        tick = TestDataStubs.quote_tick(AUDUSD_SIM)

        # Act
        analyzer.handle_quote_tick(tick)
        analyzer.handle_quote_tick(tick)

        # Assert
        assert analyzer.initialized

    def test_update_with_incorrect_tick_raises_exception(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1000)
        tick = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid_price=Price.from_str("117.80000"),
            ask_price=Price.from_str("117.80010"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        # Act, Assert
        with pytest.raises(ValueError):
            analyzer.handle_quote_tick(tick)

    def test_update_correctly_updates_analyzer(self):
        # Arrange
        analyzer = SpreadAnalyzer(AUDUSD_SIM.id, 1000)
        tick1 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        tick2 = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid_price=Price.from_str("0.80002"),
            ask_price=Price.from_str("0.80008"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Act
        analyzer.handle_quote_tick(tick1)
        analyzer.handle_quote_tick(tick2)

        # Assert
        assert analyzer.current == pytest.approx(6e-05)
        assert analyzer.average == pytest.approx(8e-05)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        instance = SpreadAnalyzer(AUDUSD_SIM.id, 1000)

        # Act
        instance.reset()

        # Assert
        assert not instance.initialized
        assert instance.current == 0
