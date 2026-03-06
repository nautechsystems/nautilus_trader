from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestTradeTick:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            MarkPriceUpdate.fully_qualified_name() == "nautilus_trader.model.data:MarkPriceUpdate"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        mark_price = MarkPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act, Assert
        assert isinstance(hash(mark_price), int)
        assert str(mark_price) == "BTCUSDT.BINANCE,100000.00,1,2"
        assert repr(mark_price) == "MarkPriceUpdate(BTCUSDT.BINANCE,100000.00,1,2)"

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        mark_price = MarkPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = MarkPriceUpdate.to_dict(mark_price)

        # Assert
        assert result == {
            "type": "MarkPriceUpdate",
            "instrument_id": "BTCUSDT.BINANCE",
            "value": "100000.00",
            "ts_event": 1,
            "ts_init": 2,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        mark_price = MarkPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = MarkPriceUpdate.from_dict(MarkPriceUpdate.to_dict(mark_price))

        # Assert
        assert result == mark_price

    def test_from_pyo3(self):
        # Arrange
        mark_price = MarkPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pyo3_mark_price = mark_price.to_pyo3()
        result = MarkPriceUpdate.from_pyo3(pyo3_mark_price)

        # Assert
        assert result == mark_price

    def test_to_pyo3(self):
        # Arrange
        mark_price = MarkPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pyo3_mark_price = mark_price.to_pyo3()

        # Assert
        assert isinstance(pyo3_mark_price, nautilus_pyo3.MarkPriceUpdate)
        assert pyo3_mark_price.value == nautilus_pyo3.Price.from_str("100_000.00")
        assert pyo3_mark_price.ts_event == 1
        assert pyo3_mark_price.ts_init == 2
