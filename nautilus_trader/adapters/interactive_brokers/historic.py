# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime
import logging
from typing import Dict, List, Literal, Union

import pandas as pd
import pytz
from ib_insync import IB
from ib_insync import BarData
from ib_insync import BarDataList
from ib_insync import Contract
from ib_insync import HistoricalTickBidAsk
from ib_insync import HistoricalTickLast

from nautilus_trader.adapters.interactive_brokers.parsing.data import generate_trade_id
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser
from nautilus_trader.model.c_enums.price_type import PriceTypeParser
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import write_objects


logger = logging.getLogger(__name__)


def generate_filename(
    catalog: ParquetDataCatalog,
    instrument_id: InstrumentId,
    kind: Literal["BID_ASK", "TRADES"],
    date: datetime.date,
) -> str:
    fn_kind = {"BID_ASK": "quote_tick", "TRADES": "trade_tick", "BARS": "bars"}[kind.split("-")[0]]
    return f"{catalog.path}/data/{fn_kind}.parquet/instrument_id={instrument_id.value}/{date:%Y%m%d}-0.parquet"


def back_fill_catalog(
    ib: IB,
    catalog: ParquetDataCatalog,
    contracts: List[Contract],
    start_date: datetime.date,
    end_date: datetime.date,
    tz_name: str,
    kinds=("BID_ASK", "TRADES"),
):
    """
    Back fill the data catalog with market data from Interactive Brokers.

    Parameters
    ----------
    ib : IB
        The ib_insync client.
    catalog : ParquetDataCatalog
        The data catalog to write the data to.
    contracts : List[Contract]
        The list of IB Contracts to collect data for.
    start_date : datetime.date
        The start_date for the back fill.
    end_date : datetime.date
        The end_date for the back fill.
    tz_name : str
        The timezone of the contracts
    kinds : tuple[str] (default: ('BID_ASK', 'TRADES')
        The kinds to query data for, can be any of:
        - BID_ASK
        - TRADES
        - A bar specification, i.e. BARS-1-MINUTE-LAST or BARS-5-SECOND-MID
    """
    for date in pd.bdate_range(start_date, end_date, tz=tz_name):
        for contract in contracts:
            [details] = ib.reqContractDetails(contract=contract)
            instrument = parse_instrument(contract_details=details)

            # Check if this instrument exists in the catalog, if not, write it.
            if not catalog.instruments(instrument_ids=[instrument.id.value], as_nautilus=True):
                write_objects(catalog=catalog, chunk=[instrument])

            for kind in kinds:
                fn = generate_filename(catalog, instrument_id=instrument.id, kind=kind, date=date)
                if catalog.fs.exists(fn):
                    logger.info(
                        f"file for {instrument.id.value} {kind} {date:%Y-%m-%d} exists, skipping"
                    )
                    continue
                logger.info(f"Fetching {instrument.id.value} {kind} for {date:%Y-%m-%d}")

                data = request_data(
                    contract=contract,
                    instrument=instrument,
                    date=date.date(),
                    kind=kind,
                    tz_name=tz_name,
                    ib=ib,
                )
                if data is None:
                    continue

                template = f"{date:%Y%m%d}" + "-{i}.parquet"
                write_objects(catalog=catalog, chunk=data, basename_template=template)


def request_data(
    contract: Contract,
    instrument: Instrument,
    date: datetime.date,
    kind: str,
    tz_name: str,
    ib: IB = None,
):
    if kind in ("TRADES", "BID_ASK"):
        raw = request_tick_data(contract=contract, date=date, kind=kind, tz_name=tz_name, ib=ib)
    elif kind.split("-")[0] == "BARS":
        bar_spec = BarSpecification.from_str(kind.split("-", maxsplit=1)[1])
        raw = request_bar_data(
            contract=contract, date=date, bar_spec=bar_spec, tz_name=tz_name, ib=ib
        )
    else:
        raise RuntimeError(f"Unknown {kind=}")

    if not raw:
        logging.info(f"No ticks for {date=} {kind=} {contract=}, skipping")
        return
    logger.info(f"Fetched {len(raw)} raw {kind}")
    if kind == "TRADES":
        return parse_historic_trade_ticks(historic_ticks=raw, instrument_id=instrument.id)
    elif kind == "BID_ASK":
        return parse_historic_quote_ticks(historic_ticks=raw, instrument_id=instrument.id)
    elif kind.split("-")[0] == "BARS":
        return parse_historic_bars(historic_bars=raw, instrument=instrument, kind=kind)
    else:
        raise RuntimeError(f"Unknown {kind=}")


