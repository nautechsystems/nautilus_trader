#!/usr/bin/env python3
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
Comprehensive tests for Interactive Brokers spread execution functionality.
"""

from unittest.mock import MagicMock

from nautilus_trader.adapters.interactive_brokers.execution import InteractiveBrokersExecutionClient
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.identifiers import generic_spread_id_to_list
from nautilus_trader.model.identifiers import is_generic_spread_id
from nautilus_trader.model.identifiers import new_generic_spread_id
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.instruments import OptionSpread
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class TestSpreadExecutionDetection:
    """
    Test cases for spread instrument detection using instrument.is_spread().
    """

    def test_is_spread_instrument_with_spread_instruments(self):
        """
        Test spread detection using instrument.is_spread() method.
        """
        # Create spread instruments using new_generic_spread_id
        leg1_id = InstrumentId.from_str("SPY C400.SMART")
        leg2_id = InstrumentId.from_str("SPY C410.SMART")
        spread1_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

        leg3_id = InstrumentId.from_str("E4DN5 P6350.XCME")
        leg4_id = InstrumentId.from_str("E4DN5 P6355.XCME")
        spread2_id = new_generic_spread_id([(leg3_id, 1), (leg4_id, -2)])

        # Create option spread instruments
        spread1 = OptionSpread(
            instrument_id=spread1_id,
            raw_symbol=spread1_id.symbol,
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="SPY",
            strategy_type="VERTICAL",
            activation_ns=0,
            expiration_ns=1640995200000000000,
            ts_event=0,
            ts_init=0,
        )

        spread2 = OptionSpread(
            instrument_id=spread2_id,
            raw_symbol=spread2_id.symbol,
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="E4DN5",
            strategy_type="RATIO",
            activation_ns=0,
            expiration_ns=1640995200000000000,
            ts_event=0,
            ts_init=0,
        )

        # Create non-spread instruments
        option_contract = OptionContract(
            instrument_id=InstrumentId.from_str("SPY C400.SMART"),
            raw_symbol=Symbol("SPY C400"),
            asset_class=AssetClass.EQUITY,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="SPY",
            option_kind=OptionKind.CALL,
            activation_ns=0,
            expiration_ns=1640995200000000000,
            strike_price=Price.from_str("400.0"),
            ts_event=0,
            ts_init=0,
        )

        equity = Equity(
            instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            raw_symbol=Symbol("AAPL"),
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        futures = FuturesContract(
            instrument_id=InstrumentId.from_str("ES.CME"),
            raw_symbol=Symbol("ES"),
            asset_class=AssetClass.INDEX,
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(50),
            lot_size=Quantity.from_int(1),
            underlying="ES",
            activation_ns=0,
            expiration_ns=1640995200000000000,
            ts_event=0,
            ts_init=0,
        )

        # Test spread instruments - should return True
        assert spread1.is_spread() is True, "Spread1 should be detected as spread"
        assert spread2.is_spread() is True, "Spread2 should be detected as spread"

        # Test non-spread instruments - should return False
        assert option_contract.is_spread() is False, (
            "Option contract should not be detected as spread"
        )
        assert equity.is_spread() is False, "Equity should not be detected as spread"
        assert futures.is_spread() is False, "Futures contract should not be detected as spread"


class TestSpreadLegExtraction:
    """
    Test cases for extracting leg instrument IDs from spread fills.
    """

    def test_extract_leg_instrument_id_standard_spread(self):
        """
        Test extraction from standard spread format.
        """
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("SPY C400.SMART"), 1),
                (InstrumentId.from_str("SPY C410.SMART"), -1),
            ],
        )

        leg_fill = MagicMock()
        leg_fill.instrument_id = spread_id

        leg_id = self._extract_leg_instrument_id(leg_fill)

        expected_id = InstrumentId.from_str("SPY C400.SMART")
        assert leg_id == expected_id

    def test_extract_leg_instrument_id_ratio_spread(self):
        """
        Test extraction from ratio spread format.
        """
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("E4DN5 P6350.XCME"), 1),
                (InstrumentId.from_str("E4DN5 P6355.XCME"), -2),
            ],
        )

        leg_fill = MagicMock()
        leg_fill.instrument_id = spread_id

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

        assert leg_id == leg_fill.instrument_id

    def _extract_leg_instrument_id(self, leg_fill):
        """
        Test implementation of leg instrument ID extraction.
        """
        try:
            if is_generic_spread_id(leg_fill.instrument_id):
                leg_tuples = generic_spread_id_to_list(leg_fill.instrument_id)
                if leg_tuples:
                    return leg_tuples[0][0]

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
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("SPY C400.SMART"), 1),
                (InstrumentId.from_str("SPY C410.SMART"), -1),
            ],
        )
        leg_fill = self.create_test_leg_fill(
            str(spread_id),
            OrderSide.SELL,
            3,
            5.25,
        )

        combo_fill = self._create_combo_fill(leg_fill)

        assert combo_fill is not None
        assert combo_fill.instrument_id == leg_fill.instrument_id
        assert combo_fill.order_side == OrderSide.BUY
        assert combo_fill.last_qty == leg_fill.last_qty
        assert combo_fill.last_px == leg_fill.last_px
        assert combo_fill.client_order_id == leg_fill.client_order_id

    def test_create_combo_fill_ratio_spread(self):
        """
        Test creating combo fill from ratio spread leg fill.
        """
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("E4DN5 P6350.XCME"), 1),
                (InstrumentId.from_str("E4DN5 P6355.XCME"), -2),
            ],
        )
        leg_fill = self.create_test_leg_fill(
            str(spread_id),
            OrderSide.SELL,
            6,  # 3 spreads x 2 ratio = 6 contracts
            2.50,
        )

        combo_fill = self._create_combo_fill(leg_fill)

        assert combo_fill is not None
        assert combo_fill.instrument_id == leg_fill.instrument_id
        assert combo_fill.order_side == OrderSide.BUY
        assert combo_fill.last_qty == Quantity.from_int(3)  # 6 contracts / 2 ratio = 3 spreads

    def test_create_leg_fill_basic_spread(self):
        """
        Test creating individual leg fill from spread execution.
        """
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("SPY C400.SMART"), 1),
                (InstrumentId.from_str("SPY C410.SMART"), -1),
            ],
        )
        leg_fill = self.create_test_leg_fill(
            str(spread_id),
            OrderSide.SELL,
            3,
            5.25,
        )

        individual_leg_fill = self._create_leg_fill(leg_fill)

        assert individual_leg_fill is not None
        assert individual_leg_fill.instrument_id == InstrumentId.from_str("SPY C400.SMART")
        assert individual_leg_fill.order_side == OrderSide.SELL
        assert individual_leg_fill.last_qty == Quantity.from_int(3)
        assert individual_leg_fill.client_order_id == leg_fill.client_order_id
        assert "SPY C400.SMART-STRATEGY-001" in str(individual_leg_fill.position_id)

    def test_create_leg_fill_ratio_spread(self):
        """
        Test creating individual leg fill from ratio spread execution.
        """
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("E4DN5 P6350.XCME"), 1),
                (InstrumentId.from_str("E4DN5 P6355.XCME"), -2),
            ],
        )
        leg_fill = self.create_test_leg_fill(
            str(spread_id),
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
            ratio = 1
            if is_generic_spread_id(leg_fill.instrument_id):
                try:
                    leg_tuples = generic_spread_id_to_list(leg_fill.instrument_id)
                    if leg_tuples:
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
                instrument_id=leg_fill.instrument_id,
                client_order_id=leg_fill.client_order_id,
                venue_order_id=leg_fill.venue_order_id,
                account_id=leg_fill.account_id,
                trade_id=leg_fill.trade_id,
                order_side=OrderSide.BUY,
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
                instrument_id=leg_instrument_id,
                client_order_id=leg_fill.client_order_id,
                venue_order_id=leg_fill.venue_order_id,
                account_id=leg_fill.account_id,
                trade_id=leg_fill.trade_id,
                order_side=leg_fill.order_side,
                order_type=leg_fill.order_type,
                last_qty=leg_fill.last_qty,
                last_px=leg_fill.last_px,
                currency=Currency.from_str("USD"),
                liquidity_side=leg_fill.liquidity_side,
                event_id=UUID4(),
                ts_event=leg_fill.ts_event,
                ts_init=leg_fill.ts_init,
                reconciliation=False,
                position_id=PositionId(
                    f"{leg_instrument_id}-{leg_fill.strategy_id}",
                ),
                commission=leg_fill.commission,
            )
        except Exception:
            return None

    def _extract_leg_instrument_id(self, leg_fill: OrderFilled) -> InstrumentId | None:
        """
        Test implementation of leg instrument ID extraction.
        """
        try:
            if is_generic_spread_id(leg_fill.instrument_id):
                leg_tuples = generic_spread_id_to_list(leg_fill.instrument_id)
                if leg_tuples:
                    return leg_tuples[0][0]

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
            if is_generic_spread_id(leg_fill.instrument_id):
                leg_tuples = generic_spread_id_to_list(leg_fill.instrument_id)
                if leg_tuples:
                    return leg_tuples[0]

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

        assert client_order_id not in tracking

        tracking[client_order_id] = {fill_id}

        assert fill_id in tracking[client_order_id]
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

        tracking[order1] = {fill1}
        tracking[order2] = {fill2}

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

        tracking[client_order_id] = {fill1, fill2}

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
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("SPY C400.SMART"), 1),
                (InstrumentId.from_str("SPY C410.SMART"), -1),
            ],
        )
        leg_fill = self._create_test_fill(str(spread_id))

        self.client._create_combo_fill = MagicMock(return_value=leg_fill)
        self.client._create_leg_fill = MagicMock(return_value=leg_fill)
        self.client._send_order_fill_event = MagicMock()

        self._handle_spread_execution(leg_fill)

        assert self.client._create_combo_fill.call_count == 1
        assert self.client._create_leg_fill.call_count == 1
        assert self.client._send_order_fill_event.call_count == 2

        self.client._create_combo_fill.reset_mock()
        self.client._create_leg_fill.reset_mock()
        self.client._send_order_fill_event.reset_mock()

        self._handle_spread_execution(leg_fill)

        assert self.client._create_combo_fill.call_count == 0
        assert self.client._create_leg_fill.call_count == 0
        assert self.client._send_order_fill_event.call_count == 0

    def test_handle_spread_execution_error_handling(self):
        """
        Test error handling in spread execution.
        """
        spread_id = new_generic_spread_id(
            [
                (InstrumentId.from_str("SPY C400.SMART"), 1),
                (InstrumentId.from_str("SPY C410.SMART"), -1),
            ],
        )
        leg_fill = self._create_test_fill(str(spread_id))

        self.client._create_combo_fill = MagicMock(side_effect=Exception("Test error"))
        self.client._send_order_fill_event = MagicMock()

        self._handle_spread_execution(leg_fill)

        assert self.client._log.error.called
        assert self.client._send_order_fill_event.called
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
            fill_id = str(leg_fill.trade_id)
            client_order_id = leg_fill.client_order_id

            if client_order_id not in self.client._spread_fill_tracking:
                self.client._spread_fill_tracking[client_order_id] = set()

            if fill_id in self.client._spread_fill_tracking[client_order_id]:
                return

            self.client._spread_fill_tracking[client_order_id].add(fill_id)

            combo_fill = self.client._create_combo_fill(leg_fill)
            if combo_fill:
                self.client._send_order_fill_event(combo_fill)

            leg_fill_event = self.client._create_leg_fill(leg_fill)
            if leg_fill_event:
                self.client._send_order_fill_event(leg_fill_event)

        except Exception as e:
            self.client._log.error(f"Error handling spread execution: {e}")
            self.client._send_order_fill_event(leg_fill)
