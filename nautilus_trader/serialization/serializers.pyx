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

import decimal

from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t

import msgpack

from nautilus_trader.cache.identifier cimport IdentifierCache
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport maybe_dt_to_unix_nanos
from nautilus_trader.core.datetime cimport maybe_nanos_to_unix_dt
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.c_enums.account_type cimport AccountTypeParser
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_class cimport AssetClassParser
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.asset_type cimport AssetTypeParser
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.order_type cimport OrderTypeParser
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForceParser
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport TradingCommand
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelRejected
from nautilus_trader.model.events cimport OrderCanceled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInitialized
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderPendingCancel
from nautilus_trader.model.events cimport OrderPendingReplace
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderTriggered
from nautilus_trader.model.events cimport OrderUpdateRejected
from nautilus_trader.model.events cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.crypto_swap cimport CryptoSwap
from nautilus_trader.model.instruments.currency cimport CurrencySpot
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.serialization.base cimport CommandSerializer
from nautilus_trader.serialization.base cimport EventSerializer
from nautilus_trader.serialization.base cimport InstrumentSerializer
from nautilus_trader.serialization.base cimport OrderSerializer


cdef inline str convert_camel_to_snake(str value):
    return ''.join([f'_{c.lower()}' if c.isupper() else c for c in value]).lstrip('_').upper()

cdef inline str convert_snake_to_camel(str value):
    return ''.join(x.title() for x in value.split('_'))


cdef class MsgPackSerializer:
    """
    Provides a serializer for the MessagePack specification.

    """
    @staticmethod
    cdef bytes serialize(dict message):
        """
        Serialize the given message to `MessagePack` specification bytes.

        Parameters
        ----------
        message : dict
            The message to serialize.

        Returns
        -------
        bytes

        """
        Condition.not_none(message, "message")

        return msgpack.packb(message)

    @staticmethod
    cdef dict deserialize(bytes message_bytes):
        """
        Deserialize the given `MessagePack` specification bytes to a dictionary.

        Parameters
        ----------
        message_bytes : bytes
            The message bytes to deserialize.

        Returns
        -------
        dict[str, object]

        """
        Condition.not_none(message_bytes, "message_bytes")

        return msgpack.unpackb(message_bytes)


