from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVenue:
    def test_instrument_status(self):
        # Arrange
        status = InstrumentStatus(
            instrument_id=InstrumentId.from_str("MSFT.XNAS"),
            action=MarketStatusAction.TRADING,
            ts_event=0,
            ts_init=0,
            is_trading=True,
            is_quoting=True,
            is_short_sell_restricted=False,
        )

        # Act, Assert
        assert InstrumentStatus.from_dict(InstrumentStatus.to_dict(status)) == status
        assert (
            repr(status)
            == "InstrumentStatus(instrument_id=MSFT.XNAS, action=TRADING, reason=None, trading_event=None, is_trading=True, is_quoting=True, is_short_sell_restricted=False, ts_event=0)"
        )

    def test_instrument_status_to_pyo3(self):
        # Arrange
        status = InstrumentStatus(
            instrument_id=InstrumentId.from_str("ESM4.GLBX"),
            action=MarketStatusAction.TRADING,
            ts_event=1,
            ts_init=2,
            reason="Scheduled",
            trading_event=None,
            is_trading=True,
            is_quoting=True,
            is_short_sell_restricted=None,
        )

        # Act
        pyo3_status = status.to_pyo3()

        # Assert
        assert isinstance(pyo3_status, nautilus_pyo3.InstrumentStatus)
        assert pyo3_status.action == nautilus_pyo3.MarketStatusAction.TRADING
        assert pyo3_status.ts_event == 1
        assert pyo3_status.ts_init == 2
        assert pyo3_status.reason == "Scheduled"
        assert pyo3_status.trading_event is None
        assert pyo3_status.is_trading
        assert pyo3_status.is_quoting
        assert pyo3_status.is_short_sell_restricted is None

    def test_instrument_status_from_pyo3(self):
        # Arrange
        pyo3_status = nautilus_pyo3.InstrumentStatus(
            instrument_id=nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX"),
            action=nautilus_pyo3.MarketStatusAction.TRADING,
            ts_event=1,
            ts_init=2,
            reason="Scheduled",
            trading_event=None,
            is_trading=True,
            is_quoting=True,
            is_short_sell_restricted=None,
        )

        # Act
        status = InstrumentStatus.from_pyo3(pyo3_status)

        # Assert
        assert isinstance(status, InstrumentStatus)
        assert status.action == MarketStatusAction.TRADING
        assert status.ts_event == 1
        assert status.ts_init == 2
        assert status.reason == "Scheduled"
        assert status.trading_event is None
        assert status.is_trading
        assert status.is_quoting
        assert status.is_short_sell_restricted is None

    def test_instrument_close(self):
        # Arrange
        update = InstrumentClose(
            instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
            close_price=Price(100.0, precision=0),
            close_type=InstrumentCloseType.CONTRACT_EXPIRED,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert InstrumentClose.from_dict(InstrumentClose.to_dict(update)) == update
        assert (
            repr(update)
            == "InstrumentClose(instrument_id=BTCUSDT.BINANCE, close_price=100, close_type=CONTRACT_EXPIRED)"
        )
