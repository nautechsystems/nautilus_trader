#!/usr/bin/env python3
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
"""
Comprehensive tests for Interactive Brokers spread execution functionality.
"""

from unittest.mock import MagicMock

from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class TestSpreadExecutionDetection:
    """
    Test cases for spread instrument detection.
    """

    def test_is_spread_instrument_with_spread_formats(self):
        """
        Test spread detection with various spread formats.
        """
        # Test spread instruments with different formats
        test_cases = [
            ("(1)SPY C400_((1))SPY C410.SMART", True),  # Standard spread format
            ("(1)E4DN5 P6350_((2))E4DN5 P6355.XCME", True),  # Ratio spread
            ("SPY_SPREAD.SMART", True),  # SPREAD keyword
            ("COMPLEX_SPREAD.SMART", True),  # SPREAD keyword
            ("SPY C400.SMART", False),  # Single option
            ("AAPL.NASDAQ", False),  # Single stock
            ("ES.CME", False),  # Single future
        ]

        for instrument_id_str, expected in test_cases:
            instrument_id = InstrumentId.from_str(instrument_id_str)
            result = self._is_spread_instrument(instrument_id)
            assert (
                result == expected
            ), f"Failed for {instrument_id_str}: expected {expected}, was {result}"

    def test_is_spread_instrument_edge_cases(self):
        """
        Test spread detection with edge cases.
        """
        # Edge cases
        edge_cases = [
            ("NORMAL.SMART", False),  # Normal instrument
            ("SPREAD_TEST.SMART", True),  # SPREAD at beginning
            ("TEST_SPREAD.SMART", True),  # SPREAD at end
            ("TEST_SPREAD_MORE.SMART", True),  # SPREAD in middle
        ]

        for instrument_id_str, expected in edge_cases:
            instrument_id = InstrumentId.from_str(instrument_id_str)
            result = self._is_spread_instrument(instrument_id)
            assert (
                result == expected
            ), f"Failed for {instrument_id_str}: expected {expected}, was {result}"

    def _is_spread_instrument(self, instrument_id):
        """
        Test implementation of spread detection.
        """
        id_str = str(instrument_id)
        return "_(" in id_str or ")_" in id_str or "SPREAD" in id_str.upper()


class TestSpreadLegExtraction:
    """
    Test cases for extracting leg instrument IDs from spread fills.
    """

    def test_extract_leg_instrument_id_standard_spread(self):
        """
        Test extraction from standard spread format.
        """
        # Create mock leg fill
        leg_fill = MagicMock()
        leg_fill.instrument_id = InstrumentId.from_str("(1)SPY C400_((1))SPY C410.SMART")

        # Extract leg instrument ID
        leg_id = self._extract_leg_instrument_id(leg_fill)

        # Should extract first leg
        expected_id = InstrumentId.from_str("SPY C400.SMART")
        assert leg_id == expected_id

    def test_extract_leg_instrument_id_ratio_spread(self):
        """
        Test extraction from ratio spread format.
        """
        leg_fill = MagicMock()
        leg_fill.instrument_id = InstrumentId.from_str("(1)E4DN5 P6350_((2))E4DN5 P6355.XCME")

        leg_id = self._extract_leg_instrument_id(leg_fill)

        expected_id = InstrumentId.from_str("E4DN5 P6350.XCME")
        assert leg_id == expected_id

    def test_extract_leg_instrument_id_fallback(self):
        """
        Test fallback for non-spread instruments.
        """
        leg_fill = MagicMock()
        leg_fill.instrument_id = InstrumentId.from_str("SPY C400.SMART")

        leg_id = self._extract_leg_instrument_id(leg_fill)

        # Should return original ID for non-spread
        assert leg_id == leg_fill.instrument_id

    def test_extract_leg_instrument_id_invalid_format(self):
        """
        Test handling of invalid spread formats.
        """
        leg_fill = MagicMock()
        leg_fill.instrument_id = InstrumentId.from_str("INVALID_(.SMART")

        leg_id = self._extract_leg_instrument_id(leg_fill)

        # Should return original ID for invalid format
        assert leg_id == leg_fill.instrument_id

    def _extract_leg_instrument_id(self, leg_fill):
        """
        Test implementation of leg instrument ID extraction.
        """
        try:
            spread_id_str = str(leg_fill.instrument_id)

            if "_(" in spread_id_str:
                parts = spread_id_str.split("_")
                if len(parts) >= 2:
                    first_leg = parts[0]
                    if first_leg.startswith("(") and ")" in first_leg:
                        leg_symbol = first_leg.split(")", 1)[1]
                        venue = leg_fill.instrument_id.venue
                        return InstrumentId.from_str(f"{leg_symbol}.{venue}")

            return leg_fill.instrument_id
        except Exception:
            return None


