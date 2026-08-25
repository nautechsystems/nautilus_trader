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
"""
Test tester configs behavior.
"""

from decimal import Decimal

from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.testkit import DataTesterConfig
from nautilus_trader.testkit import ExecTesterConfig


def test_data_tester_config_readback() -> None:
    """
    Test data tester config readback.
    """
    actor_id = ActorId("DATA-TESTER-001")
    client_id = ClientId("DATA-001")
    instrument_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    config = DataTesterConfig(
        actor_id=actor_id,
        client_id=client_id,
        instrument_ids=[instrument_id],
        subscribe_book_deltas=True,
        subscribe_book_depth=True,
        subscribe_book_at_interval=True,
        subscribe_quotes=True,
        subscribe_trades=True,
        subscribe_mark_prices=True,
        subscribe_index_prices=True,
        subscribe_funding_rates=True,
        subscribe_bars=True,
        subscribe_instrument=True,
        subscribe_instrument_status=True,
        subscribe_instrument_close=True,
        subscribe_option_greeks=True,
        can_unsubscribe=False,
        request_instruments=True,
        request_quotes=True,
        request_trades=True,
        request_bars=True,
        request_book_snapshot=True,
        request_book_deltas=True,
        request_funding_rates=True,
        book_depth=10,
        book_interval_ms=20,
        book_levels_to_print=30,
        manage_book=False,
        log_data=False,
        stats_interval_secs=40,
        log_events=False,
        log_commands=False,
    )

    assert config.actor_id == actor_id
    assert config.client_id == client_id
    assert config.instrument_ids == [instrument_id]
    assert config.bar_types is None
    assert config.subscribe_book_deltas is True
    assert config.subscribe_book_depth is True
    assert config.subscribe_book_at_interval is True
    assert config.subscribe_quotes is True
    assert config.subscribe_trades is True
    assert config.subscribe_mark_prices is True
    assert config.subscribe_index_prices is True
    assert config.subscribe_funding_rates is True
    assert config.subscribe_bars is True
    assert config.subscribe_instrument is True
    assert config.subscribe_instrument_status is True
    assert config.subscribe_instrument_close is True
    assert config.subscribe_option_greeks is True
    assert config.can_unsubscribe is False
    assert config.request_instruments is True
    assert config.request_quotes is True
    assert config.request_trades is True
    assert config.request_bars is True
    assert config.request_book_snapshot is True
    assert config.request_book_deltas is True
    assert config.request_funding_rates is True
    assert config.book_depth == 10
    assert config.book_interval_ms == 20
    assert config.book_levels_to_print == 30
    assert config.manage_book is False
    assert config.log_data is False
    assert config.stats_interval_secs == 40
    assert config.log_events is False
    assert config.log_commands is False


def test_exec_tester_config_readback() -> None:
    """
    Test exec tester config readback.
    """
    strategy_id = StrategyId("EXEC-TESTER-001")
    instrument_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    client_id = ClientId("EXEC-001")
    config = ExecTesterConfig(
        strategy_id=strategy_id,
        order_id_tag="001",
        use_hyphens_in_client_order_ids=False,
        use_uuid_client_order_ids=True,
        external_order_claims=[instrument_id],
        instrument_id=instrument_id,
        client_id=client_id,
        order_qty=Quantity.from_str("0.01"),
        subscribe_book=True,
        subscribe_quotes=False,
        subscribe_trades=False,
        open_position_on_start_qty=Decimal(1),
        open_position_on_first_quote=True,
        open_position_time_in_force=TimeInForce.IOC,
        enable_limit_buys=False,
        enable_limit_sells=False,
        enable_stop_buys=True,
        enable_stop_sells=True,
        tob_offset_ticks=2,
        limit_time_in_force=TimeInForce.FOK,
        use_post_only=True,
        limit_aggressive=True,
        use_quote_quantity=True,
        use_individual_cancels_on_stop=True,
        cancel_orders_on_stop=False,
        close_positions_on_stop=False,
        close_positions_qty_precision=2,
        close_positions_time_in_force=TimeInForce.IOC,
        reduce_only_on_stop=False,
        dry_run=True,
        log_data=False,
        can_unsubscribe=False,
        clamp_to_instrument_price_range=True,
        log_events=False,
        log_commands=False,
    )

    assert config.strategy_id == strategy_id
    assert config.order_id_tag == "001"
    assert config.use_hyphens_in_client_order_ids is False
    assert config.use_uuid_client_order_ids is True
    assert config.external_order_claims == [instrument_id]
    assert config.instrument_id == instrument_id
    assert config.client_id == client_id
    assert config.order_qty == Quantity.from_str("0.01")
    assert config.subscribe_book is True
    assert config.subscribe_quotes is False
    assert config.subscribe_trades is False
    assert config.open_position_on_start_qty == Decimal(1)
    assert config.open_position_on_first_quote is True
    assert config.open_position_time_in_force == TimeInForce.IOC
    assert config.enable_limit_buys is False
    assert config.enable_limit_sells is False
    assert config.enable_stop_buys is True
    assert config.enable_stop_sells is True
    assert config.tob_offset_ticks == 2
    assert config.limit_time_in_force == TimeInForce.FOK
    assert config.use_post_only is True
    assert config.limit_aggressive is True
    assert config.use_quote_quantity is True
    assert config.use_individual_cancels_on_stop is True
    assert config.cancel_orders_on_stop is False
    assert config.close_positions_on_stop is False
    assert config.close_positions_qty_precision == 2
    assert config.close_positions_time_in_force == TimeInForce.IOC
    assert config.reduce_only_on_stop is False
    assert config.dry_run is True
    assert config.log_data is False
    assert config.can_unsubscribe is False
    assert config.clamp_to_instrument_price_range is True
    assert config.log_events is False
    assert config.log_commands is False


def test_exec_tester_config_defaults_to_hyphenated_client_order_ids() -> None:
    """
    Test exec tester config defaults to hyphenated client order ids.
    """
    config = ExecTesterConfig()

    assert config.use_hyphens_in_client_order_ids is True


def test_exec_tester_config_disables_hyphens_in_client_order_ids() -> None:
    """
    Test exec tester config disables hyphens in client order ids.
    """
    config = ExecTesterConfig(use_hyphens_in_client_order_ids=False)

    assert config.use_hyphens_in_client_order_ids is False


def test_exec_tester_config_uses_uuid_client_order_ids() -> None:
    """
    Test exec tester config uses uuid client order ids.
    """
    config = ExecTesterConfig(use_uuid_client_order_ids=True)

    assert config.use_uuid_client_order_ids is True


def test_exec_tester_config_uses_quote_quantity() -> None:
    """
    Test exec tester config uses quote quantity.
    """
    config = ExecTesterConfig(use_quote_quantity=True)

    assert config.use_quote_quantity is True


def test_exec_tester_config_uses_individual_cancels_on_stop() -> None:
    """
    Test exec tester config uses individual cancels on stop.
    """
    config = ExecTesterConfig(use_individual_cancels_on_stop=True)

    assert config.use_individual_cancels_on_stop is True
