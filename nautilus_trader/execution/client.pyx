# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport ModifyOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport SubmitOrderList
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
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
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class ExecutionClient(Component):
    """
    The abstract base class for all execution clients.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        VenueType venue_type,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,  # Can be None
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``ExecutionClient`` class.

        Parameters
        ----------
        client_id : ClientId
            The client ID.
        venue_type : VenueType
            The venue type for the client (determines venue -> client_id mapping).
        account_id : AccountId
            The account ID for the client.
        account_type : AccountType
            The account type for the client.
        base_currency : Currency, optional
            The account base currency. Use ``None`` for multi-currency accounts.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : Clock
            The clock for the client.
        logger : Logger
            The logger for the client.
        config : dict[str, object], optional
            The configuration for the instance.

        Raises
        ------
        ValueError
            If client_id is not equal to account_id.issuer.

        """
        Condition.equal(client_id.value, account_id.issuer, "client_id.value", "account_id.issuer")

        if config is None:
            config = {}
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=client_id,
            component_name=config.get("name", f"ExecClient-{client_id.value}"),
            msgbus=msgbus,
            config=config,
        )

        self._cache = cache
        self._account = None  # Initialized on connection

        self.trader_id = msgbus.trader_id
        self.venue = Venue(client_id.value) if venue_type != VenueType.BROKERAGE_MULTI_VENUE else None
        self.venue_type = venue_type
        self.account_id = account_id
        self.account_type = account_type
        self.base_currency = base_currency

        self.is_connected = False

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

    cpdef void _set_connected(self, bint value=True) except *:
        """
        Setter for pure Python implementations to change the readonly property.

        Parameters
        ----------
        value : bool
            The value to set for is_connected.

        """
        self.is_connected = value

    cpdef Account get_account(self):
        """
        Return the account for the client (if registered).

        Returns
        -------
        Account or ``None``

        """
        # Check account
        if self._account is None:
            account = self._cache.account_for_venue(self.venue)
            if account is None:
                self._log.error(
                    "Cannot generate OrderFilled: "
                    f"no account found for venue {self.venue}."
                )
                return
            self._account = account

        return self._account

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void submit_order_list(self, SubmitOrderList command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void modify_order(self, ModifyOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void cancel_order(self, CancelOrder command) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cpdef void generate_account_state(
        self,
        list balances,
        bint reported,
        int64_t ts_event,
        dict info=None,
    ) except *:
        """
        Generate an `AccountState` event and publish on the message bus.

        Parameters
        ----------
        balances : list[AccountBalance]
            The account balances.
        reported : bool
            If the balances are reported directly from the exchange.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the account state event occurred.
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
            info=info or {},
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_account_state(account_state)

    cpdef void generate_order_submitted(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        int64_t ts_event,
    ) except *:
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
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order submitted event occurred.

        """
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            trader_id=self._msgbus.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            event_id=self._uuid_factory.generate(),
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
        int64_t ts_event,
    ) except *:
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
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order rejected event occurred.

        """
        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(rejected)

    cpdef void generate_order_accepted(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order accepted event occurred.

        """
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(accepted)

    cpdef void generate_order_pending_update(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t ts_event,
    ) except *:
        """
        Generate an `OrderPendingUpdate` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_event : datetime
            The UNIX timestamp (nanoseconds) when the order pending update event occurred.

        """
        # Generate event
        cdef OrderPendingUpdate pending_replace = OrderPendingUpdate(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(pending_replace)

    cpdef void generate_order_pending_cancel(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        int64_t ts_event,
    ) except *:
        """
        Generate an `OrderPendingCancel` event and send it to the `ExecutionEngine`.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID associated with the event.
        instrument_id : InstrumentId
            The instrument ID.
        client_order_id : ClientOrderId
            The client order ID.
        venue_order_id : VenueOrderId
            The venue order ID.
        ts_event : datetime
            The UNIX timestamp (nanoseconds) when the order pending cancel event occurred.

        """
        # Generate event
        cdef OrderPendingCancel pending_cancel = OrderPendingCancel(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(pending_cancel)

    cpdef void generate_order_modify_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        reason : str
            The order update rejected reason.
        ts_event : datetime
            The UNIX timestamp (nanoseconds) when the order update rejection event occurred.

        """
        # Generate event
        cdef OrderModifyRejected modify_rejected = OrderModifyRejected(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
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
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        reason : str
            The order cancel rejected reason.
        ts_event : datetime
            The UNIX timestamp (nanoseconds) when the order cancel rejected event occurred.

        """
        # Generate event
        cdef OrderCancelRejected cancel_rejected = OrderCancelRejected(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            reason=reason,
            event_id=self._uuid_factory.generate(),
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
        Price trigger,
        int64_t ts_event,
        bint venue_order_id_modified=False,
    ) except *:
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
            The venue order ID.
        quantity : Quantity
            The orders current quantity.
        price : Price
            The orders current price.
        trigger : Price, optional
            The orders current trigger price.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order update event occurred.
        venue_order_id_modified : bool
            If the ID was modified for this event.

        """
        Condition.not_none(client_order_id, "client_order_id")
        Condition.not_none(venue_order_id, "venue_order_id")

        # Check venue_order_id against cache, only allow modification when `venue_order_id_modified=True`
        if not venue_order_id_modified:
            existing = self._cache.venue_order_id(client_order_id)
            Condition.equal(existing, venue_order_id, "existing", "order.venue_order_id")

        # Generate event
        cdef OrderUpdated updated = OrderUpdated(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            account_id=self.account_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            quantity=quantity,
            price=price,
            trigger=trigger,
            event_id=self._uuid_factory.generate(),
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
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when order canceled event occurred.

        """
        # Generate event
        cdef OrderCanceled canceled = OrderCanceled(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            event_id=self._uuid_factory.generate(),
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
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order triggered event occurred.

        """
        # Generate event
        cdef OrderTriggered triggered = OrderTriggered(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            event_id=self._uuid_factory.generate(),
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
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order expired event occurred.

        """
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            event_id=self._uuid_factory.generate(),
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
        PositionId venue_position_id,  # Can be None
        ExecutionId execution_id,
        OrderSide order_side,
        OrderType order_type,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side,
        int64_t ts_event,
    ) except *:
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
            The venue order ID.
        execution_id : ExecutionId
            The execution ID.
        venue_position_id : PositionId, optional
            The venue position ID associated with the order. If the trading
            venue has assigned a position ID / ticket then pass that here,
            otherwise pass ``None`` and the execution engine OMS will handle
            position ID resolution.
        order_side : OrderSide
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
        liquidity_side : LiquiditySide
            The execution liquidity side.
        ts_event : int64
            The UNIX timestamp (nanoseconds) when the order filled event occurred.

        """
        Condition.not_none(instrument_id, "instrument_id")

        # Generate event
        cdef OrderFilled fill = OrderFilled(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            execution_id=execution_id,
            position_id=venue_position_id,
            order_side=order_side,
            order_type=order_type,
            last_qty=last_qty,
            last_px=last_px,
            currency=quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            event_id=self._uuid_factory.generate(),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_event(fill)

# --------------------------------------------------------------------------------------------------

    cdef void _send_account_state(self, AccountState account_state) except *:
        self._msgbus.send(
            endpoint=f"Portfolio.update_account",
            msg=account_state,
        )

    cdef void _send_order_event(self, OrderEvent event) except *:
        self._msgbus.send(
            endpoint="ExecEngine.process",
            msg=event,
        )
