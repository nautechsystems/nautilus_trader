from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from stubs.cache.base import CacheFacade
from stubs.common.component import Clock
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import OrderListId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order
from stubs.model.orders.limit import LimitOrder
from stubs.model.orders.limit_if_touched import LimitIfTouchedOrder
from stubs.model.orders.list import OrderList
from stubs.model.orders.market import MarketOrder
from stubs.model.orders.market_if_touched import MarketIfTouchedOrder
from stubs.model.orders.market_to_limit import MarketToLimitOrder
from stubs.model.orders.stop_limit import StopLimitOrder
from stubs.model.orders.stop_market import StopMarketOrder
from stubs.model.orders.trailing_stop_limit import TrailingStopLimitOrder
from stubs.model.orders.trailing_stop_market import TrailingStopMarketOrder

class OrderFactory:

    trader_id: TraderId
    strategy_id: StrategyId
    use_uuid_client_order_ids: bool
    use_hyphens_in_client_order_ids: bool

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        clock: Clock,
        cache: CacheFacade | None = None,
        use_uuid_client_order_ids: bool = False,
        use_hyphens_in_client_order_ids: bool = True,
    ) -> None: ...
    def get_client_order_id_count(self) -> int: ...
    def get_order_list_id_count(self) -> int: ...
    def set_client_order_id_count(self, count: int) -> None: ...
    def set_order_list_id_count(self, count: int) -> None: ...
    def generate_client_order_id(self) -> ClientOrderId: ...
    def generate_order_list_id(self) -> OrderListId: ...
    def reset(self) -> None: ...
    def create_list(self, orders: list[Order]) -> OrderList: ...
    def market(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce = ...,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> MarketOrder: ...
    def limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> LimitOrder: ...
    def stop_market(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType = ...,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> StopMarketOrder: ...
    def stop_limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType = ...,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> StopLimitOrder: ...
    def market_to_limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> MarketToLimitOrder: ...
    def market_if_touched(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType = ...,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> MarketIfTouchedOrder: ...
    def limit_if_touched(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType = ...,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> LimitIfTouchedOrder: ...
    def trailing_stop_market(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trailing_offset: Decimal,
        activation_price: Price | None = None,
        trigger_price: Price | None = None,
        trigger_type: TriggerType = ...,
        trailing_offset_type: TrailingOffsetType = ...,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> TrailingStopMarketOrder: ...
    def trailing_stop_limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        price: Price | None = None,
        activation_price: Price | None = None,
        trigger_price: Price | None = None,
        trigger_type: TriggerType = ...,
        trailing_offset_type: TrailingOffsetType = ...,
        time_in_force: TimeInForce = ...,
        expire_time: datetime | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        tags: list[str] | None = None,
        client_order_id: ClientOrderId | None = None,
    ) -> TrailingStopLimitOrder: ...
    def bracket(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        contingency_type: ContingencyType = ...,

        # Entry order
        entry_order_type: OrderType = ...,
        entry_price: Price | None = None,
        entry_trigger_price: Price | None = None,
        expire_time: datetime | None = None,
        time_in_force: TimeInForce = ...,
        entry_post_only: bool = False,
        entry_exec_algorithm_id: ExecAlgorithmId | None = None,
        entry_exec_algorithm_params: dict[str, Any] | None = None,
        entry_tags: list[str] | None = None,
        entry_client_order_id: ClientOrderId | None = None,

        # Take-profit order
        tp_order_type: OrderType = ...,
        tp_price: Price | None = None,
        tp_trigger_price: Price | None = None,
        tp_trigger_type: TriggerType = ...,
        tp_activation_price: Price | None = None,
        tp_trailing_offset: Decimal | None = None,
        tp_trailing_offset_type: TrailingOffsetType = ...,
        tp_limit_offset: Decimal | None = None,
        tp_time_in_force: TimeInForce = ...,
        tp_post_only: bool = True,
        tp_exec_algorithm_id: ExecAlgorithmId | None = None,
        tp_exec_algorithm_params: dict[str, Any] | None = None,
        tp_tags: list[str] | None = None,
        tp_client_order_id: ClientOrderId | None = None,

        # Stop-loss order
        sl_order_type: OrderType = ...,
        sl_trigger_price: Price | None = None,
        sl_trigger_type: TriggerType = ...,
        sl_activation_price: Price | None = None,
        sl_trailing_offset: Decimal | None = None,
        sl_trailing_offset_type: TrailingOffsetType = ...,
        sl_time_in_force: TimeInForce = ...,
        sl_exec_algorithm_id: ExecAlgorithmId | None = None,
        sl_exec_algorithm_params: dict[str, Any] | None = None,
        sl_tags: list[str] | None = None,
        sl_client_order_id: ClientOrderId | None = None,
    ) -> OrderList: ...
