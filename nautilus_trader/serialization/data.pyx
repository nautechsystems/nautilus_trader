# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from typing import List

from bson import BSON
from bson.raw_bson import RawBSONDocument

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal, Quantity, Price, Tick, Bar, Instrument, ForexInstrument
from nautilus_trader.model.c_enums.currency cimport currency_to_string, currency_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType, security_type_to_string, security_type_from_string
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport InstrumentSerializer
from nautilus_trader.serialization.common cimport convert_datetime_to_string, convert_string_to_datetime


cdef class Utf8TickSerializer:
    """
    Provides a tick serializer for the UTF-8 specification.
    """
    @staticmethod
    cdef bytes serialize(Tick tick):
        """
        Serialize the given tick to UTF-8 specification bytes.

        :param tick: The tick to serialize.
        :return bytes.
        """
        Condition.not_none(tick, 'tick')

        return tick.to_string().encode(UTF8)

    @staticmethod
    cdef list serialize_ticks_list(list ticks):
        """
        Serialize the given tick to UTF-8 specification bytes.

        :param ticks: The ticks to serialize.
        :return bytes.
        """
        Condition.not_none(ticks, 'ticks')

        return [tick.to_string().encode(UTF8) for tick in ticks]

    @staticmethod
    cdef Tick deserialize(Symbol symbol, bytes tick_bytes):
        """
        Deserialize the given tick bytes to a tick.

        :param symbol: The symbol for the tick.
        :param tick_bytes: The tick bytes to deserialize.
        :return Tick.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.not_none(tick_bytes, 'tick_bytes')

        return Tick.from_string_with_symbol(symbol, tick_bytes.decode(UTF8))

    @staticmethod
    cdef list deserialize_bytes_list(Symbol symbol, list tick_values):
        """
        Deserialize the given bar bytes to a bar.

        :param symbol: The symbol for the tick.
        :param tick_values: The tick values to deserialize.
        :return Bar.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.not_none(tick_values, 'tick_values')

        return [Tick.from_string_with_symbol(symbol, values.decode(UTF8)) for values in tick_values]

    @staticmethod
    def py_serialize(Tick tick) -> bytes:
        return Utf8TickSerializer.serialize(tick)

    @staticmethod
    def py_serialize_ticks_list(list ticks) -> List[bytes]:
        return Utf8TickSerializer.serialize_ticks_list(ticks)

    @staticmethod
    def py_deserialize(Symbol symbol, bytes values_bytes) -> Tick:
        return Utf8TickSerializer.deserialize(symbol, values_bytes)

    @staticmethod
    def py_deserialize_bytes_list(Symbol symbol, list tick_values) -> List[Tick]:
        return Utf8TickSerializer.deserialize_bytes_list(symbol, tick_values)


cdef class Utf8BarSerializer:
    """
    Provides a bar serializer for the UTF-8 specification.
    """
    @staticmethod
    cdef bytes serialize(Bar bar):
        """
        Serialize the given bar to UTF-8 specification bytes.

        :param bar: The bar to serialize.
        :return bytes.
        """
        Condition.not_none(bar, 'bar')

        return bar.to_string().encode(UTF8)

    @staticmethod
    cdef list serialize_bars_list(list bars):
        """
        Serialize the given bar to UTF-8 specification bytes.

        :param bars: The bars to serialize.
        :return bytes.
        """
        Condition.not_none(bars, 'bars')

        return [bar.to_string().encode(UTF8) for bar in bars]

    @staticmethod
    cdef Bar deserialize(bytes bar_bytes):
        """
        Deserialize the given bar bytes to a bar.

        :param bar_bytes: The bar bytes to deserialize.
        :return Bar.
        """
        Condition.not_none(bar_bytes, 'bar_bytes')

        return Bar.from_string(bar_bytes.decode(UTF8))

    @staticmethod
    cdef list deserialize_bytes_list(list bar_values):
        """
        Deserialize the given bar bytes to a bar.

        :param bar_values: The bar values to deserialize.
        :return Bar.
        """
        Condition.not_none(bar_values, 'bar_values')

        return [Bar.from_string(values.decode(UTF8)) for values in bar_values]

    @staticmethod
    def py_serialize(Bar bar) -> bytes:
        return Utf8BarSerializer.serialize(bar)

    @staticmethod
    def py_serialize_bars_list(list bars) -> List[bytes]:
        return Utf8BarSerializer.serialize_bars_list(bars)

    @staticmethod
    def py_deserialize(bytes bar_bytes) -> Bar:
        return Utf8BarSerializer.deserialize(bar_bytes)

    @staticmethod
    def py_deserialize_bytes_list(list bar_values) -> List[Bar]:
        return Utf8BarSerializer.deserialize_bytes_list(bar_values)


