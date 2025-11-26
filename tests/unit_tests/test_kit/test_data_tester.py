# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from unittest.mock import patch

import pandas as pd
import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture
def clock():
    return TestClock()


@pytest.fixture
def trader_id():
    return TestIdStubs.trader_id()


@pytest.fixture
def msgbus(trader_id, clock):
    return MessageBus(
        trader_id=trader_id,
        clock=clock,
    )


@pytest.fixture
def cache():
    return TestComponentStubs.cache()


@pytest.fixture
def portfolio(msgbus, cache, clock):
    return Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture
def data_engine(msgbus, cache, clock):
    return DataEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture
def instrument():
    return TestInstrumentProvider.btcusdt_binance()


@pytest.fixture
def instrument_id(instrument):
    return instrument.id


@pytest.fixture
def create_tester_factory(trader_id, msgbus, cache, clock, portfolio):
    testers = []

    def _create_tester(config):
        tester = DataTester(config)
        tester.register_base(
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        testers.append(tester)
        return tester

    return _create_tester


# ================================================================================================
# Configuration Tests
# ================================================================================================


def test_config_default_values():
    # Arrange, Act
    config = DataTesterConfig(
        instrument_ids=[InstrumentId.from_str("BTC-USDT.BINANCE")],
    )

    # Assert
    assert config.subscribe_book_deltas is False
    assert config.subscribe_book_depth is False
    assert config.subscribe_book_at_interval is False
    assert config.subscribe_quotes is False
    assert config.subscribe_trades is False
    assert config.subscribe_mark_prices is False
    assert config.subscribe_index_prices is False
    assert config.subscribe_funding_rates is False
    assert config.subscribe_bars is False
    assert config.subscribe_instrument is False
    assert config.subscribe_instrument_status is False
    assert config.subscribe_instrument_close is False
    assert config.can_unsubscribe is True
    assert config.request_instruments is False
    assert config.request_quotes is False
    assert config.request_trades is False
    assert config.request_bars is False
    assert config.book_type == BookType.L2_MBP
    assert config.book_interval_ms == 1000
    assert config.book_levels_to_print == 10
    assert config.manage_book is False
    assert config.use_pyo3_book is False
    assert config.log_data is True


@pytest.mark.parametrize(
    ("subscribe_flag", "value"),
    [
        ("subscribe_quotes", True),
        ("subscribe_trades", True),
        ("subscribe_mark_prices", True),
        ("subscribe_index_prices", True),
        ("subscribe_funding_rates", True),
        ("subscribe_bars", True),
        ("subscribe_instrument", True),
        ("subscribe_instrument_status", True),
        ("subscribe_instrument_close", True),
        ("subscribe_book_deltas", True),
        ("subscribe_book_depth", True),
        ("subscribe_book_at_interval", True),
    ],
)
def test_config_subscribe_flags(subscribe_flag, value):
    # Arrange
    instrument_id = InstrumentId.from_str("BTC-USDT.BINANCE")

    # Act
    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        **{subscribe_flag: value},
    )

    # Assert
    assert getattr(config, subscribe_flag) == value


@pytest.mark.parametrize(
    ("request_flag", "value"),
    [
        ("request_instruments", True),
        ("request_quotes", True),
        ("request_trades", True),
        ("request_bars", True),
    ],
)
def test_config_request_flags(request_flag, value):
    # Arrange
    instrument_id = InstrumentId.from_str("BTC-USDT.BINANCE")

    # Act
    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        **{request_flag: value},
    )

    # Assert
    assert getattr(config, request_flag) == value


@pytest.mark.parametrize(
    ("book_type_value"),
    [
        BookType.L1_MBP,
        BookType.L2_MBP,
        BookType.L3_MBO,
    ],
)
def test_config_book_types(book_type_value):
    # Arrange
    instrument_id = InstrumentId.from_str("BTC-USDT.BINANCE")

    # Act
    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        book_type=book_type_value,
    )

    # Assert
    assert config.book_type == book_type_value


# ================================================================================================
# Subscription Tests
# ================================================================================================