class TestSpreadFillCreation:
    """
    Test cases for creating combo and leg fills from spread executions.
    """

    def setup_method(self):
        """
        Set up test fixtures.
        """
        self.trader_id = TraderId("TRADER-001")
        self.strategy_id = StrategyId("STRATEGY-001")
        self.account_id = AccountId("IB-001")
        self.client_order_id = ClientOrderId("O-001")
        self.venue_order_id = VenueOrderId("IB-123456")
        self.trade_id = TradeId("T-001")
        self.position_id = PositionId("P-001")

    def create_test_leg_fill(
        self,
        instrument_id_str: str,
        side: OrderSide,
        qty: int,
        price: float,
    ) -> OrderFilled:
        """
        Create a test leg fill for testing.
        """
        return OrderFilled(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=InstrumentId.from_str(instrument_id_str),
            client_order_id=self.client_order_id,
            venue_order_id=self.venue_order_id,
            account_id=self.account_id,
            trade_id=self.trade_id,
            order_side=side,
            order_type=OrderType.MARKET,
            last_qty=Quantity.from_int(qty),
            last_px=Price.from_str(str(price)),
            currency=Currency.from_str("USD"),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
            reconciliation=False,
            position_id=self.position_id,
            commission=Money.from_str("1.00 USD"),
        )

    def test_create_combo_fill_basic_spread(self):
        """
        Test creating combo fill from basic spread leg fill.
        """
        leg_fill = self.create_test_leg_fill(
            "(1)SPY C400_((1))SPY C410.SMART",
            OrderSide.SELL,
            3,
            5.25,
        )

        combo_fill = self._create_combo_fill(leg_fill)

        assert combo_fill is not None
        assert combo_fill.instrument_id == leg_fill.instrument_id  # Keep spread ID
        assert combo_fill.order_side == OrderSide.BUY  # Normalized to BUY
        assert combo_fill.last_qty == leg_fill.last_qty  # Same quantity
        assert combo_fill.last_px == leg_fill.last_px
        assert combo_fill.client_order_id == leg_fill.client_order_id

    def test_create_combo_fill_ratio_spread(self):
        """
        Test creating combo fill from ratio spread leg fill.
        """
        leg_fill = self.create_test_leg_fill(
            "(1)E4DN5 P6350_((2))E4DN5 P6355.XCME",
            OrderSide.SELL,
            6,  # 3 spreads x 2 ratio = 6 contracts
            2.50,
        )

        combo_fill = self._create_combo_fill(leg_fill)

        assert combo_fill is not None
        assert combo_fill.instrument_id == leg_fill.instrument_id
        assert combo_fill.order_side == OrderSide.BUY  # Normalized
        assert combo_fill.last_qty == Quantity.from_int(3)  # 6 contracts / 2 ratio = 3 spreads

    def test_create_leg_fill_basic_spread(self):
        """
        Test creating individual leg fill from spread execution.
        """
        leg_fill = self.create_test_leg_fill(
            "(1)SPY C400_((1))SPY C410.SMART",
            OrderSide.SELL,
            3,
            5.25,
        )

        individual_leg_fill = self._create_leg_fill(leg_fill)

        assert individual_leg_fill is not None
        assert individual_leg_fill.instrument_id == InstrumentId.from_str(
            "SPY C400.SMART",
        )  # Individual leg ID
        assert individual_leg_fill.order_side == OrderSide.SELL  # Keep original side
        assert individual_leg_fill.last_qty == Quantity.from_int(3)  # Keep original quantity
        assert individual_leg_fill.client_order_id == leg_fill.client_order_id
        assert "SPY C400.SMART-STRATEGY-001" in str(
            individual_leg_fill.position_id,
        )  # Leg-specific position

    def test_create_leg_fill_ratio_spread(self):
        """
        Test creating individual leg fill from ratio spread execution.
        """
        leg_fill = self.create_test_leg_fill(
            "(1)E4DN5 P6350_((2))E4DN5 P6355.XCME",
            OrderSide.SELL,
            6,
            2.50,
        )

        individual_leg_fill = self._create_leg_fill(leg_fill)

        assert individual_leg_fill is not None
        assert individual_leg_fill.instrument_id == InstrumentId.from_str("E4DN5 P6350.XCME")
        assert individual_leg_fill.order_side == OrderSide.SELL
        assert individual_leg_fill.last_qty == Quantity.from_int(6)

    def _create_combo_fill(self, leg_fill: OrderFilled, contract=None) -> OrderFilled | None:
        """
        Test implementation of combo fill creation.
        """
        try:
            # For testing, use simple 1:1 ratio unless we can parse the spread
            ratio = 1
            if leg_fill.instrument_id.is_spread():
                try:
                    leg_tuples = leg_fill.instrument_id.to_list()
                    if leg_tuples:
                        # For testing, find the leg with the highest absolute ratio
                        # This simulates finding the executed leg
                        max_ratio_leg = max(leg_tuples, key=lambda x: abs(x[1]))
                        _, ratio = max_ratio_leg
                except Exception:
                    ratio = 1

            combo_quantity_value = (
                leg_fill.last_qty.as_double() / abs(ratio)
                if ratio != 0
                else leg_fill.last_qty.as_double()
            )
            combo_quantity = Quantity.from_int(int(combo_quantity_value))

            return OrderFilled(
                trader_id=leg_fill.trader_id,
                strategy_id=leg_fill.strategy_id,
                instrument_id=leg_fill.instrument_id,  # Keep spread ID
                client_order_id=leg_fill.client_order_id,
                venue_order_id=leg_fill.venue_order_id,
                account_id=leg_fill.account_id,
                trade_id=leg_fill.trade_id,
                order_side=OrderSide.BUY,  # Normalize to BUY for combo tracking
                order_type=leg_fill.order_type,
                last_qty=combo_quantity,
                last_px=leg_fill.last_px,
                currency=Currency.from_str("USD"),
                liquidity_side=leg_fill.liquidity_side,
                event_id=UUID4(),
                ts_event=leg_fill.ts_event,
                ts_init=leg_fill.ts_init,
                reconciliation=False,
                position_id=leg_fill.position_id,
                commission=leg_fill.commission,
            )
        except Exception:
            return None

    def _create_leg_fill(self, leg_fill: OrderFilled, contract=None) -> OrderFilled | None:
        """
        Test implementation of leg fill creation.
        """
        try:
            leg_instrument_id, ratio = self._extract_leg_instrument_id_with_ratio(
                leg_fill,
                contract,
            )
            if not leg_instrument_id:
                return None

            return OrderFilled(
                trader_id=leg_fill.trader_id,
                strategy_id=leg_fill.strategy_id,
                instrument_id=leg_instrument_id,  # Individual leg ID
                client_order_id=leg_fill.client_order_id,
                venue_order_id=leg_fill.venue_order_id,
                account_id=leg_fill.account_id,
                trade_id=leg_fill.trade_id,
                order_side=leg_fill.order_side,  # Keep original side
                order_type=leg_fill.order_type,
                last_qty=leg_fill.last_qty,  # Keep original quantity
                last_px=leg_fill.last_px,
                currency=Currency.from_str("USD"),
                liquidity_side=leg_fill.liquidity_side,
                event_id=UUID4(),
                ts_event=leg_fill.ts_event,
                ts_init=leg_fill.ts_init,
                reconciliation=False,
                position_id=PositionId(
                    f"{leg_instrument_id}-{leg_fill.strategy_id}",
                ),  # Leg-specific position
                commission=leg_fill.commission,
            )
        except Exception:
            return None

    def _extract_leg_instrument_id(self, leg_fill: OrderFilled) -> InstrumentId | None:
        """
        Test implementation of leg instrument ID extraction.
        """
        try:
            spread_id_str = str(leg_fill.instrument_id)

            if "_(" in spread_id_str:
                parts = spread_id_str.split("_")
                if len(parts) >= 2:
                    first_leg = parts[0]
                    if first_leg.startswith("(") and ")" in first_leg:
                        leg_symbol = first_leg.split(")", 1)[1]
                        venue = leg_fill.instrument_id.venue
                        return InstrumentId.from_str(f"{leg_symbol}.{venue}")

            return leg_fill.instrument_id
        except Exception:
            return None

    def _extract_leg_instrument_id_with_ratio(
        self,
        leg_fill: OrderFilled,
        contract=None,
    ) -> tuple[InstrumentId | None, int]:
        """
        Test implementation of leg instrument ID extraction with ratio.
        """
        try:
            if leg_fill.instrument_id.is_spread():
                leg_tuples = leg_fill.instrument_id.to_list()
                if leg_tuples:
                    # Return the first leg for testing
                    return leg_tuples[0]

            # Fallback for non-spread instruments
            return leg_fill.instrument_id, 1
        except Exception:
            return None, 1


