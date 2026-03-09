from __future__ import annotations

from dataclasses import dataclass

from nautilus_trader.core.data import Data
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import Instrument


@dataclass(frozen=True)
class CatalogDataResult:
    """
    Represents a catalog data query result.
    """

    data_cls: type
    data: list[Data]
    instruments: list[Instrument] | None = None
    client_id: ClientId | None = None