def test_on_start_subscribes_to_quotes(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_quotes=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_quote_ticks") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_trades(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_trades=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_trade_ticks") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_mark_prices(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_mark_prices=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_mark_prices") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_index_prices(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_index_prices=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_index_prices") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_funding_rates(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_funding_rates=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_funding_rates") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_instrument(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_instrument=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_instrument") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(instrument_id)


def test_on_start_subscribes_to_instrument_status(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_instrument_status=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_instrument_status") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_instrument_close(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_instrument_close=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_instrument_close") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_book_deltas(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        book_type=BookType.L2_MBP,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_order_book_deltas") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            book_type=BookType.L2_MBP,
            client_id=None,
            pyo3_conversion=False,
        )


def test_on_start_subscribes_to_book_depth(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_depth=True,
        book_type=BookType.L2_MBP,
        book_depth=20,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_order_book_depth") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            book_type=BookType.L2_MBP,
            depth=20,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_book_at_interval(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_at_interval=True,
        book_type=BookType.L2_MBP,
        book_depth=10,
        book_interval_ms=500,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_order_book_at_interval") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            book_type=BookType.L2_MBP,
            depth=10,
            interval_ms=500,
            client_id=None,
            params=None,
        )


def test_on_start_subscribes_to_bars(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    bar_type = BarType.from_str(f"{instrument_id.value}-1-MINUTE-LAST-EXTERNAL")

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        bar_types=[bar_type],
        subscribe_bars=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_bars") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            bar_type=bar_type,
            client_id=None,
            params=None,
        )


def test_on_start_with_multiple_instruments_subscribes_all(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
):
    # Arrange
    instrument1 = TestInstrumentProvider.btcusdt_binance()
    instrument2 = TestInstrumentProvider.ethusdt_binance()

    cache.add_instrument(instrument1)
    cache.add_instrument(instrument2)

    config = DataTesterConfig(
        instrument_ids=[instrument1.id, instrument2.id],
        subscribe_quotes=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_quote_ticks") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        assert mock_subscribe.call_count == 2
        mock_subscribe.assert_any_call(
            instrument_id=instrument1.id,
            client_id=None,
            params=None,
        )
        mock_subscribe.assert_any_call(
            instrument_id=instrument2.id,
            client_id=None,
            params=None,
        )


def test_on_start_with_client_id(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    client_id = ClientId("BINANCE")

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        client_id=client_id,
        subscribe_quotes=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_quote_ticks") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=client_id,
            params=None,
        )


def test_on_start_with_subscribe_params(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    subscribe_params = {"param1": "value1", "param2": "value2"}

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_quotes=True,
        subscribe_params=subscribe_params,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "subscribe_quote_ticks") as mock_subscribe:
        # Act
        tester.on_start()

        # Assert
        mock_subscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=subscribe_params,
        )


# ================================================================================================
# Book Management Tests
# ================================================================================================


def test_setup_book_creates_legacy_book(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        manage_book=True,
        book_type=BookType.L2_MBP,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Act
    tester.setup_book(instrument_id)

    # Assert
    assert instrument_id in tester._books
    assert tester._books[instrument_id].instrument_id == instrument_id
    assert tester._books[instrument_id].book_type == BookType.L2_MBP


def test_setup_book_pyo3_creates_pyo3_book(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        manage_book=True,
        use_pyo3_book=True,
        book_type=BookType.L2_MBP,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Act
    tester.setup_book_pyo3(instrument_id)

    # Assert
    pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
    assert pyo3_instrument_id in tester._books
    assert isinstance(tester._books[pyo3_instrument_id], nautilus_pyo3.OrderBook)


def test_on_start_manages_book_when_configured(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        manage_book=True,
        book_type=BookType.L2_MBP,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "setup_book") as mock_setup:
        # Act
        tester.on_start()

        # Assert
        mock_setup.assert_called_once_with(instrument_id)


def test_on_start_manages_pyo3_book_when_configured(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        manage_book=True,
        use_pyo3_book=True,
        book_type=BookType.L2_MBP,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "setup_book_pyo3") as mock_setup:
        # Act
        tester.on_start()

        # Assert
        mock_setup.assert_called_once_with(instrument_id)


# ================================================================================================
# Request Tests
# ================================================================================================


def test_on_start_requests_instruments(trader_id, msgbus, cache, clock, portfolio, instrument):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument.id],
        request_instruments=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "request_instruments") as mock_request:
        # Act
        tester.on_start()

        # Assert
        mock_request.assert_called_once_with(
            venue=instrument.id.venue,
            client_id=None,
            params=None,
        )


def test_on_start_requests_quotes(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        request_quotes=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "request_quote_ticks") as mock_request:
        # Act
        tester.on_start()

        # Assert
        mock_request.assert_called_once()
        call_args = mock_request.call_args
        assert call_args.kwargs["instrument_id"] == instrument_id
        assert call_args.kwargs["client_id"] is None
        assert call_args.kwargs["params"] is None


def test_on_start_requests_trades(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        request_trades=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "request_trade_ticks") as mock_request:
        # Act
        tester.on_start()

        # Assert
        mock_request.assert_called_once()
        call_args = mock_request.call_args
        assert call_args.kwargs["instrument_id"] == instrument_id
        assert call_args.kwargs["client_id"] is None
        assert call_args.kwargs["params"] is None


def test_on_start_requests_bars(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    bar_type = BarType.from_str(f"{instrument_id.value}-1-MINUTE-LAST-EXTERNAL")

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        bar_types=[bar_type],
        request_bars=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "request_bars") as mock_request:
        # Act
        tester.on_start()

        # Assert
        mock_request.assert_called_once()
        call_args = mock_request.call_args
        assert call_args.args[0] == bar_type
        assert call_args.kwargs["client_id"] is None
        assert call_args.kwargs["params"] is None


def test_on_start_requests_start_delta_uses_custom_delta(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    custom_delta = pd.Timedelta(hours=2)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        request_quotes=True,
        requests_start_delta=custom_delta,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "request_quote_ticks") as mock_request:
        # Act
        tester.on_start()

        # Assert
        call_args = mock_request.call_args
        expected_start = clock.utc_now() - custom_delta
        assert call_args.kwargs["start"] == expected_start


def test_on_start_with_request_params(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    request_params = {"limit": 1000, "custom_param": "value"}

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        request_quotes=True,
        request_params=request_params,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    with patch.object(tester, "request_quote_ticks") as mock_request:
        # Act
        tester.on_start()

        # Assert
        call_args = mock_request.call_args
        assert call_args.kwargs["params"] == request_params


# ================================================================================================
# Unsubscribe Tests
# ================================================================================================


def test_on_stop_unsubscribes_from_quotes(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_quotes=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_quote_ticks") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_trades(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_trades=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_trade_ticks") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_mark_prices(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_mark_prices=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_mark_prices") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_index_prices(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_index_prices=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_index_prices") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_funding_rates(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_funding_rates=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_funding_rates") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_instrument(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_instrument=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_instrument") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            params=None,
        )


def test_on_stop_unsubscribes_from_instrument_status(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_instrument_status=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_instrument_status") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_instrument_close(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_instrument_close=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_instrument_close") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_book_deltas(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_order_book_deltas") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_book_depth(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_depth=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_order_book_depth") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_book_at_interval(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_book_at_interval=True,
        book_interval_ms=500,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_order_book_at_interval") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            instrument_id=instrument_id,
            interval_ms=500,
            client_id=None,
            params=None,
        )


def test_on_stop_unsubscribes_from_bars(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    bar_type = BarType.from_str(f"{instrument_id.value}-1-MINUTE-LAST-EXTERNAL")

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        bar_types=[bar_type],
        subscribe_bars=True,
        can_unsubscribe=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_bars") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_called_once_with(
            bar_type=bar_type,
            client_id=None,
            params=None,
        )


def test_on_stop_with_can_unsubscribe_false_does_not_unsubscribe(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        subscribe_quotes=True,
        can_unsubscribe=False,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    with patch.object(tester, "unsubscribe_quote_ticks") as mock_unsubscribe:
        # Act
        tester.on_stop()

        # Assert
        mock_unsubscribe.assert_not_called()


# ================================================================================================
# Data Handler Tests
# ================================================================================================


def test_on_historical_data_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    data = TestDataStubs.quote_tick(instrument)

    # Act & Assert - should not raise
    tester.on_historical_data(data)


def test_on_instrument_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Act & Assert - should not raise
    tester.on_instrument(instrument)


def test_on_instruments_calls_without_error(trader_id, msgbus, cache, clock, portfolio):
    # Arrange
    instrument1 = TestInstrumentProvider.btcusdt_binance()
    instrument2 = TestInstrumentProvider.ethusdt_binance()

    cache.add_instrument(instrument1)
    cache.add_instrument(instrument2)

    config = DataTesterConfig(
        instrument_ids=[instrument1.id, instrument2.id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    instruments = [instrument1, instrument2]

    # Act & Assert - should not raise
    tester.on_instruments(instruments)


def test_on_order_book_deltas_without_book_management_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
        manage_book=False,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    deltas = TestDataStubs.order_book_deltas(instrument_id)

    # Act & Assert - should not raise
    tester.on_order_book_deltas(deltas)


def test_on_order_book_deltas_updates_book_when_managing(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
        manage_book=True,
        book_type=BookType.L2_MBP,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.setup_book(instrument_id)

    deltas = TestDataStubs.order_book_deltas(instrument_id)

    # Act
    tester.on_order_book_deltas(deltas)

    # Assert
    assert instrument_id in tester._books


def test_on_order_book_depth_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    depth = TestDataStubs.order_book_depth10(instrument_id)

    # Act & Assert - should not raise
    tester.on_order_book_depth(depth)


def test_on_order_book_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    order_book = TestDataStubs.order_book(instrument)

    # Act & Assert - should not raise
    tester.on_order_book(order_book)


def test_on_quote_tick_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    quote = TestDataStubs.quote_tick(instrument)

    # Act & Assert - should not raise
    tester.on_quote_tick(quote)


def test_on_trade_tick_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    trade = TestDataStubs.trade_tick(instrument)

    # Act & Assert - should not raise
    tester.on_trade_tick(trade)


def test_on_mark_price_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    mark_price = MarkPriceUpdate(
        instrument_id=instrument_id,
        value=Price.from_str("50000.00"),
        ts_event=0,
        ts_init=0,
    )

    # Act & Assert - should not raise
    tester.on_mark_price(mark_price)


def test_on_index_price_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    index_price = IndexPriceUpdate(
        instrument_id=instrument_id,
        value=Price.from_str("50000.00"),
        ts_event=0,
        ts_init=0,
    )

    # Act & Assert - should not raise
    tester.on_index_price(index_price)


def test_on_funding_rate_calls_without_error(
    trader_id,
    msgbus,
    cache,
    clock,
    portfolio,
    instrument,
    instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    funding_rate = FundingRateUpdate(
        instrument_id=instrument_id,
        rate=Price.from_str("0.0001"),
        ts_event=0,
        ts_init=0,
    )

    # Act & Assert - should not raise
    tester.on_funding_rate(funding_rate)


def test_on_bar_calls_without_error(
    trader_id, msgbus, cache, clock, portfolio, instrument, instrument_id,
):
    # Arrange
    cache.add_instrument(instrument)

    bar_type = BarType.from_str(f"{instrument_id.value}-1-MINUTE-LAST-EXTERNAL")

    config = DataTesterConfig(
        instrument_ids=[instrument_id],
        bar_types=[bar_type],
        log_data=True,
    )

    tester = DataTester(config)
    tester.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    bar = Bar(
        bar_type=bar_type,
        open=Price.from_str("50000.00"),
        high=Price.from_str("51000.00"),
        low=Price.from_str("49000.00"),
        close=Price.from_str("50500.00"),
        volume=Quantity.from_int(1000),
        ts_event=0,
        ts_init=0,
    )

    # Act & Assert - should not raise
    tester.on_bar(bar)
