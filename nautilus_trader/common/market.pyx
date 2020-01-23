# -------------------------------------------------------------------------------------------------
# <copyright file="market.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import inspect
import pandas as pd

from cpython.datetime cimport datetime, timedelta
from typing import List, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.functions cimport with_utc_index
from nautilus_trader.model.c_enums.price_type cimport PriceType, price_type_to_string
from nautilus_trader.model.objects cimport Price, Tick, Bar, DataBar, BarType, BarSpecification, Instrument
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure, bar_structure_to_string
from nautilus_trader.model.identifiers cimport Label
from nautilus_trader.common.clock cimport TimeEventHandler, Clock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.handlers cimport BarHandler


cdef class TickDataWrangler:
    """
    Provides a means of building lists of ticks from the given Pandas DataFrames
    of bid and ask data. Provided data can either be tick data or bar data.
    """

    def __init__(self,
                 Instrument instrument not None,
                 data_ticks: pd.DataFrame=None,
                 dict data_bars_bid=None,
                 dict data_bars_ask=None):
        """
        Initializes a new instance of the TickBuilder class.

        :param instrument: The instrument for the data wrangler.
        :param data_ticks: The optional pd.DataFrame containing the tick data.
        :param data_bars_bid: The optional dictionary containing the bars bid data.
        :param data_bars_ask: The optional dictionary containing the bars ask data.
        :raises: ValueError: If the tick_data is a type other than None or DataFrame.
        :raises: ValueError: If the bid_data is a type other than None or Dict.
        :raises: ValueError: If the ask_data is a type other than None or Dict.
        :raises: ValueError: If the tick_data is None and the bars data is None.
        """
        Condition.type_or_none(data_ticks, pd.DataFrame, 'tick_data')
        Condition.type_or_none(data_bars_bid, dict, 'bid_data')
        Condition.type_or_none(data_bars_ask, dict, 'ask_data')

        if data_ticks is not None and len(data_ticks) > 0:
            self._data_ticks = with_utc_index(data_ticks)
        else:
            Condition.true(data_bars_bid is not None, 'data_bars_bid is not None')
            Condition.true(data_bars_ask is not None, 'data_bars_ask is not None')
            self._data_bars_bid = data_bars_bid
            self._data_bars_ask = data_bars_ask

        self._symbol = instrument.symbol
        self._precision = instrument.tick_precision

        self.tick_data = []
        self.resolution = BarStructure.UNDEFINED

    cpdef void build(self, int symbol_indexer):
        """
        Return the built ticks from the held data.

        :return List[Tick].
        """
        if self._data_ticks is not None and len(self._data_ticks) > 0:
            # Build ticks from data
            self.tick_data = self._data_ticks
            self.tick_data['symbol'] = symbol_indexer

            if 'bid_size' not in self.tick_data.columns:
                self.tick_data['bid_size'] = 1.0

            if 'ask_size' not in self.tick_data.columns:
                self.tick_data['ask_size'] = 1.0

            self.resolution = BarStructure.TICK
            return

        # Build ticks from highest resolution bar data
        if BarStructure.SECOND in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarStructure.SECOND]
            bars_ask = self._data_bars_ask[BarStructure.SECOND]
            self.resolution = BarStructure.SECOND
        elif BarStructure.MINUTE in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarStructure.MINUTE]
            bars_ask = self._data_bars_ask[BarStructure.MINUTE]
            self.resolution = BarStructure.MINUTE
        elif BarStructure.HOUR in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarStructure.HOUR]
            bars_ask = self._data_bars_ask[BarStructure.HOUR]
            self.resolution = BarStructure.HOUR
        elif BarStructure.DAY in self._data_bars_bid:
            bars_bid = self._data_bars_bid[BarStructure.DAY]
            bars_ask = self._data_bars_ask[BarStructure.DAY]
            self.resolution = BarStructure.DAY

        Condition.not_none(bars_bid, 'bars_bid')
        Condition.not_none(bars_ask, 'bars_ask')
        Condition.true(len(bars_bid) > 0, 'len(bars_bid) > 0')
        Condition.true(len(bars_ask) > 0, 'len(bars_ask) > 0')
        Condition.true(all(bars_bid.index) == all(bars_ask.index), 'bars_bid.index == bars_ask.index')
        Condition.true(bars_bid.shape == bars_ask.shape, 'bars_bid.shape == bars_ask.shape')

        bars_bid = with_utc_index(bars_bid)
        bars_ask = with_utc_index(bars_ask)
        shifted_index = bars_bid.index.shift(periods=-100, freq='ms')

        cdef dict data_high = {
            'bid': bars_bid['high'].values,
            'ask': bars_ask['high'].values,
            'bid_size': bars_bid['volume'].values,
            'ask_size': bars_ask['volume'].values
        }

        cdef dict data_low = {
            'bid': bars_bid['low'].values,
            'ask': bars_ask['low'].values,
            'bid_size': bars_bid['volume'].values,
            'ask_size': bars_ask['volume'].values
        }

        cdef dict data_close = {
            'bid': bars_bid['close'],
            'ask': bars_ask['close'],
            'bid_size': bars_bid['volume'],
            'ask_size': bars_ask['volume']
        }

        df_ticks_h = pd.DataFrame(data=data_high, index=shifted_index)
        df_ticks_l = pd.DataFrame(data=data_low, index=shifted_index)
        df_ticks_c = pd.DataFrame(data=data_close)

        # Drop rows with no volume
        df_ticks_h = df_ticks_h[(df_ticks_h[['bid_size']] > 0).all(axis=1)]
        df_ticks_l = df_ticks_l[(df_ticks_l[['bid_size']] > 0).all(axis=1)]
        df_ticks_c = df_ticks_c[(df_ticks_c[['bid_size']] > 0).all(axis=1)]

        # Set high low tick volumes to zero
        df_ticks_h['bid_size'] = 0
        df_ticks_h['ask_size'] = 0
        df_ticks_l['bid_size'] = 0
        df_ticks_l['ask_size'] = 0

        # Merge tick data
        df_ticks_final = pd.concat([df_ticks_h, df_ticks_l, df_ticks_c])
        df_ticks_final.sort_index(axis=0, inplace=True)

        # Build ticks from data
        self.tick_data = df_ticks_final
        self.tick_data['symbol'] = symbol_indexer

    cpdef Tick _build_tick_from_values_with_sizes(self, double[:] values, datetime timestamp):
        """
        Build a tick from the given values. The function expects the values to
        be an ndarray with 2 elements [bid, ask] of type double.
        """
        return Tick(self._symbol,
                    Price(values[0], self._precision),
                    Price(values[1], self._precision),
                    timestamp,
                    bid_size=values[2],
                    ask_size=values[3])

    cpdef Tick _build_tick_from_values(self, double[:] values, datetime timestamp):
        """
        Build a tick from the given values. The function expects the values to
        be an ndarray with 2 elements [bid, ask] of type double.
        """
        return Tick(self._symbol,
                    Price(values[0], self._precision),
                    Price(values[1], self._precision),
                    timestamp)


