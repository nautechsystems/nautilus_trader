# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from bson import BSON
from bson.raw_bson import RawBSONDocument

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.c_enums.currency cimport currency_from_string
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.security_type cimport security_type_from_string
from nautilus_trader.model.c_enums.security_type cimport security_type_to_string
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport ForexInstrument
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Decimal64
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.serialization.base cimport InstrumentSerializer
from nautilus_trader.serialization.common cimport convert_datetime_to_string
from nautilus_trader.serialization.common cimport convert_string_to_datetime
from nautilus_trader.serialization.constants cimport *


cdef class Utf8QuoteTickSerializer:
    """
    Provides a quote tick serializer for the UTF-8 specification.
    """

    @staticmethod
    cdef bytes serialize(QuoteTick tick):
        """
        Serialize the given quote tick to UTF-8 specification bytes.

        :param tick: The quote tick to serialize.
        :return bytes.
        """
        Condition.not_none(tick, "tick")

        return tick.to_serializable_string().encode(UTF8)

    @staticmethod
    cdef list serialize_ticks_list(list ticks):
        """
        Serialize the given quote ticks to a list of UTF-8 specification bytes.

        :param ticks: The quote ticks to serialize.
        :return bytes.
        """
        Condition.not_none(ticks, "ticks")

        cdef QuoteTick tick
        return [tick.to_serializable_string().encode(UTF8) for tick in ticks]

    @staticmethod
    cdef QuoteTick deserialize(Symbol symbol, bytes tick_bytes):
        """
        Deserialize the given quote tick bytes to a tick.

        :param symbol: The quote tick symbol.
        :param tick_bytes: The quote tick bytes to deserialize.
        :return QuoteTick.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(tick_bytes, "tick_bytes")

        return QuoteTick.from_serializable_string(symbol, tick_bytes.decode(UTF8))

    @staticmethod
    cdef list deserialize_bytes_list(Symbol symbol, list tick_values):
        """
        Deserialize the given inputs to a list of quote ticks.

        :param symbol: The quote tick symbol.
        :param tick_values: The quote tick values to deserialize.
        :return List[QuoteTick].
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(tick_values, "tick_values")

        return [QuoteTick.from_serializable_string(symbol, values.decode(UTF8)) for values in tick_values]

    @staticmethod
    def py_serialize(QuoteTick tick) -> bytes:
        return Utf8QuoteTickSerializer.serialize(tick)

    @staticmethod
    def py_serialize_ticks_list(list ticks) -> [bytes]:
        return Utf8QuoteTickSerializer.serialize_ticks_list(ticks)

    @staticmethod
    def py_deserialize(Symbol symbol, bytes values_bytes) -> QuoteTick:
        return Utf8QuoteTickSerializer.deserialize(symbol, values_bytes)

    @staticmethod
    def py_deserialize_bytes_list(Symbol symbol, list tick_values) -> [QuoteTick]:
        return Utf8QuoteTickSerializer.deserialize_bytes_list(symbol, tick_values)


