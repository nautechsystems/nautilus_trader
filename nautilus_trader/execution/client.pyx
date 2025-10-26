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

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport

from libc.stdint cimport uint64_t

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport QueryAccount
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class ExecutionClient(Component):
    """
    The base class for all execution clients.

    Parameters
    ----------
    client_id : ClientId
        The client ID.
    venue : Venue or ``None``
        The client venue. If multi-venue then can be ``None``.
    oms_type : OmsType
        The venues order management system type.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency or ``None``
        The account base currency. Use ``None`` for multi-currency accounts.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    config : NautilusConfig, optional
        The configuration for the instance.

    Raises
    ------
    ValueError
        If `client_id` is not equal to `account_id.get_issuer()`.
    ValueError
        If `oms_type` is ``UNSPECIFIED`` (must be specified).

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        Venue venue: Venue | None,
        OmsType oms_type,
        AccountType account_type,
        Currency base_currency: Currency | None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        config: NautilusConfig | None = None,
    ):
        Condition.not_equal(oms_type, OmsType.UNSPECIFIED, "oms_type", "UNSPECIFIED")

        super().__init__(
            clock=clock,
            component_id=client_id,
            component_name=f"ExecClient-{client_id}",
            msgbus=msgbus,
            config=config,
        )

        self._cache = cache

        self.trader_id = msgbus.trader_id
        self.venue = venue
        self.oms_type = oms_type
        self.account_id = None  # Initialized on connection
        self.account_type = account_type
        self.base_currency = base_currency

        self.is_connected = False

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

    cpdef void _set_connected(self, bint value=True):
        # Setter for Python implementations to change the readonly property
        self.is_connected = value

    cpdef void _set_account_id(self, AccountId account_id):
        Condition.not_none(account_id, "account_id")
        Condition.equal(self.id.to_str(), account_id.get_issuer(), "id.value", "account_id.get_issuer()")

        self.account_id = account_id

    cpdef Account get_account(self):
        """
        Return the account for the client (if registered).

        Returns
        -------
        Account or ``None``

        """
        return self._cache.account(self.account_id)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command):
        """
        Submit the order contained in the given command for execution.

        Parameters
        ----------
        command : SubmitOrder
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `submit_order` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `submit_order` must be implemented in the subclass")

    cpdef void submit_order_list(self, SubmitOrderList command):
        """
        Submit the order list contained in the given command for execution.

        Parameters
        ----------
        command : SubmitOrderList
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `submit_order_list` method for this client",  # pragma: no cover   # noqa
        )
        raise NotImplementedError("method `submit_order_list` must be implemented in the subclass")

    cpdef void modify_order(self, ModifyOrder command):
        """
        Modify the order with parameters contained in the command.

        Parameters
        ----------
        command : ModifyOrder
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `modify_order` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `modify_order` must be implemented in the subclass")

    cpdef void cancel_order(self, CancelOrder command):
        """
        Cancel the order with the client order ID contained in the given command.

        Parameters
        ----------
        command : CancelOrder
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `cancel_order` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `cancel_order` must be implemented in the subclass")

    cpdef void cancel_all_orders(self, CancelAllOrders command):
        """
        Cancel all orders for the instrument ID contained in the given command.

        Parameters
        ----------
        command : CancelAllOrders
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `cancel_all_orders` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `cancel_all_orders` must be implemented in the subclass")

    cpdef void batch_cancel_orders(self, BatchCancelOrders command):
        """
        Batch cancel orders for the instrument ID contained in the given command.

        Parameters
        ----------
        command : BatchCancelOrders
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `batch_cancel_orders` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `batch_cancel_orders` must be implemented in the subclass")

    cpdef void query_account(self, QueryAccount command):
        """
        Query the account specified by the command which will generate an `AccountState` event.

        Parameters
        ----------
        command : QueryAccount
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `query_account` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `query_account` must be implemented in the subclass")

    cpdef void query_order(self, QueryOrder command):
        """
        Initiate a reconciliation for the queried order which will generate an
        `OrderStatusReport`.

        Parameters
        ----------
        command : QueryOrder
            The command to execute.

        """
        self._log.error(  # pragma: no cover
            f"Cannot execute command {command}: not implemented. "  # pragma: no cover
            f"You can implement by overriding the `query_order` method for this client",  # pragma: no cover  # noqa
        )
        raise NotImplementedError("method `query_order` must be implemented in the subclass")

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void generate_account_state(
        self,
        list balances,
        list margins,
        bint reported,
        uint64_t ts_event,
        dict info = None,
    ):
        """
        Generate an `AccountState` event and publish on the message bus.

        Parameters
        ----------
        balances : list[AccountBalance]
            The account balances.
        margins : list[MarginBalance]
            The margin balances.
        reported : bool
            If the balances are reported directly from the exchange.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the account state event occurred.
        info : dict [str, object]
            The additional implementation specific account information.

        """
        # Generate event
        cdef AccountState account_state = AccountState(
            account_id=self.account_id,
            account_type=self.account_type,
            base_currency=self.base_currency,
            reported=reported,
            balances=balances,
            margins=margins,
            info=info or {},
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_account_state(account_state)

    cpdef void generate_order_denied(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        str reason,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderDenied` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        reason : str
            The order denied reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order denied event occurred.

        """
        Condition.not_none(instrument_id, "instrument_id")

        # Generate event
        cdef OrderDenied denied = OrderDenied(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        denied._mem.ts_event = ts_event

        self._send_order_event(denied)

    cpdef void generate_order_submitted(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderSubmitted` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order submitted event occurred.

        """
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            trader_id=self._msgbus.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            account_id=self.account_id,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(submitted)

    cpdef void generate_order_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        str reason,
        uint64_t ts_event,
        bint due_post_only=False,
    ):
        """
        Generate an `OrderRejected` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        reason : datetime
            The order rejected reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order rejected event occurred.
        due_post_only : bool, default False
            If the order was rejected because it was post-only and would execute immediately as a taker.

        """
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            account_id=self.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
            due_post_only=due_post_only,
        )

        self._send_order_event(rejected)

    cpdef void generate_order_accepted(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderAccepted` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order accepted event occurred.

        """
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(accepted)

    cpdef void generate_order_modify_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderModifyRejected` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        reason : str
            The order update rejected reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order update rejection event occurred.

        """
        # Generate event
        cdef OrderModifyRejected modify_rejected = OrderModifyRejected(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(modify_rejected)

    cpdef void generate_order_cancel_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderCancelRejected` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        reason : str
            The order cancel rejected reason.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order cancel rejected event occurred.

        """
        # Generate event
        cdef OrderCancelRejected cancel_rejected = OrderCancelRejected(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            reason=reason,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(cancel_rejected)

    cpdef void generate_order_updated(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        Quantity quantity,
        Price price,
        Price trigger_price,
        uint64_t ts_event,
        bint venue_order_id_modified=False,
    ):
        """
        Generate an `OrderUpdated` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        quantity : Quantity
            The orders current quantity.
        price : Price
            The orders current price.
        trigger_price : Price or ``None``
            The orders current trigger price.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order update event occurred.
        venue_order_id_modified : bool
            If the ID was modified for this event.

        """
        Condition.not_none(client_order_id, "client_order_id")
        Condition.not_none(venue_order_id, "venue_order_id")

        # Check venue_order_id against cache, only allow modification when `venue_order_id_modified=True`
        if not venue_order_id_modified:
            existing = self._cache.venue_order_id(client_order_id)
            if existing is not None:
                Condition.equal(existing, venue_order_id, "existing", "order.venue_order_id")
            else:
                self._log.warning(f"{venue_order_id} does not match existing {repr(existing)}")

        # Generate event
        cdef OrderUpdated updated = OrderUpdated(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(updated)

    cpdef void generate_order_canceled(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderCanceled` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when order canceled event occurred.

        """
        # Generate event
        cdef OrderCanceled canceled = OrderCanceled(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(canceled)

    cpdef void generate_order_triggered(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderTriggered` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order triggered event occurred.

        """
        # Generate event
        cdef OrderTriggered triggered = OrderTriggered(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(triggered)

    cpdef void generate_order_expired(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    ):
        """
        Generate an `OrderExpired` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order expired event occurred.

        """
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(expired)

    cpdef void generate_order_filled(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        PositionId venue_position_id: PositionId | None,
        TradeId trade_id,
        OrderSide order_side,
        OrderType order_type,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side,
        uint64_t ts_event,
        dict info = None,
    ):
        """
        Generate an `OrderFilled` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID (assigned by the venue).
        trade_id : TradeId
            The trade ID.
        venue_position_id : PositionId or ``None``
            The venue position ID associated with the order. If the trading
            venue has assigned a position ID / ticket then pass that here,
            otherwise pass ``None`` and the execution engine OMS will handle
            position ID resolution.
        order_side : OrderSide {``BUY``, ``SELL``}
            The execution order side.
        order_type : OrderType
            The execution order type.
        last_qty : Quantity
            The fill quantity for this execution.
        last_px : Price
            The fill price for this execution (not average price).
        quote_currency : Currency
            The currency of the price.
        commission : Money
            The fill commission.
        liquidity_side : LiquiditySide {``NO_LIQUIDITY_SIDE``, ``MAKER``, ``TAKER``}
            The execution liquidity side.
        ts_event : uint64_t
            UNIX timestamp (nanoseconds) when the order filled event occurred.
        info : dict[str, object], optional
            The additional fill information.

        """
        Condition.not_none(instrument_id, "instrument_id")

        # Generate event
        cdef OrderFilled fill = OrderFilled(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=self.account_id,
            trade_id=trade_id,
            position_id=venue_position_id,
            order_side=order_side,
            order_type=order_type,
            last_qty=last_qty,
            last_px=last_px,
            currency=quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            event_id=UUID4(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
            info=info,
        )

        self._send_order_event(fill)

# --------------------------------------------------------------------------------------------------

    cpdef void _send_account_state(self, account_state: AccountState):
        self._msgbus.send(
            endpoint=f"Portfolio.update_account",
            msg=account_state,
        )

    cpdef void _send_order_event(self, event: OrderEvent):
        self._msgbus.send(
            endpoint="ExecEngine.process",
            msg=event,
        )

    cpdef void _send_mass_status_report(self, report: ExecutionMassStatus):
        self._msgbus.send(
            endpoint="ExecEngine.reconcile_execution_mass_status",
            msg=report,
        )

    cpdef void _send_order_status_report(self, report: OrderStatusReport):
        self._msgbus.send(
            endpoint="ExecEngine.reconcile_execution_report",
            msg=report,
        )

    cpdef void _send_fill_report(self, report: FillReport):
        self._msgbus.send(
            endpoint="ExecEngine.reconcile_execution_report",
            msg=report,
        )

    cpdef void _send_position_status_report(self, report: PositionStatusReport):
        self._msgbus.send(
            endpoint="ExecEngine.reconcile_execution_report",
            msg=report,
        )
