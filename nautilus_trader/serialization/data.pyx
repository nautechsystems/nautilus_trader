# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from bson import BSON
from bson.raw_bson import RawBSONDocument

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Quantity, Decimal, Tick, Bar, Instrument
from nautilus_trader.model.c_enums.currency cimport currency_to_string, currency_from_string
from nautilus_trader.model.c_enums.security_type cimport security_type_to_string, security_type_from_string
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
    cdef Tick deserialize(Symbol symbol, bytes tick_bytes):
        """
        Deserialize the given tick bytes to a tick.

        :param symbol: The symbol to deserialize.
        :param tick_bytes: The tick bytes to deserialize.
        :return Tick.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.not_none(tick_bytes, 'tick_bytes')

        return Tick.from_string_with_symbol(symbol, tick_bytes.decode(UTF8))

    @staticmethod
    def py_serialize(Tick tick) -> bytes:
        return Utf8TickSerializer.serialize(tick)

    @staticmethod
    def py_deserialize(Symbol symbol, bytes values_bytes) -> Tick:
        return Utf8TickSerializer.deserialize(symbol, values_bytes)


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
    cdef Bar deserialize(bytes bar_bytes):
        """
        Deserialize the given bar bytes to a bar.

        :param bar_bytes: The bar bytes to deserialize.
        :return Bar.
        """
        Condition.not_none(bar_bytes, 'bar_bytes')

        return Bar.from_string(bar_bytes.decode(UTF8))

    @staticmethod
    cdef list deserialize_bars(bytes[:] bar_bytes_array):
        """
        Return a list of deserialized bars from the given bars bytes.
    
        :param bar_bytes_array: The bar bytes to deserialize.
        :return List[Bar].
        """
        Condition.not_none(bar_bytes_array, 'bar_bytes_array')

        cdef int i
        cdef int array_length = len(bar_bytes_array)
        cdef list bars = []
        for i in range(array_length):
            bars.append(Utf8BarSerializer.deserialize(bar_bytes_array[i]))
        return bars

    @staticmethod
    def py_serialize(Bar bar) -> bytes:
        return Utf8BarSerializer.serialize(bar)

    @staticmethod
    def py_deserialize(bytes bar_bytes) -> Bar:
        return Utf8BarSerializer.deserialize(bar_bytes)

    @staticmethod
    def py_deserialize_bars(bytearray bar_bytes_array):
        return Utf8BarSerializer.deserialize_bars(bar_bytes_array)


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

        return BsonSerializer.serialize({
            SYMBOL: instrument.symbol.value,
            BROKER_SYMBOL: instrument.broker_symbol,
            BASE_CURRENCY: currency_to_string(instrument.base_currency),
            SECURITY_TYPE: security_type_to_string(instrument.security_type),
            TICK_PRECISION: instrument.tick_precision,
            TICK_SIZE: str(instrument.tick_size),
            ROUND_LOT_SIZE: instrument.round_lot_size.value,
            MIN_STOP_DISTANCE_ENTRY: instrument.min_stop_distance_entry,
            MIN_STOP_DISTANCE: instrument.min_stop_distance,
            MIN_LIMIT_DISTANCE_ENTRY: instrument.min_limit_distance_entry,
            MIN_LIMIT_DISTANCE: instrument.min_limit_distance,
            MIN_TRADE_SIZE: instrument.min_trade_size.value,
            MAX_TRADE_SIZE: instrument.max_trade_size.value,
            ROLL_OVER_INTEREST_BUY: str(instrument.rollover_interest_buy),
            ROLL_OVER_INTEREST_SELL: str(instrument.rollover_interest_sell),
            TIMESTAMP: convert_datetime_to_string(instrument.timestamp),
        })

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Return the instrument deserialized from the given MessagePack specification bytes.

        :param instrument_bytes: The bytes to deserialize.
        :return Instrument.
        """
        Condition.not_none(instrument_bytes, 'instrument_bytes')

        cdef dict deserialized = BsonSerializer.deserialize(instrument_bytes)

        return Instrument(
            symbol=Symbol.from_string(deserialized[SYMBOL]),
            broker_symbol=deserialized[BROKER_SYMBOL],
            base_currency=currency_from_string(deserialized[BASE_CURRENCY]),
            security_type=security_type_from_string(deserialized[SECURITY_TYPE]),
            tick_precision=deserialized[TICK_PRECISION],
            tick_size=Decimal.from_string_to_decimal(str(deserialized[TICK_SIZE])),
            round_lot_size=Quantity(deserialized[ROUND_LOT_SIZE]),
            min_stop_distance_entry=deserialized[MIN_STOP_DISTANCE_ENTRY],
            min_stop_distance=deserialized[MIN_STOP_DISTANCE],
            min_limit_distance_entry=deserialized[MIN_LIMIT_DISTANCE_ENTRY],
            min_limit_distance=deserialized[MIN_LIMIT_DISTANCE],
            min_trade_size=Quantity(deserialized[MIN_TRADE_SIZE]),
            max_trade_size=Quantity(deserialized[MAX_TRADE_SIZE]),
            rollover_interest_buy=Decimal.from_string_to_decimal(str(deserialized[ROLL_OVER_INTEREST_BUY])),
            rollover_interest_sell=Decimal.from_string_to_decimal(str(deserialized[ROLL_OVER_INTEREST_SELL])),
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
            DATA_TYPE: type(ticks[0]).__name__,
            SYMBOL: ticks[0].symbol.value,
            DATA: [tick.to_string() for tick in ticks]
        }

    cpdef dict map_bars(self, list bars, BarType bar_type):
        Condition.not_empty(bars, 'bars')
        Condition.not_none(bar_type, 'bar_type')
        Condition.type(bars[0], Bar, 'bars')

        return {
            DATA_TYPE: type(bars[0]).__name__,
            SYMBOL: bar_type.symbol.value,
            SPECIFICATION: bar_type.specification.to_string(),
            DATA: [bar.to_string() for bar in bars]
        }

    cpdef dict map_instruments(self, list instruments):
        Condition.not_empty(instruments, 'instruments')
        Condition.type(instruments[0], Instrument, 'instruments')

        return {
            DATA_TYPE: type(instruments[0]).__name__,
            DATA: [self.instrument_serializer.serialize(instrument) for instrument in instruments]
        }
