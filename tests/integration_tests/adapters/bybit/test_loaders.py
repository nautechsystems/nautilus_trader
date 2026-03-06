from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.bybit.loaders import BybitOrderBookDeltaDataLoader
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_load_bybit_deltas() -> None:
    # Arrange
    instrument = TestInstrumentProvider.xrpusdt_linear_bybit()
    data_path = TEST_DATA_DIR / "bybit" / "xrpusdt-ob500.data.zip"
    df = BybitOrderBookDeltaDataLoader.load(data_path)

    wrangler = OrderBookDeltaDataWrangler(instrument)

    # Act
    deltas = wrangler.process(df)

    # Assert
    assert len(deltas) == 3968
    assert deltas[0].action == BookAction.CLEAR
    assert deltas[1].action == BookAction.ADD
    assert deltas[1].order.side == OrderSide.SELL
    assert deltas[1].flags == RecordFlag.F_SNAPSHOT
    assert deltas[1002].action == BookAction.UPDATE
    assert deltas[1235].order.side == OrderSide.SELL
