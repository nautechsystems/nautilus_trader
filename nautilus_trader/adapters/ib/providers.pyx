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

from decimal import Decimal

import ib_insync
from ib_insync.contract import ContractDetails

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.time cimport unix_timestamp_ns
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_class cimport AssetClassParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.future cimport Future
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class IBInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects through Interactive Brokers.
    """

    def __init__(
        self,
        client not None: ib_insync.IB,
        str host="127.0.0.1",
        str port="7497",
        int client_id=1,
    ):
        """
        Initialize a new instance of the ``IBInstrumentProvider`` class.

        Parameters
        ----------
        client : ib_insync.IB
            The Interactive Brokers client.
        host : str
            The client host name or IP address.
        port : str
            The client port number.
        client_id : int
            The unique client identifier number for the connection.

        """
        super().__init__()

        self._client = client
        self._host = host
        self._port = port
        self._client_id = client_id

    cpdef void connect(self):
        self._client.connect(
            host=self._host,
            port=self._port,
            clientId=self._client_id,
        )

    cpdef void load(self, InstrumentId instrument_id, dict details) except *:
        """
        Load the instrument for the given identifier and details.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier.
        details : dict
            The instrument details.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(details, "details")

        if not self._client.client.CONNECTED:
            self.connect()

        contract = ib_insync.contract.Future(
            symbol=instrument_id.symbol.value,
            lastTradeDateOrContractMonth=details.get("expiry"),
            exchange=instrument_id.venue.value,
            multiplier=details.get("multiplier"),
            currency=details.get("currency"),
        )

        cdef list contract_details = self._client.reqContractDetails(contract=contract)
        cdef Future future = self._parse_futures_contract(
            instrument_id=instrument_id,
            asset_class=AssetClassParser.from_str(details.get("asset_class")),
            details_list=contract_details,
        )

        self._instruments[instrument_id] = future

    cdef int _tick_size_to_precision(self, double tick_size) except *:
        cdef tick_size_str = f"{tick_size:f}"
        return len(tick_size_str.partition('.')[2].rstrip('0'))

    cdef Future _parse_futures_contract(
        self,
        InstrumentId instrument_id,
        AssetClass asset_class,
        list details_list,
    ):
        if not details_list:
            raise ValueError(f"No contract details found for the given instrument identifier {instrument_id}")
        elif len(details_list) > 1:
            raise ValueError(f"Multiple contract details found for the given instrument identifier {instrument_id}")

        details: ContractDetails = details_list[0]

        cdef int price_precision = self._tick_size_to_precision(details.minTick)

        timestamp = unix_timestamp_ns()
        cdef Future future = Future(
            instrument_id=instrument_id,
            asset_class=asset_class,
            currency=Currency.from_str_c(details.contract.currency),
            price_precision=price_precision,
            price_increment=Price(details.minTick, price_precision),
            multiplier=Quantity.from_int_c(int(details.contract.multiplier)),
            lot_size=Quantity.from_int_c(1),
            expiry=details.contract.lastTradeDateOrContractMonth,
            contract_id=details.contract.conId,
            local_symbol=details.contract.localSymbol,
            trading_class=details.contract.tradingClass,
            market_name=details.marketName,
            long_name=details.longName,
            contract_month=details.contractMonth,
            time_zone_id=details.timeZoneId,
            trading_hours=details.tradingHours,
            liquid_hours=details.liquidHours,
            last_trade_time=details.lastTradeTime,
            ts_event_ns=timestamp,
            ts_recv_ns=timestamp,
        )

        return future