cdef class BarDataWrangler:
    """
    Provides a means of building lists of bars from a given Pandas DataFrame of
    the correct specification.
    """

    def __init__(self,
                 int precision,
                 int volume_multiple=1,
                 data: pd.DataFrame=None):
        """
        Initializes a new instance of the BarBuilder class.

        :param precision: The decimal precision for bar prices (>= 0).
        :param data: The the bars market data.
        :param volume_multiple: The volume multiple for the builder (> 0).
        :raises: ValueError: If the decimal_precision is negative (< 0).
        :raises: ValueError: If the volume_multiple is not positive (> 0).
        :raises: ValueError: If the data is a type other than DataFrame.
        """
        Condition.not_negative_int(precision, 'precision')
        Condition.positive_int(volume_multiple, 'volume_multiple')
        Condition.type(data, pd.DataFrame, 'data')

        self._precision = precision
        self._volume_multiple = volume_multiple
        self._data = with_utc_index(data)

    cpdef list build_databars_all(self):
        """
        Return a list of DataBars from all data.
        
        :return List[DataBar].
        """
        return list(map(self._build_databar,
                        self._data.values,
                        pd.to_datetime(self._data.index)))

    cpdef list build_databars_from(self, int index=0):
        """
        Return a list of DataBars from the given index.
        
        :return List[DataBar].
        """
        Condition.not_negative_int(index, 'index')

        return list(map(self._build_databar,
                        self._data.iloc[index:].values,
                        pd.to_datetime(self._data.iloc[index:].index)))

    cpdef list build_databars_range(self, int start=0, int end=-1):
        """
        Return a list of DataBars within the given range.
        
        :return List[DataBar].
        """
        Condition.not_negative_int(start, 'start')

        return list(map(self._build_databar,
                        self._data.iloc[start:end].values,
                        pd.to_datetime(self._data.iloc[start:end].index)))

    cpdef list build_bars_all(self):
        """
        Return a list of Bars from all data.

        :return List[Bar].
        """
        return list(map(self._build_bar,
                        self._data.values,
                        pd.to_datetime(self._data.index)))

    cpdef list build_bars_from(self, int index=0):
        """
        Return a list of Bars from the given index (>= 0).

        :return List[Bar].
        """
        Condition.not_negative_int(index, 'index')

        return list(map(self._build_bar,
                        self._data.iloc[index:].values,
                        pd.to_datetime(self._data.iloc[index:].index)))

    cpdef list build_bars_range(self, int start=0, int end=-1):
        """
        Return a list of Bars within the given range.

        :return List[Bar].
        """
        Condition.not_negative_int(start, 'start')

        return list(map(self._build_bar,
                        self._data.iloc[start:end].values,
                        pd.to_datetime(self._data.iloc[start:end].index)))

    cpdef DataBar _build_databar(self, double[:] values, datetime timestamp):
        # Build a DataBar from the given index and values. The function expects the
        # values to be an ndarray with 5 elements [open, high, low, close, volume].
        return DataBar(values[0],
                       values[1],
                       values[2],
                       values[3],
                       values[4] * self._volume_multiple,
                       timestamp)

    cpdef Bar _build_bar(self, double[:] values, datetime timestamp):
        # Build a bar from the given index and values. The function expects the
        # values to be an ndarray with 5 elements [open, high, low, close, volume].
        return Bar(Price(values[0], self._precision),
                   Price(values[1], self._precision),
                   Price(values[2], self._precision),
                   Price(values[3], self._precision),
                   int(values[4] * self._volume_multiple),
                   timestamp)

