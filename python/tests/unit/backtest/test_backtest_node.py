import pytest

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType


def test_node_construction():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    assert node is not None


def test_node_empty_configs_raises():
    with pytest.raises(RuntimeError, match="At least one run config"):
        BacktestNode([])


def test_node_venue_mismatch_raises():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("BTC/USDT.BINANCE"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    with pytest.raises(RuntimeError, match="No venue config found for venue"):
        BacktestNode([config])


def test_node_repr():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    assert "BacktestNode" in repr(node)


def test_node_dispose():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    node.dispose()
