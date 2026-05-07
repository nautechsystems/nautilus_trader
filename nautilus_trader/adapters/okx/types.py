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

from dataclasses import dataclass
from typing import Final

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import GreeksConvention
from nautilus_trader.core.nautilus_pyo3 import OKXGreeksType
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId


OkxInstrument = (
    nautilus_pyo3.CurrencyPair
    | nautilus_pyo3.CryptoPerpetual
    | nautilus_pyo3.CryptoFuture
    | nautilus_pyo3.CryptoOption
    | nautilus_pyo3.BinaryOption
)

OKX_INSTRUMENT_TYPES: Final[
    tuple[
        type[nautilus_pyo3.CurrencyPair],
        type[nautilus_pyo3.CryptoPerpetual],
        type[nautilus_pyo3.CryptoFuture],
        type[nautilus_pyo3.CryptoOption],
        type[nautilus_pyo3.BinaryOption],
    ]
] = (
    nautilus_pyo3.CurrencyPair,
    nautilus_pyo3.CryptoPerpetual,
    nautilus_pyo3.CryptoFuture,
    nautilus_pyo3.CryptoOption,
    nautilus_pyo3.BinaryOption,
)

GREEKS_CONVENTION_TO_TYPE: Final[dict[GreeksConvention, OKXGreeksType]] = {
    GreeksConvention.BLACK_SCHOLES: OKXGreeksType.BS,
    GreeksConvention.PRICE_ADJUSTED: OKXGreeksType.PA,
}


@dataclass(frozen=True)
class OKXAttachedOcoBinding:
    parent_client_order_id: ClientOrderId
    attach_client_order_id: ClientOrderId
    instrument_id: InstrumentId
    sl_client_order_id: ClientOrderId | None
    tp_client_order_id: ClientOrderId | None

    def child_client_order_ids(self) -> list[ClientOrderId]:
        child_ids: list[ClientOrderId] = []

        if self.sl_client_order_id is not None:
            child_ids.append(self.sl_client_order_id)
        if self.tp_client_order_id is not None and self.tp_client_order_id not in child_ids:
            child_ids.append(self.tp_client_order_id)
        return child_ids

    def all_client_order_ids(self) -> list[ClientOrderId]:
        ids = [self.parent_client_order_id, self.attach_client_order_id]
        for child_id in self.child_client_order_ids():
            if child_id not in ids:
                ids.append(child_id)
        return ids
