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

from os import PathLike

from nautilus_trader.core.nautilus_pyo3 import CryptoHFTDataDataLoader as _Pyo3Loader
from nautilus_trader.model.identifiers import InstrumentId


class CryptoHFTDataDataLoader:
    """
    Provides a Python-friendly wrapper over the Rust CHD data loader.
    """

    def __init__(
        self,
        batch_size: int | None = None,
        gap_policy: str | None = None,
    ) -> None:
        self._pyo3_loader = _Pyo3Loader(batch_size=batch_size, gap_policy=gap_policy)

    def load_trades(
        self,
        path: PathLike[str] | str,
        exchange: str,
        symbol: str,
        instrument_id: InstrumentId | None = None,
    ):
        return self._pyo3_loader.load_trades(str(path), exchange, symbol, instrument_id)

    def load_order_book_deltas(
        self,
        path: PathLike[str] | str,
        exchange: str,
        symbol: str,
        instrument_id: InstrumentId | None = None,
    ):
        return self._pyo3_loader.load_order_book_deltas(str(path), exchange, symbol, instrument_id)

    def load_bars(
        self,
        path: PathLike[str] | str,
        exchange: str,
        symbol: str,
        instrument_id: InstrumentId | None = None,
    ):
        return self._pyo3_loader.load_bars(str(path), exchange, symbol, instrument_id)

    def load_price_updates(
        self,
        path: PathLike[str] | str,
        exchange: str,
        symbol: str,
        instrument_id: InstrumentId | None = None,
    ):
        return self._pyo3_loader.load_price_updates(str(path), exchange, symbol, instrument_id)

    def load_open_interest(
        self,
        path: PathLike[str] | str,
        exchange: str,
        symbol: str,
        instrument_id: InstrumentId | None = None,
    ):
        return self._pyo3_loader.load_open_interest(str(path), exchange, symbol, instrument_id)

    def load_liquidations(
        self,
        path: PathLike[str] | str,
        exchange: str,
        symbol: str,
        instrument_id: InstrumentId | None = None,
    ):
        return self._pyo3_loader.load_liquidations(str(path), exchange, symbol, instrument_id)


__all__ = ["CryptoHFTDataDataLoader"]