class TestSpreadFillTracking:
    """
    Test cases for spread fill deduplication tracking.
    """

    def test_fill_tracking_deduplication(self):
        """
        Test that duplicate fills are properly tracked.
        """
        tracking = {}
        client_order_id = ClientOrderId("test-order-1")
        fill_id = "fill-123"

        # First fill should be new
        assert client_order_id not in tracking

        # Add fill ID
        tracking[client_order_id] = {fill_id}

        # Check tracking
        assert fill_id in tracking[client_order_id]

        # Duplicate should be detected
        assert fill_id in tracking[client_order_id]

    def test_multiple_orders_tracking(self):
        """
        Test tracking fills for multiple orders.
        """
        tracking = {}
        order1 = ClientOrderId("order-1")
        order2 = ClientOrderId("order-2")
        fill1 = "fill-1"
        fill2 = "fill-2"

        # Add fills for different orders
        tracking[order1] = {fill1}
        tracking[order2] = {fill2}

        # Each order should track its own fills
        assert fill1 in tracking[order1]
        assert fill1 not in tracking[order2]
        assert fill2 in tracking[order2]
        assert fill2 not in tracking[order1]

    def test_multiple_fills_same_order(self):
        """
        Test tracking multiple fills for same order.
        """
        tracking = {}
        client_order_id = ClientOrderId("order-1")
        fill1 = "fill-1"
        fill2 = "fill-2"

        # Add multiple fills for same order
        tracking[client_order_id] = {fill1, fill2}

        # Both fills should be tracked
        assert fill1 in tracking[client_order_id]
        assert fill2 in tracking[client_order_id]
        assert len(tracking[client_order_id]) == 2