cdef class Utf8TradeTickSerializer:
    """
    Provides a trade tick serializer for the UTF-8 specification.
    """

    @staticmethod
    cdef bytes serialize(TradeTick tick):
        """
        Serialize the given trade tick to UTF-8 specification bytes.

        :param tick: The trade tick to serialize.
        :return bytes.
        """
        Condition.not_none(tick, "tick")

        return tick.to_serializable_string().encode(UTF8)

    @staticmethod
    cdef list serialize_ticks_list(list ticks):
        """
        Serialize the given trade ticks to a list of UTF-8 specification bytes.

        :param ticks: The trade ticks to serialize.
        :return bytes.
        """
        Condition.not_none(ticks, "ticks")

        cdef TradeTick tick
        return [tick.to_serializable_string().encode(UTF8) for tick in ticks]

    @staticmethod
    cdef TradeTick deserialize(Symbol symbol, bytes tick_bytes):
        """
        Deserialize the given trade tick bytes to a tick.

        :param symbol: The trade tick symbol.
        :param tick_bytes: The trade tick bytes to deserialize.
        :return TradeTick.
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(tick_bytes, "tick_bytes")

        return TradeTick.from_serializable_string(symbol, tick_bytes.decode(UTF8))

    @staticmethod
    cdef list deserialize_bytes_list(Symbol symbol, list tick_values):
        """
        Deserialize the given inputs to a list of trade ticks.

        :param symbol: The trade tick symbol.
        :param tick_values: The trade tick values to deserialize.
        :return List[TradeTick].
        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(tick_values, "tick_values")

        return [TradeTick.from_serializable_string(symbol, values.decode(UTF8)) for values in tick_values]

    @staticmethod
    def py_serialize(TradeTick tick) -> bytes:
        return Utf8TradeTickSerializer.serialize(tick)

    @staticmethod
    def py_serialize_ticks_list(list ticks) -> [bytes]:
        return Utf8TradeTickSerializer.serialize_ticks_list(ticks)

    @staticmethod
    def py_deserialize(Symbol symbol, bytes values_bytes) -> QuoteTick:
        return Utf8TradeTickSerializer.deserialize(symbol, values_bytes)

    @staticmethod
    def py_deserialize_bytes_list(Symbol symbol, list tick_values) -> [QuoteTick]:
        return Utf8TradeTickSerializer.deserialize_bytes_list(symbol, tick_values)


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
        Condition.not_none(bar, "bar")

        return bar.to_serializable_string().encode(UTF8)

    @staticmethod
    cdef list serialize_bars_list(list bars):
        """
        Serialize the given bar to UTF-8 specification bytes.

        :param bars: The bars to serialize.
        :return bytes.
        """
        Condition.not_none(bars, "bars")

        cdef Bar bar
        return [bar.to_serializable_string().encode(UTF8) for bar in bars]

    @staticmethod
    cdef Bar deserialize(bytes bar_bytes):
        """
        Deserialize the given bar bytes to a bar.

        :param bar_bytes: The bar bytes to deserialize.
        :return Bar.
        """
        Condition.not_none(bar_bytes, "bar_bytes")

        return Bar.from_serializable_string(bar_bytes.decode(UTF8))

    @staticmethod
    cdef list deserialize_bytes_list(list bar_values):
        """
        Deserialize the given bar bytes to a bar.

        :param bar_values: The bar values to deserialize.
        :return Bar.
        """
        Condition.not_none(bar_values, "bar_values")

        return [Bar.from_serializable_string(values.decode(UTF8)) for values in bar_values]

    @staticmethod
    def py_serialize(Bar bar) -> bytes:
        return Utf8BarSerializer.serialize(bar)

    @staticmethod
    def py_serialize_bars_list(list bars) -> [bytes]:
        return Utf8BarSerializer.serialize_bars_list(bars)

    @staticmethod
    def py_deserialize(bytes bar_bytes) -> Bar:
        return Utf8BarSerializer.deserialize(bar_bytes)

    @staticmethod
    def py_deserialize_bytes_list(list bar_values) -> [Bar]:
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
        Condition.not_none(data, "data")

        return bytes(RawBSONDocument(BSON.encode(data)).raw)

    @staticmethod
    cdef dict deserialize(bytes data_bytes):
        """
        Deserialize the given BSON specification bytes to a data object.

        :param data_bytes: The data bytes to deserialize.
        :return Dict.
        """
        Condition.not_none(data_bytes, "data_bytes")

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
        Condition.not_none(data, "data")

        return BsonSerializer.serialize(data)

    cpdef dict deserialize(self, bytes data_bytes):
        """
        Deserialize the given bytes to a mapping of data.

        :param data_bytes: The data bytes to deserialize.
        :return Dict.
        """
        Condition.not_none(data_bytes, "data_bytes")

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
        Condition.not_none(instrument, "instrument")

        cdef dict bson_map = {
            SYMBOL: instrument.symbol.value,
            QUOTE_CURRENCY: currency_to_string(instrument.quote_currency),
            SECURITY_TYPE: security_type_to_string(instrument.security_type),
            PRICE_PRECISION: instrument.price_precision,
            SIZE_PRECISION: instrument.size_precision,
            TICK_SIZE: instrument.tick_size.to_string(),
            LOT_SIZE: instrument.lot_size.to_string(),
            MIN_TRADE_SIZE: instrument.min_trade_size.to_string(),
            MAX_TRADE_SIZE: instrument.max_trade_size.to_string(),
            ROLL_OVER_INTEREST_BUY: instrument.rollover_interest_buy.to_string(),
            ROLL_OVER_INTEREST_SELL: instrument.rollover_interest_sell.to_string(),
            TIMESTAMP: convert_datetime_to_string(instrument.timestamp),
        }

        if isinstance(instrument, ForexInstrument):
            bson_map[BASE_CURRENCY] = currency_to_string(instrument.base_currency)
            bson_map[MIN_STOP_DISTANCE_ENTRY] = instrument.min_stop_distance_entry
            bson_map[MIN_STOP_DISTANCE] = instrument.min_stop_distance
            bson_map[MIN_LIMIT_DISTANCE_ENTRY] = instrument.min_limit_distance_entry
            bson_map[MIN_LIMIT_DISTANCE] = instrument.min_limit_distance

        return BsonSerializer.serialize(bson_map)

    cpdef Instrument deserialize(self, bytes instrument_bytes):
        """
        Return the instrument deserialized from the given MessagePack specification bytes.

        :param instrument_bytes: The bytes to deserialize.
        :return Instrument.
        """
        Condition.not_none(instrument_bytes, "instrument_bytes")

        cdef dict deserialized = BsonSerializer.deserialize(instrument_bytes)

        cdef SecurityType security_type = security_type_from_string(deserialized[SECURITY_TYPE].upper())
        if security_type == SecurityType.FOREX:
            return ForexInstrument(
                symbol=Symbol.from_string(deserialized[SYMBOL]),
                price_precision=deserialized[PRICE_PRECISION],
                size_precision=deserialized[SIZE_PRECISION],
                min_stop_distance_entry=deserialized[MIN_STOP_DISTANCE_ENTRY],
                min_stop_distance=deserialized[MIN_STOP_DISTANCE],
                min_limit_distance_entry=deserialized[MIN_LIMIT_DISTANCE_ENTRY],
                min_limit_distance=deserialized[MIN_LIMIT_DISTANCE],
                tick_size=Price.from_string(deserialized[TICK_SIZE]),
                lot_size=Quantity.from_string(deserialized[LOT_SIZE]),
                min_trade_size=Quantity.from_string(deserialized[MIN_TRADE_SIZE]),
                max_trade_size=Quantity.from_string(deserialized[MAX_TRADE_SIZE]),
                rollover_interest_buy=Decimal64.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_BUY]),
                rollover_interest_sell=Decimal64.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_SELL]),
                timestamp=convert_string_to_datetime(deserialized[TIMESTAMP]),
            )

        return Instrument(
            symbol=Symbol.from_string(deserialized[SYMBOL]),
            quote_currency=currency_from_string(deserialized[QUOTE_CURRENCY]),
            security_type=security_type,
            price_precision=deserialized[PRICE_PRECISION],
            size_precision=deserialized[SIZE_PRECISION],
            tick_size=Price.from_string(deserialized[TICK_SIZE]),
            lot_size=Quantity.from_string(deserialized[LOT_SIZE]),
            min_trade_size=Quantity.from_string(deserialized[MIN_TRADE_SIZE]),
            max_trade_size=Quantity.from_string(deserialized[MAX_TRADE_SIZE]),
            rollover_interest_buy=Decimal64.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_BUY]),
            rollover_interest_sell=Decimal64.from_string_to_decimal(deserialized[ROLL_OVER_INTEREST_SELL]),
            timestamp=convert_string_to_datetime(deserialized[TIMESTAMP]),
        )