def request_tick_data(
    contract: Contract, date: datetime.date, kind: str, tz_name: str, ib=None
) -> List:
    assert kind in ("TRADES", "BID_ASK")
    data: List = []

    while True:
        start_time = _determine_next_timestamp(
            date=date, timestamps=[d.time for d in data], tz_name=tz_name
        )
        logger.debug(f"Using start_time: {start_time}")

        ticks = _request_historical_ticks(
            ib=ib,
            contract=contract,
            start_time=start_time.strftime("%Y%m%d %H:%M:%S %Z"),
            what=kind,
        )

        ticks = [t for t in ticks if t not in data]

        if not ticks or ticks[-1].time < start_time:
            break

        logger.debug(f"Received {len(ticks)} ticks between {ticks[0].time} and {ticks[-1].time}")

        last_timestamp = pd.Timestamp(ticks[-1].time)
        last_date = last_timestamp.astimezone(tz_name).date()

        if last_date != date:
            # May contain data from next date, filter this out
            data.extend(
                [
                    tick
                    for tick in ticks
                    if pd.Timestamp(tick.time).astimezone(tz_name).date() == date
                ]
            )
            break
        else:
            data.extend(ticks)
    return data


def request_bar_data(
    contract: Contract, date: datetime.date, tz_name: str, bar_spec: BarSpecification, ib=None
) -> List:
    data: List = []

    start_time = pd.Timestamp(date).tz_localize(tz_name).tz_convert("UTC")
    end_time = start_time + datetime.timedelta(days=1)

    while True:
        logger.debug(f"Using end_time: {end_time}")

        bar_data_list: BarDataList = _request_historical_bars(
            ib=ib,
            contract=contract,
            end_time=end_time.strftime("%Y%m%d %H:%M:%S %Z"),
            bar_spec=bar_spec,
        )

        bars = [bar for bar in bar_data_list if bar not in data and bar.volume != 0]

        if not bars:
            break

        logger.info(f"Received {len(bars)} bars between {bars[0].date} and {bars[-1].date}")

        # We're requesting from end_date backwards, set our timestamp to the earliest timestamp
        first_timestamp = pd.Timestamp(bars[0].date).tz_convert(tz_name)
        first_date = first_timestamp.date()

        if first_date != date:
            # May contain data from next date, filter this out
            data.extend(
                [
                    bar
                    for bar in bars
                    if parse_response_datetime(bar.date, tz_name=tz_name).date() == date
                ]
            )
            break
        else:
            data.extend(bars)

        end_time = first_timestamp

    return data


def _request_historical_ticks(ib: IB, contract: Contract, start_time: str, what="BID_ASK"):
    return ib.reqHistoricalTicks(
        contract=contract,
        startDateTime=start_time,
        endDateTime="",
        numberOfTicks=1000,
        whatToShow=what,
        useRth=False,
    )


def _bar_spec_to_hist_data_request(bar_spec: BarSpecification) -> Dict[str, str]:
    aggregation = BarAggregationParser.to_str_py(bar_spec.aggregation)
    price_type = PriceTypeParser.to_str_py(bar_spec.price_type)
    accepted_aggregations = ("SECOND", "MINUTE", "HOUR")

    err = f"Loading historic bars is for intraday data, bar_spec.aggregation should be {accepted_aggregations}"
    assert aggregation in accepted_aggregations, err

    price_mapping = {"MID": "MIDPOINT", "LAST": "TRADES"}
    what_to_show = price_mapping.get(price_type, price_type)

    size_mapping = {"SECOND": "sec", "MINUTE": "min", "HOUR": "hour"}
    suffix = "" if bar_spec.step == 1 and aggregation != "SECOND" else "s"
    bar_size = size_mapping.get(aggregation, aggregation)
    bar_size_setting = f"{bar_spec.step} {bar_size + suffix}"
    return {"durationStr": "1 D", "barSizeSetting": bar_size_setting, "whatToShow": what_to_show}


