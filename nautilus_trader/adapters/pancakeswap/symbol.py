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

from eth_utils import to_checksum_address

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


def pool_venue(chain: str, dex_type: str) -> Venue:
    """
    Build a DEX venue from chain and dex name components.

    Parameters
    ----------
    chain : str
        The chain name (for example ``"Bsc"``).
    dex_type : str
        The dex name (for example ``"PancakeSwapV2"``).

    Returns
    -------
    Venue

    """
    return Venue(f"{chain}:{dex_type}")


def pool_instrument_id(pool_address: str, chain: str, dex_type: str) -> InstrumentId:
    """
    Build and validate a pool instrument ID.

    Address validation and normalization are strict and checksum-aware.

    Parameters
    ----------
    pool_address : str
        The configured pool contract address.
    chain : str
        The chain name.
    dex_type : str
        The dex name.

    Returns
    -------
    InstrumentId

    Raises
    ------
    ValueError
        If the pool address is invalid for a DEX instrument ID.

    """
    normalized_pool_address = _normalize_checksum_address(
        pool_address,
        chain=chain,
        dex_type=dex_type,
        label="pool address",
    )
    return InstrumentId(Symbol(normalized_pool_address), pool_venue(chain, dex_type))


def normalize_address(address: str, chain: str, dex_type: str, label: str = "address") -> str:
    """
    Validate and normalize an EVM address using DEX instrument-ID parsing.

    Parameters
    ----------
    address : str
        The address to normalize.
    chain : str
        The chain name.
    dex_type : str
        The dex name.
    label : str, default "address"
        Label used in raised error messages.

    Returns
    -------
    str
        The normalized (checksummed) address string.

    Raises
    ------
    ValueError
        If the address is invalid.

    """
    return _normalize_checksum_address(
        address,
        chain=chain,
        dex_type=dex_type,
        label=label,
    )


def _normalize_checksum_address(address: str, chain: str, dex_type: str, label: str) -> str:
    if not address or address.strip() != address:
        raise ValueError(
            f"Invalid {label} '{address}' for venue '{chain}:{dex_type}': blank or padded values are not allowed",
        )

    try:
        return to_checksum_address(address)
    except ValueError as exc:
        raise ValueError(
            f"Invalid {label} '{address}' for venue '{chain}:{dex_type}': {exc}",
        ) from exc


def validate_factory_pair_address(
    pool_address: str,
    factory_pair_address: str,
    chain: str,
    dex_type: str,
) -> None:
    """
    Validate optional onboarding `factory.getPair(...)` result against configured pool address.

    Parameters
    ----------
    pool_address : str
        The normalized configured pool address.
    factory_pair_address : str
        The onboarding `factory.getPair(token0, token1)` result.
    chain : str
        The chain name.
    dex_type : str
        The dex name.

    Raises
    ------
    ValueError
        If `factory_pair_address` is provided and does not match `pool_address`.

    """
    normalized_factory_pair = normalize_address(
        factory_pair_address,
        chain=chain,
        dex_type=dex_type,
        label="factory.getPair address",
    )

    if normalized_factory_pair != pool_address:
        raise ValueError(
            "Configured pool address does not match factory.getPair(token0, token1): "
            f"pool={pool_address}, factory.getPair={normalized_factory_pair}",
        )
