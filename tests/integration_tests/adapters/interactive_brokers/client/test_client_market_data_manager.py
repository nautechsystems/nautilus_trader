from unittest import mock

from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestIncomingMessages


@mock.patch(
    "nautilus_trader.adapters.interactive_brokers.client.market_data_handler.InteractiveBrokersMarketDataManager._handle_data",
)
def test_process_bid_ask_tick(mock_handle_data, ib_client):
    # Arrange
    msg = IBTestIncomingMessages.get_msg("quote_tick_1.txt")
    quote_tick = QuoteTick(...)

    # Act
    ib_client._process_message(msg)

    # Assert
    assert ib_client.market_data_handler.tickByTickBidAsk.called
    assert ib_client.market_data_handler.mock_handle_data.assert_called_with(quote_tick)


def test_process_trade_tick(mock_handle_data, ib_client):
    # Arrange
    msg = IBTestIncomingMessages.get_msg("trade_tick_1.txt")
    trade_tick = TradeTick(...)

    # Act
    ib_client._process_message(msg)

    # Assert
    assert ib_client.market_data_handler.tickByTickAllLast.called
    assert ib_client.market_data_handler.mock_handle_data.assert_called_with(trade_tick)


def test_get_historical_bars(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_get_historical_ticks(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_process_bar_data(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_convert_ib_bar_date_to_unix_nanos():
    # Arrange

    # Act

    # Assert
    pass


def test_ib_bar_to_nautilus_bar():
    # Arrange

    # Act

    # Assert
    pass


def test_process_trade_ticks():
    # Arrange

    # Act

    # Assert
    pass
