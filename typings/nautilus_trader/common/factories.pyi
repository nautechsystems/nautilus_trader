from datetime import datetime
from decimal import Decimal
from typing import Dict, List, Optional

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.component import Clock
from nautilus_trader.core.model import (
    ContingencyType,
    OrderSide,
    OrderType,
    TimeInForce,
    TrailingOffsetType,
    TriggerType,
)
from nautilus_trader.model.identifiers import (
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    OrderListId,
    StrategyId,
    TraderId,
)
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.limit_if_touched import LimitIfTouchedOrder
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.market_if_touched import MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit import MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit import TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market import TrailingStopMarketOrder

class OrderFactory:
    """
    A factory class which provides different order types.

    The `TraderId` tag and `StrategyId` tag will be inserted into all IDs generated.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID (only numerical tag sent to venue).
    strategy_id : StrategyId
        The strategy ID (only numerical tag sent to venue).
    clock : Clock
        The clock for the factory.
    cache : CacheFacade, optional
        The cache facade for the order factory.
    initial_order_id_count : int, optional
        The initial order ID count for the factory.
    initial_order_list_id_count : int, optional
        The initial order list ID count for the factory.

    Raises
    ------
    ValueError
        If `initial_order_id_count` is negative (< 0).
    ValueError
        If `initial_order_list_id_count` is negative (< 0).
    """

    trader_id: TraderId
    strategy_id: StrategyId

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        clock: Clock,
        cache: Optional[CacheFacade] = None,
        initial_order_id_count: int = 0,
        initial_order_list_id_count: int = 0,
    ) -> None: ...
    def set_client_order_id_count(self, count: int) -> None: ...
    def set_order_list_id_count(self, count: int) -> None: ...
    def generate_client_order_id(self) -> ClientOrderId: ...
    def generate_order_list_id(self) -> OrderListId: ...
    def reset(self) -> None: ...
    def create_list(self, orders: List) -> OrderList: ...
    def market(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce = TimeInForce.GTC,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> MarketOrder: ...
    def limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> LimitOrder: ...
    def stop_market(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType = TriggerType.DEFAULT,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> StopMarketOrder: ...
    def stop_limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType = TriggerType.DEFAULT,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> StopLimitOrder: ...
    def market_to_limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Optional[Quantity] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> MarketToLimitOrder: ...
    def market_if_touched(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType = TriggerType.DEFAULT,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> MarketIfTouchedOrder: ...
    def limit_if_touched(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType = TriggerType.DEFAULT,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> LimitIfTouchedOrder: ...
    def trailing_stop_market(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        trailing_offset: Decimal,
        trigger_price: Optional[Price] = None,
        trigger_type: TriggerType = TriggerType.DEFAULT,
        trailing_offset_type: TrailingOffsetType = TrailingOffsetType.PRICE,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> TrailingStopMarketOrder: ...
    def trailing_stop_limit(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        limit_offset: Decimal,
        trailing_offset: Decimal,
        price: Optional[Price] = None,
        trigger_price: Optional[Price] = None,
        trigger_type: TriggerType = TriggerType.DEFAULT,
        trailing_offset_type: TrailingOffsetType = TrailingOffsetType.PRICE,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict] = None,
        tags: Optional[List[str]] = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> TrailingStopLimitOrder: ...
    def bracket(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        entry_trigger_price: Optional[Price] = None,
        entry_price: Optional[Price] = None,
        sl_trigger_price: Optional[Price] = None,
        tp_trigger_price: Optional[Price] = None,
        tp_price: Optional[Price] = None,
        entry_order_type: OrderType = OrderType.MARKET,
        tp_order_type: OrderType = OrderType.LIMIT,
        time_in_force: TimeInForce = TimeInForce.GTC,
        sl_time_in_force: TimeInForce = TimeInForce.GTC,
        tp_time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        entry_post_only: bool = False,
        tp_post_only: bool = True,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        contingency_type: ContingencyType = ContingencyType.OUO,
        entry_exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        sl_exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        tp_exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        entry_exec_algorithm_params: Optional[Dict] = None,
        sl_exec_algorithm_params: Optional[Dict] = None,
        tp_exec_algorithm_params: Optional[Dict] = None,
        entry_tags: Optional[List[str]] = None,
        sl_tags: Optional[List[str]] = None,
        tp_tags: Optional[List[str]] = None,
        entry_client_order_id: Optional[ClientOrderId] = None,
        sl_client_order_id: Optional[ClientOrderId] = None,
        tp_client_order_id: Optional[ClientOrderId] = None,
    ) -> OrderList: ...
