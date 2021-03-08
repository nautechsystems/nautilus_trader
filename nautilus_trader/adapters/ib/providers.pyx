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

from cpython.datetime cimport datetime

from ib_insync.contract import ContractDetails

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.currency cimport Currency
#from nautilus_trader.model.identifiers cimport Security
from nautilus_trader.model.identifiers cimport Symbol
#from nautilus_trader.model.instrument cimport Future
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.instrument cimport Quantity


cdef class IBInstrumentProvider:
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
        Initialize a new instance of the `IBInstrumentProvider` class.

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
        self.count = 0
        self._instruments = {}  # type: dict[Symbol, Instrument]
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

    # cpdef Future load_future(
    #     self,
    #     Security security,
    #     AssetClass asset_class=AssetClass.UNDEFINED,
    # ):
    #     """
    #     Return the future contract instrument for the given security identifier.
    #
    #     Parameters
    #     ----------
    #     security : Security
    #         The security identifier for the futures contract.
    #     asset_class : AssetClass, optional
    #         The optional asset class for the future (not used to filter).
    #
    #     Returns
    #     -------
    #     Future or None
    #
    #     """
    #     Condition.not_none(security, "security")
    #
    #     if not self._client.client.CONNECTED:
    #         self.connect()
    #
    #     contract = ib_insync.contract.Future(
    #         symbol=security.code,
    #         lastTradeDateOrContractMonth=security.expiry,
    #         exchange=security.venue.value,
    #         multiplier=security.multiplier,
    #         currency=security.currency,
    #     )
    #
    #     cdef list details = self._client.reqContractDetails(contract=contract)
    #     cdef Future future = self._parse_futures_contract(security, asset_class, details)
    #
    #     self._instruments[future.symbol] = future
    #
    #     return future
    #
    # cpdef Instrument get(self, Security security):
    #     """
    #     Return the instrument for the given security (if found).
    #
    #     Returns
    #     -------
    #     Instrument or None
    #
    #     """
    #     return self._instruments.get(security)
    #
    # cdef inline int _tick_size_to_precision(self, double tick_size) except *:
    #     cdef tick_size_str = f"{tick_size:f}"
    #     return len(tick_size_str.partition('.')[2].rstrip('0'))
    #
    # cdef Future _parse_futures_contract(
    #     self,
    #     Security security,
    #     AssetClass asset_class,
    #     list details_list,
    # ):
    #     if len(details_list) == 0:
    #         raise ValueError(f"No contract details found for the given security identifier {security}")
    #     elif len(details_list) > 1:
    #         raise ValueError(f"Multiple contract details found for the given security identifier {security}")
    #
    #     details: ContractDetails = details_list[0]
    #
    #     cdef Currency currency = Currency.from_str_c(security.currency)
    #     cdef int price_precision = self._tick_size_to_precision(details.minTick)
    #
    #     cdef Future future = Future(
    #         security=security,
    #         asset_class=asset_class,
    #         contract_id=details.contract.conId,
    #         local_symbol=details.contract.localSymbol,
    #         trading_class=details.contract.tradingClass,
    #         market_name=details.marketName,
    #         long_name=details.longName,
    #         contract_month=details.contractMonth,
    #         time_zone_id=details.timeZoneId,
    #         trading_hours=details.tradingHours,
    #         liquid_hours=details.liquidHours,
    #         last_trade_time=details.lastTradeTime,
    #         price_precision=price_precision,
    #         tick_size=Decimal(f"{details.minTick:.{price_precision}f}"),
    #         lot_size=Quantity(1),
    #         timestamp=datetime.utcnow(),
    #     )
    #
    #     return future