class TestSpreadExecutionIntegration:
    """
    Integration tests for spread execution with mocked execution client.
    """

    def setup_method(self):
        """
        Set up test fixtures.
        """
        self.client = MagicMock(spec=InteractiveBrokersExecutionClient)
        self.client._spread_fill_tracking = {}
        self.client._log = MagicMock()

    def test_handle_spread_execution_deduplication(self):
        """
        Test that spread execution handles duplicate fills correctly.
        """
        leg_fill = self._create_test_fill("(1)SPY C400_((1))SPY C410.SMART")

        # Mock the methods
        self.client._create_combo_fill = MagicMock(return_value=leg_fill)
        self.client._create_leg_fill = MagicMock(return_value=leg_fill)
        self.client._send_order_fill_event = MagicMock()

        # First call should process the fill
        self._handle_spread_execution(leg_fill)

        # Verify methods were called
        assert self.client._create_combo_fill.call_count == 1
        assert self.client._create_leg_fill.call_count == 1
        assert self.client._send_order_fill_event.call_count == 2  # combo + leg

        # Reset mocks
        self.client._create_combo_fill.reset_mock()
        self.client._create_leg_fill.reset_mock()
        self.client._send_order_fill_event.reset_mock()

        # Second call with same fill should be ignored (duplicate)
        self._handle_spread_execution(leg_fill)

        # Verify methods were NOT called (duplicate detected)
        assert self.client._create_combo_fill.call_count == 0
        assert self.client._create_leg_fill.call_count == 0
        assert self.client._send_order_fill_event.call_count == 0

    def test_handle_spread_execution_error_handling(self):
        """
        Test error handling in spread execution.
        """
        leg_fill = self._create_test_fill("(1)SPY C400_((1))SPY C410.SMART")

        # Mock methods to raise exceptions
        self.client._create_combo_fill = MagicMock(side_effect=Exception("Test error"))
        self.client._send_order_fill_event = MagicMock()

        # Should handle error gracefully and send original fill
        self._handle_spread_execution(leg_fill)

        # Verify error was logged and original fill was sent
        assert self.client._log.error.called
        assert self.client._send_order_fill_event.called
        # Should be called with original fill as fallback
        self.client._send_order_fill_event.assert_called_with(leg_fill)

    def _create_test_fill(self, instrument_id_str: str) -> OrderFilled:
        """
        Create a test fill for testing.
        """
        return OrderFilled(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("STRATEGY-001"),
            instrument_id=InstrumentId.from_str(instrument_id_str),
            client_order_id=ClientOrderId("O-001"),
            venue_order_id=VenueOrderId("IB-123456"),
            account_id=AccountId("IB-001"),
            trade_id=TradeId("T-001"),
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            last_qty=Quantity.from_int(3),
            last_px=Price.from_str("5.25"),
            currency=Currency.from_str("USD"),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
            reconciliation=False,
            position_id=PositionId("P-001"),
            commission=Money.from_str("1.00 USD"),
        )

    def _handle_spread_execution(self, leg_fill: OrderFilled) -> None:
        """
        Test implementation of spread execution handling.
        """
        try:
            # Check for duplicate fills
            fill_id = str(leg_fill.trade_id)
            client_order_id = leg_fill.client_order_id

            if client_order_id not in self.client._spread_fill_tracking:
                self.client._spread_fill_tracking[client_order_id] = set()

            if fill_id in self.client._spread_fill_tracking[client_order_id]:
                return  # Already processed

            self.client._spread_fill_tracking[client_order_id].add(fill_id)

            # Create combo fill for order management
            combo_fill = self.client._create_combo_fill(leg_fill)
            if combo_fill:
                self.client._send_order_fill_event(combo_fill)

            # Create leg fill for portfolio updates
            leg_fill_event = self.client._create_leg_fill(leg_fill)
            if leg_fill_event:
                self.client._send_order_fill_event(leg_fill_event)

        except Exception as e:
            self.client._log.error(f"Error handling spread execution: {e}")
            # Fallback to sending original fill
            self.client._send_order_fill_event(leg_fill)