cdef str _BID = 'bid'
cdef str _ASK = 'ask'
cdef str _POINT = 'point'
cdef str _PRICE = 'price'
cdef str _MID = 'mid'
cdef str _OPEN = 'open'
cdef str _HIGH = 'high'
cdef str _LOW = 'low'
cdef str _CLOSE = 'close'
cdef str _VOLUME = 'volume'
cdef str _TIMESTAMP = 'timestamp'


cdef class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with an indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """

    def __init__(self,
                 indicator not None,
                 input_method: Callable=None,
                 list outputs: List[str]=None):
        """
        Initializes a new instance of the IndicatorUpdater class.

        :param indicator: The indicator for updating.
        :param input_method: The indicators input method.
        :param outputs: The list of the indicators output properties.
        :raises TypeError: If the input_method is not of type Callable or None.
        """
        Condition.callable_or_none(input_method, 'input_method')

        self._indicator = indicator
        if input_method is None:
            self._input_method = indicator.update
        else:
            self._input_method = input_method

        self._input_params = []

        cdef dict param_map = {
            _BID: _BID,
            _ASK: _ASK,
            _POINT: _CLOSE,
            _PRICE: _CLOSE,
            _MID: _CLOSE,
            _OPEN: _OPEN,
            _HIGH: _HIGH,
            _LOW: _LOW,
            _CLOSE: _CLOSE,
            _TIMESTAMP: _TIMESTAMP
        }

        for param in inspect.signature(self._input_method).parameters:
            if param == 'self':
                self._include_self = True
            else:
                self._input_params.append(param_map[param])

        if outputs is None or not outputs:
            self._outputs = ['value']
        else:
            self._outputs = outputs

    cpdef void update_tick(self, Tick tick) except *:
        """
        Update the indicator with the given tick.
        
        :param tick: The tick to update with.
        """
        Condition.not_none(tick, 'tick')

        cdef str param
        if self._include_self:
            self._input_method(self._indicator, *[tick.__getattribute__(param).as_double() for param in self._input_params])
        else:
            self._input_method(*[tick.__getattribute__(param).as_double() for param in self._input_params])

    cpdef void update_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The bar to update with.
        """
        Condition.not_none(bar, 'bar')

        cdef str param
        if self._include_self:
            self._input_method(self._indicator, *[bar.__getattribute__(param).as_double() for param in self._input_params])
        else:
            self._input_method(*[bar.__getattribute__(param).as_double() for param in self._input_params])

    cpdef void update_databar(self, DataBar bar) except *:
        """
        Update the indicator with the given data bar.

        :param bar: The bar to update with.
        """
        Condition.not_none(bar, 'bar')

        cdef str param
        self._input_method(*[bar.__getattribute__(param) for param in self._input_params])

    cpdef dict build_features_ticks(self, list ticks):
        """
        Return a dictionary of output features from the given bars data.
        
        :return Dict[str, float].
        """
        Condition.not_none(ticks, 'ticks')

        cdef dict features = {}
        for output in self._outputs:
            features[output] = []

        cdef Bar bar
        cdef tuple value
        for tick in ticks:
            self.update_tick(tick)
            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cpdef dict build_features_bars(self, list bars):
        """
        Return a dictionary of output features from the given bars data.
        
        :return Dict[str, float].
        """
        Condition.not_none(bars, 'bars')

        cdef dict features = {}
        for output in self._outputs:
            features[output] = []

        cdef Bar bar
        cdef tuple value
        for bar in bars:
            self.update_bar(bar)
            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cpdef dict build_features_databars(self, list bars):
        """
        Return a dictionary of output features from the given bars data.
        
        :return Dict[str, float].
        """
        Condition.not_none(bars, 'bars')

        cdef dict features = {}
        for output in self._outputs:
            features[output] = []

        cdef DataBar bar
        cdef tuple value
        for bar in bars:
            self.update_databar(bar)
            for value in self._get_values():
                features[value[0]].append(value[1])

        return features

    cdef list _get_values(self):
        # Create a list of the current indicator outputs. The list will contain
        # a tuple of the name of the output and the float value. Returns List[(str, float)].
        cdef str output
        return [(output, self._indicator.__getattribute__(output)) for output in self._outputs]