def _request_historical_bars(ib: IB, contract: Contract, end_time: str, bar_spec: BarSpecification):
    spec = _bar_spec_to_hist_data_request(bar_spec=bar_spec)
    return ib.reqHistoricalData(
        contract=contract,
        endDateTime=end_time,
        durationStr=spec["durationStr"],
        barSizeSetting=spec["barSizeSetting"],
        whatToShow=spec["whatToShow"],
        useRTH=False,
        formatDate=2,
    )


def _determine_next_timestamp(timestamps: List[pd.Timestamp], date: datetime.date, tz_name: str):
    """
    While looping over available data, it is possible for very liquid products that a 1s period may contain 1000 ticks,
    at which point we need to step the time forward to avoid getting stuck when iterating.
    """
    if not timestamps:
        return pd.Timestamp(date, tz=tz_name).tz_convert("UTC")
    unique_values = set(timestamps)
    if len(unique_values) == 1:
        timestamp = timestamps[-1]
        return timestamp + pd.Timedelta(seconds=1)
    else:
        return timestamps[-1]


def parse_response_datetime(
    dt: Union[datetime.datetime, pd.Timestamp], tz_name: str
) -> datetime.datetime:
    if isinstance(dt, pd.Timestamp):
        dt = dt.to_pydatetime()
    if dt.tzinfo is None:
        tz = pytz.timezone(tz_name)
        dt = tz.localize(dt)
    return dt


def parse_historic_quote_ticks(
    historic_ticks: List[HistoricalTickBidAsk], instrument_id: InstrumentId
) -> List[QuoteTick]:
    trades = []
    for tick in historic_ticks:
        ts_init = dt_to_unix_nanos(tick.time)
        quote_tick = QuoteTick(
            instrument_id=instrument_id,
            bid=Price.from_str(str(tick.priceBid)),
            bid_size=Quantity.from_str(str(tick.sizeBid)),
            ask=Price.from_str(str(tick.priceAsk)),
            ask_size=Quantity.from_str(str(tick.sizeAsk)),
            ts_init=ts_init,
            ts_event=ts_init,
        )
        trades.append(quote_tick)

    return trades


def parse_historic_trade_ticks(
    historic_ticks: List[HistoricalTickLast], instrument_id: InstrumentId
) -> List[TradeTick]:
    trades = []
    for tick in historic_ticks:
        ts_init = dt_to_unix_nanos(tick.time)
        trade_tick = TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(str(tick.price)),
            size=Quantity.from_str(str(tick.size)),
            aggressor_side=AggressorSide.UNKNOWN,
            trade_id=generate_trade_id(
                ts_event=ts_init,
                price=tick.price,
                size=tick.size,
            ),
            ts_init=ts_init,
            ts_event=ts_init,
        )
        trades.append(trade_tick)

    return trades


def parse_historic_bars(
    historic_bars: List[BarData], instrument: Instrument, kind: str
) -> List[Bar]:
    bars = []
    bar_type = BarType(
        bar_spec=BarSpecification.from_str(kind.split("-", maxsplit=1)[1]),
        instrument_id=instrument.id,
        aggregation_source=AggregationSource.EXTERNAL,
    )
    precision = instrument.price_precision
    for bar in historic_bars:
        ts_init = dt_to_unix_nanos(bar.date)
        trade_tick = Bar(
            bar_type=bar_type,
            open=Price(bar.open, precision),
            high=Price(bar.high, precision),
            low=Price(bar.low, precision),
            close=Price(bar.close, precision),
            volume=Quantity(bar.volume, instrument.size_precision),
            ts_init=ts_init,
            ts_event=ts_init,
        )
        bars.append(trade_tick)

    return bars
