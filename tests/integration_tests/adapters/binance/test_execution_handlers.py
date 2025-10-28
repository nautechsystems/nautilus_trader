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

import json
import pkgutil
from unittest.mock import call

import msgspec

from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesOrderUpdateWrapper
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotOrderUpdateWrapper
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestBinanceSpotExecutionHandlers:
    """
    Tests for Binance Spot execution report handler methods with mocked dependencies.
    """

    def test_trade_execution_generates_fill_with_correct_params(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.BUY
        exec_client._enum_parser.parse_binance_order_type.return_value = mocker.MagicMock()

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_filled.assert_called_once()
        call_kwargs = exec_client.generate_order_filled.call_args.kwargs
        assert call_kwargs["last_qty"] == Quantity.from_str("0.50000000")
        assert call_kwargs["last_px"] == Price.from_str("2499.50000000")
        assert call_kwargs["liquidity_side"] == LiquiditySide.MAKER  # m=true in test data

    def test_trade_execution_with_l_zero_filled_generates_fill_with_warning(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_l_zero.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.BUY
        exec_client._enum_parser.parse_binance_order_type.return_value = mocker.MagicMock()

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert - Terminal FILLED status with L=0 should generate fill to close order
        exec_client.generate_order_filled.assert_called_once()
        assert exec_client._log.warning.call_count == 2
        warning_calls = [call[0][0] for call in exec_client._log.warning.call_args_list]
        assert any("L=0" in msg for msg in warning_calls)
        assert any("Generating OrderFilled with L=0" in msg for msg in warning_calls)

        # Verify fill has L=0
        call_kwargs = exec_client.generate_order_filled.call_args.kwargs
        assert call_kwargs["last_px"] == Price.from_str("0.00000000")
        assert call_kwargs["last_qty"] == Quantity.from_str("0.00100000")

    def test_trade_execution_with_l_zero_canceled_generates_order_canceled(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_l_zero_canceled.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert - Terminal CANCELED status with L=0 should generate order_canceled
        exec_client.generate_order_canceled.assert_called_once()
        exec_client.generate_order_filled.assert_not_called()
        exec_client._log.warning.assert_called_once()
        assert "L=0" in exec_client._log.warning.call_args[0][0]

    def test_trade_execution_with_l_zero_expired_generates_order_expired(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_l_zero_expired.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert - Terminal EXPIRED status with L=0 should generate order_expired
        exec_client.generate_order_expired.assert_called_once()
        exec_client.generate_order_filled.assert_not_called()
        exec_client._log.warning.assert_called_once()
        assert "L=0" in exec_client._log.warning.call_args[0][0]

    def test_trade_execution_with_l_zero_non_terminal_skips_fill(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_l_zero_new.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert - Non-terminal status with L=0 should skip fill generation
        exec_client.generate_order_filled.assert_not_called()
        exec_client.generate_order_canceled.assert_not_called()
        exec_client.generate_order_expired.assert_not_called()
        exec_client._log.warning.assert_called_once()
        assert "L=0" in exec_client._log.warning.call_args[0][0]

    def test_calculated_execution_generates_fill_with_taker_side(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_calculated.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = BTCUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.SELL
        exec_client._enum_parser.parse_binance_order_type.return_value = mocker.MagicMock()

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client._log.info.assert_called_once()
        assert "CALCULATED" in exec_client._log.info.call_args[0][0]
        assert "liquidation" in exec_client._log.info.call_args[0][0]

        exec_client.generate_order_filled.assert_called_once()
        call_kwargs = exec_client.generate_order_filled.call_args.kwargs
        assert call_kwargs["last_qty"] == Quantity.from_str("0.01000000")
        assert call_kwargs["last_px"] == Price.from_str("49500.00000000")
        assert call_kwargs["liquidity_side"] == LiquiditySide.TAKER  # Liquidations always taker

    def test_trade_prevention_logs_but_no_fill(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_prevention.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_filled.assert_not_called()
        exec_client._log.info.assert_called_once()
        assert "Self-trade prevention" in exec_client._log.info.call_args[0][0]

    def test_canceled_execution_generates_order_canceled(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_canceled.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_canceled.assert_called_once()
        call_kwargs = exec_client.generate_order_canceled.call_args.kwargs
        assert call_kwargs["client_order_id"] == ClientOrderId(wrapper.data.c)

    def test_expired_execution_generates_order_expired(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_expired.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_expired.assert_called_once()
        call_kwargs = exec_client.generate_order_expired.call_args.kwargs
        assert call_kwargs["client_order_id"] == ClientOrderId(wrapper.data.c)

    def test_rejected_execution_generates_order_rejected_with_post_only_flag(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_rejected.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_rejected.assert_called_once()
        call_kwargs = exec_client.generate_order_rejected.call_args.kwargs
        assert call_kwargs["client_order_id"] == ClientOrderId(wrapper.data.c)
        assert call_kwargs["reason"] == "GTX_ORDER_REJECT"
        assert call_kwargs["due_post_only"] is True  # GTX order rejected

    def test_new_execution_limit_order_price_match_generates_order_updated(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_new_price_match.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked order with different price than in message
        mock_order = mocker.MagicMock()
        mock_order.order_type = mocker.MagicMock()
        mock_order.order_type.name = "LIMIT"
        mock_order.price = Price.from_str("2500.00000000")  # Original price
        mock_order.quantity = Quantity.from_str("1.00000000")
        mock_order.has_price = True  # LIMIT orders have prices
        mock_order.has_trigger_price = False  # LIMIT orders don't have trigger prices

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._cache.order.return_value = mock_order
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE

        # Mock order type to be LIMIT
        from nautilus_trader.model.enums import OrderType

        mock_order.order_type = OrderType.LIMIT

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_accepted.assert_called_once()
        exec_client.generate_order_updated.assert_called_once()

        # Verify OrderUpdated was called with new price
        update_kwargs = exec_client.generate_order_updated.call_args.kwargs
        assert update_kwargs["price"] == Price.from_str("2495.50000000")  # Price from message
        assert update_kwargs["quantity"] == mock_order.quantity
        assert update_kwargs["trigger_price"] is None  # LIMIT order has no trigger

    def test_new_execution_stop_limit_order_price_match_preserves_trigger_price(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_new_stop_limit_price_match.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked order with different price than in message
        mock_order = mocker.MagicMock()
        mock_order.price = Price.from_str("2410.00000000")  # Original limit price
        mock_order.trigger_price = Price.from_str("2400.00000000")  # Original stop price
        mock_order.quantity = Quantity.from_str("1.00000000")
        mock_order.has_price = True  # STOP_LIMIT orders have prices
        mock_order.has_trigger_price = True  # STOP_LIMIT orders have trigger prices

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._cache.order.return_value = mock_order
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE

        # Mock order type to be STOP_LIMIT
        from nautilus_trader.model.enums import OrderType

        mock_order.order_type = OrderType.STOP_LIMIT

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_accepted.assert_called_once()
        exec_client.generate_order_updated.assert_called_once()

        # Verify OrderUpdated was called with new price but preserved trigger price
        update_kwargs = exec_client.generate_order_updated.call_args.kwargs
        assert update_kwargs["price"] == Price.from_str(
            "2405.75000000",
        )  # New limit price from message
        assert update_kwargs["trigger_price"] == Price.from_str(
            "2400.00000000",
        )  # Preserved stop price
        assert update_kwargs["quantity"] == mock_order.quantity

    def test_new_execution_limit_if_touched_order_price_match_preserves_trigger_price(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_new_limit_if_touched_price_match.json",
        )
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked order with different price than in message
        mock_order = mocker.MagicMock()
        mock_order.price = Price.from_str("2510.00000000")  # Original limit price
        mock_order.trigger_price = Price.from_str("2500.00000000")  # Original trigger price
        mock_order.quantity = Quantity.from_str("1.00000000")
        mock_order.has_price = True  # LIMIT_IF_TOUCHED orders have prices
        mock_order.has_trigger_price = True  # LIMIT_IF_TOUCHED orders have trigger prices

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._cache.order.return_value = mock_order
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE

        # Mock order type to be LIMIT_IF_TOUCHED
        from nautilus_trader.model.enums import OrderType

        mock_order.order_type = OrderType.LIMIT_IF_TOUCHED

        # Act
        wrapper.data.handle_execution_report(exec_client)

        # Assert
        exec_client.generate_order_accepted.assert_called_once()
        exec_client.generate_order_updated.assert_called_once()

        # Verify OrderUpdated was called with new price but preserved trigger price
        update_kwargs = exec_client.generate_order_updated.call_args.kwargs
        assert update_kwargs["price"] == Price.from_str(
            "2505.25000000",
        )  # New limit price from message
        assert update_kwargs["trigger_price"] == Price.from_str(
            "2500.00000000",
        )  # Preserved trigger price
        assert update_kwargs["quantity"] == mock_order.quantity


class TestBinanceFuturesExecutionHandlers:
    """
    Tests for Binance Futures ORDER_TRADE_UPDATE handler methods with mocked
    dependencies.
    """

    def test_liquidation_order_sends_order_status_then_fill_report(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_liquidation.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        exec_client = mocker.MagicMock()
        exec_client.account_id = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = None
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = BTCUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.SELL
        exec_client._clock.timestamp_ns.return_value = 1759347763167000000
        exec_client.use_position_ids = False

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client._log.warning.assert_called_once()
        assert "Received liquidation order" in exec_client._log.warning.call_args[0][0]
        assert "autoclose-" in exec_client._log.warning.call_args[0][0]

        exec_client._send_order_status_report.assert_called_once()
        order_report = exec_client._send_order_status_report.call_args[0][0]
        assert order_report.client_order_id == ClientOrderId("autoclose-1234567890123456")
        assert order_report.venue_order_id == VenueOrderId("9876543210")

        exec_client._send_fill_report.assert_called_once()
        fill_report = exec_client._send_fill_report.call_args[0][0]
        assert fill_report.last_qty == Quantity.from_str("0.100")
        assert fill_report.last_px == Price.from_str("50000.00")
        assert fill_report.liquidity_side == LiquiditySide.TAKER
        assert fill_report.client_order_id == ClientOrderId("autoclose-1234567890123456")

        # Verify OrderStatusReport sent before FillReport
        status_call = call._send_order_status_report(order_report)
        fill_call = call._send_fill_report(fill_report)
        status_idx = exec_client.mock_calls.index(status_call)
        fill_idx = exec_client.mock_calls.index(fill_call)
        assert status_idx < fill_idx, "OrderStatusReport must be sent before FillReport"

    def test_adl_order_sends_order_status_then_fill_report(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_adl.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        exec_client = mocker.MagicMock()
        exec_client.account_id = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = None
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.BUY
        exec_client._clock.timestamp_ns.return_value = 1759347763200000000
        exec_client.use_position_ids = False

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client._log.warning.assert_called_once()
        assert "Received ADL order" in exec_client._log.warning.call_args[0][0]
        assert "adl_autoclose" in exec_client._log.warning.call_args[0][0]

        exec_client._send_order_status_report.assert_called_once()
        order_report = exec_client._send_order_status_report.call_args[0][0]
        assert order_report.client_order_id == ClientOrderId("adl_autoclose")

        exec_client._send_fill_report.assert_called_once()
        fill_report = exec_client._send_fill_report.call_args[0][0]
        assert fill_report.last_qty == Quantity.from_str("1.000")
        assert fill_report.last_px == Price.from_str("2500.00")
        assert fill_report.liquidity_side == LiquiditySide.TAKER
        assert fill_report.client_order_id == ClientOrderId("adl_autoclose")

        # Verify OrderStatusReport sent before FillReport
        status_call = call._send_order_status_report(order_report)
        fill_call = call._send_fill_report(fill_report)
        status_idx = exec_client.mock_calls.index(status_call)
        fill_idx = exec_client.mock_calls.index(fill_call)
        assert status_idx < fill_idx, "OrderStatusReport must be sent before FillReport"

    def test_settlement_order_sends_order_status_then_fill_report(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_settlement.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        exec_client = mocker.MagicMock()
        exec_client.account_id = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = None
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = BTCUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.BUY
        exec_client._clock.timestamp_ns.return_value = 1759347763300000000
        exec_client.use_position_ids = False

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client._log.warning.assert_called_once()
        assert "Received settlement order" in exec_client._log.warning.call_args[0][0]
        assert "settlement_autoclose-" in exec_client._log.warning.call_args[0][0]

        exec_client._send_order_status_report.assert_called_once()
        order_report = exec_client._send_order_status_report.call_args[0][0]

        exec_client._send_fill_report.assert_called_once()
        fill_report = exec_client._send_fill_report.call_args[0][0]
        assert fill_report.last_qty == Quantity.from_str("0.050")
        assert fill_report.last_px == Price.from_str("51000.00")
        assert fill_report.liquidity_side == LiquiditySide.TAKER

        # Verify OrderStatusReport sent before FillReport
        status_call = call._send_order_status_report(order_report)
        fill_call = call._send_fill_report(fill_report)
        status_idx = exec_client.mock_calls.index(status_call)
        fill_idx = exec_client.mock_calls.index(fill_call)
        assert status_idx < fill_idx, "OrderStatusReport must be sent before FillReport"

    def test_liquidation_order_zero_quantity_skipped(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_liquidation_zero_qty.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = None
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = BTCUSDT_BINANCE

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        warning_calls = [call[0][0] for call in exec_client._log.warning.call_args_list]
        assert any("Received liquidation order" in msg for msg in warning_calls)
        assert any("l=0" in msg for msg in warning_calls)

        exec_client._send_order_status_report.assert_not_called()
        exec_client._send_fill_report.assert_not_called()

    def test_liquidation_fill_commission_calculated_when_not_provided(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_liquidation.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        data = json.loads(raw)
        data["data"]["o"]["N"] = None
        data["data"]["o"]["n"] = None
        wrapper = decoder.decode(json.dumps(data).encode())

        exec_client = mocker.MagicMock()
        exec_client.account_id = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = None
        exec_client._get_cached_instrument_id.return_value = BTCUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = BTCUSDT_BINANCE
        exec_client._enum_parser.parse_binance_order_side.return_value = OrderSide.SELL
        exec_client._clock.timestamp_ns.return_value = 1759347763167000000
        exec_client.use_position_ids = False

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client._send_fill_report.assert_called_once()
        fill_report = exec_client._send_fill_report.call_args[0][0]
        expected_commission = float(
            Quantity.from_str("0.100") * Price.from_str("50000.00") * BTCUSDT_BINANCE.taker_fee,
        )
        assert fill_report.commission.as_double() == expected_commission

        # Verify OrderStatusReport sent before FillReport
        exec_client._send_order_status_report.assert_called_once()
        order_report = exec_client._send_order_status_report.call_args[0][0]
        status_call = call._send_order_status_report(order_report)
        fill_call = call._send_fill_report(fill_report)
        status_idx = exec_client.mock_calls.index(status_call)
        fill_idx = exec_client.mock_calls.index(fill_call)
        assert status_idx < fill_idx, "OrderStatusReport must be sent before FillReport"

    def test_new_execution_limit_order_price_match_generates_order_updated(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_new_price_match.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked order with different price than in message
        mock_order = mocker.MagicMock()
        mock_order.price = Price.from_str("2500.00")  # Original price
        mock_order.quantity = Quantity.from_str("1.000")
        mock_order.has_price = True  # LIMIT orders have prices
        mock_order.has_trigger_price = False  # LIMIT orders don't have trigger prices

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._cache.order.return_value = mock_order
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE

        # Mock order type to be LIMIT
        from nautilus_trader.model.enums import OrderType

        mock_order.order_type = OrderType.LIMIT

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client.generate_order_accepted.assert_called_once()
        exec_client.generate_order_updated.assert_called_once()

        # Verify OrderUpdated was called with new price
        update_kwargs = exec_client.generate_order_updated.call_args.kwargs
        assert update_kwargs["price"] == Price.from_str("2495.50")  # Price from message
        assert update_kwargs["quantity"] == mock_order.quantity
        assert update_kwargs["trigger_price"] is None  # LIMIT order has no trigger

    def test_new_execution_stop_limit_order_price_match_preserves_trigger_price(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_new_stop_limit_price_match.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked order with different price than in message
        mock_order = mocker.MagicMock()
        mock_order.price = Price.from_str("2410.00")  # Original limit price
        mock_order.trigger_price = Price.from_str("2400.00")  # Original stop price
        mock_order.quantity = Quantity.from_str("1.000")
        mock_order.has_price = True  # STOP_LIMIT orders have prices
        mock_order.has_trigger_price = True  # STOP_LIMIT orders have trigger prices

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._cache.order.return_value = mock_order
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE

        # Mock order type to be STOP_LIMIT
        from nautilus_trader.model.enums import OrderType

        mock_order.order_type = OrderType.STOP_LIMIT

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client.generate_order_accepted.assert_called_once()
        exec_client.generate_order_updated.assert_called_once()

        # Verify OrderUpdated was called with new price but preserved trigger price
        update_kwargs = exec_client.generate_order_updated.call_args.kwargs
        assert update_kwargs["price"] == Price.from_str("2405.75")  # New limit price from message
        assert update_kwargs["trigger_price"] == Price.from_str("2400.00")  # Preserved stop price
        assert update_kwargs["quantity"] == mock_order.quantity

    def test_new_execution_limit_if_touched_order_price_match_preserves_trigger_price(self, mocker):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_order_update_new_limit_if_touched_price_match.json",
        )
        decoder = msgspec.json.Decoder(BinanceFuturesOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Create mocked order with different price than in message
        mock_order = mocker.MagicMock()
        mock_order.price = Price.from_str("2510.00")  # Original limit price
        mock_order.trigger_price = Price.from_str("2500.00")  # Original trigger price
        mock_order.quantity = Quantity.from_str("1.000")
        mock_order.has_price = True  # LIMIT_IF_TOUCHED orders have prices
        mock_order.has_trigger_price = True  # LIMIT_IF_TOUCHED orders have trigger prices

        # Create mocked exec_client
        exec_client = mocker.MagicMock()
        exec_client._cache.strategy_id_for_order.return_value = StrategyId("S-001")
        exec_client._cache.order.return_value = mock_order
        exec_client._get_cached_instrument_id.return_value = ETHUSDT_BINANCE.id
        exec_client._instrument_provider.find.return_value = ETHUSDT_BINANCE

        # Mock order type to be LIMIT_IF_TOUCHED
        from nautilus_trader.model.enums import OrderType

        mock_order.order_type = OrderType.LIMIT_IF_TOUCHED

        # Act
        wrapper.data.o.handle_order_trade_update(exec_client)

        # Assert
        exec_client.generate_order_accepted.assert_called_once()
        exec_client.generate_order_updated.assert_called_once()

        # Verify OrderUpdated was called with new price but preserved trigger price
        update_kwargs = exec_client.generate_order_updated.call_args.kwargs
        assert update_kwargs["price"] == Price.from_str("2505.25")  # New limit price from message
        assert update_kwargs["trigger_price"] == Price.from_str(
            "2500.00",
        )  # Preserved trigger price
        assert update_kwargs["quantity"] == mock_order.quantity
