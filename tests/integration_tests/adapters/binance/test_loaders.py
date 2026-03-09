from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.binance.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_load_binance_deltas() -> None:
    # Arrange
    instrument = TestInstrumentProvider.btcusdt_binance()
    data_path = TEST_DATA_DIR / "binance" / "btcusdt-depth-snap.csv"
    df = BinanceOrderBookDeltaDataLoader.load(data_path)

    wrangler = OrderBookDeltaDataWrangler(instrument)

    # Act
    deltas = wrangler.process(df)

    # Assert
    assert len(deltas) == 101
    assert deltas[0].action == BookAction.CLEAR
    assert deltas[1].action == BookAction.ADD
    assert deltas[1].order.side == OrderSide.BUY
    assert deltas[1].flags == RecordFlag.F_SNAPSHOT