cdef class BarBuilder:
    """
    The base class for all bar builders.
    """

    def __init__(self, BarSpecification bar_spec not None, bint use_previous_close=False):
        """
        Initializes a new instance of the BarBuilder class.

        :param bar_spec: The bar specification for the builder.
        :param use_previous_close: Set true if the previous close price should be the open price of a new bar.
        """
        self.bar_spec = bar_spec
        self.last_update = None
        self.count = 0

        self._open = None
        self._high = None
        self._low = None
        self._close = None
        self._volume = 0.0
        self._use_previous_close = use_previous_close

    cpdef void update(self, Tick tick) except *:
        """
        Update the builder with the given tick.

        :param tick: The tick to update with.
        """
        Condition.not_none(tick, 'tick')

        cdef Price price = self._get_price(tick)

        if self._open is None:
            # Initialize builder
            self._open = price
            self._high = price
            self._low = price
        elif price.gt(self._high):
            self._high = price
        elif price.lt(self._low):
            self._low = price

        self._close = price
        self._volume += self._get_volume(tick)
        self.count += 1
        self.last_update = tick.timestamp

    cpdef Bar build(self, datetime close_time=None):
        """
        Return a bar from the internal properties.

        :param close_time: The optional closing time for the bar (if None will be last updated time).

        :return: Bar.
        """
        if close_time is None:
            close_time = self.last_update

        cdef Bar bar = Bar(
            open_price=self._open,
            high_price=self._high,
            low_price=self._low,
            close_price=self._close,
            volume=self._volume,
            timestamp=close_time)

        self._reset()
        return bar

    cdef void _reset(self) except *:
        if self._use_previous_close:
            self._open = self._close
            self._high = self._close
            self._low = self._close
        else:
            self._open = None
            self._high = None
            self._low = None
            self._close = None

        self._volume = 0
        self.count = 0

    cdef Price _get_price(self, Tick tick):
        if self.bar_spec.price_type == PriceType.MID:
            return Price((tick.bid.as_double() + tick.ask.as_double()) / 2, tick.bid.precision + 1)
        elif self.bar_spec.price_type == PriceType.BID:
            return tick.bid
        elif self.bar_spec.price_type == PriceType.ASK:
            return tick.ask
        else:
            raise ValueError(f"The PriceType {price_type_to_string(self.bar_spec.price_type)} is not supported.")

    cdef double _get_volume(self, Tick tick):
        if self.bar_spec.price_type == PriceType.MID:
            return (tick.bid_size + tick.ask_size) / 2.0
        elif self.bar_spec.price_type == PriceType.BID:
            return tick.bid_size
        elif self.bar_spec.price_type == PriceType.ASK:
            return tick.ask_size
        else:
            raise ValueError(f"The PriceType {price_type_to_string(self.bar_spec.price_type)} is not supported.")


cdef class BarAggregator:
    """
    Provides a means of aggregating built bars to the registered handler.
    """

    def __init__(self,
                 BarType bar_type not None,
                 handler not None,
                 Logger logger not None,
                 bint use_previous_close):
        """
        Initializes a new instance of the BarAggregator class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The bar handler for the aggregator.
        :param logger: The logger for the aggregator.
        :param use_previous_close: If the previous close price should be the open price of a new bar.
        """
        self.bar_type = bar_type
        self._handler = BarHandler(handler)
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._builder = BarBuilder(
            bar_spec=self.bar_type.specification,
            use_previous_close=use_previous_close)

    cpdef void update(self, Tick tick) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _handle_bar(self, Bar bar) except *:
        # self._log.debug(f"Built {self.bar_type} Bar({bar})")
        self._handler.handle(self.bar_type, bar)