cdef class BsonSerializer:
    """
    Provides a serializer for the BSON specification.
    """
    @staticmethod
    cdef bytes serialize(dict data):
        """
        Serialize the given data to BSON specification bytes.

        :param data: The data to serialize.
        :return bytes.
        """
        Condition.not_none(data, 'data')

        return bytes(RawBSONDocument(BSON.encode(data)).raw)

    @staticmethod
    cdef dict deserialize(bytes data_bytes):
        """
        Deserialize the given BSON specification bytes to a data object.

        :param data_bytes: The data bytes to deserialize.
        :return Dict.
        """
        Condition.not_none(data_bytes, 'data_bytes')

        return BSON.decode(data_bytes)


cdef class BsonDataSerializer(DataSerializer):
    """
    Provides a serializer for data objects to BSON specification.
    """
    cpdef bytes serialize(self, dict data):
        """
        Serialize the given data mapping to bytes.

        :param data: The data to serialize.
        :return bytes.
        """
        Condition.not_none(data, 'data')

        return BsonSerializer.serialize(data)

    cpdef dict deserialize(self, bytes data_bytes):
        """
        Deserialize the given bytes to a mapping of data.

        :param data_bytes: The data bytes to deserialize.
        :return Dict.
        """
        Condition.not_none(data_bytes, 'data_bytes')

        return BsonSerializer.deserialize(data_bytes)


