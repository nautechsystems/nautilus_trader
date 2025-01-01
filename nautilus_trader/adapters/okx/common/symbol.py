# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Final

from nautilus_trader.adapters.okx.common.constants import OKX_VENUE
from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


VALID_SUFFIXES: Final[list[str]] = [
    "-SPOT",
    "-MARGIN",
    "-LINEAR",
    "-INVERSE",
    "-OPTION",
]


def has_valid_okx_suffix(symbol: str) -> bool:
    """
    Return whether the given `symbol` string contains a valid OKX suffix.

    Parameters
    ----------
    symbol : str
        The symbol string value to check.

    Returns
    -------
    bool
        True if contains a valid suffix, else False.

    """
    for suffix in VALID_SUFFIXES:
        if suffix in symbol:
            return True
    return False


class OKXSymbol(str):
    """
    Represents an OKX specific symbol containing a instrument type suffix.
    """

    def __new__(cls, symbol: str) -> "OKXSymbol":
        PyCondition.valid_string(symbol, "symbol")
        if not has_valid_okx_suffix(symbol):
            raise ValueError(
                f"Invalid symbol '{symbol}': "
                f"does not contain a valid suffix from {VALID_SUFFIXES}",
            )

        return super().__new__(
            cls,
            symbol.upper(),
        )

    @staticmethod
    def from_raw_symbol(
        raw_symbol: str,
        instrument_type: OKXInstrumentType,
        contract_type: OKXContractType | None = None,
    ) -> "OKXSymbol":
        if instrument_type in [OKXInstrumentType.FUTURES, OKXInstrumentType.SWAP]:
            assert contract_type is not None and contract_type != OKXContractType.NONE, (
                f"`contract_type` must be either {OKXContractType.LINEAR} or "
                f"{OKXContractType.INVERSE} to parse SWAP and FUTURES instruments, got "
                f"{contract_type}"
            )

        if instrument_type == OKXInstrumentType.SPOT:
            return OKXSymbol(raw_symbol + "-SPOT")
        elif instrument_type == OKXInstrumentType.MARGIN:
            return OKXSymbol(raw_symbol + "-MARGIN")
        elif instrument_type == OKXInstrumentType.OPTION:
            return OKXSymbol(raw_symbol + "-OPTION")
        elif instrument_type in [OKXInstrumentType.FUTURES, OKXInstrumentType.SWAP]:
            suffix = "-LINEAR" if contract_type == OKXContractType.LINEAR else "-INVERSE"
            return OKXSymbol(raw_symbol + suffix)
        else:
            raise ValueError(
                f"Cannot parse raw symbol {raw_symbol!r} to nautilus OKXSymbol, unknown instrument "
                f"type {instrument_type}",
            )

    @property
    def raw_symbol(self) -> str:
        """
        Return the raw OKX symbol (without the instrument type suffix).

        Returns
        -------
        str

        """
        return str(self).rpartition("-")[0]

    @property
    def instrument_type(self) -> OKXInstrumentType:
        """
        Return the OKX instrument type for the symbol.

        Returns
        -------
        OKXInstrumentType

        """
        if self.endswith("-SPOT"):
            return OKXInstrumentType.SPOT
        elif self.endswith("-MARGIN"):
            return OKXInstrumentType.MARGIN
        elif self.endswith(("-SWAP-LINEAR", "-SWAP-INVERSE")):
            # NOTE: okx puts "-SWAP" in perp symbols
            # NOTE: the order here matters, we check for SWAP before FUTURES because FUTURES symbols
            # do not contain a corresponding identifying suffix (i.e., OKX does not add 'FUTURES'
            # do those symbols)
            return OKXInstrumentType.SWAP
        elif self.endswith(("-LINEAR", "-INVERSE")):
            return OKXInstrumentType.FUTURES
        elif self.endswith("-OPTION"):
            return OKXInstrumentType.OPTION
        else:
            raise ValueError(f"Unknown instrument type for symbol {self}")

    @property
    def contract_type(self) -> OKXContractType:
        """
        Return the OKX contract type for the symbol.

        Returns
        -------
        OKXContractType

        """
        if self.endswith("-LINEAR"):
            return OKXContractType.LINEAR
        elif self.endswith("-INVERSE"):
            return OKXContractType.INVERSE
        elif self.endswith(("-SPOT", "-MARGIN", "-OPTION")):
            return OKXContractType.NONE
        else:
            raise ValueError(
                f"Unknown contract type for symbol {self} due to unrecognized suffix - valid "
                f"suffixes: {VALID_SUFFIXES}",
            )

    @property
    def is_spot(self) -> bool:
        """
        Return whether a SPOT instrument type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXInstrumentType.SPOT

    @property
    def is_margin(self) -> bool:
        """
        Return whether a MARGIN instrument type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXInstrumentType.MARGIN

    @property
    def is_swap(self) -> bool:
        """
        Return whether a SWAP instrument type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXInstrumentType.SWAP

    @property
    def is_futures(self) -> bool:
        """
        Return whether a FUTURES instrument type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXInstrumentType.FUTURES

    @property
    def is_linear(self) -> bool:
        """
        Return whether a linear contract type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXContractType.LINEAR

    @property
    def is_inverse(self) -> bool:
        """
        Return whether an inverse contract type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXContractType.INVERSE

    @property
    def is_option(self) -> bool:
        """
        Return whether an OPTION instrument type.

        Returns
        -------
        bool

        """
        return self.instrument_type == OKXInstrumentType.OPTION

    def to_instrument_id(self) -> InstrumentId:
        """
        Parse the OKX symbol into a Nautilus instrument ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId(Symbol(str(self)), OKX_VENUE)
