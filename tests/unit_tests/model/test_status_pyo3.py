from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import InstrumentStatus
from nautilus_trader.core.nautilus_pyo3 import MarketStatusAction


def test_instrument_status():
    # Arrange
    update = InstrumentStatus(
        instrument_id=InstrumentId.from_str("MSFT.XNAS"),
        action=MarketStatusAction.TRADING,
        ts_event=0,
        ts_init=0,
        reason=None,
        trading_event=None,
        is_trading=True,
        is_quoting=True,
        is_short_sell_restricted=False,
    )

    # Act, Assert
    assert InstrumentStatus.from_dict(InstrumentStatus.to_dict(update)) == update
    assert repr(update) == "InstrumentStatus(MSFT.XNAS,TRADING,0,0)"  # TODO: Improve repr from Rust