cdef class BsonInstrumentSerializer(InstrumentSerializer):
    """
    Provides an instrument serializer for the MessagePack specification.
    """

    cpdef bytes serialize(self, Instrument instrument):
        """
        Return the MessagePack specification bytes serialized from the given instrument.

        :param instrument: The instrument to serialize.
        :return bytes.
        """
        Condition.not_none(instrument, 'instrument')

        cdef dict bson_map = {
            SYMBOL: instrument.symbol.value,
            BROKER_SYMBOL: instrument.broker_symbol,
            QUOTE_CURRENCY: currency_to_string(instrument.quote_currency),
            SECURITY_TYPE: security_type_to_string(instrument.security_type),
            PRICE_PRECISION: instrument.price_precision,
            SIZE_PRECISION: instrument.size_precision,
            MIN_STOP_DISTANCE_ENTRY: instrument.min_stop_distance_entry,
            MIN_STOP_DISTANCE: instrument.min_stop_distance,
            MIN_LIMIT_DISTANCE_ENTRY: instrument.min_limit_distance_entry,
            MIN_LIMIT_DISTANCE: instrument.min_limit_distance,
            TICK_SIZE: instrument.tick_size.to_string(),
            ROUND_LOT_SIZE: instrument.round_lot_size.to_string(),
            MIN_TRADE_SIZE: instrument.min_trade_size.to_string(),
            MAX_TRADE_SIZE: instrument.max_trade_size.to_string(),
            ROLL_OVER_INTEREST_BUY: instrument.rollover_interest_buy.to_string(),
            ROLL_OVER_INTEREST_SELL: instrument.rollover_interest_sell.to_string(),
            TIMESTAMP: convert_datetime_to_string(instrument.timestamp),
        }

        if isinstance(instrument, ForexInstrument):
            bson_map[BASE_CURRENCY] = currency_to_string(instrument.base_currency)

        return BsonSerializer.serialize(bson_map)

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Return the instrument deserialized from the given MessagePack specification bytes.

        :param instrument_bytes: The bytes to deserialize.
        :return Instrument.
        """
        Condition.not_none(instrument_bytes, 'instrument_bytes')

        cdef dict deserialized = BsonSerializer.deserialize(instrument_bytes)

        cdef SecurityType security_type = security_type_from_string(deserialized[SECURITY_TYPE].upper())
        if security_type == SecurityType.FOREX:
            return ForexInstrument(
                symbol=Symbol.from_string(deserialized[SYMBOL]),
                broker_symbol=deserialized[BROKER_SYMBOL],
                price_precision=deserialized[PRICE_PRECISION],
                size_precision=deserialized[SIZE_PRECISION],
                min_stop_distance_entry=deserialized[MIN_STOP_DISTANCE_ENTRY],
                min_stop_distance=deserialized[MIN_STOP_DISTANCE],
                min_limit_distance_entry=deserialized[MIN_LIMIT_DISTANCE_ENTRY],
                min_limit_distance=deserialized[MIN_LIMIT_DISTANCE],
                tick_size=Price.from_string(deserialized[TICK_SIZE]),
                round_lot_size=Quantity.from_string(deserialized[ROUND_LOT_SIZE]),
                min_trade_size=Quantity.from_string(deserialized[MIN_TRADE_SIZE]),
                max_trade_size=Quantity.from_string(deserialized[MAX_TRADE_SIZE]),
                rollover_interest_buy=Decimal.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_BUY]),
                rollover_interest_sell=Decimal.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_SELL]),
                timestamp=convert_string_to_datetime(deserialized[TIMESTAMP]))

        return Instrument(
            symbol=Symbol.from_string(deserialized[SYMBOL]),
            broker_symbol=deserialized[BROKER_SYMBOL],
            quote_currency=currency_from_string(deserialized[QUOTE_CURRENCY]),
            security_type=security_type,
            price_precision=deserialized[PRICE_PRECISION],
            size_precision=deserialized[SIZE_PRECISION],
            min_stop_distance_entry=deserialized[MIN_STOP_DISTANCE_ENTRY],
            min_stop_distance=deserialized[MIN_STOP_DISTANCE],
            min_limit_distance_entry=deserialized[MIN_LIMIT_DISTANCE_ENTRY],
            min_limit_distance=deserialized[MIN_LIMIT_DISTANCE],
            tick_size=Price.from_string(deserialized[TICK_SIZE]),
            round_lot_size=Quantity.from_string(deserialized[ROUND_LOT_SIZE]),
            min_trade_size=Quantity.from_string(deserialized[MIN_TRADE_SIZE]),
            max_trade_size=Quantity.from_string(deserialized[MAX_TRADE_SIZE]),
            rollover_interest_buy=Decimal.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_BUY]),
            rollover_interest_sell=Decimal.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_SELL]),
            timestamp=convert_string_to_datetime(deserialized[TIMESTAMP]))


cdef class DataMapper:
    """
    Provides a data mapper for data objects.
    """

    def __init__(self):
        """
        Initializes a new instance of the DataMapper class.
        """
        self.instrument_serializer = BsonInstrumentSerializer()

    cpdef dict map_ticks(self, list ticks):
        Condition.not_empty(ticks, 'ticks')
        Condition.type(ticks[0], Tick, 'ticks')

        return {
            DATA: Utf8TickSerializer.serialize_ticks_list(ticks),
            DATA_TYPE: 'Tick[]',
            METADATA: { SYMBOL: ticks[0].symbol.value },
        }

    cpdef dict map_bars(self, list bars, BarType bar_type):
        Condition.not_empty(bars, 'bars')
        Condition.not_none(bar_type, 'bar_type')
        Condition.type(bars[0], Bar, 'bars')

        return {
            DATA: Utf8BarSerializer.serialize_bars_list(bars),
            DATA_TYPE: 'Bar[]',
            METADATA: { SYMBOL: bar_type.symbol.value,
                        SPECIFICATION: bar_type.specification.to_string()},
        }

    cpdef dict map_instruments(self, list instruments):
        Condition.not_empty(instruments, 'instruments')
        Condition.type(instruments[0], Instrument, 'instruments')

        return {
            DATA: [self.instrument_serializer.serialize(instrument) for instrument in instruments],
            DATA_TYPE: 'Instrument[]',
        }
