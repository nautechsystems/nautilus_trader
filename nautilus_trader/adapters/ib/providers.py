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

import time
from typing import Dict, List

import ib_insync
from ib_insync import ContractDetails

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.c_enums.asset_class import AssetClass
from nautilus_trader.model.c_enums.asset_class import AssetClassParser
from nautilus_trader.model.c_enums.asset_type import AssetType
from nautilus_trader.model.c_enums.asset_type import AssetTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.future import Future
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class IBInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects through Interactive Brokers.
    """

    def __init__(
        self,
        client: ib_insync.IB,
        host: str = "127.0.0.1",
        port: int = 7497,
        client_id: int = 1,
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
            The unique client ID number for the connection.

        """
        super().__init__()

        self._client = client
        self._host = host
        self._port = port
        self._client_id = client_id

    def connect(self):
        self._client.connect(
            host=self._host,
            port=self._port,
            clientId=self._client_id,
        )

    def load(self, instrument_id: InstrumentId, details: Dict):
        """
        Load the instrument for the given ID and details.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID.
        details : dict
            The instrument details.

        """
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.not_none(details, "details")
        PyCondition.is_in("asset_type", details, "asset_type", "details")

        if not self._client.client.CONNECTED:
            self.connect()

        contract = ib_insync.contract.Contract(
            symbol=instrument_id.symbol.value,
            exchange=instrument_id.venue.value,
            multiplier=details.get("multiplier"),
            currency=details.get("currency"),
        )

        contract_details: List[ContractDetails] = self._client.reqContractDetails(contract=contract)
        if not contract_details:
            raise ValueError(
                f"No contract details found for the given instrument ID {instrument_id}"
            )
        elif len(contract_details) > 1:
            raise ValueError(
                f"Multiple contract details found for the given instrument ID {instrument_id}"
            )

        instrument: Instrument = self._parse_instrument(
            asset_type=AssetTypeParser.from_str(details.get("asset_type")),
            instrument_id=instrument_id,
            details=details,
            contract_details=contract_details[0],
        )

        self._instruments[instrument_id] = instrument

    def _parse_instrument(
        self,
        asset_type: AssetType,
        instrument_id: InstrumentId,
        details: Dict,
        contract_details: ContractDetails,
    ) -> Instrument:
        if asset_type == AssetType.FUTURE:
            PyCondition.is_in("asset_class", details, "asset_class", "details")
            return self._parse_futures_contract(
                instrument_id=instrument_id,
                asset_class=AssetClassParser.from_str(details["asset_class"]),
                details=contract_details,
            )
        elif asset_type == AssetType.SPOT:
            return self._parse_equity_contract(
                instrument_id=instrument_id, details=contract_details
            )
        else:
            raise TypeError(f"No parser for asset_type {asset_type}")

    def _tick_size_to_precision(self, tick_size: float) -> int:
        tick_size_str = f"{tick_size:f}"
        return len(tick_size_str.partition(".")[2].rstrip("0"))

    def _parse_futures_contract(
        self,
        instrument_id: InstrumentId,
        asset_class: AssetClass,
        details: ContractDetails,
    ) -> Future:
        price_precision: int = self._tick_size_to_precision(details.minTick)
        timestamp = time.time_ns()
        future = Future(
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
            ts_event=timestamp,
            ts_init=timestamp,
        )

        return future

    def _parse_equity_contract(
        self,
        instrument_id: InstrumentId,
        details: ContractDetails,
    ) -> Equity:
        price_precision: int = self._tick_size_to_precision(details.minTick)
        timestamp = time.time_ns()
        equity = Equity(
            instrument_id=instrument_id,
            currency=Currency.from_str_c(details.contract.currency),
            price_precision=price_precision,
            price_increment=Price(details.minTick, price_precision),
            multiplier=Quantity.from_int_c(
                int(details.contract.multiplier or details.mdSizeMultiplier)
            ),  # is this right?
            lot_size=Quantity.from_int_c(1),
            contract_id=details.contract.conId,
            local_symbol=details.contract.localSymbol,
            trading_class=details.contract.tradingClass,
            market_name=details.contract.primaryExchange,
            long_name=details.longName,
            time_zone_id=details.timeZoneId,
            trading_hours=details.tradingHours,
            last_trade_time=details.lastTradeTime,
            ts_event=timestamp,
            ts_init=timestamp,
        )
        return equity