cdef class MsgPackInstrumentSerializer(InstrumentSerializer):
    """
    Provides an `Instrument` serializer for the `MessagePack` specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the ``MsgPackOrderSerializer`` class.

        """
        super().__init__()

        self.instrument_id_cache = ObjectCache(InstrumentId, InstrumentId.from_str_c)

    cpdef bytes serialize(self, Instrument instrument):
        """
        Serialize the given instrument to `MessagePack` specification bytes.

        Parameters
        ----------
        instrument : Instrument
            The instrument to serialize.

        Returns
        -------
        bytes

        """
        Condition.not_none(instrument, "instrument")

        cdef str asset_class = AssetClassParser.to_str(instrument.asset_class)
        cdef str asset_type = AssetTypeParser.to_str(instrument.asset_type)

        cdef dict package = {
            TYPE: type(instrument).__name__,
            ID: instrument.id.value,
            ASSET_CLASS: convert_snake_to_camel(asset_class),
            ASSET_TYPE: convert_snake_to_camel(asset_type),
            QUOTE_CURRENCY: instrument.quote_currency.code,
            IS_INVERSE: instrument.is_inverse,
            PRICE_PRECISION: instrument.price_precision,
            SIZE_PRECISION: instrument.size_precision,
            PRICE_INCREMENT: str(instrument.price_increment),
            SIZE_INCREMENT: str(instrument.size_increment),
            MULTIPLIER: str(instrument.multiplier),
            LOT_SIZE: str(instrument.lot_size) if instrument.lot_size is not None else None,
            MAX_QUANTITY: str(instrument.max_quantity) if instrument.max_quantity is not None else None,
            MIN_QUANTITY: str(instrument.min_quantity) if instrument.min_quantity is not None else None,
            MAX_NOTIONAL: instrument.max_notional.to_str() if instrument.max_notional is not None else None,
            MIN_NOTIONAL: instrument.min_notional.to_str() if instrument.min_notional is not None else None,
            MAX_PRICE: str(instrument.max_price) if instrument.max_price is not None else None,
            MIN_PRICE: str(instrument.min_price) if instrument.min_price is not None else None,
            MARGIN_INIT: str(instrument.margin_init),
            MARGIN_MAINT: str(instrument.margin_maint),
            MAKER_FEE: str(instrument.maker_fee),
            TAKER_FEE: str(instrument.taker_fee),
            TS_EVENT: str(instrument.ts_event_ns),
            TS_RECV: str(instrument.ts_recv_ns),
        }

        if isinstance(instrument, CurrencySpot):
            package[BASE_CURRENCY] = instrument.base_currency.code
        elif isinstance(instrument, CryptoSwap):
            package[BASE_CURRENCY] = instrument.base_currency.code
            package[SETTLEMENT_CURRENCY] = instrument.settlement_currency.code
        elif isinstance(instrument, BettingInstrument):
            package["BettingInfo"] = {
                "event_type_id": instrument.event_type_id,
                "event_type_name": instrument.event_type_name,
                "competition_id": instrument.competition_id,
                "competition_name": instrument.competition_name,
                "event_id": instrument.event_id,
                "event_name": instrument.event_name,
                "event_country_code": instrument.event_country_code,
                "event_open_date": instrument.event_open_date,
                "betting_type": instrument.betting_type,
                "market_id": instrument.market_id,
                "market_name": instrument.market_name,
                "market_start_time": instrument.market_start_time,
                "market_type": instrument.market_type,
                "selection_id": instrument.selection_id,
                "selection_name": instrument.selection_name,
                "selection_handicap": instrument.selection_handicap,
            }

        # TODO(cs): Other instruments as required

        return MsgPackSerializer.serialize(package)

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Deserialize the given `MessagePack` specification bytes to an instrument.

        Parameters
        ----------
        instrument_bytes : bytes
            The instrument bytes to deserialize.

        Returns
        -------
        Instrument

        Raises
        ------
        ValueError
            If instrument_bytes is empty.

        """
        Condition.not_empty(instrument_bytes, "instrument_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(instrument_bytes)

        cdef str instrument_type = unpacked[TYPE]
        cdef InstrumentId instrument_id = self.instrument_id_cache.get(unpacked[ID])
        cdef AssetClass asset_class = AssetClassParser.from_str(convert_camel_to_snake(unpacked[ASSET_CLASS]))
        cdef AssetType asset_type = AssetTypeParser.from_str(convert_camel_to_snake(unpacked[ASSET_TYPE]))
        cdef Currency quote_currency = Currency.from_str_c(unpacked[QUOTE_CURRENCY])
        cdef bint is_inverse = unpacked[IS_INVERSE]
        cdef uint8_t price_precision = unpacked[PRICE_PRECISION]
        cdef uint8_t size_precision = unpacked[SIZE_PRECISION]
        cdef Price price_increment = Price.from_str_c(unpacked[PRICE_INCREMENT])
        cdef Quantity size_increment = Quantity.from_str_c(unpacked[SIZE_INCREMENT])
        cdef Quantity multiplier = Quantity.from_str_c(unpacked[MULTIPLIER])

        # Parse limits
        cdef str lot_size_str = unpacked[LOT_SIZE]
        cdef str max_quantity_str = unpacked.get(MAX_QUANTITY)
        cdef str min_quantity_str = unpacked.get(MIN_QUANTITY)
        cdef str max_notional_str = unpacked.get(MAX_NOTIONAL)
        cdef str min_notional_str = unpacked.get(MIN_NOTIONAL)
        cdef str max_price_str = unpacked.get(MAX_PRICE)
        cdef str min_price_str = unpacked.get(MIN_PRICE)
        cdef Quantity lot_size = Quantity.from_str_c(lot_size_str) if lot_size_str is not None else None
        cdef Quantity max_quantity = Quantity.from_str_c(max_quantity_str) if max_quantity_str is not None else None
        cdef Quantity min_quantity = Quantity.from_str_c(min_quantity_str) if min_quantity_str is not None else None
        cdef Money max_notional = Money.from_str_c(max_notional_str.replace(',', '')) if max_notional_str is not None else None
        cdef Money min_notional = Money.from_str_c(min_notional_str.replace(',', '')) if min_notional_str is not None else None
        cdef Price max_price = Price.from_str_c(max_price_str) if max_price_str is not None else None
        cdef Price min_price = Price.from_str_c(min_price_str) if min_price_str is not None else None

        cdef object margin_init = decimal.Decimal(unpacked[MARGIN_INIT])
        cdef object margin_maint = decimal.Decimal(unpacked[MARGIN_MAINT])
        cdef object maker_fee = decimal.Decimal(unpacked[MAKER_FEE])
        cdef object taker_fee = decimal.Decimal(unpacked[TAKER_FEE])
        cdef int64_t ts_event_ns = int(unpacked[TS_EVENT])
        cdef int64_t ts_recv_ns = int(unpacked[TS_RECV])

        if instrument_type == Instrument.__name__:
            return Instrument(
                instrument_id=instrument_id,
                asset_class=asset_class,
                asset_type=asset_type,
                quote_currency=quote_currency,
                is_inverse=is_inverse,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                multiplier=multiplier,
                lot_size=lot_size,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=max_notional,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event_ns=ts_event_ns,
                ts_recv_ns=ts_recv_ns,
            )
        elif instrument_type == CurrencySpot.__name__:
            return CurrencySpot(
                instrument_id=instrument_id,
                base_currency=Currency.from_str_c(unpacked[BASE_CURRENCY]),
                quote_currency=quote_currency,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                lot_size=lot_size,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=max_notional,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event_ns=ts_event_ns,
                ts_recv_ns=ts_recv_ns,
            )
        elif instrument_type == CryptoSwap.__name__:
            return CryptoSwap(
                instrument_id=instrument_id,
                base_currency=Currency.from_str_c(unpacked[BASE_CURRENCY]),
                quote_currency=quote_currency,
                settlement_currency=Currency.from_str_c(unpacked[SETTLEMENT_CURRENCY]),
                is_inverse=is_inverse,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=max_notional,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event_ns=ts_event_ns,
                ts_recv_ns=ts_recv_ns,
            )
        elif instrument_type == BettingInstrument.__name__:
            betting_info = unpacked["BettingInfo"]
            return BettingInstrument(
                venue_name="BETFAIR",  # TODO(bm): Hardcoded for now
                betting_type="ODDS",
                event_country_code=betting_info.get("event_country_code"),
                event_id=betting_info.get("event_id"),
                competition_id=betting_info.get("competition_id"),
                competition_name=betting_info.get("competition_name"),
                event_name=betting_info.get("event_name"),
                event_open_date=betting_info.get("event_open_date"),
                event_type_id=betting_info.get("event_type_id"),
                event_type_name=betting_info.get("event_type_name"),
                market_id=betting_info.get("market_id"),
                market_name=betting_info.get("market_name"),
                market_start_time=betting_info.get("market_start_time"),
                market_type=betting_info.get("market_type"),
                currency=betting_info.get("currency"),
                selection_id=betting_info.get("selection_id"),
                selection_name=betting_info.get("selection_name"),
                selection_handicap=betting_info.get("selection_handicap"),
                ts_event_ns=ts_event_ns,
                ts_recv_ns=ts_recv_ns,
            )

        raise ValueError(f"Invalid instrument type: was {instrument_type}")

cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides an `Order` serializer for the `MessagePack` specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the ``MsgPackOrderSerializer`` class.

        """
        super().__init__()

        self.instrument_id_cache = ObjectCache(InstrumentId, InstrumentId.from_str_c)

    cpdef bytes serialize(self, Order order):
        """
        Return the serialized MessagePack specification bytes from the given order.

        Parameters
        ----------
        order : Order
            The order to serialize (can be None).

        Returns
        -------
        bytes

        """
        if order is None:
            return MsgPackSerializer.serialize({})  # Null order

        cdef dict package = {
            ID: order.client_order_id.value,
            STRATEGY_ID: order.strategy_id.value,
            INSTRUMENT_ID: order.instrument_id.value,
            ORDER_SIDE: convert_snake_to_camel(OrderSideParser.to_str(order.side)),
            ORDER_TYPE: convert_snake_to_camel(OrderTypeParser.to_str(order.type)),
            QUANTITY: str(order.quantity),
            TIME_IN_FORCE: TimeInForceParser.to_str(order.time_in_force),
            INIT_ID: order.init_id.value,
            TIMESTAMP: str(order.timestamp_ns),
        }

        if isinstance(order, PassiveOrder):
            package[PRICE] = str(order.price)
            package[EXPIRE_TIME] = maybe_dt_to_unix_nanos(order.expire_time)

        if isinstance(order, LimitOrder):
            package[POST_ONLY] = order.is_post_only
            package[REDUCE_ONLY] = order.is_reduce_only
            package[HIDDEN] = order.is_hidden
        elif isinstance(order, StopMarketOrder):
            package[REDUCE_ONLY] = order.is_reduce_only
        elif isinstance(order, StopLimitOrder):
            package[TRIGGER] = str(order.trigger)
            package[POST_ONLY] = order.is_post_only
            package[REDUCE_ONLY] = order.is_reduce_only
            package[HIDDEN] = order.is_hidden

        return MsgPackSerializer.serialize(package)

    cpdef Order deserialize(self, bytes order_bytes):
        """
        Return the `Order` deserialized from the given MessagePack specification bytes.

        Parameters
        ----------
        order_bytes : bytes
            The bytes to deserialize.

        Returns
        -------
        Order

        Raises
        ------
        ValueError
            If order_bytes is empty.

        """
        Condition.not_empty(order_bytes, "order_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(order_bytes)

        if not unpacked:
            return None  # Null order

        cdef ClientOrderId client_order_id = ClientOrderId(unpacked[ID])
        cdef StrategyId strategy_id = StrategyId(unpacked[STRATEGY_ID])
        cdef InstrumentId instrument_id = self.instrument_id_cache.get(unpacked[INSTRUMENT_ID])
        cdef OrderSide order_side = OrderSideParser.from_str(convert_camel_to_snake(unpacked[ORDER_SIDE]))
        cdef OrderType order_type = OrderTypeParser.from_str(convert_camel_to_snake(unpacked[ORDER_TYPE]))
        cdef Quantity quantity = Quantity.from_str_c(unpacked[QUANTITY])
        cdef TimeInForce time_in_force = TimeInForceParser.from_str(unpacked[TIME_IN_FORCE])
        cdef UUID init_id = UUID.from_str_c(unpacked[INIT_ID])
        cdef int64_t timestamp_ns = int(unpacked[TIMESTAMP])

        if order_type == OrderType.MARKET:
            return MarketOrder(
                client_order_id=client_order_id,
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                order_side=order_side,
                quantity=quantity,
                time_in_force=time_in_force,
                init_id=init_id,
                timestamp_ns=timestamp_ns,
            )

        if order_type == OrderType.LIMIT:
            return LimitOrder(
                client_order_id=client_order_id,
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                order_side=order_side,
                quantity=quantity,
                price=Price.from_str_c(unpacked[PRICE]),
                time_in_force=time_in_force,
                expire_time=maybe_nanos_to_unix_dt(unpacked.get(EXPIRE_TIME)),
                init_id=init_id,
                timestamp_ns=timestamp_ns,
                post_only=unpacked[POST_ONLY],
                reduce_only=unpacked[REDUCE_ONLY],
                hidden=unpacked[HIDDEN],
            )

        if order_type == OrderType.STOP_MARKET:
            return StopMarketOrder(
                client_order_id=client_order_id,
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                order_side=order_side,
                quantity=quantity,
                price=Price.from_str_c(unpacked[PRICE]),
                time_in_force=time_in_force,
                expire_time=maybe_nanos_to_unix_dt(unpacked.get(EXPIRE_TIME)),
                init_id=init_id,
                timestamp_ns=timestamp_ns,
                reduce_only=unpacked[REDUCE_ONLY],
            )

        if order_type == OrderType.STOP_LIMIT:
            return StopLimitOrder(
                client_order_id=client_order_id,
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                order_side=order_side,
                quantity=quantity,
                price=Price.from_str_c(unpacked[PRICE]),
                trigger=Price.from_str_c(unpacked[TRIGGER]),
                time_in_force=time_in_force,
                expire_time=maybe_nanos_to_unix_dt(unpacked.get(EXPIRE_TIME)),
                init_id=init_id,
                timestamp_ns=timestamp_ns,
                post_only=unpacked[POST_ONLY],
                reduce_only=unpacked[REDUCE_ONLY],
                hidden=unpacked[HIDDEN],
            )

        raise ValueError(f"Invalid order_type: was {OrderTypeParser.to_str(order_type)}")


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a `Command` serializer for the MessagePack specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the ``MsgPackCommandSerializer`` class.

        """
        super().__init__()

        self.identifier_cache = IdentifierCache()
        self.order_serializer = MsgPackOrderSerializer()

    cpdef bytes serialize(self, Command command):
        """
        Return the serialized `MessagePack` specification bytes from the given command.

        Parameters
        ----------
        command : Command
            The command to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If the command cannot be serialized.

        """
        Condition.not_none(command, "command")

        cdef dict package = {
            TYPE: type(command).__name__,
            ID: command.id.value,
            TIMESTAMP: str(command.timestamp_ns),
        }

        if isinstance(command, TradingCommand):
            package[TRADER_ID] = command.trader_id.value
            package[STRATEGY_ID] = command.strategy_id.value
            package[INSTRUMENT_ID] = command.instrument_id.value

        if isinstance(command, SubmitOrder):
            package[POSITION_ID] = command.position_id.value
            package[ORDER] = self.order_serializer.serialize(command.order)
        elif isinstance(command, SubmitBracketOrder):
            package[ENTRY] = self.order_serializer.serialize(command.bracket_order.entry)
            package[STOP_LOSS] = self.order_serializer.serialize(command.bracket_order.stop_loss)
            package[TAKE_PROFIT] = self.order_serializer.serialize(command.bracket_order.take_profit)
        elif isinstance(command, UpdateOrder):
            package[CLIENT_ORDER_ID] = command.client_order_id.value
            package[VENUE_ORDER_ID] = command.venue_order_id.value
            package[QUANTITY] = str(command.quantity)
            package[PRICE] = str(command.price)
        elif isinstance(command, CancelOrder):
            package[CLIENT_ORDER_ID] = command.client_order_id.value
            package[VENUE_ORDER_ID] = command.venue_order_id.value
        else:
            raise RuntimeError(f"Cannot serialize command: unrecognized command {command}")

        return MsgPackSerializer.serialize(package)

    cpdef Command deserialize(self, bytes command_bytes):
        """
        Return the command deserialize from the given MessagePack specification command_bytes.

        Parameters
        ----------
        command_bytes : bytes
            The command to deserialize.

        Returns
        -------
        Command

        Raises
        ------
        ValueError
            If command_bytes is empty.
        RuntimeError
            If command cannot be deserialized.

        """
        Condition.not_empty(command_bytes, "command_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(command_bytes)  # type: dict[str, bytes]

        cdef str command_type = unpacked[TYPE]
        cdef UUID command_id = UUID.from_str_c(unpacked[ID])
        cdef int64_t timestamp_ns = int(unpacked[TIMESTAMP])

        if command_type == SubmitOrder.__name__:
            return SubmitOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                PositionId(unpacked[POSITION_ID]),
                self.order_serializer.deserialize(unpacked[ORDER]),
                command_id,
                timestamp_ns,
            )
        elif command_type == SubmitBracketOrder.__name__:
            return SubmitBracketOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                BracketOrder(
                    self.order_serializer.deserialize(unpacked[ENTRY]),
                    self.order_serializer.deserialize(unpacked[STOP_LOSS]),
                    self.order_serializer.deserialize(unpacked[TAKE_PROFIT]),
                ),
                command_id,
                timestamp_ns,
            )
        elif command_type == UpdateOrder.__name__:
            return UpdateOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                self.identifier_cache.get_instrument_id(unpacked[INSTRUMENT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                Quantity.from_str_c(unpacked[QUANTITY]),
                Price.from_str_c(unpacked[PRICE]),
                command_id,
                timestamp_ns,
            )
        elif command_type == CancelOrder.__name__:
            return CancelOrder(
                self.identifier_cache.get_trader_id(unpacked[TRADER_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                self.identifier_cache.get_instrument_id(unpacked[INSTRUMENT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                command_id,
                timestamp_ns,
            )
        else:
            raise RuntimeError("Cannot deserialize command: unrecognized bytes pattern")


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an `Event` serializer for the `MessagePack` specification.

    """

    def __init__(self):
        """
        Initialize a new instance of the ``MsgPackEventSerializer`` class.

        """
        super().__init__()

        self.identifier_cache = IdentifierCache()

    cpdef bytes serialize(self, Event event):
        """
        Return the MessagePack specification bytes serialized from the given event.

        Parameters
        ----------
        event : Event
            The event to serialize.

        Returns
        -------
        bytes

        Raises
        ------
        RuntimeError
            If the event cannot be serialized.

        """
        Condition.not_none(event, "event")

        cdef dict package = {
            TYPE: type(event).__name__,
            ID: event.id.value,
            TIMESTAMP: str(event.timestamp_ns),
        }

        if isinstance(event, AccountState):
            package[ACCOUNT_ID] = event.account_id.value
            package[ACCOUNT_TYPE] = AccountTypeParser.to_str(event.account_type)
            package[BASE_CURRENCY] = event.base_currency.code if event.base_currency is not None else None
            package[IS_REPORTED] = event.is_reported
            package[BALANCES] = [[b.currency.code, str(b.total), str(b.locked), str(b.free)] for b in event.balances]
            package[TS_UPDATED] = str(event.ts_updated_ns)
            package[INFO] = event.info
        elif isinstance(event, OrderInitialized):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[STRATEGY_ID] = event.strategy_id.value
            package[INSTRUMENT_ID] = event.instrument_id.value
            package[ORDER_SIDE] = convert_snake_to_camel(OrderSideParser.to_str(event.order_side))
            package[ORDER_TYPE] = convert_snake_to_camel(OrderTypeParser.to_str(event.order_type))
            package[QUANTITY] = str(event.quantity)
            package[TIME_IN_FORCE] = TimeInForceParser.to_str(event.time_in_force)

            if event.order_type == OrderType.LIMIT:
                package[PRICE] = str(event.options[PRICE])
                package[EXPIRE_TIME] = maybe_dt_to_unix_nanos(event.options.get(EXPIRE_TIME))
                package[POST_ONLY] = event.options[POST_ONLY]
                package[REDUCE_ONLY] = event.options[REDUCE_ONLY]
                package[HIDDEN] = event.options[HIDDEN]
            elif event.order_type == OrderType.STOP_MARKET:
                package[PRICE] = str(event.options[PRICE])
                package[EXPIRE_TIME] = maybe_dt_to_unix_nanos(event.options.get(EXPIRE_TIME))
                package[REDUCE_ONLY] = event.options[REDUCE_ONLY]
            elif event.order_type == OrderType.STOP_LIMIT:
                package[PRICE] = str(event.options[PRICE])
                package[TRIGGER] = str(event.options[TRIGGER])
                package[EXPIRE_TIME] = maybe_dt_to_unix_nanos(event.options.get(EXPIRE_TIME))
                package[POST_ONLY] = event.options[POST_ONLY]
                package[REDUCE_ONLY] = event.options[REDUCE_ONLY]
                package[HIDDEN] = event.options[HIDDEN]
        elif isinstance(event, OrderInvalid):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[REASON] = event.reason
        elif isinstance(event, OrderDenied):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[REASON] = event.reason
        elif isinstance(event, OrderSubmitted):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_SUBMITTED] = str(event.ts_submitted_ns)
        elif isinstance(event, OrderRejected):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_REJECTED] = str(event.ts_rejected_ns)
            package[REASON] = event.reason
        elif isinstance(event, OrderAccepted):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[TS_ACCEPTED] = str(event.ts_accepted_ns)
        elif isinstance(event, OrderPendingReplace):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_PENDING] = str(event.ts_pending_ns)
        elif isinstance(event, OrderPendingCancel):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_PENDING] = str(event.ts_pending_ns)
        elif isinstance(event, OrderUpdateRejected):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_REJECTED] = str(event.ts_rejected_ns)
            package[RESPONSE_TO] = event.response_to
            package[REASON] = event.reason
        elif isinstance(event, OrderCancelRejected):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_REJECTED] = str(event.ts_rejected_ns)
            package[RESPONSE_TO] = event.response_to
            package[REASON] = event.reason
        elif isinstance(event, OrderUpdated):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[TS_UPDATED] = str(event.ts_updated_ns)
            package[QUANTITY] = str(event.quantity)
            package[PRICE] = str(event.price)
        elif isinstance(event, OrderCanceled):
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[ACCOUNT_ID] = event.account_id.value
            package[TS_CANCELED] = str(event.ts_canceled_ns)
        elif isinstance(event, OrderTriggered):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[TS_TRIGGERED] = str(event.ts_triggered_ns)
        elif isinstance(event, OrderExpired):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[TS_EXPIRED] = str(event.ts_expired_ns)
        elif isinstance(event, OrderFilled):
            package[ACCOUNT_ID] = event.account_id.value
            package[CLIENT_ORDER_ID] = event.client_order_id.value
            package[VENUE_ORDER_ID] = event.venue_order_id.value
            package[EXECUTION_ID] = event.execution_id.value
            package[POSITION_ID] = event.position_id.value
            package[STRATEGY_ID] = event.strategy_id.value
            package[INSTRUMENT_ID] = event.instrument_id.value
            package[ORDER_SIDE] = convert_snake_to_camel(OrderSideParser.to_str(event.order_side))
            package[LAST_QTY] = str(event.last_qty)
            package[LAST_PX] = str(event.last_px)
            package[CURRENCY] = event.currency.code
            package[COMMISSION] = event.commission.to_str().replace(',', '')
            package[LIQUIDITY_SIDE] = LiquiditySideParser.to_str(event.liquidity_side)
            package[TS_FILLED] = str(event.ts_filled_ns)
        else:
            raise RuntimeError(f"Cannot serialize event: unrecognized event {event}")

        return MsgPackSerializer.serialize(package)

    cpdef Event deserialize(self, bytes event_bytes):
        """
        Return the event deserialized from the given MessagePack specification event_bytes.

        Parameters
        ----------
        event_bytes
            The bytes to deserialize.

        Returns
        -------
        Event

        Raises
        ------
        ValueError
            If event_bytes is empty.
        RuntimeError
            If event cannot be deserialized.

        """
        Condition.not_empty(event_bytes, "event_bytes")

        cdef dict unpacked = MsgPackSerializer.deserialize(event_bytes)  # type: dict[str, bytes]

        cdef str event_type = unpacked[TYPE]
        cdef UUID event_id = UUID.from_str_c(unpacked[ID])
        cdef int64_t timestamp_ns = int(unpacked[TIMESTAMP])

        cdef dict options          # typing for OrderInitialized
        cdef OrderType order_type  # typing for OrderInitialized
        if event_type == AccountState.__name__:
            base_currency = unpacked[BASE_CURRENCY]
            return AccountState(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                AccountTypeParser.from_str(unpacked[ACCOUNT_TYPE]),
                Currency.from_str_c(base_currency) if base_currency is not None else None,
                unpacked[IS_REPORTED],
                [AccountBalance(
                    currency=Currency.from_str_c(b[0]),
                    total=Money(b[1], Currency.from_str_c(b[0])),
                    locked=Money(b[2], Currency.from_str_c(b[0])),
                    free=Money(b[3], Currency.from_str_c(b[0])),
                )
                    for b in unpacked[BALANCES]],
                unpacked[INFO],
                event_id,
                int(unpacked[TS_UPDATED]),
                timestamp_ns,
            )
        elif event_type == OrderInitialized.__name__:
            options = {}
            order_type = OrderTypeParser.from_str(convert_camel_to_snake(unpacked[ORDER_TYPE]))
            if order_type == OrderType.LIMIT:
                options[PRICE] = unpacked[PRICE]
                options[EXPIRE_TIME] = maybe_nanos_to_unix_dt(unpacked.get(EXPIRE_TIME))
                options[POST_ONLY] = unpacked[POST_ONLY]
                options[REDUCE_ONLY] = unpacked[REDUCE_ONLY]
                options[HIDDEN] = unpacked[HIDDEN]
            elif order_type == OrderType.STOP_MARKET:
                options[PRICE] = unpacked[PRICE]
                options[EXPIRE_TIME] = maybe_nanos_to_unix_dt(unpacked.get(EXPIRE_TIME))
                options[REDUCE_ONLY] = unpacked[REDUCE_ONLY]
            elif order_type == OrderType.STOP_LIMIT:
                options[PRICE] = unpacked[PRICE]
                options[TRIGGER] = unpacked[TRIGGER]
                options[EXPIRE_TIME] = maybe_nanos_to_unix_dt(unpacked.get(EXPIRE_TIME))
                options[POST_ONLY] = unpacked[POST_ONLY]
                options[REDUCE_ONLY] = unpacked[REDUCE_ONLY]
                options[HIDDEN] = unpacked[HIDDEN]

            return OrderInitialized(
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                self.identifier_cache.get_instrument_id(unpacked[INSTRUMENT_ID]),
                OrderSideParser.from_str(convert_camel_to_snake(unpacked[ORDER_SIDE])),
                order_type,
                Quantity.from_str_c(unpacked[QUANTITY]),
                TimeInForceParser.from_str(unpacked[TIME_IN_FORCE]),
                event_id,
                timestamp_ns,
                options,
            )
        elif event_type == OrderInvalid.__name__:
            return OrderInvalid(
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                unpacked[REASON],
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderDenied.__name__:
            return OrderDenied(
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                unpacked[REASON],
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderSubmitted.__name__:
            return OrderSubmitted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                int(unpacked[TS_SUBMITTED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderRejected.__name__:
            return OrderRejected(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                unpacked[REASON],
                int(unpacked[TS_REJECTED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderAccepted.__name__:
            return OrderAccepted(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                int(unpacked[TS_ACCEPTED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderPendingReplace.__name__:
            return OrderPendingReplace(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                int(unpacked[TS_PENDING]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderPendingCancel.__name__:
            return OrderPendingCancel(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                int(unpacked[TS_PENDING]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderUpdateRejected.__name__:
            return OrderUpdateRejected(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                unpacked[RESPONSE_TO],
                unpacked[REASON],
                int(unpacked[TS_REJECTED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderCancelRejected.__name__:
            return OrderCancelRejected(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                unpacked[RESPONSE_TO],
                unpacked[REASON],
                int(unpacked[TS_REJECTED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderUpdated.__name__:
            return OrderUpdated(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                Quantity.from_str_c(unpacked[QUANTITY]),
                Price.from_str_c(unpacked[PRICE]),
                int(unpacked[TS_UPDATED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderCanceled.__name__:
            return OrderCanceled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                int(unpacked[TS_CANCELED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderTriggered.__name__:
            return OrderExpired(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                int(unpacked[TS_TRIGGERED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderExpired.__name__:
            return OrderExpired(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                int(unpacked[TS_EXPIRED]),
                event_id,
                timestamp_ns,
            )
        elif event_type == OrderFilled.__name__:
            return OrderFilled(
                self.identifier_cache.get_account_id(unpacked[ACCOUNT_ID]),
                ClientOrderId(unpacked[CLIENT_ORDER_ID]),
                VenueOrderId(unpacked[VENUE_ORDER_ID]),
                ExecutionId(unpacked[EXECUTION_ID]),
                PositionId(unpacked[POSITION_ID]),
                self.identifier_cache.get_strategy_id(unpacked[STRATEGY_ID]),
                self.identifier_cache.get_instrument_id(unpacked[INSTRUMENT_ID]),
                OrderSideParser.from_str(convert_camel_to_snake(unpacked[ORDER_SIDE])),
                Quantity.from_str_c(unpacked[LAST_QTY]),
                Price.from_str_c(unpacked[LAST_PX]),
                Currency.from_str_c(unpacked[CURRENCY]),
                Money.from_str_c(unpacked[COMMISSION]),
                LiquiditySideParser.from_str(unpacked[LIQUIDITY_SIDE]),
                int(unpacked[TS_FILLED]),
                event_id,
                timestamp_ns,
            )
        else:
            raise RuntimeError(f"Cannot deserialize event: unrecognized event {event_type}")
