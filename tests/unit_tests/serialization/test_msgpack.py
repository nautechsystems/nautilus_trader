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

from base64 import b64encode
from decimal import Decimal

import msgspec
import pandas as pd

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import ShutdownSystem
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateExecutionMassStatus
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderEmulated
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderReleased
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.commodity import Commodity
from nautilus_trader.model.instruments.crypto_option import CryptoOption
from nautilus_trader.model.instruments.index import IndexInstrument
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitIfTouchedOrder
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketToLimitOrder
from nautilus_trader.model.orders import OrderUnpacker
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.model.orders import TrailingStopLimitOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.serialization.serializer import MsgSpecSerializer
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()
BTCUSDT_220325 = TestInstrumentProvider.btcusdt_future_binance()


class TestMsgSpecSerializer:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()
        self.venue = Venue("SIM")

        self.unpacker = OrderUnpacker()
        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

        self.serializer = MsgSpecSerializer(encoding=msgspec.msgpack)

    def test_serialize_and_deserialize_fx_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(AUDUSD_SIM)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == AUDUSD_SIM
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_crypto_perpetual_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(ETHUSDT_BINANCE)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == ETHUSDT_BINANCE
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_crypto_future_instrument(self):
        # Arrange, Act
        serialized = self.serializer.serialize(BTCUSDT_220325)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == BTCUSDT_220325
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_index_instrument(self):
        # Arrange
        instrument = IndexInstrument(
            instrument_id=InstrumentId.from_str("SPX.CBOE"),
            raw_symbol=Symbol("SPX"),
            currency=Currency.from_str("USD"),
            price_precision=2,
            size_precision=0,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("1"),
            ts_event=1640995200000000000,
            ts_init=1640995200000000000,
            info={"description": "S&P 500 Index"},
        )

        # Act
        serialized = self.serializer.serialize(instrument)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == instrument
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_commodity_instrument(self):
        # Arrange
        instrument = Commodity(
            instrument_id=InstrumentId.from_str("CL.NYMEX"),
            raw_symbol=Symbol("CL"),
            asset_class=AssetClass.COMMODITY,
            quote_currency=Currency.from_str("USD"),
            price_precision=2,
            size_precision=0,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("1"),
            ts_event=1640995200000000000,
            ts_init=1640995200000000000,
            info={"description": "Crude Oil"},
        )

        # Act
        serialized = self.serializer.serialize(instrument)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == instrument
        print(b64encode(serialized))
        print(deserialized)

    def test_serialize_and_deserialize_crypto_option_instrument(self):
        # Arrange
        instrument = CryptoOption(
            instrument_id=InstrumentId.from_str("BTC-25DEC21-50000-C.DERIBIT"),
            raw_symbol=Symbol("BTC-25DEC21-50000-C"),
            underlying=Currency.from_str("BTC"),
            quote_currency=Currency.from_str("USD"),
            settlement_currency=Currency.from_str("BTC"),
            is_inverse=False,
            option_kind=OptionKind.CALL,
            strike_price=Price.from_str("50000.0"),
            activation_ns=1640995200000000000,
            expiration_ns=1640995200000000000,
            price_precision=2,
            size_precision=8,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.00000001"),
            ts_event=1640995200000000000,
            ts_init=1640995200000000000,
            info={"description": "Bitcoin Call Option"},
        )

        # Act
        serialized = self.serializer.serialize(instrument)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == instrument
        print(b64encode(serialized))
        print(deserialized)

    def test_pack_and_unpack_market_orders(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100_000, precision=0),
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_limit_orders(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            Price(1.00000, precision=5),
            TimeInForce.DAY,
            display_qty=Quantity(50_000, precision=0),
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_limit_orders_with_expiration(self):
        # Arrange
        order = LimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            price=Price(1.00000, precision=5),
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            tags=["tag-01", "tag-02", "tag-03"],
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_market_orders(self):
        # Arrange
        order = StopMarketOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            trigger_price=Price(1.00000, precision=5),
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTC,
            expire_time_ns=0,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_market_orders_with_expiration(self):
        # Arrange
        order = StopMarketOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            trigger_price=Price(1.00000, precision=5),
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_limit_orders(self):
        # Arrange
        order = StopLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            price=Price(1.00000, precision=5),
            trigger_price=Price(1.00010, precision=5),
            trigger_type=TriggerType.BID_ASK,
            time_in_force=TimeInForce.GTC,
            expire_time_ns=0,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_market_to_limit__orders(self):
        # Arrange
        order = MarketToLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            time_in_force=TimeInForce.GTD,  # <-- invalid
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_market_if_touched_orders(self):
        # Arrange
        order = MarketIfTouchedOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            trigger_price=Price(1.00000, precision=5),
            trigger_type=TriggerType.DEFAULT,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_limit_if_touched_orders(self):
        # Arrange
        order = LimitIfTouchedOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            price=Price(1.00000, precision=5),
            trigger_price=Price(1.00010, precision=5),
            trigger_type=TriggerType.BID_ASK,
            time_in_force=TimeInForce.GTC,
            expire_time_ns=0,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_stop_limit_orders_with_expiration(self):
        # Arrange
        order = StopLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            price=Price(1.00000, precision=5),
            trigger_price=Price(1.00010, precision=5),
            trigger_type=TriggerType.LAST_PRICE,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_trailing_stop_market_orders_with_expiration(self):
        # Arrange
        order = TrailingStopMarketOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            trigger_price=Price(1.00000, precision=5),
            trigger_type=TriggerType.DEFAULT,
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_trailing_stop_market_orders_no_initial_prices(self):
        # Arrange
        order = TrailingStopMarketOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            trigger_price=None,
            trigger_type=TriggerType.DEFAULT,
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_trailing_stop_limit_orders_with_expiration(self):
        # Arrange
        order = TrailingStopLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            price=Price(1.00000, precision=5),
            trigger_price=Price(1.00010, precision=5),
            trigger_type=TriggerType.MARK_PRICE,
            limit_offset=Decimal("50"),
            trailing_offset=Decimal("50"),
            trailing_offset_type=TrailingOffsetType.TICKS,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_pack_and_unpack_trailing_stop_limit_orders_with_no_initial_prices(self):
        # Arrange
        order = TrailingStopLimitOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            price=None,
            trigger_price=None,
            trigger_type=TriggerType.MARK_PRICE,
            limit_offset=Decimal("50"),
            trailing_offset=Decimal("50"),
            trailing_offset_type=TrailingOffsetType.TICKS,
            time_in_force=TimeInForce.GTD,
            expire_time_ns=1_000_000_000 * 60,
            init_id=UUID4(),
            ts_init=0,
        )

        # Act
        packed = OrderInitialized.to_dict(order.last_event)
        unpacked = self.unpacker.unpack(packed)

        # Assert
        assert unpacked == order

    def test_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
        )

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            order=order,
            position_id=PositionId("P-123456"),
            command_id=UUID4(),
            ts_init=0,
            client_id=ClientId("SIM"),
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        assert deserialized.order == order
        print(command)
        print(len(serialized))
        print(serialized)
        print(b64encode(serialized))

    def test_serialize_and_deserialize_submit_order_list_commands(self):
        # Arrange
        bracket = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity(100_000, precision=0),
            sl_trigger_price=Price(0.99900, precision=5),
            tp_price=Price(1.00010, precision=5),
        )

        command = SubmitOrderList(
            client_id=ClientId("SIM"),
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            order_list=bracket,
            position_id=PositionId("P-123456"),
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        assert deserialized.order_list == bracket
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_modify_order_commands(self):
        # Arrange
        command = ModifyOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("001"),
            Quantity(100_000, precision=0),
            Price(1.00001, precision=5),
            None,
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        command = CancelOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("001"),
            UUID4(),
            0,
            ClientId("SIM-001"),
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_generate_order_status_report_command(self):
        # Arrange
        command = GenerateOrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            client_order_id=TestIdStubs.client_order_id(),
            venue_order_id=TestIdStubs.venue_order_id(),
            command_id=UUID4(),
            ts_init=TestClock().timestamp_ns(),
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, GenerateOrderStatusReport)
        assert GenerateOrderStatusReport.to_dict(deserialized) == GenerateOrderStatusReport.to_dict(
            command,
        )

    def test_serialize_and_deserialize_generate_order_status_reports_command(self):
        # Arrange
        start = pd.Timestamp("2025-01-01T00:00:00Z")
        end = pd.Timestamp("2025-01-02T00:00:00Z")
        command = GenerateOrderStatusReports(
            instrument_id=AUDUSD_SIM.id,
            start=start,
            end=end,
            open_only=True,
            command_id=UUID4(),
            ts_init=TestClock().timestamp_ns(),
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, GenerateOrderStatusReports)
        assert pd.Timestamp(deserialized.start).value == start.value
        assert pd.Timestamp(deserialized.end).value == end.value
        assert deserialized.open_only is True

    def test_serialize_and_deserialize_generate_fill_reports_command(self):
        # Arrange
        command = GenerateFillReports(
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=TestIdStubs.venue_order_id(),
            start=pd.Timestamp("2025-01-01T01:00:00Z"),
            end=pd.Timestamp("2025-01-01T02:00:00Z"),
            command_id=UUID4(),
            ts_init=TestClock().timestamp_ns(),
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, GenerateFillReports)
        assert pd.Timestamp(deserialized.start).value == pd.Timestamp(command.start).value
        assert pd.Timestamp(deserialized.end).value == pd.Timestamp(command.end).value

    def test_serialize_and_deserialize_generate_position_status_reports_command(self):
        # Arrange
        command = GeneratePositionStatusReports(
            instrument_id=AUDUSD_SIM.id,
            start=pd.Timestamp("2025-01-03T00:00:00Z"),
            end=pd.Timestamp("2025-01-04T00:00:00Z"),
            command_id=UUID4(),
            ts_init=TestClock().timestamp_ns(),
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, GeneratePositionStatusReports)
        assert pd.Timestamp(deserialized.start).value == pd.Timestamp(command.start).value
        assert pd.Timestamp(deserialized.end).value == pd.Timestamp(command.end).value

    def test_serialize_and_deserialize_generate_execution_mass_status_command(self):
        # Arrange
        command = GenerateExecutionMassStatus(
            trader_id=TestIdStubs.trader_id(),
            client_id=ClientId("SIM"),
            command_id=UUID4(),
            ts_init=TestClock().timestamp_ns(),
            venue=self.venue,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, GenerateExecutionMassStatus)
        assert GenerateExecutionMassStatus.to_dict(
            deserialized,
        ) == GenerateExecutionMassStatus.to_dict(command)

    def test_serialize_and_deserialize_shutdown_system_commands(self):
        # Arrange
        command = ShutdownSystem(
            trader_id=TestIdStubs.trader_id(),
            component_id=ComponentId("Controller"),
            reason="Maintenance",
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == command

    def test_serialize_and_deserialize_component_state_changed_events(self):
        # Arrange
        event = ComponentStateChanged(
            trader_id=TestIdStubs.trader_id(),
            component_id=ComponentId("MyActor-001"),
            component_type="MyActor",
            state=ComponentState.RUNNING,
            config={"do_something": True},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_status_report(self):
        # Arrange
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=TestIdStubs.venue_order_id(),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100_000),
            filled_qty=Quantity.from_int(25_000),
            report_id=UUID4(),
            ts_accepted=1_000,
            ts_last=2_000,
            ts_init=3_000,
            client_order_id=TestIdStubs.client_order_id(),
            order_list_id=TestIdStubs.order_list_id(),
            venue_position_id=TestIdStubs.position_id(),
            linked_order_ids=[TestIdStubs.client_order_id(2)],
            parent_order_id=TestIdStubs.client_order_id(3),
            contingency_type=ContingencyType.NO_CONTINGENCY,
            expire_time=pd.Timestamp("2025-01-01T00:00:00Z"),
            price=Price.from_str("1.00010"),
            trigger_price=None,
            trigger_type=TriggerType.NO_TRIGGER,
            limit_offset=None,
            trailing_offset=None,
            trailing_offset_type=TrailingOffsetType.NO_TRAILING_OFFSET,
            avg_px=Decimal("1.00005"),
            display_qty=None,
            post_only=False,
            reduce_only=False,
            cancel_reason=None,
            ts_triggered=None,
        )

        # Act
        serialized = self.serializer.serialize(report)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, OrderStatusReport)
        assert deserialized.to_dict() == report.to_dict()

    def test_serialize_and_deserialize_fill_report(self):
        # Arrange
        report = FillReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=TestIdStubs.venue_order_id(),
            trade_id=TestIdStubs.trade_id(),
            order_side=OrderSide.SELL,
            last_qty=Quantity.from_int(10_000),
            last_px=Price.from_str("1.00005"),
            commission=Money("5.25", USD),
            liquidity_side=LiquiditySide.MAKER,
            report_id=UUID4(),
            ts_event=4_000,
            ts_init=5_000,
            client_order_id=TestIdStubs.client_order_id(),
            venue_position_id=TestIdStubs.position_id(),
        )

        # Act
        serialized = self.serializer.serialize(report)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, FillReport)
        assert deserialized.to_dict() == report.to_dict()

    def test_serialize_and_deserialize_position_status_report(self):
        # Arrange
        report = PositionStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(50_000),
            report_id=UUID4(),
            ts_last=6_000,
            ts_init=7_000,
            venue_position_id=TestIdStubs.position_id(),
            avg_px_open=Decimal("1.00020"),
        )

        # Act
        serialized = self.serializer.serialize(report)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, PositionStatusReport)
        assert deserialized.to_dict() == report.to_dict()

    def test_serialize_and_deserialize_execution_mass_status(self):
        # Arrange
        order_report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=TestIdStubs.venue_order_id(),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100_000),
            filled_qty=Quantity.from_int(0),
            report_id=UUID4(),
            ts_accepted=10_000,
            ts_last=11_000,
            ts_init=12_000,
            client_order_id=TestIdStubs.client_order_id(),
            order_list_id=TestIdStubs.order_list_id(),
            venue_position_id=TestIdStubs.position_id(),
        )
        fill_report = FillReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            venue_order_id=TestIdStubs.venue_order_id(),
            trade_id=TestIdStubs.trade_id(),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(5_000),
            last_px=Price.from_str("1.00001"),
            commission=Money("1.25", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=UUID4(),
            ts_event=13_000,
            ts_init=14_000,
        )
        position_report = PositionStatusReport(
            account_id=self.account_id,
            instrument_id=AUDUSD_SIM.id,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(95_000),
            report_id=UUID4(),
            ts_last=15_000,
            ts_init=16_000,
        )
        mass_status = ExecutionMassStatus(
            client_id=ClientId("SIM-CLIENT"),
            account_id=self.account_id,
            venue=self.venue,
            report_id=UUID4(),
            ts_init=17_000,
        )
        mass_status.add_order_reports([order_report])
        mass_status.add_fill_reports([fill_report])
        mass_status.add_position_reports([position_report])

        # Act
        serialized = self.serializer.serialize(mass_status)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert isinstance(deserialized, ExecutionMassStatus)
        assert deserialized.to_dict() == mass_status.to_dict()

    def test_serialize_and_deserialize_account_state_with_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_525_000, USD),
                    Money(25_000, USD),
                    Money(1_500_000, USD),
                ),
            ],
            margins=[
                MarginBalance(
                    Money(5000, USD),
                    Money(20_000, USD),
                    AUDUSD_SIM.id,
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_account_state_without_base_currency_events(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=None,
            reported=True,
            balances=[
                AccountBalance(
                    Money(10000, USDT),
                    Money(0, USDT),
                    Money(10000, USDT),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_market_order_initialized_events(self):
        # Arrange
        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(100_000, precision=0),
            TimeInForce.FOK,
            post_only=False,
            reduce_only=True,
            quote_quantity=False,
            options={},
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=["ENTRY"],
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_limit_order_initialized_events(self):
        # Arrange
        options = {
            "expire_time_ns": 1_000_000_000,
            "price": "1.0010",
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.LIMIT,
            Quantity(100_000, precision=0),
            TimeInForce.DAY,
            post_only=True,
            reduce_only=False,
            quote_quantity=True,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=None,
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options

    def test_serialize_and_deserialize_stop_market_order_initialized_events(self):
        # Arrange
        options = {
            "trigger_price": "1.0005",
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.STOP_MARKET,
            Quantity(100_000, precision=0),
            TimeInForce.DAY,
            post_only=False,
            reduce_only=True,
            quote_quantity=False,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=None,
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options

    def test_serialize_and_deserialize_stop_limit_order_initialized_events(self):
        # Arrange
        options = {
            "expire_time_ns": 0,
            "price": "1.0005",
            "trigger_price": "1.0010",
        }

        event = OrderInitialized(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100_000, precision=0),
            TimeInForce.DAY,
            post_only=True,
            reduce_only=True,
            quote_quantity=False,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-123457"), ClientOrderId("O-123458")],
            parent_order_id=ClientOrderId("O-123455"),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
            exec_spawn_id=ClientOrderId("O-1"),
            tags=["entry", "bulk"],
            event_id=UUID4(),
            ts_init=0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.options == options
        assert deserialized.tags == ["entry", "bulk"]

    def test_serialize_and_deserialize_order_denied_events(self):
        # Arrange
        event = OrderDenied(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            "Exceeds MAX_NOTIONAL_PER_ORDER",
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_emulated_events(self):
        # Arrange
        event = OrderEmulated(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_released_events(self):
        # Arrange
        event = OrderReleased(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            Price.from_str("1.00000"),
            UUID4(),
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        event = OrderSubmitted(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_accepted_events(self):
        # Arrange
        event = OrderAccepted(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("B-123456"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_rejected_events(self):
        # Arrange
        event = OrderRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            self.account_id,
            "ORDER_ID_INVALID",
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.due_post_only is False

    def test_serialize_and_deserialize_order_rejected_with_due_post_only(self):
        # Arrange
        event = OrderRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            self.account_id,
            "POST_ONLY_WOULD_EXECUTE",
            UUID4(),
            0,
            0,
            due_post_only=True,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
        assert deserialized.due_post_only is True

    def test_serialize_and_deserialize_order_pending_cancel_events(self):
        # Arrange
        event = OrderPendingCancel(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_pending_replace_events(self):
        # Arrange
        event = OrderPendingUpdate(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_canceled_events(self):
        # Arrange
        event = OrderCanceled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_update_reject_events(self):
        # Arrange
        event = OrderModifyRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            "ORDER_DOES_NOT_EXIST",
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        event = OrderCancelRejected(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            "ORDER_DOES_NOT_EXIST",
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_modify_events(self):
        # Arrange
        event = OrderUpdated(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            Quantity(100_000, precision=0),
            Price(0.80010, precision=5),
            Price(0.80050, precision=5),
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_expired_events(self):
        # Arrange
        event = OrderExpired(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_triggered_events(self):
        # Arrange
        event = OrderTriggered(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            TradeId("E123456"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(50_000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.MAKER,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_order_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            VenueOrderId("1"),
            self.account_id,
            TradeId("E123456"),
            PositionId("T123456"),
            OrderSide.SELL,
            OrderType.MARKET,
            Quantity(100_000, precision=0),
            Price(1.00000, precision=5),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.TAKER,
            UUID4(),
            0,
            0,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_position_opened_events(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        uuid = UUID4()
        event = PositionOpened.create(position, fill, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_position_changed_events(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = UUID4()
        event = PositionChanged.create(position, fill2, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event

    def test_serialize_and_deserialize_position_closed_events(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = UUID4()
        event = PositionClosed.create(position, fill2, uuid, 0)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        assert deserialized == event
