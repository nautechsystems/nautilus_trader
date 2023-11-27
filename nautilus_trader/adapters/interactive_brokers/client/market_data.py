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
from decimal import Decimal
from typing import Literal

import pandas as pd
import pytz
from ibapi.common import BarData
from ibapi.common import MarketDataTypeEnum
from ibapi.common import SetOfFloat
from ibapi.common import SetOfString
from ibapi.common import TickAttribBidAsk
from ibapi.common import TickAttribLast
from ibapi.utils import current_fn_name
from ibapi.wrapper import EWrapper

# fmt: off
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.parsing.data import bar_spec_to_bar_size
from nautilus_trader.adapters.interactive_brokers.parsing.data import generate_trade_id
from nautilus_trader.adapters.interactive_brokers.parsing.data import timedelta_to_duration_str
from nautilus_trader.adapters.interactive_brokers.parsing.data import what_to_show
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import ib_contract_to_instrument_id
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId


# fmt: on


class InteractiveBrokersMarketDataManager(EWrapper):
    """
    Handles market data subscriptions and data processing for the
    InteractiveBrokersClient.

    This class handles real-time and historical market data subscription management,
    including subscribing and unsubscribing to ticks, bars, and other market data types.
    It processes and formats the received data to be compatible with the Nautilus
    Trader.

    """

    def __init__(self, client):
        self._client = client
        self._eclient = client._eclient
        self._log = client._log

        # Hot cache
        self._bar_type_to_last_bar: dict[str, BarData | None] = {}

    async def set_market_data_type(self, market_data_type: MarketDataTypeEnum) -> None:
        """
        Set the market data type for data subscriptions. This method configures the type
        of market data (live, delayed, etc.) to be used for subsequent data requests.

        Parameters
        ----------
        market_data_type : MarketDataTypeEnum
            The market data type to be set

        Returns
        -------
        None

        """
        self._log.info(f"Setting Market DataType to {MarketDataTypeEnum.to_str(market_data_type)}")
        self._eclient.reqMarketDataType(market_data_type)

    async def _manage_subscription(
        self,
        action: Literal["subscribe", "unsubscribe"],
        subscription_method,
        cancellation_method,
        name,
        *args,
        **kwargs,
    ) -> None:
        """
        Manage the subscription and unsubscription process for market data. This
        internal method is responsible for handling the logic to subscribe or
        unsubscribe to different market data types (ticks, bars, etc.). It uses the
        provided subscription and cancellation methods to control the data flow.

        Parameters
        ----------
        action : Literal["subscribe", "subscribe"]
            The action to perform, either 'subscribe' or 'unsubscribe'.
        subscription_method : Callable
            The method to call for subscribing to market data.
        cancellation_method : Callable
            The method to call for unsubscribing from market data.
        name : Any
            A unique identifier for the subscription.
        *args
            Variable length argument list for the subscription method.
        **kwargs
            Arbitrary keyword arguments for the subscription method.

        Returns
        -------
        None

        """
        if action == "subscribe":
            if not (subscription := self._client.subscriptions.get(name=name)):
                req_id = self._client.next_req_id()
                subscription = self._client.subscriptions.add(
                    req_id=req_id,
                    name=name,
                    handle=functools.partial(subscription_method, reqId=req_id, *args, **kwargs),
                    cancel=functools.partial(cancellation_method, reqId=req_id),
                )
                subscription.handle()
            else:
                self._log.info(f"Subscription already exists for {subscription}")
        elif action == "unsubscribe":
            if subscription := self._client.subscriptions.get(name=name):
                self._client.subscriptions.remove(subscription.req_id)
                cancellation_method(reqId=subscription.req_id)
                self._log.debug(f"Unsubscribed from {subscription}")
            else:
                self._log.debug(f"Subscription doesn't exist for {name}")

    async def subscribe_ticks(
        self,
        instrument_id: InstrumentId,
        contract: IBContract,
        tick_type: str,
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

        Returns
        -------
        None

        """
        name = (str(instrument_id), tick_type)
        await self._manage_subscription(
            "subscribe",
            self._eclient.reqTickByTickData,
            self._eclient.cancelTickByTickData,
            name,
            contract,
            tick_type,
            0,
            True,
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

        Returns
        -------
        None

        """
        name = (str(instrument_id), tick_type)
        await self._manage_subscription(
            "unsubscribe",
            None,
            self._eclient.cancelTickByTickData,
            name,
        )

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

        Returns
        -------
        None

        """
        name = str(bar_type)
        await self._manage_subscription(
            "subscribe",
            self._eclient.reqRealTimeBars,
            self._eclient.cancelRealTimeBars,
            name,
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

        Returns
        -------
        None

        """
        await self._manage_subscription(
            "unsubscribe",
            None,
            self._eclient.cancelRealTimeBars,
            str(bar_type),
        )

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

        Returns
        -------
        None

        """
        if not (subscription := self._client.subscriptions.get(name=str(bar_type))):
            req_id = self._client.next_req_id()
            subscription = self._client.subscriptions.add(
                req_id=req_id,
                name=str(bar_type),
                handle=functools.partial(
                    self.subscribe_historical_bars,
                    bar_type=bar_type,
                    contract=contract,
                    use_rth=use_rth,
                    handle_revised_bars=handle_revised_bars,
                ),
                cancel=functools.partial(
                    self._eclient.cancelHistoricalData,
                    reqId=req_id,
                ),
            )
        else:
            self._log.info(f"Subscription already exist for {subscription}")

        # Check and download the gaps or approx 300 bars whichever is less
        last_bar: Bar = self._client.cache.bar(bar_type)
        if last_bar is None:
            duration = pd.Timedelta(bar_type.spec.timedelta.total_seconds() * 300, "sec")
        else:
            duration = pd.Timedelta(self._client.clock.timestamp_ns() - last_bar.ts_event, "ns")
        bar_size_setting: str = bar_spec_to_bar_size(bar_type.spec)
        self._client.reqHistoricalData(
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

        Returns
        -------
        None

        """
        await self._manage_subscription(
            "unsubscribe",
            None,
            self._eclient.cancelHistoricalData,
            str(bar_type),
        )

    async def get_historical_bars(
        self,
        bar_type: BarType,
        contract: IBContract,
        use_rth: bool,
        end_date_time: str,
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
            The end time for the historical data request, formatted as a string.
        duration : str
            The duration for which historical data is requested.
        timeout : int, optional
            The maximum time in seconds to wait for the historical data response.

        Returns
        -------
        list[Bar]

        """
        name = str(bar_type)
        if not (request := self._client.requests.get(name=name)):
            req_id = self._client.next_req_id()
            bar_size_setting = bar_spec_to_bar_size(bar_type.spec)
            request = self._client.requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._client.reqHistoricalData,
                    reqId=req_id,
                    contract=contract,
                    endDateTime=end_date_time,
                    durationStr=duration,
                    barSizeSetting=bar_size_setting,
                    whatToShow=what_to_show(bar_type),
                    useRTH=use_rth,
                    formatDate=2,
                    keepUpToDate=False,
                    chartOptions=[],
                ),
                cancel=functools.partial(self._client.cancelHistoricalData, reqId=req_id),
            )
            self._log.debug(f"reqHistoricalData: {request.req_id=}, {contract=}")
            request.handle()
            return await self._client.await_request(request, timeout)
        else:
            self._log.info(f"Request already exist for {request}")
            return None

    async def get_historical_ticks(
        self,
        contract: IBContract,
        tick_type: str,
        start_date_time: pd.Timestamp | str = "",
        end_date_time: pd.Timestamp | str = "",
        use_rth: bool = True,
        timeout: int = 60,
    ) -> list[QuoteTick | TradeTick]:
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
            The end time for the historical data request. Format is similar to start_date_time.
        use_rth : bool, optional
            Whether to use regular trading hours (RTH) only for the data.
        timeout : int, optional
            The maximum time in seconds to wait for the historical data response.

        Returns
        -------
        list[QuoteTick | TradeTick]

        """
        if isinstance(start_date_time, pd.Timestamp):
            start_date_time = start_date_time.strftime("%Y%m%d %H:%M:%S %Z")
        if isinstance(end_date_time, pd.Timestamp):
            end_date_time = end_date_time.strftime("%Y%m%d %H:%M:%S %Z")

        name = (str(ib_contract_to_instrument_id(contract)), tick_type)
        if not (request := self._client.requests.get(name=name)):
            req_id = self._client.next_req_id()
            request = self._client.requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._client.reqHistoricalTicks,
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
                cancel=functools.partial(self._client.cancelHistoricalData, reqId=req_id),
            )
            request.handle()
            return await self._client.await_request(request, timeout)
        else:
            self._log.info(f"Request already exist for {request}")
            return None

    def _process_bar_data(
        self,
        bar_type_str: str,
        bar: BarData,
        handle_revised_bars: bool,
        historical: bool | None = False,
    ) -> Bar | None:
        """
        Process received bar data and convert it into Nautilus Trader's Bar format. This
        method determines whether the bar is new or a revision of an existing bar and
        converts the bar data to the Nautilus Trader's format.

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
        Bar | None

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
        ts_init = self._client.clock.timestamp_ns()
        if not handle_revised_bars:
            if previous_bar and is_new_bar:
                bar = previous_bar
            else:
                return None  # Wait for bar to close

            if historical:
                ts_init = self._ib_bar_to_ts_init(bar, bar_type)
                if ts_init >= self._client.clock.timestamp_ns():
                    return None  # The bar is incomplete

        # Process the bar
        bar = self._ib_bar_to_nautilus_bar(
            bar_type=bar_type,
            bar=bar,
            ts_init=ts_init,
            is_revision=not is_new_bar,
        )
        return bar

    def _convert_ib_bar_date_to_unix_nanos(self, bar: BarData, bar_type: BarType) -> int:
        """
        Convert the date from BarData to unix nanoseconds.

        If the bar type's aggregation is 14, the bar date is always returned in the
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
        if bar_type.spec.aggregation == 14:
            # Day bars are always returned with bar date in YYYYMMDD format
            ts = pd.to_datetime(bar.date, format="%Y%m%d", utc=True)
        else:
            ts = pd.Timestamp.fromtimestamp(int(bar.date), tz=pytz.utc)

        return ts.value

    def _ib_bar_to_ts_init(self, bar: BarData, bar_type: BarType) -> int:
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
        ts = self._convert_ib_bar_date_to_unix_nanos(bar, bar_type)
        return ts + pd.Timedelta(bar_type.spec.timedelta).value

    def _ib_bar_to_nautilus_bar(
        self,
        bar_type: BarType,
        bar: BarData,
        ts_init: int,
        is_revision: bool = False,
    ) -> Bar:
        """
        Convert Interactive Brokers bar data to Nautilus Trader's bar type.

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
        instrument = self._client.cache.instrument(bar_type.instrument_id)

        ts_event = self._convert_ib_bar_date_to_unix_nanos(bar, bar_type)

        bar = Bar(
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

        return bar

    def _process_trade_ticks(self, req_id: int, ticks: list) -> None:
        """
        Process received trade tick data, convert it to Nautilus Trader TradeTick type,
        and add it to the relevant request's result.

        Parameters
        ----------
        req_id : int
            The request identifier for which the trade ticks are being processed.
        ticks : list
            A list of trade tick data received from Interactive Brokers.

        Returns
        -------
        None

        """
        if request := self._client.requests.get(req_id=req_id):
            instrument_id = InstrumentId.from_str(request.name[0])
            instrument = self._client.cache.instrument(instrument_id)

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

            self._client.end_request(req_id)

    def _handle_data(self, data: Data) -> None:
        """
        Handle and forward processed data to the appropriate destination. This method is
        a generic data handler that forwards processed market data, such as bars or
        ticks, to the DataEngine.process message bus endpoint.

        Parameters
        ----------
        data : Data
            The processed market data ready to be forwarded.

        Returns
        -------
        None

        """
        self._client.msgbus.send(endpoint="DataEngine.process", msg=data)

    # -- EWrapper overrides -----------------------------------------------------------------------
    def marketDataType(self, req_id: int, market_data_type: int) -> None:
        """
        Return the market data type (real-time, frozen, delayed, delayed-frozen)
        of ticker sent by EClientSocket::reqMktData when TWS switches from real-time
        to frozen and back and from delayed to delayed-frozen and back.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if market_data_type == MarketDataTypeEnum.REALTIME:
            self._log.debug(f"Market DataType is {MarketDataTypeEnum.to_str(market_data_type)}")
        else:
            self._log.warning(f"Market DataType is {MarketDataTypeEnum.to_str(market_data_type)}")

    def tickByTickBidAsk(
        self,
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
        self._client.logAnswer(current_fn_name(), vars())
        if not (subscription := self._client.subscriptions.get(req_id=req_id)):
            return

        instrument_id = InstrumentId.from_str(subscription.name[0])
        instrument = self._client.cache.instrument(instrument_id)
        ts_event = pd.Timestamp.fromtimestamp(time, tz=pytz.utc).value

        quote_tick = QuoteTick(
            instrument_id=instrument_id,
            bid_price=instrument.make_price(bid_price),
            ask_price=instrument.make_price(ask_price),
            bid_size=instrument.make_qty(bid_size),
            ask_size=instrument.make_qty(ask_size),
            ts_event=ts_event,
            ts_init=max(self._client.clock.timestamp_ns(), ts_event),  # `ts_event` <= `ts_init`
        )

        self._handle_data(quote_tick)

    def tickByTickAllLast(
        self,
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
        self._client.logAnswer(current_fn_name(), vars())
        if not (subscription := self._client.subscriptions.get(req_id=req_id)):
            return

        # Halted tick
        if price == 0 and size == 0 and tick_attrib_last.pastLimit:
            return

        instrument_id = InstrumentId.from_str(subscription.name[0])
        instrument = self._client.cache.instrument(instrument_id)
        ts_event = pd.Timestamp.fromtimestamp(time, tz=pytz.utc).value

        trade_tick = TradeTick(
            instrument_id=instrument_id,
            price=instrument.make_price(price),
            size=instrument.make_qty(size),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=generate_trade_id(ts_event=ts_event, price=price, size=size),
            ts_event=ts_event,
            ts_init=max(self._client.clock.timestamp_ns(), ts_event),  # `ts_event` <= `ts_init`
        )

        self._handle_data(trade_tick)

    def realtimeBar(
        self,
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
        self._client.logAnswer(current_fn_name(), vars())
        if not (subscription := self._client.subscriptions.get(req_id=req_id)):
            return
        bar_type = BarType.from_str(subscription.name)
        instrument = self._client.cache.instrument(bar_type.instrument_id)

        bar = Bar(
            bar_type=bar_type,
            open=instrument.make_price(open_),
            high=instrument.make_price(high),
            low=instrument.make_price(low),
            close=instrument.make_price(close),
            volume=instrument.make_qty(0 if volume == -1 else volume),
            ts_event=pd.Timestamp.fromtimestamp(time, tz=pytz.utc).value,
            ts_init=self._client.clock.timestamp_ns(),
            is_revision=False,
        )

        self._handle_data(bar)

    def historicalData(self, req_id: int, bar: BarData) -> None:
        """
        Return the requested historical data bars.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if request := self._client.requests.get(req_id=req_id):
            bar_type = BarType.from_str(request.name)
            bar = self._ib_bar_to_nautilus_bar(
                bar_type=bar_type,
                bar=bar,
                ts_init=self._ib_bar_to_ts_init(bar, bar_type),
            )
            if bar:
                request.result.append(bar)
        elif request := self._client.subscriptions.get(req_id=req_id):
            bar = self._process_bar_data(
                bar_type_str=request.name,
                bar=bar,
                handle_revised_bars=False,
                historical=True,
            )
            if bar:
                self._handle_data(bar)
        else:
            self._log.debug(f"Received {bar=} on {req_id=}")
            return

    def historicalDataEnd(self, req_id: int, start: str, end: str) -> None:
        """
        Mark the end of receiving historical bars.
        """
        self._client.logAnswer(current_fn_name(), vars())
        self._client.end_request(req_id)
        if req_id == 1 and not self._client.is_ib_ready.is_set():  # probe successful
            self._log.info(f"`is_ib_ready` set by historicalDataEnd {req_id=}", LogColor.BLUE)
            self._client.is_ib_ready.set()

    def historicalDataUpdate(self, req_id: int, bar: BarData) -> None:
        """
        Receive bars in real-time if keepUpToDate is set as True in reqHistoricalData.

        Similar to realTimeBars function, except returned data is a composite of
        historical data and real time data that is equivalent to TWS chart functionality
        to keep charts up to date. Returned bars are successfully updated using real-
        time data.

        """
        self._client.logAnswer(current_fn_name(), vars())
        if not (subscription := self._client.subscriptions.get(req_id=req_id)):
            return
        if bar := self._process_bar_data(
            bar_type_str=subscription.name,
            bar=bar,
            handle_revised_bars=subscription.handle.keywords.get("handle_revised_bars", False),
        ):
            if bar.is_single_price() and bar.open.as_double() == 0:
                self._log.debug(f"Ignoring Zero priced {bar=}")
            else:
                self._handle_data(bar)

    def historicalTicksBidAsk(
        self,
        req_id: int,
        ticks: list,
        done: bool,
    ) -> None:
        self._client.logAnswer(current_fn_name(), vars())
        if not done:
            return
        if request := self._client.requests.get(req_id=req_id):
            instrument_id = InstrumentId.from_str(request.name[0])
            instrument = self._client.cache.instrument(instrument_id)

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

            self._client.end_request(req_id)

    def historicalTicksLast(self, req_id: int, ticks: list, done: bool) -> None:
        self._client.logAnswer(current_fn_name(), vars())
        if not done:
            return
        self._process_trade_ticks(req_id, ticks)

    def historicalTicks(self, req_id: int, ticks: list, done: bool) -> None:
        self._client.logAnswer(current_fn_name(), vars())
        if not done:
            return
        self._process_trade_ticks(req_id, ticks)

    def securityDefinitionOptionParameter(
        self,
        req_id: int,
        exchange: str,
        underlying_con_id: int,
        trading_class: str,
        multiplier: str,
        expirations: SetOfString,
        strikes: SetOfFloat,
    ) -> None:
        """
        Return the option chain for an underlying on an exchange specified in
        reqSecDefOptParams There will be multiple callbacks to
        securityDefinitionOptionParameter if multiple exchanges are specified in
        reqSecDefOptParams.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if request := self._client.requests.get(req_id=req_id):
            request.result.append((exchange, expirations))

    def securityDefinitionOptionParameterEnd(self, req_id: int) -> None:
        """
        Call when all callbacks to securityDefinitionOptionParameter are complete.
        """
        self._client.logAnswer(current_fn_name(), vars())
        self._client.end_request(req_id)