cdef class TickBarAggregator(BarAggregator):
    """
    Provides a means of building tick bars from ticks.
    """

    def __init__(self,
                 BarType bar_type not None,
                 handler not None,
                 Logger logger not None):
        """
        Initializes a new instance of the TickBarBuilder class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The bar handler for the aggregator.
        :param logger: The logger for the aggregator.
        """
        super().__init__(bar_type=bar_type,
                         handler=handler,
                         logger=logger,
                         use_previous_close=False)

        self.step = bar_type.specification.step

    cpdef void update(self, Tick tick) except *:
        """
        Update the builder with the given tick.

        :param tick: The tick for the update.
        """
        Condition.not_none(tick, 'tick')

        self._builder.update(tick)

        cdef Bar bar
        if self._builder.count == self.step:
            try:
                bar = self._builder.build()
            except ValueError as ex:
                # Bar was somehow malformed
                self._log.exception(ex)
                return

            self._handle_bar(bar)


cdef class TimeBarAggregator(BarAggregator):
    """
    Provides a means of building time bars from ticks with an internal timer.
    """
    def __init__(self,
                 BarType bar_type not None,
                 handler not None,
                 Clock clock not None,
                 Logger logger not None):
        """
        Initializes a new instance of the TickBarBuilder class.

        :param bar_type: The bar type for the aggregator.
        :param handler: The bar handler for the aggregator.
        :param clock: If the clock for the aggregator.
        :param logger: The logger for the aggregator.
        """
        super().__init__(bar_type=bar_type,
                         handler=handler,
                         logger=logger,
                         use_previous_close=True)

        self._clock = clock
        self.interval = self._get_interval()
        self.next_close = self._clock.next_event_time
        self._set_build_timer()

    cpdef void update(self, Tick tick) except *:
        """
        Update the builder with the given tick.

        :param tick: The tick for the update.
        """
        Condition.not_none(tick, 'tick')

        self._builder.update(tick)

        cdef TimeEventHandler event_handler
        if self._clock.is_test_clock:
            if self._clock.next_event_time <= tick.timestamp:
                for event_handler in self._clock.advance_time(tick.timestamp):
                    event_handler.handle()
                self.next_close = self._clock.next_event_time

    cpdef void _build_event(self, TimeEvent event) except *:
        cdef Bar bar
        try:
            bar = self._builder.build(event.timestamp)
        except ValueError as ex:
            # Bar was somehow malformed
            self._log.exception(ex)
            return

        self._handle_bar(bar)

    cdef timedelta _get_interval(self):
        if self.bar_type.specification.structure == BarStructure.SECOND:
            return timedelta(seconds=(1 * self.bar_type.specification.step))
        elif self.bar_type.specification.structure == BarStructure.MINUTE:
            return timedelta(minutes=(1 * self.bar_type.specification.step))
        elif self.bar_type.specification.structure == BarStructure.HOUR:
            return timedelta(hours=(1 * self.bar_type.specification.step))
        elif self.bar_type.specification.structure == BarStructure.DAY:
            return timedelta(days=(1 * self.bar_type.specification.step))
        else:
            raise ValueError(f"The BarStructure {bar_structure_to_string(self.bar_type.specification.structure)} is not supported.")

    cdef datetime _get_start_time(self):
        cdef datetime now = self._clock.time_now()
        if self.bar_type.specification.structure == BarStructure.SECOND:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                second=now.second,
                tzinfo=now.tzinfo
            )
        elif self.bar_type.specification.structure == BarStructure.MINUTE:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                minute=now.minute,
                tzinfo=now.tzinfo
            )
        elif self.bar_type.specification.structure == BarStructure.HOUR:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
                hour=now.hour,
                tzinfo=now.tzinfo
            )
        elif self.bar_type.specification.structure == BarStructure.DAY:
            return datetime(
                year=now.year,
                month=now.month,
                day=now.day,
            )
        else:
            raise ValueError(f"The BarStructure {bar_structure_to_string(self.bar_type.specification.structure)} is not supported.")

    cdef void _set_build_timer(self) except *:
        self._clock.set_timer(
            label=Label(self.bar_type.to_string()),
            interval=self._get_interval(),
            start_time=self._get_start_time(),
            stop_time=None,
            handler=self._build_event)
