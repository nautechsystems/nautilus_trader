import asyncio
import datetime

import pandas as pd

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.gateway import InteractiveBrokersGateway
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.msgbus.bus import MessageBus


class HistoricInteractiveBrokersClient:
    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
    ):
        loop = asyncio.get_event_loop()
        clock = LiveClock()
        logger = Logger(clock)
        self.log = LoggerAdapter("HistoricInteractiveBrokersClient", logger)
        msgbus = MessageBus(
            TraderId("historic_interactive_brokers_client-001"),
            clock,
            logger,
        )
        cache = Cache(logger)
        self.client = InteractiveBrokersClient(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            host=host,
            port=port,
            client_id=client_id,
        )

    async def connect(self):
        await self.client._socket_connect()

    async def request_trade_ticks(
        self,
        contract: IBContract,
        date: datetime.date,
        tz_name: str,
    ) -> list[TradeTick]:
        data: list[TradeTick] = []
        while True:
            start_time = _determine_next_timestamp(
                date=date,
                timestamps=[d.time for d in data],
                tz_name=tz_name,
            )
            _ = await self.client.get_historical_ticks(
                contract=contract,
                tick_type="TRADES",
                end_date_time=start_time,
                use_rth=True,
            )

        return data

    # def request_tick_data(
    #     self,
    #     contract: Contract,
    #     date: datetime.date,
    #     kind: str,
    #     tz_name: str,
    # ) -> list:
    #     assert kind in ("TRADES", "BID_ASK")
    #     data: list = []
    #
    #     while True:
    #         start_time = _determine_next_timestamp(
    #             date=date,
    #             timestamps=[d.time for d in data],
    #             tz_name=tz_name,
    #         )
    #         self.log.debug(f"Using start_time: {start_time}")
    #
    #         ticks = _request_historical_ticks(
    #             client=client,
    #             contract=contract,
    #             start_time=start_time.strftime("%Y%m%d %H:%M:%S %Z"),
    #             what=kind,
    #         )
    #
    #         ticks = [t for t in ticks if t not in data]
    #
    #         if not ticks or ticks[-1].time < start_time:
    #             break
    #
    #         self.log.debug(
    #             f"Received {len(ticks)} ticks between {ticks[0].time} and {ticks[-1].time}",
    #         )
    #
    #         last_timestamp = pd.Timestamp(ticks[-1].time)
    #         last_date = last_timestamp.astimezone(tz_name).date()
    #
    #         if last_date != date:
    #             # May contain data from next date, filter this out
    #             data.extend(
    #                 [
    #                     tick
    #                     for tick in ticks
    #                     if pd.Timestamp(tick.time).astimezone(tz_name).date() == date
    #                 ],
    #             )
    #             break
    #         else:
    #             data.extend(ticks)
    #     return data

    # def request_bar_data(
    #     self,
    #     client: InteractiveBrokersClient,
    #     contract: Contract,
    #     date: datetime.date,
    #     tz_name: str,
    #     bar_spec: BarSpecification,
    # ) -> list:
    #     data: list = []
    #
    #     start_time = pd.Timestamp(date).tz_localize(tz_name).tz_convert("UTC")
    #     end_time = start_time + datetime.timedelta(days=1)
    #
    #     while True:
    #         self.self.log.debug(f"Using end_time: {end_time}")
    #
    #         # bar_data_list: BarDataList = _request_historical_bars(
    #         bar_data_list = _request_historical_bars(
    #             client=client,
    #             contract=contract,
    #             end_time=end_time.strftime("%Y%m%d %H:%M:%S %Z"),
    #             bar_spec=bar_spec,
    #         )
    #
    #         bars = [bar for bar in bar_data_list if bar not in data and bar.volume != 0]
    #
    #         if not bars:
    #             break
    #
    #         self.log.info(f"Received {len(bars)} bars between {bars[0].date} and {bars[-1].date}")
    #
    #         # We're requesting from end_date backwards, set our timestamp to the earliest timestamp
    #         first_timestamp = pd.Timestamp(bars[0].date).tz_convert(tz_name)
    #         first_date = first_timestamp.date()
    #
    #         if first_date != date:
    #             # May contain data from next date, filter this out
    #             data.extend(
    #                 [
    #                     bar
    #                     for bar in bars
    #                     if parse_response_datetime(bar.date, tz_name=tz_name).date() == date
    #                 ],
    #             )
    #             break
    #         else:
    #             data.extend(bars)
    #
    #         end_time = first_timestamp
    #
    #     return data
    #
    # def _request_historical_ticks(
    #     self,
    #     client: InteractiveBrokersClient,
    #     contract: Contract,
    #     start_time: str,
    #     what="BID_ASK",
    # ):
    #     return client.reqHistoricalTicks(
    #         contract=contract,
    #         startDateTime=start_time,
    #         endDateTime="",
    #         numberOfTicks=1000,
    #         whatToShow=what,
    #         useRth=False,
    #     )
    #
    # def _bar_spec_to_hist_data_request(self, bar_spec: BarSpecification) -> dict[str, str]:
    #     aggregation = bar_aggregation_to_str(bar_spec.aggregation)
    #     price_type = price_type_to_str(bar_spec.price_type)
    #     accepted_aggregations = ("SECOND", "MINUTE", "HOUR")
    #
    #     err = f"Loading historic bars is for intraday data, bar_spec.aggregation should be {accepted_aggregations}"
    #     assert aggregation in accepted_aggregations, err
    #
    #     price_mapping = {"MID": "MIDPOINT", "LAST": "TRADES"}
    #     what_to_show = price_mapping.get(price_type, price_type)
    #
    #     size_mapping = {"SECOND": "sec", "MINUTE": "min", "HOUR": "hour"}
    #     suffix = "" if bar_spec.step == 1 and aggregation != "SECOND" else "s"
    #     bar_size = size_mapping.get(aggregation, aggregation)
    #     bar_size_setting = f"{bar_spec.step} {bar_size + suffix}"
    #     return {
    #         "durationStr": "1 D",
    #         "barSizeSetting": bar_size_setting,
    #         "whatToShow": what_to_show,
    #     }
    #
    # def _request_historical_bars(
    #     self,
    #     contract: Contract,
    #     end_time: str,
    #     bar_spec: BarSpecification,
    # ):
    #     spec = _bar_spec_to_hist_data_request(bar_spec=bar_spec)
    #     return client._client.reqHistoricalData(
    #         contract=contract,
    #         endDateTime=end_time,
    #         durationStr=spec["durationStr"],
    #         barSizeSetting=spec["barSizeSetting"],
    #         whatToShow=spec["whatToShow"],
    #         useRTH=False,
    #         formatDate=2,
    #     )

    # def parse_response_datetime(
    #         dt: datetime.datetime | pd.Timestamp,
    #         tz_name: str,
    # ) -> datetime.datetime:
    #     if isinstance(dt, pd.Timestamp):
    #         dt = dt.to_pydatetime()
    #     if dt.tzinfo is None:
    #         tz = pytz.timezone(tz_name)
    #         dt = tz.localize(dt)
    #     return dt
    #
    # def parse_historic_quote_ticks(
    #         historic_ticks: list[HistoricalTickBidAsk],
    #         instrument: Instrument,
    # ) -> list[QuoteTick]:
    #     trades = []
    #     for tick in historic_ticks:
    #         ts_init = dt_to_unix_nanos(tick.time)
    #         quote_tick = QuoteTick(
    #             instrument_id=instrument.id,
    #             bid=Price(value=tick.priceBid, precision=instrument.price_precision),
    #             bid_size=Quantity(value=tick.sizeBid, precision=instrument.size_precision),
    #             ask=Price(value=tick.priceAsk, precision=instrument.price_precision),
    #             ask_size=Quantity(value=tick.sizeAsk, precision=instrument.size_precision),
    #             ts_init=ts_init,
    #             ts_event=ts_init,
    #         )
    #         trades.append(quote_tick)
    #
    #     return trades
    #
    # def parse_historic_trade_ticks(
    #         historic_ticks: list[HistoricalTickLast],
    #         instrument: Instrument,
    # ) -> list[TradeTick]:
    #     trades = []
    #     for tick in historic_ticks:
    #         ts_init = dt_to_unix_nanos(tick.time)
    #         trade_tick = TradeTick(
    #             instrument_id=instrument.id,
    #             price=Price(value=tick.price, precision=instrument.price_precision),
    #             size=Quantity(value=tick.size, precision=instrument.size_precision),
    #             aggressor_side=AggressorSide.NO_AGGRESSOR,
    #             trade_id=generate_trade_id(
    #                 ts_event=ts_init,
    #                 price=tick.price,
    #                 size=tick.size,
    #             ),
    #             ts_init=ts_init,
    #             ts_event=ts_init,
    #         )
    #         trades.append(trade_tick)
    #
    #     return trades
    #
    # def parse_historic_bars(
    #         historic_bars: list[BarData],
    #         instrument: Instrument,
    #         kind: str,
    # ) -> list[Bar]:
    #     bars = []
    #     bar_type = BarType(
    #         bar_spec=BarSpecification.from_str(kind.split("-", maxsplit=1)[1]),
    #         instrument_id=instrument.id,
    #         aggregation_source=AggregationSource.EXTERNAL,
    #     )
    #     precision = instrument.price_precision
    #     for bar in historic_bars:
    #         ts_init = dt_to_unix_nanos(bar.date)
    #         trade_tick = Bar(
    #             bar_type=bar_type,
    #             open=Price(bar.open, precision),
    #             high=Price(bar.high, precision),
    #             low=Price(bar.low, precision),
    #             close=Price(bar.close, precision),
    #             volume=Quantity(bar.volume, instrument.size_precision),
    #             ts_init=ts_init,
    #             ts_event=ts_init,
    #         )
    #         bars.append(trade_tick)
    #
    #     return bars


def _determine_next_timestamp(timestamps: list[pd.Timestamp], date: datetime.date, tz_name: str):
    """
    While looping over available data, it is possible for very liquid products that a 1s
    period may contain 1000 ticks, at which point we need to step the time forward to
    avoid getting stuck when iterating.
    """
    if not timestamps:
        return pd.Timestamp(date, tz=tz_name).tz_convert("UTC")
    unique_values = set(timestamps)
    if len(unique_values) == 1:
        timestamp = timestamps[-1]
        return timestamp + pd.Timedelta(seconds=1)
    else:
        return timestamps[-1]


async def main():
    contract = IBContract(secType="STK", exchange="SM", localSymbol="AAPL", currency="USD")
    _ = InteractiveBrokersGateway(start=True)
    client = HistoricInteractiveBrokersClient(port=4002)
    await client.connect()
    await client.request_trade_ticks(
        contract=contract,
        date=datetime.date(2023, 10, 25),
        tz_name="America/New_York",
    )


if __name__ == "__main__":
    asyncio.run(main())
