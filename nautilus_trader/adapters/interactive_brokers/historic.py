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
from typing import List, Literal

import pandas as pd
from ib_insync import IB
from ib_insync import Contract
from ib_insync import HistoricalTickBidAsk
from ib_insync import HistoricalTickLast

from nautilus_trader.adapters.interactive_brokers.factories import get_cached_ib_client
from nautilus_trader.adapters.interactive_brokers.parsing.data import generate_trade_id
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_objects


logger = logging.getLogger(__name__)


def make_filename(
    instrument_id: InstrumentId, kind: Literal["BID_ASK", "TRADES"], date: datetime.date
):
    fn_kind = {"BID_ASK": "quote_tick", "TRADES": "trade_tick"}[kind]
    return f"{fn_kind}.parquet/instrument_id={instrument_id.value}/{date}-0.parquet"


def back_fill_catalog(
    ib: IB,
    catalog: DataCatalog,
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
    catalog : DataCatalog
        DataCatalog to write the data to
    contracts : List[Contract]
        The list of IB Contracts to collect data for
    start_date : datetime.date
        The start_date for the back fill.
    end_date : datetime.date
        The end_date for the back fill.
    tz_name : str
        The timezone of the contracts
    kinds : tuple[str] (default: ('BID_ASK', 'TRADES')
        The kinds to query data for
    """
    for contract in contracts:
        [details] = ib.reqContractDetails(contract=contract)
        instrument = parse_instrument(contract_details=details)
        for date in pd.bdate_range(start_date, end_date, tz=tz_name):
            for kind in kinds:
                fn = make_filename(instrument_id=instrument.id, kind=kind, date=date)
                if catalog.fs.exists(fn):
                    logger.info(f"{fn} exists, skipping")
                    continue
                raw = fetch_market_data(
                    contract=contract, date=date.to_pydatetime(), kind=kind, tz_name=tz_name, ib=ib
                )
                logger.info(f"Got {len(raw)} raw ticks")
                if not raw:
                    logging.info("No ticks, skipping")
                    continue
                if kind == "TRADES":
                    ticks = parse_historic_trade_ticks(
                        historic_ticks=raw, instrument_id=instrument.id
                    )
                elif kind == "BID_ASK":
                    ticks = parse_historic_quote_ticks(
                        historic_ticks=raw, instrument_id=instrument.id
                    )
                else:
                    raise RuntimeError()
                template = f"{date:%Y%m%d}" + "-{i}.parquet"
                write_objects(catalog=catalog, chunk=ticks, basename_template=template)


def fetch_market_data(
    contract: Contract, date: datetime.date, kind: str, tz_name: str, ib=None
) -> List:
    if isinstance(date, datetime.datetime):
        date = date.date()
    assert kind in ("TRADES", "BID_ASK")
    data: List = []

    while True:
        start_time = _determine_next_timestamp(
            date=date, timestamps=[d.time for d in data], tz_name=tz_name
        )
        logger.info(f"Using start_time: {start_time}")

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
            data.extend([tick for tick in ticks if pd.to_datetime(tick)])
            break
        else:
            data.extend(ticks)
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
                symbol=instrument_id.symbol.value,
                ts_event=ts_init,
                price=tick.price,
                size=tick.size,
            ),
            ts_init=ts_init,
            ts_event=ts_init,
        )
        trades.append(trade_tick)

    return trades
