# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Provides a PyO3-based historical data client for Interactive Brokers.

This adapter uses PyO3 bindings to call the Rust implementation of the Interactive
Brokers adapter, providing the same API as the Python adapter but with Rust performance.

"""

from __future__ import annotations

from datetime import UTC
from datetime import datetime

from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import IBContractSpec
from nautilus_trader.adapters.interactive_brokers_pyo3._contracts import ib_contract_specs_to_dicts
from nautilus_trader.cache.transformers import transform_instrument_from_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.core.nautilus_pyo3 import InstrumentId as PyO3InstrumentId
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.instruments import Instrument


try:
    from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
        HistoricalInteractiveBrokersClient as RustHistoricalInteractiveBrokersClient,
    )
except ImportError:
    RustHistoricalInteractiveBrokersClient = None


class HistoricalInteractiveBrokersClient:
    """
    Provides a PyO3-based historical data client for Interactive Brokers.

    This class wraps the Rust implementation via PyO3 bindings, providing
    the same API as the Python adapter but using the Rust implementation.

    Parameters
    ----------
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.
    config : InteractiveBrokersInstrumentProviderConfig
        Configuration for the client.

    Raises
    ------
    ImportError
        If the PyO3 bindings are not available.

    """

    def __init__(
        self,
        instrument_provider,  # InteractiveBrokersInstrumentProvider
        config,  # InteractiveBrokersInstrumentProviderConfig
    ) -> None:
        if RustHistoricalInteractiveBrokersClient is None:
            raise ImportError(
                "PyO3 bindings for Interactive Brokers are not available. "
                "Please ensure the extension module is built with the 'extension-module' feature.",
            )

        # Initialize the Rust client via PyO3
        self._rust_client = RustHistoricalInteractiveBrokersClient(
            instrument_provider._rust_provider,
            config,
        )

    @staticmethod
    def _normalize_instrument_ids(
        instrument_ids: list[str] | None,
    ) -> list[PyO3InstrumentId] | None:
        if not instrument_ids:
            return None

        return [PyO3InstrumentId.from_str(inst_id) for inst_id in instrument_ids]

    @staticmethod
    def _normalize_instruments(instruments: list[Instrument]) -> list[Instrument]:
        return [transform_instrument_from_pyo3(item) for item in instruments]

    @staticmethod
    def _normalize_bars(bars: list[Bar]) -> list[Bar]:
        if bars and not isinstance(bars[0], Bar):
            return Bar.from_pyo3_list(bars)

        return bars

    @staticmethod
    def _normalize_data(data: list[Data]) -> list[Data]:
        if data and type(data[0]).__name__ == "PyCapsule":
            return [capsule_to_data(item) for item in data]

        return data

    @staticmethod
    def _normalize_request_datetime(value: datetime, field_name: str) -> datetime:
        if value.tzinfo is None:
            return value.replace(tzinfo=UTC)

        offset = value.utcoffset()
        if offset is None or offset.total_seconds() != 0:
            raise ValueError(
                f"{field_name} must be UTC. Configure TWS / IB Gateway to use UTC timestamps.",
            )

        return value.astimezone(UTC)

    async def request_instruments(
        self,
        instrument_ids: list[str] | None = None,
        contracts: list[IBContractSpec] | None = None,
    ) -> list[Instrument]:
        """
        Request instruments from Interactive Brokers.

        Parameters
        ----------
        instrument_ids : list[str], optional
            List of instrument ID strings (e.g., "AAPL.NASDAQ").
        contracts : list[IBContract], optional
            List of IB contracts.

        Returns
        -------
        list[Instrument]
            List of loaded instruments.

        """
        contracts_dicts = ib_contract_specs_to_dicts(contracts)

        # Convert instrument ID strings to InstrumentId objects
        instrument_id_objs = self._normalize_instrument_ids(instrument_ids)

        # Call Rust client method
        result = await self._rust_client.request_instruments(
            instrument_ids=instrument_id_objs,
            contracts=contracts_dicts,
        )

        return self._normalize_instruments(result)

    async def request_bars(
        self,
        bar_specifications: list[str],
        end_date_time: datetime,
        start_date_time: datetime | None = None,
        duration: str | None = None,
        contracts: list[IBContractSpec] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
        timeout: int = 120,
    ) -> list[Bar]:
        """
        Request historical bars from Interactive Brokers.

        Parameters
        ----------
        bar_specifications : list[str]
            List of bar specifications (e.g., ["1-HOUR-LAST", "5-MINUTE-MID"]).
        end_date_time : datetime
            End date/time for the bars.
        start_date_time : datetime, optional
            Start date/time. Either this or duration must be provided.
        duration : str, optional
            Duration string (e.g., "1 D", "1 W"). Either this or start_date_time must be provided.
        contracts : list[IBContract], optional
            List of IB contracts.
        instrument_ids : list[str], optional
            List of instrument ID strings.
        use_rth : bool, default True
            Use regular trading hours only.
        timeout : int, default 120
            Request timeout in seconds.

        Returns
        -------
        list[Bar]
            List of historical bars.

        """
        contracts_dicts = ib_contract_specs_to_dicts(contracts)

        # Convert instrument ID strings to InstrumentId objects
        instrument_id_objs = self._normalize_instrument_ids(instrument_ids)

        end_date_time = self._normalize_request_datetime(end_date_time, "end_date_time")

        start_dt_utc = None

        if start_date_time:
            start_dt_utc = self._normalize_request_datetime(
                start_date_time,
                "start_date_time",
            )

        # Call Rust client method
        result = await self._rust_client.request_bars(
            bar_specifications=bar_specifications,
            end_date_time=end_date_time,
            start_date_time=start_dt_utc,
            duration=duration,
            contracts=contracts_dicts,
            instrument_ids=instrument_id_objs,
            use_rth=use_rth,
            timeout=timeout,
        )

        return self._normalize_bars(result)

    async def request_ticks(
        self,
        tick_type: str,
        start_date_time: datetime,
        end_date_time: datetime,
        contracts: list[IBContractSpec] | None = None,
        instrument_ids: list[str] | None = None,
        use_rth: bool = True,
        timeout: int = 120,
    ) -> list[Data]:
        """
        Request historical ticks from Interactive Brokers.

        Parameters
        ----------
        tick_type : str
            Type of ticks: "TRADES" or "BID_ASK".
        start_date_time : datetime
            Start date/time for the ticks.
        end_date_time : datetime
            End date/time for the ticks.
        contracts : list[IBContract], optional
            List of IB contracts.
        instrument_ids : list[str], optional
            List of instrument ID strings.
        use_rth : bool, default True
            Use regular trading hours only.
        timeout : int, default 120
            Request timeout in seconds.

        Returns
        -------
        list[Data]
            List of historical ticks (QuoteTick or TradeTick).

        """
        contracts_dicts = ib_contract_specs_to_dicts(contracts)

        # Convert instrument ID strings to InstrumentId objects
        instrument_id_objs = self._normalize_instrument_ids(instrument_ids)

        start_date_time = self._normalize_request_datetime(start_date_time, "start_date_time")
        end_date_time = self._normalize_request_datetime(end_date_time, "end_date_time")

        # Call Rust client method
        result = await self._rust_client.request_ticks(
            tick_type=tick_type,
            start_date_time=start_date_time,
            end_date_time=end_date_time,
            contracts=contracts_dicts,
            instrument_ids=instrument_id_objs,
            use_rth=use_rth,
            timeout=timeout,
        )

        return self._normalize_data(result)

    async def connect(self) -> None:
        """
        Maintain compatibility with the legacy historical client API.
        """
        return None


HistoricInteractiveBrokersClient = HistoricalInteractiveBrokersClient
