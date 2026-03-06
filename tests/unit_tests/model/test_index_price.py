from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestTradeTick:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            IndexPriceUpdate.fully_qualified_name() == "nautilus_trader.model.data:IndexPriceUpdate"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act, Assert
        assert isinstance(hash(index_price), int)
        assert str(index_price) == "BTCUSDT.BINANCE,100000.00,1,2"
        assert repr(index_price) == "IndexPriceUpdate(BTCUSDT.BINANCE,100000.00,1,2)"

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = IndexPriceUpdate.to_dict(index_price)

        # Assert
        assert result == {
            "type": "IndexPriceUpdate",
            "instrument_id": "BTCUSDT.BINANCE",
            "value": "100000.00",
            "ts_event": 1,
            "ts_init": 2,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = IndexPriceUpdate.from_dict(IndexPriceUpdate.to_dict(index_price))

        # Assert
        assert result == index_price

    def test_from_pyo3(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pyo3_index_price = index_price.to_pyo3()
        result = IndexPriceUpdate.from_pyo3(pyo3_index_price)

        # Assert
        assert result == index_price

    def test_to_pyo3(self):
        # Arrange
        index_price = IndexPriceUpdate(
            instrument_id=BTCUSDT_BINANCE.id,
            value=Price.from_str("100_000.00"),
            ts_event=1,
            ts_init=2,
        )

        # Act
        pyo3_index_price = index_price.to_pyo3()

        # Assert
        assert isinstance(pyo3_index_price, nautilus_pyo3.IndexPriceUpdate)
        assert pyo3_index_price.value == nautilus_pyo3.Price.from_str("100_000.00")
        assert pyo3_index_price.ts_event == 1
        assert pyo3_index_price.ts_init == 2
