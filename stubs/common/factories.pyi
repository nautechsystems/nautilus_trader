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
    use_uuid_client_order_ids : bool, default False
        If UUID4's should be used for client order ID values.
    use_hyphens_in_client_order_ids : bool, default True
        If hyphens should be used in generated client order ID values.

    """

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
    def get_client_order_id_count(self) -> int:
        """
        Return the client order ID count for the factory.

        Returns
        -------
        int

        """
    def get_order_list_id_count(self) -> int:
        """
        Return the order list ID count for the factory.

        Returns
        -------
        int

        """
    def set_client_order_id_count(self, count: int) -> None:
        """
        Set the internal order ID generator count to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
    def set_order_list_id_count(self, count: int) -> None:
        """
        Set the internal order list ID generator count to the given count.

        Parameters
        ----------
        count : int
            The count to set.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
    def generate_client_order_id(self) -> ClientOrderId:
        """
        Generate and return a new client order ID.

        The identifier will be the next in the logical sequence.

        Returns
        -------
        ClientOrderId

        """
    def generate_order_list_id(self) -> OrderListId:
        """
        Generate and return a new order list ID.

        The identifier will be the next in the logical sequence.

        Returns
        -------
        OrderListId

        """
    def reset(self) -> None:
        """
        Reset the order factory.

        All stateful fields are reset to their initial value.
        """
    def create_list(self, orders: list[Order]) -> OrderList:
        """
        Return a new order list containing the given `orders`.

        Parameters
        ----------
        orders : list[Order]
            The orders for the list.

        Returns
        -------
        OrderList

        Raises
        ------
        ValueError
            If `orders` is empty.

        Notes
        -----
        The order at index 0 in the list will be considered the 'first' order.

        """
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
    ) -> MarketOrder:
        """
        Create a new ``MARKET`` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The orders time in force. Often not applicable for market orders.
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD``.

        """
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
    ) -> LimitOrder:
        """
        Create a new ``LIMIT`` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders price.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
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
    ) -> StopMarketOrder:
        """
        Create a new ``STOP_MARKET`` conditional order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trigger_price : Price
            The orders trigger price (STOP).
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce  {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        StopMarketOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.

        """
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
    ) -> StopLimitOrder:
        """
        Create a new ``STOP_LIMIT`` conditional order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders limit price.
        trigger_price : Price
            The orders trigger stop price.
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        StopLimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
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
    ) -> MarketToLimitOrder:
        """
        Create a new ``MARKET`` order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``GTD``, ``IOC``, ``FOK``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        display_qty : Quantity, optional
            The quantity of the limit order to display on the public book (iceberg).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        MarketToLimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.

        """
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
    ) -> MarketIfTouchedOrder:
        """
        Create a new ``MARKET_IF_TOUCHED`` (MIT) conditional order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trigger_price : Price
            The orders trigger price (STOP).
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        MarketIfTouchedOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.

        """
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
    ) -> LimitIfTouchedOrder:
        """
        Create a new ``LIMIT_IF_TOUCHED`` (LIT) conditional order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        price : Price
            The orders limit price.
        trigger_price : Price
            The orders trigger stop price.
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        LimitIfTouchedOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
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
    ) -> TrailingStopMarketOrder:
        """
        Create a new ``TRAILING_STOP_MARKET`` conditional order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trailing_offset : Decimal
            The trailing offset for the trigger price (STOP).
        activation_price : Price, optional
            The price for the order to become active. If ``None`` then the order will be activated right after the order is accepted.
        trigger_price : Price, optional
            The order trigger price (STOP). If ``None`` then will typically default
            to the delta of market price and `trailing_offset`.
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        trailing_offset_type : TrailingOffsetType, default ``PRICE``
            The order trailing offset type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        TrailingStopMarketOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `trailing_offset_type` is ``NO_TRAILING_OFFSET``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.

        """
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
    ) -> TrailingStopLimitOrder:
        """
        Create a new ``TRAILING_STOP_LIMIT`` conditional order.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The orders side.
        quantity : Quantity
            The orders quantity (> 0).
        trailing_offset : Decimal
            The trailing offset for the trigger price (STOP).
        limit_offset : Decimal
            The trailing offset for the order price (LIMIT).
        price : Price, optional
            The order price (LIMIT). If ``None`` then will typically default to the
            delta of market price and `limit_offset`.
        activation_price : Price, optional
            The price for the order to become active. If ``None`` then the order will be activated right after the order is accepted.
        trigger_price : Price, optional
            The order trigger price (STOP). If ``None`` then will typically default
            to the delta of market price and `trailing_offset`.
        trigger_type : TriggerType, default ``DEFAULT``
            The order trigger type.
        trailing_offset_type : TrailingOffsetType, default ``PRICE``
            The order trailing offset type.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
            The orders time in force.
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the order carries the 'reduce-only' execution instruction.
        quote_quantity : bool
            If the order quantity is denominated in the quote currency.
        display_qty : Quantity, optional
            The quantity of the order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        exec_algorithm_id : ExecAlgorithmId, optional
            The execution algorithm ID for the order.
        exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tags : list[str], optional
            The custom user tags for the order.
        client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        TrailingStopLimitOrder

        Raises
        ------
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `trigger_type` is ``NO_TRIGGER``.
        ValueError
            If `trailing_offset_type` is ``NO_TRAILING_OFFSET``.
        ValueError
            If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
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
    ) -> OrderList:
        """
        Create a bracket order with optional entry of take-profit order types.

        The stop-loss order will always be ``STOP_MARKET``.

        Parameters
        ----------
        instrument_id : InstrumentId
            The orders instrument ID.
        order_side : OrderSide {``BUY``, ``SELL``}
            The entry orders side.
        quantity : Quantity
            The entry orders quantity (> 0).
        quote_quantity : bool
            If order quantity is denominated in the quote currency.
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        trigger_instrument_id : InstrumentId, optional
            The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
        contingency_type : ContingencyType, default ``OUO``
            The contingency type for the TP and SL bracket orders.
        entry_order_type : OrderType {``MARKET``, ``LIMIT``, ``LIMIT_IF_TOUCHED``, ``MARKET_IF_TOUCHED``, ``STOP_LIMIT``}, default ``MARKET``
            The entry order type.
        entry_price : Price, optional
            The entry order price (LIMIT).
        entry_trigger_price : Price, optional
            The entry order trigger price (STOP).
        expire_time : datetime, optional
            The order expiration (for ``GTD`` orders).
        time_in_force : TimeInForce, default ``GTC``
            The entry orders time in force.
        entry_post_only : bool, default False
            If the entry order will only provide liquidity (make a market).
        entry_exec_algorithm_id : ExecAlgorithmId, optional
            The entry order execution algorithm ID.
        entry_exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        entry_tags : list[str], default [ENTRY]
            The custom user tags for the entry order.
        entry_client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.
        tp_order_type : OrderType {``LIMIT``, ``LIMIT_IF_TOUCHED``, ``MARKET_IF_TOUCHED``}, default ``LIMIT``
            The take-profit order type.
        tp_price : Price, optional
            The take-profit child order price (LIMIT).
        tp_trigger_price : Price, optional
            The take-profit child order trigger price (STOP).
        tp_trigger_type : TriggerType, default ''DEFAULT''
            The take-profit order's trigger type
        tp_activation_price : Price, optional
            The price for the take-profit order to become active.
        tp_trailing_offset : Decimal
            The trailing offset for the take-profit order's trigger price (STOP).
        tp_trailing_offset_type : TrailingOffsetType, default ``PRICE``
            The trailing offset type for the take-profit order.
        tp_limit_offset : Decimal
            The trailing offset for the take-profit order's price (LIMIT).
        tp_time_in_force : TimeInForce, default ``GTC``
            The take-profit orders time in force.
        tp_post_only : bool, default False
            If the take-profit order will only provide liquidity (make a market).
        tp_exec_algorithm_id : ExecAlgorithmId, optional
            The take-profit order execution algorithm ID.
        tp_exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        tp_tags : list[str], default ["TAKE_PROFIT"]
            The custom user tags for the take-profit order.
        tp_client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.
        sl_order_type : OrderType {``STOP_MARKET``, ``TRAILING_STOP_MARKET``}, default ``STOP_MARKET``
            The stop-loss order type.
        sl_trigger_price : Price, optional
            The stop-loss child order trigger price (STOP).
        sl_trigger_type : TriggerType, default ''DEFAULT''
            The stop-loss order's trigger type
        sl_activation_price : Price, optional
            The price for the stop-loss order to become active.
        sl_trailing_offset : Decimal
            The trailing offset for the stoploss order's trigger price (STOP).
        sl_trailing_offset_type : TrailingOffsetType, default ``PRICE``
            The trailing offset type for the stop-loss order.
        sl_time_in_force : TimeInForce, default ``GTC``
            The stop-loss orders time in force.
        sl_exec_algorithm_id : ExecAlgorithmId, optional
            The stop-loss order execution algorithm ID.
        sl_exec_algorithm_params : dict[str, Any], optional
            The execution algorithm parameters for the order.
        sl_tags : list[str], default ["STOP_LOSS"]
            The custom user tags for the stop-loss order.
        sl_client_order_id : ClientOrderId, optional
            The custom client order ID for the order.
            If a client order ID is not provided then one will be generated by the factory.

        Returns
        -------
        OrderList

        """

