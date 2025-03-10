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

import functools
from collections.abc import Callable
from decimal import Decimal
from inspect import iscoroutinefunction
from typing import Any
from zoneinfo import ZoneInfo

import pandas as pd
import pytz
from ibapi.common import BarData
from ibapi.common import HistoricalTickLast
from ibapi.common import MarketDataTypeEnum
from ibapi.common import TickAttribBidAsk
from ibapi.common import TickAttribLast

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client.common import BaseMixin
from nautilus_trader.adapters.interactive_brokers.client.common import Subscription
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.data import bar_spec_to_bar_size
from nautilus_trader.adapters.interactive_brokers.parsing.data import generate_trade_id
from nautilus_trader.adapters.interactive_brokers.parsing.data import timedelta_to_duration_str
from nautilus_trader.adapters.interactive_brokers.parsing.data import what_to_show
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId


# fmt: on


class InteractiveBrokersClientMarketDataMixin(BaseMixin):
    """
    Handles market data requests, subscriptions and data processing for the
    InteractiveBrokersClient.

    This class handles real-time and historical market data subscription management,
    including subscribing and unsubscribing to ticks, bars, and other market data types.
    It processes and formats the received data to be compatible with the Nautilus
    Trader.

    """

    async def set_market_data_type(self, market_data_type: MarketDataTypeEnum) -> None:
        """
        Set the market data type for data subscriptions. This method configures the type
        of market data (live, delayed, etc.) to be used for subsequent data requests.

        Parameters
        ----------
        market_data_type : MarketDataTypeEnum
            The market data type to be set

        """
        self._log.info(f"Setting Market DataType to {MarketDataTypeEnum.toStr(market_data_type)}")
        self._eclient.reqMarketDataType(market_data_type)

    async def _subscribe(
        self,
        name: str | tuple,
        subscription_method: Callable | functools.partial,
        cancellation_method: Callable,
        *args: Any,
        **kwargs: Any,
    ) -> Subscription:
        """
        Manage the subscription and unsubscription process for market data. This
        internal method is responsible for handling the logic to subscribe or
        unsubscribe to different market data types (ticks, bars, etc.). It uses the
        provided subscription and cancellation methods to control the data flow.

        Parameters
        ----------
        name : Any
            A unique identifier for the subscription.
        subscription_method : Callable
            The method to call for subscribing to market data.
        cancellation_method : Callable
            The method to call for unsubscribing from market data.
        *args
            Variable length argument list for the subscription method.
        **kwargs
            Arbitrary keyword arguments for the subscription method.

        Returns
        -------
        Subscription

        """
        if not (subscription := self._subscriptions.get(name=name)):
            self._log.info(
                f"Creating and registering a new Subscription instance for {name}",
            )
            req_id = self._next_req_id()
            if subscription_method == self.subscribe_historical_bars:
                handle_func = functools.partial(
                    subscription_method,
                    *args,
                    **kwargs,
                )
            else:
                handle_func = functools.partial(subscription_method, req_id, *args, **kwargs)

            # Add subscription
            subscription = self._subscriptions.add(
                req_id=req_id,
                name=name,
                handle=handle_func,
                cancel=functools.partial(cancellation_method, req_id),
            )

            # Intentionally skipping the call to historical request handler
            if subscription_method != self.subscribe_historical_bars:
                if iscoroutinefunction(subscription.handle):
                    await subscription.handle()
                else:
                    subscription.handle()
        else:
            self._log.info(f"Reusing existing Subscription instance for {subscription}")

        return subscription

    async def _unsubscribe(
        self,
        name: str | tuple,
        cancellation_method: Callable,
    ) -> None:
        """
        Manage the unsubscription process for market data. This internal method is
        responsible for handling the logic to unsubscribe to different market data types
        (ticks, bars, etc.). It uses the provided cancellation method to control the
        data flow.

        Parameters
        ----------
        cancellation_method : Callable
            The method to call for unsubscribing from market data.
        name : Any
            A unique identifier for the subscription.

        """
        if subscription := self._subscriptions.get(name=name):
            self._subscriptions.remove(subscription.req_id)
            cancellation_method(reqId=subscription.req_id)
            self._log.debug(f"Unsubscribed from {subscription}")
        else:
            self._log.debug(f"Subscription doesn't exist for {name}")

    async def subscribe_ticks(
        self,
        instrument_id: InstrumentId,
        contract: IBContract,
        tick_type: str,
        ignore_size: bool,
    ) -> None:
        """
        Subscribe to tick data for a specified instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The identifier of the instrument for which to subscribe.
        contract : IBContract
            The contract details for the instrument.
        tick_type : str
            The type of tick data to subscribe to.
        ignore_size : bool
            Omit updates that reflect only changes in size, and not price.
            Applicable to Bid_Ask data requests.

        """
        name = (str(instrument_id), tick_type)
        await self._subscribe(
            name,
            self._eclient.reqTickByTickData,
            self._eclient.cancelTickByTickData,
            contract,
            tick_type,
            0,
            ignore_size,
        )

    async def unsubscribe_ticks(self, instrument_id: InstrumentId, tick_type: str) -> None:
        """
        Unsubscribes from tick data for a specified instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The identifier of the instrument for which to unsubscribe.
        tick_type : str
            The type of tick data to unsubscribe from.

        """
        name = (str(instrument_id), tick_type)
        await self._unsubscribe(name, self._eclient.cancelTickByTickData)

    async def subscribe_realtime_bars(
        self,
        bar_type: BarType,
        contract: IBContract,
        use_rth: bool,
    ) -> None:
        """
        Subscribe to real-time bar data for a specified bar type.

        Parameters
        ----------
        bar_type : BarType
            The type of bar to subscribe to.
        contract : IBContract
            The Interactive Brokers contract details for the instrument.
        use_rth : bool
            Whether to use regular trading hours (RTH) only.

        """
        name = str(bar_type)
        await self._subscribe(
            name,
            self._eclient.reqRealTimeBars,
            self._eclient.cancelRealTimeBars,
            contract,
            bar_type.spec.step,
            what_to_show(bar_type),
            use_rth,
            [],
        )

    async def unsubscribe_realtime_bars(self, bar_type: BarType) -> None:
        """
        Unsubscribes from real-time bar data for a specified bar type.

        Parameters
        ----------
        bar_type : BarType
            The type of bar to unsubscribe from.

        """
        name = str(bar_type)
        await self._unsubscribe(name, self._eclient.cancelRealTimeBars)

    async def subscribe_historical_bars(
        self,
        bar_type: BarType,
        contract: IBContract,
        use_rth: bool,
        handle_revised_bars: bool,
    ) -> None:
        """
        Subscribe to historical bar data for a specified bar type and contract. It
        allows configuration for regular trading hours and handling of revised bars.

        Parameters
        ----------
        bar_type : BarType
            The type of bar to subscribe to.
        contract : IBContract
            The Interactive Brokers contract details for the instrument.
        use_rth : bool
            Whether to use regular trading hours (RTH) only.
        handle_revised_bars : bool
            Whether to handle revised bars or not.

        """
        name = str(bar_type)
        subscription = await self._subscribe(
            name,
            self.subscribe_historical_bars,
            self._eclient.cancelHistoricalData,
            bar_type=bar_type,
            contract=contract,
            use_rth=use_rth,
            handle_revised_bars=handle_revised_bars,
        )

        # Check and download the gaps or approx 300 bars whichever is less
        last_bar: Bar = self._cache.bar(bar_type)
        if last_bar is None:
            duration = pd.Timedelta(bar_type.spec.timedelta.total_seconds() * 300, "sec")
        else:
            duration = pd.Timedelta(self._clock.timestamp_ns() - last_bar.ts_event, "ns")
        bar_size_setting: str = bar_spec_to_bar_size(bar_type.spec)
        self._eclient.reqHistoricalData(
            reqId=subscription.req_id,
            contract=contract,
            endDateTime="",
            durationStr=timedelta_to_duration_str(duration),
            barSizeSetting=bar_size_setting,
            whatToShow=what_to_show(bar_type),
            useRTH=use_rth,
            formatDate=2,
            keepUpToDate=True,
            chartOptions=[],
        )

    async def unsubscribe_historical_bars(self, bar_type: BarType) -> None:
        """
        Unsubscribe from historical bar data for a specified bar type.

        Parameters
        ----------
        bar_type : BarType
            The type of bar to unsubscribe from.

        """
        name = str(bar_type)
        await self._unsubscribe(name, self._eclient.cancelHistoricalData)

    async def get_historical_bars(
        self,
        bar_type: BarType,
        contract: IBContract,
        use_rth: bool,
        end_date_time: pd.Timestamp,
        duration: str,
        timeout: int = 60,
    ) -> list[Bar]:
        """
        Request and retrieve historical bar data for a specified bar type.

        Parameters
        ----------
        bar_type : BarType
            The type of bar for which historical data is requested.
        contract : IBContract
            The Interactive Brokers contract details for the instrument.
        use_rth : bool
            Whether to use regular trading hours (RTH) only for the data.
        end_date_time : str
            The end time for the historical data request, formatted "%Y%m%d-%H:%M:%S".
        duration : str
            The duration for which historical data is requested, formatted as a string.
        timeout : int, optional
            The maximum time in seconds to wait for the historical data response.

        Returns
        -------
        list[Bar]

        """
        # Ensure the requested `end_date_time` is in UTC and set formatDate=2 to ensure returned dates are in UTC.
        if end_date_time.tzinfo is None:
            end_date_time = end_date_time.replace(tzinfo=ZoneInfo("UTC"))
        else:
            end_date_time = end_date_time.astimezone(ZoneInfo("UTC"))

        name = (bar_type, end_date_time)
        if not (request := self._requests.get(name=name)):
            req_id = self._next_req_id()
            bar_size_setting = bar_spec_to_bar_size(bar_type.spec)
            request = self._requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqHistoricalData,
                    reqId=req_id,
                    contract=contract,
                    endDateTime=end_date_time.strftime("%Y%m%d %H:%M:%S %Z"),
                    durationStr=duration,
                    barSizeSetting=bar_size_setting,
                    whatToShow=what_to_show(bar_type),
                    useRTH=use_rth,
                    formatDate=2,
                    keepUpToDate=False,
                    chartOptions=[],
                ),
                cancel=functools.partial(self._eclient.cancelHistoricalData, reqId=req_id),
            )
            if not request:
                return []
            self._log.debug(f"reqHistoricalData: {request.req_id=}, {contract=}")
            request.handle()
            return await self._await_request(request, timeout, default_value=[])
        else:
            self._log.info(f"Request already exist for {request}")
            return []

    async def get_historical_ticks(
        self,
        contract: IBContract,
        tick_type: str,
        start_date_time: pd.Timestamp | str = "",
        end_date_time: pd.Timestamp | str = "",
        use_rth: bool = True,
        timeout: int = 60,
    ) -> list[QuoteTick | TradeTick] | None:
        """
        Request and retrieve historical tick data for a specified contract and tick
        type.

        Parameters
        ----------
        contract : IBContract
            The Interactive Brokers contract details for the instrument.
        tick_type : str
            The type of tick data to request (e.g., 'BID_ASK', 'TRADES').
        start_date_time : pd.Timestamp | str, optional
            The start time for the historical data request. Can be a pandas Timestamp
            or a string formatted as 'YYYYMMDD HH:MM:SS [TZ]'.
        end_date_time : pd.Timestamp | str, optional
            The end time for the historical data request. Same format as start_date_time.
        use_rth : bool, optional
            Whether to use regular trading hours (RTH) only for the data.
        timeout : int, optional
            The maximum time in seconds to wait for the historical data response.

        Returns
        -------
        list[QuoteTick | TradeTick] | ``None``

        """
        if isinstance(start_date_time, pd.Timestamp):
            start_date_time = start_date_time.strftime("%Y%m%d %H:%M:%S %Z")
        if isinstance(end_date_time, pd.Timestamp):
            end_date_time = end_date_time.strftime("%Y%m%d %H:%M:%S %Z")

        name = (str(ib_contract_to_instrument_id(contract)), tick_type)
        if not (request := self._requests.get(name=name)):
            req_id = self._next_req_id()
            request = self._requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqHistoricalTicks,
                    reqId=req_id,
                    contract=contract,
                    startDateTime=start_date_time,
                    endDateTime=end_date_time,
                    numberOfTicks=1000,
                    whatToShow=tick_type,
                    useRth=use_rth,
                    ignoreSize=False,
                    miscOptions=[],
                ),
                cancel=functools.partial(self._eclient.cancelHistoricalData, reqId=req_id),
            )
            if not request:
                return None
            request.handle()
            return await self._await_request(request, timeout)
        else:
            self._log.info(f"Request already exist for {request}")
            return None

    async def _process_bar_data(
        self,
        bar_type_str: str,
        bar: BarData,
        handle_revised_bars: bool,
        historical: bool | None = False,
    ) -> Bar | None:
        """
        Process received bar data and convert it into NautilusTrader's Bar format. This
        method determines whether the bar is new or a revision of an existing bar and
        converts the bar data to the NautilusTrader's format.

        Parameters
        ----------
        bar_type_str : str
            The string representation of the bar type.
        bar : BarData
            The bar data received from Interactive Brokers.
        handle_revised_bars : bool
            Indicates whether revised bars should be handled or not.
        historical : bool | None, optional
            Indicates whether the bar data is historical. Defaults to False.

        Returns
        -------
        Bar | ``None``

        """
        previous_bar = self._bar_type_to_last_bar.get(bar_type_str)
        previous_ts = 0 if not previous_bar else int(previous_bar.date)
        current_ts = int(bar.date)

        if current_ts > previous_ts:
            is_new_bar = True
        elif current_ts == previous_ts:
            is_new_bar = False
        else:
            return None  # Out of sync

        self._bar_type_to_last_bar[bar_type_str] = bar
        bar_type: BarType = BarType.from_str(bar_type_str)
        ts_init = self._clock.timestamp_ns()
        if not handle_revised_bars:
            if previous_bar and is_new_bar:
                bar = previous_bar
            else:
                return None  # Wait for bar to close

            if historical:
                ts_init = await self._ib_bar_to_ts_init(bar, bar_type)
                if ts_init >= self._clock.timestamp_ns():
                    return None  # The bar is incomplete

        # Process the bar
        return await self._ib_bar_to_nautilus_bar(
            bar_type=bar_type,
            bar=bar,
            ts_init=ts_init,
            is_revision=not is_new_bar,
        )

    async def _convert_ib_bar_date_to_unix_nanos(self, bar: BarData, bar_type: BarType) -> int:
        """
        Convert the date from BarData to unix nanoseconds.

        If the bar type's aggregation is 14 - 16, the bar date is always returned in the
        YYYYMMDD format from IB. For all other aggregations, the bar date is returned
        in system time.

        Parameters
        ----------
        bar : BarData
            The bar data containing the date to be converted.
        bar_type : BarType
            The bar type that specifies the aggregation level.

        Returns
        -------
        int

        """
        if bar_type.spec.aggregation in [14, 15, 16]:
            # Day/Week/Month bars are always returned with bar date in YYYYMMDD format
            ts = pd.to_datetime(bar.date, format="%Y%m%d", utc=True)
        else:
            ts = pd.Timestamp.fromtimestamp(int(bar.date), tz=pytz.utc)

        return ts.value

    async def _ib_bar_to_ts_event(self, bar: BarData, bar_type: BarType) -> int:
        """
        Calculate the ts_event timestamp for a bar.

        This method computes the timestamp at which data event occurred, by adjusting
        the provided bar's timestamp based on the bar type's duration. ts_event is set
        to the start of the bar period.

        Week/Month bars's date returned from IB represents ending date,
        the start of bar period should be start of the week and month respectively

        Parameters
        ----------
        bar : BarData
            The bar data to be used for the calculation.
        bar_type : BarType
            The type of the bar, which includes information about the bar's duration.

        Returns
        -------
        int

        """
        ts_event = 0
        if bar_type.spec.aggregation in [15, 16]:
            date_obj = pd.to_datetime(bar.date, format="%Y%m%d", utc=True)
            if bar_type.spec.aggregation == 15:
                first_day_of_week = date_obj - pd.Timedelta(days=date_obj.weekday())
                ts_event = first_day_of_week.value
            else:
                first_day_of_month = date_obj.replace(day=1)
                ts_event = first_day_of_month.value
        else:
            ts_event = await self._convert_ib_bar_date_to_unix_nanos(bar, bar_type)
        return ts_event

    async def _ib_bar_to_ts_init(self, bar: BarData, bar_type: BarType) -> int:
        """
        Calculate the initialization timestamp for a bar.

        This method computes the timestamp at which a bar is initialized, by adjusting
        the provided bar's timestamp based on the bar type's duration. ts_init is set
        to the end of the bar period and not the start.

        Parameters
        ----------
        bar : BarData
            The bar data to be used for the calculation.
        bar_type : BarType
            The type of the bar, which includes information about the bar's duration.

        Returns
        -------
        int

        """
        ts = await self._convert_ib_bar_date_to_unix_nanos(bar, bar_type)
        if bar_type.spec.aggregation in [15, 16]:
            # Week/Month bars's date represents ending date
            return ts
        elif bar_type.spec.aggregation == 14:
            # -1 to make day's bar ts_event and ts_init on the same day
            return ts + pd.Timedelta(bar_type.spec.timedelta).value - 1
        else:
            return ts + pd.Timedelta(bar_type.spec.timedelta).value

    async def _ib_bar_to_nautilus_bar(
        self,
        bar_type: BarType,
        bar: BarData,
        ts_init: int,
        is_revision: bool = False,
    ) -> Bar:
        """
        Convert Interactive Brokers bar data to NautilusTrader's bar type.

        Parameters
        ----------
        bar_type : BarType
            The type of the bar.
        bar : BarData
            The bar data received from Interactive Brokers.
        ts_init : int
            The unix nanosecond timestamp representing the bar's initialization time.
        is_revision : bool, optional
            Indicates whether the bar is a revision of an existing bar. Defaults to False.

        Returns
        -------
        Bar

        """
        instrument = self._cache.instrument(bar_type.instrument_id)
        if not instrument:
            raise ValueError(f"No cached instrument for {bar_type.instrument_id}")

        ts_event = await self._ib_bar_to_ts_event(bar, bar_type)
        # used to be _convert_ib_bar_date_to_unix_nanos

        return Bar(
            bar_type=bar_type,
            open=instrument.make_price(bar.open),
            high=instrument.make_price(bar.high),
            low=instrument.make_price(bar.low),
            close=instrument.make_price(bar.close),
            volume=instrument.make_qty(0 if bar.volume == -1 else bar.volume),
            ts_event=ts_event,
            ts_init=ts_init,
            is_revision=is_revision,
        )

    async def _process_trade_ticks(self, req_id: int, ticks: list[HistoricalTickLast]) -> None:
        """
        Process received trade tick data, convert it to NautilusTrader TradeTick type,
        and add it to the relevant request's result.

        Parameters
        ----------
        req_id : int
            The request identifier for which the trades are being processed.
        ticks : list
            A list of trade tick data received from Interactive Brokers.

        """
        if request := self._requests.get(req_id=req_id):
            instrument_id = InstrumentId.from_str(request.name[0])
            instrument = self._cache.instrument(instrument_id)

            for tick in ticks:
                ts_event = pd.Timestamp.fromtimestamp(tick.time, tz=pytz.utc).value
                trade_tick = TradeTick(
                    instrument_id=instrument_id,
                    price=instrument.make_price(tick.price),
                    size=instrument.make_qty(tick.size),
                    aggressor_side=AggressorSide.NO_AGGRESSOR,
                    trade_id=generate_trade_id(ts_event=ts_event, price=tick.price, size=tick.size),
                    ts_event=ts_event,
                    ts_init=ts_event,
                )
                request.result.append(trade_tick)

            self._end_request(req_id)

    async def _handle_data(self, data: Data) -> None:
        """
        Handle and forward processed data to the appropriate destination. This method is
        a generic data handler that forwards processed market data, such as bars or
        ticks, to the DataEngine.process message bus endpoint.

        Parameters
        ----------
        data : Data
            The processed market data ready to be forwarded.

        """
        self._msgbus.send(endpoint="DataEngine.process", msg=data)

    async def process_market_data_type(self, *, req_id: int, market_data_type: int) -> None:
        """
        Return the market data type (real-time, frozen, delayed, delayed-frozen)
        of ticker sent by EClientSocket::reqMktData when TWS switches from real-time
        to frozen and back and from delayed to delayed-frozen and back.
        """
        if market_data_type == MarketDataTypeEnum.REALTIME:
            self._log.debug(f"Market DataType is {MarketDataTypeEnum.toStr(market_data_type)}")
        else:
            self._log.warning(f"Market DataType is {MarketDataTypeEnum.toStr(market_data_type)}")

    async def process_tick_by_tick_bid_ask(
        self,
        *,
        req_id: int,
        time: int,
        bid_price: float,
        ask_price: float,
        bid_size: Decimal,
        ask_size: Decimal,
        tick_attrib_bid_ask: TickAttribBidAsk,
    ) -> None:
        """
        Return "BidAsk" tick-by-tick real-time tick data.
        """
        if not (subscription := self._subscriptions.get(req_id=req_id)):
            return

        instrument_id = InstrumentId.from_str(subscription.name[0])
        instrument = self._cache.instrument(instrument_id)
        ts_event = pd.Timestamp.fromtimestamp(time, tz=pytz.utc).value

        quote_tick = QuoteTick(
            instrument_id=instrument_id,
            bid_price=instrument.make_price(bid_price),
            ask_price=instrument.make_price(ask_price),
            bid_size=instrument.make_qty(bid_size),
            ask_size=instrument.make_qty(ask_size),
            ts_event=ts_event,
            ts_init=max(self._clock.timestamp_ns(), ts_event),  # `ts_event` <= `ts_init`
        )

        await self._handle_data(quote_tick)

    async def process_tick_by_tick_all_last(
        self,
        *,
        req_id: int,
        tick_type: int,
        time: int,
        price: float,
        size: Decimal,
        tick_attrib_last: TickAttribLast,
        exchange: str,
        special_conditions: str,
    ) -> None:
        """
        Return "Last" or "AllLast" (trades) tick-by-tick real-time tick.
        """
        if not (subscription := self._subscriptions.get(req_id=req_id)):
            return

        # Halted tick
        if price == 0 and size == 0 and tick_attrib_last.pastLimit:
            return

        instrument_id = InstrumentId.from_str(subscription.name[0])
        instrument = self._cache.instrument(instrument_id)
        ts_event = pd.Timestamp.fromtimestamp(time, tz=pytz.utc).value

        trade_tick = TradeTick(
            instrument_id=instrument_id,
            price=instrument.make_price(price),
            size=instrument.make_qty(size),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=generate_trade_id(ts_event=ts_event, price=price, size=size),
            ts_event=ts_event,
            ts_init=max(self._clock.timestamp_ns(), ts_event),  # `ts_event` <= `ts_init`
        )

        await self._handle_data(trade_tick)

    async def process_realtime_bar(
        self,
        *,
        req_id: int,
        time: int,
        open_: float,
        high: float,
        low: float,
        close: float,
        volume: Decimal,
        wap: Decimal,
        count: int,
    ) -> None:
        """
        Update real-time 5 second bars.
        """
        if not (subscription := self._subscriptions.get(req_id=req_id)):
            return
        bar_type = BarType.from_str(subscription.name)
        instrument = self._cache.instrument(bar_type.instrument_id)

        bar = Bar(
            bar_type=bar_type,
            open=instrument.make_price(open_),
            high=instrument.make_price(high),
            low=instrument.make_price(low),
            close=instrument.make_price(close),
            volume=instrument.make_qty(0 if volume == -1 else volume),
            ts_event=pd.Timestamp.fromtimestamp(time, tz=pytz.utc).value,
            ts_init=self._clock.timestamp_ns(),
            is_revision=False,
        )

        await self._handle_data(bar)

    async def process_historical_data(self, *, req_id: int, bar: BarData) -> None:
        """
        Return the requested historical data bars.
        """
        if request := self._requests.get(req_id=req_id):
            bar_type = request.name[0]
            bar = await self._ib_bar_to_nautilus_bar(
                bar_type=bar_type,
                bar=bar,
                ts_init=await self._ib_bar_to_ts_init(bar, bar_type),
            )
            if bar:
                request.result.append(bar)
        elif subscription := self._subscriptions.get(req_id=req_id):
            bar = await self._process_bar_data(
                bar_type_str=str(subscription.name),
                bar=bar,
                handle_revised_bars=False,
                historical=True,
            )
            if bar:
                await self._handle_data(bar)
        else:
            self._log.debug(f"Received {bar=} on {req_id=}")
            return

    async def process_historical_data_end(self, *, req_id: int, start: str, end: str) -> None:
        """
        Mark the end of receiving historical bars.
        """
        self._end_request(req_id)

    async def process_historical_data_update(self, *, req_id: int, bar: BarData) -> None:
        """
        Receive bars in real-time if keepUpToDate is set as True in reqHistoricalData.

        Similar to realTimeBars function, except returned data is a composite of
        historical data and real time data that is equivalent to TWS chart functionality
        to keep charts up to date. Returned bars are successfully updated using real-
        time data.

        """
        if not (subscription := self._subscriptions.get(req_id=req_id)):
            return
        if not isinstance(subscription.handle, functools.partial):
            raise TypeError(f"Expecting partial type subscription method. {subscription=}")
        if bar := await self._process_bar_data(
            bar_type_str=str(subscription.name),
            bar=bar,
            handle_revised_bars=subscription.handle.keywords.get("handle_revised_bars", False),
        ):
            if bar.is_single_price() and bar.open.as_double() == 0:
                self._log.debug(f"Ignoring Zero priced {bar=}")
            else:
                await self._handle_data(bar)

    async def process_historical_ticks_bid_ask(
        self,
        *,
        req_id: int,
        ticks: list,
        done: bool,
    ) -> None:
        """
        Return the requested historic bid/ask ticks.
        """
        if not done:
            return
        if request := self._requests.get(req_id=req_id):
            instrument_id = InstrumentId.from_str(request.name[0])
            instrument = self._cache.instrument(instrument_id)

            for tick in ticks:
                ts_event = pd.Timestamp.fromtimestamp(tick.time, tz=pytz.utc).value
                quote_tick = QuoteTick(
                    instrument_id=instrument_id,
                    bid_price=instrument.make_price(tick.priceBid),
                    ask_price=instrument.make_price(tick.priceAsk),
                    bid_size=instrument.make_qty(tick.sizeBid),
                    ask_size=instrument.make_qty(tick.sizeAsk),
                    ts_event=ts_event,
                    ts_init=ts_event,
                )
                request.result.append(quote_tick)

            self._end_request(req_id)

    async def process_historical_ticks_last(self, *, req_id: int, ticks: list, done: bool) -> None:
        """
        Return the requested historic trades.
        """
        if not done:
            return
        await self._process_trade_ticks(req_id, ticks)

    async def process_historical_ticks(self, *, req_id: int, ticks: list, done: bool) -> None:
        """
        Return the requested historic ticks.
        """
        if not done:
            return
        await self._process_trade_ticks(req_id, ticks)

    async def get_price(self, contract, tick_type="MidPoint"):
        """
        Request market data for a specific contract and tick type.

        This method requests market data from Interactive Brokers for the given
        contract and tick type, waits for the response, and returns the result.

        Parameters
        ----------
        contract : IBContract
            The contract details for which market data is requested.
        tick_type : str, optional
            The type of tick data to request (default is "MidPoint").

        Returns
        -------
        Any
            The market data result.

        Raises
        ------
        asyncio.TimeoutError
            If the request times out.

        """
        req_id = self._next_req_id()
        request = self._requests.add(
            req_id=req_id,
            name=f"{contract.symbol}-{tick_type}",
            handle=functools.partial(
                self._eclient.reqMktData,
                req_id,
                contract,
                tick_type,
                False,
                False,
                [],
            ),
            cancel=functools.partial(self._eclient.cancelMktData, req_id),
        )
        request.handle()
        return await self._await_request(request, timeout=60)
