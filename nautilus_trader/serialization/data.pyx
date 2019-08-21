# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import iso8601

from bson import BSON
from bson.raw_bson import RawBSONDocument
from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Quantity, Price, Tick, Bar, Instrument
from nautilus_trader.model.enums import Currency, SecurityType
from nautilus_trader.model.c_enums.currency cimport Currency, currency_to_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType, security_type_to_string
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.base cimport InstrumentSerializer
from nautilus_trader.serialization.common cimport convert_datetime_to_string, convert_string_to_datetime


cpdef Tick deserialize_tick(Symbol symbol, bytes tick_bytes):
    """
    Return a deserialized tick from the given symbol and bytes.

    :param symbol: The ticks symbol.
    :param tick_bytes: The tick bytes to deserialize.
    :return: Tick.
    """
    cdef list values = tick_bytes.decode(UTF8).split(',')

    return Tick(
        symbol,
        Price(values[0]),
        Price(values[1]),
        iso8601.parse_date(values[2]))

cpdef Bar deserialize_bar(bytes bar_bytes):
    """
    Return the deserialized bar from the give bytes.
    
    :param bar_bytes: The bar bytes to deserialize.
    :return: Bar.
    """
    cdef list values = bar_bytes.decode(UTF8).split(',')

    return Bar(
        Price(values[0]),
        Price(values[1]),
        Price(values[2]),
        Price(values[3]),
        Quantity(values[4]),
        iso8601.parse_date(values[5]))

cpdef list deserialize_bars(bytes[:] bar_bytes_array):
    """
    Return a list of deserialized bars from the given bars bytes.
    
    :param bar_bytes_array: The bar bytes to deserialize.
    :return: List[Bar].
    """
    cdef list bars = []
    cdef int i
    cdef int array_length = len(bar_bytes_array)
    for i in range(array_length):
        bars.append(deserialize_bar(bar_bytes_array[i]))

    return bars


cdef class BsonSerializer:
    """
    Provides a serializer for the BSON specification.
    """
    @staticmethod
    cdef bytes serialize(dict data):
        """
        Serialize the given data to bytes.

        :param: data: The data to serialize.
        :return: bytes.
        """
        return bytes(RawBSONDocument(BSON.encode(data)).raw)

    @staticmethod
    cdef dict deserialize(bytes data_bytes):
        """
        Deserialize the given bytes to a data object.

        :param: data_bytes: The data bytes to deserialize.
        :return: Dict.
        """
        return BSON.decode(data_bytes)


cdef class BsonDataSerializer(DataSerializer):
    """
    Provides a serializer for data objects to BSON specification.
    """
    cpdef bytes serialize(self, dict data):
        """
        Serialize the given data mapping to bytes.

        :param: data: The data to serialize.
        :return: bytes.
        """
        return BsonSerializer.serialize(data)

    cpdef dict deserialize(self, bytes data_bytes):
        """
        Deserialize the given bytes to a mapping of data.

        :param: data_bytes: The data bytes to deserialize.
        :return: Dict.
        """
        return BsonSerializer.deserialize(data_bytes)


cdef class BsonInstrumentSerializer(InstrumentSerializer):
    """
    Provides an instrument serializer for the MessagePack specification.
    """

    cpdef bytes serialize(self, Instrument instrument):
        """
        Return the MessagePack specification bytes serialized from the given instrument.

        :param instrument: The instrument to serialize.
        :return: bytes.
        """
        return BsonSerializer.serialize({
            ID: instrument.id.value,
            SYMBOL: instrument.symbol.value,
            BROKER_SYMBOL: instrument.broker_symbol,
            QUOTE_CURRENCY: currency_to_string(instrument.quote_currency),
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
        :return: Instrument.
        """
        cdef dict deserialized = BsonSerializer.deserialize(instrument_bytes)

        return Instrument(
            instrument_id=InstrumentId(deserialized[ID]),
            symbol=Symbol.from_string(deserialized[SYMBOL]),
            broker_symbol=deserialized[BROKER_SYMBOL],
            quote_currency=Currency[(deserialized[QUOTE_CURRENCY])],
            security_type=SecurityType[(deserialized[SECURITY_TYPE])],
            tick_precision=deserialized[TICK_PRECISION],
            tick_size=Decimal(str(deserialized[TICK_SIZE])),
            round_lot_size=Quantity(deserialized[ROUND_LOT_SIZE]),
            min_stop_distance_entry=deserialized[MIN_STOP_DISTANCE_ENTRY],
            min_stop_distance=deserialized[MIN_STOP_DISTANCE],
            min_limit_distance_entry=deserialized[MIN_LIMIT_DISTANCE_ENTRY],
            min_limit_distance=deserialized[MIN_LIMIT_DISTANCE],
            min_trade_size=Quantity(deserialized[MIN_TRADE_SIZE]),
            max_trade_size=Quantity(deserialized[MAX_TRADE_SIZE]),
            rollover_interest_buy=Decimal(str(deserialized[ROLL_OVER_INTEREST_BUY])),
            rollover_interest_sell=Decimal(str(deserialized[ROLL_OVER_INTEREST_SELL])),
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
            DATA: [str(tick) for tick in ticks]
        }

    cpdef dict map_bars(self, list bars, BarType bar_type):
        Condition.not_empty(bars, 'bars')
        Condition.type(bars[0], Bar, 'bars')

        return {
            DATA_TYPE: type(bars[0]).__name__,
            SYMBOL: bar_type.symbol.value,
            SPECIFICATION: str(bar_type.specification),
            DATA: [str(bar) for bar in bars]
        }

    cpdef dict map_instruments(self, list instruments):
        Condition.not_empty(instruments, 'instruments')
        Condition.type(instruments[0], Instrument, 'instruments')

        return {
            DATA_TYPE: type(instruments[0]).__name__,
            DATA: [self.instrument_serializer.serialize(instrument) for instrument in instruments]
        }
