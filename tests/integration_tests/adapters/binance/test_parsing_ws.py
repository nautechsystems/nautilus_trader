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

import pkgutil

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTickerData
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesTradeLiteMsg
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotOrderUpdateWrapper
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT = TestInstrumentProvider.ethusdt_binance()


class TestBinanceWebSocketParsing:
    def test_parse_ticker(self):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_ticker_24hr.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceTickerData)
        data = decoder.decode(raw)
        result = data.parse_to_binance_ticker(
            instrument_id=ETHUSDT.id,
            ts_init=9999999999999991,
        )

        # Assert
        assert result.instrument_id == ETHUSDT.id

    def test_parse_trade_lite(self):
        # Arrange
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_futures_trade_lite.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceFuturesTradeLiteMsg)
        data = decoder.decode(raw)

        # Assert
        assert data.s == "ETHUSDT"

    def test_parse_spot_execution_report_binance_us(self):
        # Arrange: Load Binance US execution report with W and V fields
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_binance_us.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Assert
        assert wrapper.data.e.value == "executionReport"
        assert wrapper.data.s == "BTCUSD"
        assert wrapper.data.x == BinanceExecutionType.TRADE
        assert wrapper.data.l == "0.00042000"
        assert wrapper.data.L == "117290.77000000"
        assert wrapper.data.W == 1759347763167  # Working time field
        assert wrapper.data.V == "EXPIRE_MAKER"  # Self-Trade Prevention Mode

    def test_parse_to_order_status_report_with_filled_status(self):
        # Arrange: Load Binance US execution report with FILLED status
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_binance_us.json",
        )
        assert raw

        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Act: Parse to OrderStatusReport
        from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEnumParser
        from nautilus_trader.core.datetime import millis_to_nanos
        from nautilus_trader.model.identifiers import AccountId
        from nautilus_trader.model.identifiers import ClientOrderId
        from nautilus_trader.model.identifiers import VenueOrderId

        enum_parser = BinanceSpotEnumParser()
        report = wrapper.data.parse_to_order_status_report(
            account_id=AccountId("BINANCE-001"),
            instrument_id=ETHUSDT.id,
            client_order_id=ClientOrderId("test-001"),
            venue_order_id=VenueOrderId("12345"),
            ts_event=millis_to_nanos(wrapper.data.T),
            ts_init=0,
            enum_parser=enum_parser,
        )

        # Assert: Status should be FILLED, not ACCEPTED
        from decimal import Decimal

        from nautilus_trader.model.enums import OrderStatus

        assert report.order_status == OrderStatus.FILLED
        assert report.filled_qty.as_decimal() == Decimal(wrapper.data.z)
        assert report.quantity.as_decimal() == Decimal(wrapper.data.q)

    def test_parse_to_order_status_report_with_rejected_status(self):
        # Arrange: Load execution report with REJECTED status (e.g., GTX post-only order)
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_rejected.json",
        )
        assert raw

        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Act: Parse to OrderStatusReport
        from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEnumParser
        from nautilus_trader.core.datetime import millis_to_nanos
        from nautilus_trader.model.identifiers import AccountId
        from nautilus_trader.model.identifiers import ClientOrderId
        from nautilus_trader.model.identifiers import VenueOrderId

        enum_parser = BinanceSpotEnumParser()
        report = wrapper.data.parse_to_order_status_report(
            account_id=AccountId("BINANCE-001"),
            instrument_id=ETHUSDT.id,
            client_order_id=ClientOrderId("test-reject"),
            venue_order_id=VenueOrderId("1234567890"),
            ts_event=millis_to_nanos(wrapper.data.T),
            ts_init=0,
            enum_parser=enum_parser,
        )

        # Assert: Status should be REJECTED (not crash with RuntimeError)
        from nautilus_trader.model.enums import OrderStatus

        assert report.order_status == OrderStatus.REJECTED
        assert wrapper.data.X == BinanceOrderStatus.REJECTED
        assert wrapper.data.r == "GTX_ORDER_REJECT"

    def test_parse_to_order_status_report_with_pending_cancel_status(self):
        # Arrange: Load execution report with PENDING_CANCEL status
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_pending_cancel.json",
        )
        assert raw

        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Act: Parse to OrderStatusReport
        from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEnumParser
        from nautilus_trader.core.datetime import millis_to_nanos
        from nautilus_trader.model.identifiers import AccountId
        from nautilus_trader.model.identifiers import ClientOrderId
        from nautilus_trader.model.identifiers import VenueOrderId

        enum_parser = BinanceSpotEnumParser()
        report = wrapper.data.parse_to_order_status_report(
            account_id=AccountId("BINANCE-001"),
            instrument_id=ETHUSDT.id,
            client_order_id=ClientOrderId("test-cancel"),
            venue_order_id=VenueOrderId("9876543210"),
            ts_event=millis_to_nanos(wrapper.data.T),
            ts_init=0,
            enum_parser=enum_parser,
        )

        # Assert: Status should be PENDING_CANCEL (not crash with RuntimeError)
        from nautilus_trader.model.enums import OrderStatus

        assert report.order_status == OrderStatus.PENDING_CANCEL
        assert wrapper.data.X == BinanceOrderStatus.PENDING_CANCEL

    def test_parse_spot_execution_report_trade_with_l_zero(self):
        # Arrange: Load execution report with TRADE execution type but L=0
        # This can occur with self-trade prevention or other edge cases
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_l_zero.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Assert: Message should parse successfully
        assert wrapper.data.e.value == "executionReport"
        assert wrapper.data.x == BinanceExecutionType.TRADE
        assert wrapper.data.L == "0.00000000"  # Last filled price is zero

        from decimal import Decimal

        assert Decimal(wrapper.data.L) == 0

    def test_parse_spot_execution_report_calculated(self):
        # Arrange: Load execution report with CALCULATED (liquidation) execution type
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_calculated.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Assert: Message should parse successfully
        assert wrapper.data.e.value == "executionReport"
        assert wrapper.data.s == "BTCUSDT"
        assert wrapper.data.x == BinanceExecutionType.CALCULATED
        assert wrapper.data.X == BinanceOrderStatus.FILLED
        assert wrapper.data.c.startswith("autoclose-")  # Liquidation order client ID
        assert wrapper.data.l == "0.01000000"
        assert wrapper.data.L == "49500.00000000"
        assert wrapper.data.m is False  # Liquidations are taker

        from decimal import Decimal

        assert Decimal(wrapper.data.L) > 0

    def test_parse_spot_execution_report_trade_prevention(self):
        # Arrange: Load execution report with TRADE_PREVENTION execution type
        raw = pkgutil.get_data(
            package="tests.integration_tests.adapters.binance.resources.ws_messages",
            resource="ws_spot_execution_report_trade_prevention.json",
        )
        assert raw

        # Act
        decoder = msgspec.json.Decoder(BinanceSpotOrderUpdateWrapper)
        wrapper = decoder.decode(raw)

        # Assert: Message should parse successfully
        assert wrapper.data.e.value == "executionReport"
        assert wrapper.data.s == "ETHUSDT"
        assert wrapper.data.x == BinanceExecutionType.TRADE_PREVENTION
        assert wrapper.data.X == BinanceOrderStatus.EXPIRED_IN_MATCH
        assert wrapper.data.r == "SELF_TRADE_PREVENTION"
        assert wrapper.data.l == "0.50000000"  # Prevented quantity
        assert wrapper.data.z == "0.00000000"  # No actual fill occurred
        assert wrapper.data.t == -1  # No trade ID
        assert wrapper.data.V == "EXPIRE_MAKER"  # Self-Trade Prevention Mode
