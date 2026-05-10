# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core.nautilus_pyo3 import AggregationSource
from nautilus_trader.core.nautilus_pyo3 import Cache  # type: ignore[attr-defined]
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


@pytest.fixture
def cache():
    return Cache()


class TestCachePyo3General:
    def test_get_missing_key_returns_none(self, cache):
        assert cache.get("missing") is None

    def test_add_and_get_general(self, cache):
        cache.add("test_key", b"test_value")
        result = cache.get("test_key")
        assert result == b"test_value"

    def test_reset_clears_general(self, cache):
        cache.add("test_key", b"data")
        cache.reset()
        assert cache.get("test_key") is None


class TestCachePyo3DataQueries:
    def test_quote_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.quote(instrument_id) is None

    def test_trade_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.trade(instrument_id) is None

    def test_bar_empty_returns_none(self, cache):
        bar_type = TestDataProviderPyo3.bartype_ethusdt_1min_bid()
        assert cache.bar(bar_type) is None

    def test_quote_with_index_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.quote(instrument_id, index=1) is None

    def test_quotes_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.quotes(instrument_id) is None

    def test_trades_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.trades(instrument_id) is None

    def test_bars_empty_returns_none(self, cache):
        bar_type = TestDataProviderPyo3.bartype_ethusdt_1min_bid()
        assert cache.bars(bar_type) is None

    def test_bar_types_empty(self, cache):
        result = cache.bar_types(AggregationSource.EXTERNAL)
        assert result == []

    def test_has_quote_ticks_empty(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.has_quote_ticks(instrument_id) is False

    def test_has_trade_ticks_empty(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.has_trade_ticks(instrument_id) is False

    def test_has_bars_empty(self, cache):
        bar_type = TestDataProviderPyo3.bartype_ethusdt_1min_bid()
        assert cache.has_bars(bar_type) is False

    def test_has_order_book_empty(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.has_order_book(instrument_id) is False

    def test_quote_count_empty(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.quote_count(instrument_id) == 0

    def test_trade_count_empty(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.trade_count(instrument_id) == 0

    def test_bar_count_empty(self, cache):
        bar_type = TestDataProviderPyo3.bartype_ethusdt_1min_bid()
        assert cache.bar_count(bar_type) == 0

    def test_book_update_count_empty(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.book_update_count(instrument_id) == 0

    def test_price_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.price(instrument_id, PriceType.MID) is None

    def test_order_book_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.order_book(instrument_id) is None

    def test_mark_price_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.mark_price(instrument_id) is None

    def test_index_price_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.index_price(instrument_id) is None

    def test_funding_rate_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.funding_rate(instrument_id) is None

    def test_get_mark_xrate_empty_returns_none(self, cache):
        usd = Currency.from_str("USD")
        aud = Currency.from_str("AUD")
        assert cache.get_mark_xrate(usd, aud) is None

    def test_own_order_book_empty_returns_none(self, cache):
        instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
        assert cache.own_order_book(instrument_id) is None


class TestCachePyo3InstrumentQueries:
    def test_instrument_empty_returns_none(self, cache):
        instrument_id = InstrumentId.from_str("UNKNOWN.VENUE")
        assert cache.instrument(instrument_id) is None

    def test_instrument_ids_empty(self, cache):
        assert cache.instrument_ids() == []

    def test_instruments_empty(self, cache):
        assert cache.instruments() == []

    def test_synthetic_empty_returns_none(self, cache):
        instrument_id = InstrumentId.from_str("UNKNOWN.VENUE")
        assert cache.synthetic(instrument_id) is None

    def test_synthetic_ids_empty(self, cache):
        assert cache.synthetic_ids() == []


class TestCachePyo3AccountQueries:
    def test_account_empty_returns_none(self, cache):
        from nautilus_trader.core.nautilus_pyo3 import AccountId

        account_id = AccountId("SIM-001")
        assert cache.account(account_id) is None

    def test_account_for_venue_empty_returns_none(self, cache):
        assert cache.account_for_venue(Venue("SIM")) is None

    def test_account_id_empty_returns_none(self, cache):
        assert cache.account_id(Venue("SIM")) is None


class TestCachePyo3IdentifierQueries:
    def test_client_order_ids_empty(self, cache):
        assert len(cache.client_order_ids()) == 0

    def test_client_order_ids_open_empty(self, cache):
        assert len(cache.client_order_ids_open()) == 0

    def test_client_order_ids_closed_empty(self, cache):
        assert len(cache.client_order_ids_closed()) == 0

    def test_client_order_ids_emulated_empty(self, cache):
        assert len(cache.client_order_ids_emulated()) == 0

    def test_client_order_ids_inflight_empty(self, cache):
        assert len(cache.client_order_ids_inflight()) == 0

    def test_position_ids_empty(self, cache):
        assert len(cache.position_ids()) == 0

    def test_position_open_ids_empty(self, cache):
        assert len(cache.position_open_ids()) == 0

    def test_position_closed_ids_empty(self, cache):
        assert len(cache.position_closed_ids()) == 0

    def test_actor_ids_empty(self, cache):
        assert len(cache.actor_ids()) == 0

    def test_strategy_ids_empty(self, cache):
        assert len(cache.strategy_ids()) == 0

    def test_exec_algorithm_ids_empty(self, cache):
        assert len(cache.exec_algorithm_ids()) == 0

    def test_client_order_id_empty_returns_none(self, cache):
        from nautilus_trader.core.nautilus_pyo3 import VenueOrderId

        assert cache.client_order_id(VenueOrderId("V-001")) is None

    def test_venue_order_id_empty_returns_none(self, cache):
        assert cache.venue_order_id(ClientOrderId("O-001")) is None

    def test_client_id_empty_returns_none(self, cache):
        assert cache.client_id(ClientOrderId("O-001")) is None


class TestCachePyo3OrderQueries:
    def test_order_empty_returns_none(self, cache):
        assert cache.order(ClientOrderId("O-001")) is None

    def test_orders_empty(self, cache):
        assert cache.orders() == []

    def test_orders_open_empty(self, cache):
        assert cache.orders_open() == []

    def test_orders_closed_empty(self, cache):
        assert cache.orders_closed() == []

    def test_orders_emulated_empty(self, cache):
        assert cache.orders_emulated() == []

    def test_orders_inflight_empty(self, cache):
        assert cache.orders_inflight() == []

    def test_order_exists_empty(self, cache):
        assert cache.order_exists(ClientOrderId("O-001")) is False

    def test_is_order_open_empty(self, cache):
        assert cache.is_order_open(ClientOrderId("O-001")) is False

    def test_is_order_closed_empty(self, cache):
        assert cache.is_order_closed(ClientOrderId("O-001")) is False

    def test_is_order_emulated_empty(self, cache):
        assert cache.is_order_emulated(ClientOrderId("O-001")) is False

    def test_is_order_inflight_empty(self, cache):
        assert cache.is_order_inflight(ClientOrderId("O-001")) is False

    def test_is_order_pending_cancel_local_empty(self, cache):
        assert cache.is_order_pending_cancel_local(ClientOrderId("O-001")) is False

    def test_orders_open_count_empty(self, cache):
        assert cache.orders_open_count() == 0

    def test_orders_closed_count_empty(self, cache):
        assert cache.orders_closed_count() == 0

    def test_orders_emulated_count_empty(self, cache):
        assert cache.orders_emulated_count() == 0

    def test_orders_inflight_count_empty(self, cache):
        assert cache.orders_inflight_count() == 0

    def test_orders_total_count_empty(self, cache):
        assert cache.orders_total_count() == 0


class TestCachePyo3OrderListQueries:
    def test_order_list_empty_returns_none(self, cache):
        from nautilus_trader.core.nautilus_pyo3 import OrderListId

        assert cache.order_list(OrderListId("OL-001")) is None

    def test_order_lists_empty(self, cache):
        assert cache.order_lists() == []

    def test_order_list_exists_empty(self, cache):
        from nautilus_trader.core.nautilus_pyo3 import OrderListId

        assert cache.order_list_exists(OrderListId("OL-001")) is False


class TestCachePyo3PositionQueries:
    def test_position_empty_returns_none(self, cache):
        assert cache.position(PositionId("P-001")) is None

    def test_position_for_order_empty_returns_none(self, cache):
        assert cache.position_for_order(ClientOrderId("O-001")) is None

    def test_position_id_empty_returns_none(self, cache):
        assert cache.position_id(ClientOrderId("O-001")) is None

    def test_positions_empty(self, cache):
        assert cache.positions() == []

    def test_positions_open_empty(self, cache):
        assert cache.positions_open() == []

    def test_positions_closed_empty(self, cache):
        assert cache.positions_closed() == []

    def test_position_exists_empty(self, cache):
        assert cache.position_exists(PositionId("P-001")) is False

    def test_is_position_open_empty(self, cache):
        assert cache.is_position_open(PositionId("P-001")) is False

    def test_is_position_closed_empty(self, cache):
        assert cache.is_position_closed(PositionId("P-001")) is False

    def test_positions_open_count_empty(self, cache):
        assert cache.positions_open_count() == 0

    def test_positions_closed_count_empty(self, cache):
        assert cache.positions_closed_count() == 0

    def test_positions_total_count_empty(self, cache):
        assert cache.positions_total_count() == 0


class TestCachePyo3StrategyQueries:
    def test_strategy_id_for_order_empty_returns_none(self, cache):
        assert cache.strategy_id_for_order(ClientOrderId("O-001")) is None

    def test_strategy_id_for_position_empty_returns_none(self, cache):
        assert cache.strategy_id_for_position(PositionId("P-001")) is None