cdef class DataMapper:
    """
    Provides a data mapper for data objects.
    """

    def __init__(self):
        """
        Initialize a new instance of the DataMapper class.
        """
        self.instrument_serializer = BsonInstrumentSerializer()

    cpdef dict map_quote_ticks(self, list ticks):
        Condition.not_empty(ticks, "ticks")
        Condition.type(ticks[0], QuoteTick, "ticks")

        return {
            DATA: Utf8QuoteTickSerializer.serialize_ticks_list(ticks),
            DATA_TYPE: QUOTE_TICK_ARRAY,
            METADATA: {SYMBOL: ticks[0].symbol.value},
        }

    cpdef dict map_trade_ticks(self, list ticks):
        Condition.not_empty(ticks, "ticks")
        Condition.type(ticks[0], TradeTick, "ticks")

        return {
            DATA: Utf8TradeTickSerializer.serialize_ticks_list(ticks),
            DATA_TYPE: TRADE_TICK_ARRAY,
            METADATA: {
                SYMBOL: ticks[0].symbol.value,
            },
        }

    cpdef dict map_bars(self, list bars, BarType bar_type):
        Condition.not_empty(bars, "bars")
        Condition.not_none(bar_type, "bar_type")
        Condition.type(bars[0], Bar, "bars")

        return {
            DATA: Utf8BarSerializer.serialize_bars_list(bars),
            DATA_TYPE: BAR_ARRAY,
            METADATA: {
                SYMBOL: bar_type.symbol.value,
                SPECIFICATION: bar_type.spec.to_string(),
            },
        }

    cpdef dict map_instruments(self, list instruments):
        Condition.not_empty(instruments, "instruments")
        Condition.type(instruments[0], Instrument, "instruments")

        return {
            DATA: [self.instrument_serializer.serialize(instrument) for instrument in instruments],
            DATA_TYPE: INSTRUMENT_ARRAY,
        }
