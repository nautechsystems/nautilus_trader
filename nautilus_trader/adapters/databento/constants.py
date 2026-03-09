from pathlib import Path
from typing import Final

from nautilus_trader.model.identifiers import ClientId


DATABENTO: Final[str] = "DATABENTO"
DATABENTO_CLIENT_ID: Final[ClientId] = ClientId(DATABENTO)

ALL_SYMBOLS: Final[str] = "ALL_SYMBOLS"

PUBLISHERS_FILEPATH: Final[Path] = (Path(__file__).resolve().parent / "publishers.json").resolve()
