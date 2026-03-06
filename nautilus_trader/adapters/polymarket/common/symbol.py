from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.model.identifiers import InstrumentId


def get_polymarket_instrument_id(condition_id: str, token_id: str | int) -> InstrumentId:
    return InstrumentId.from_str(f"{condition_id}-{token_id}.{POLYMARKET_VENUE}")


def get_polymarket_condition_id(instrument_id: InstrumentId) -> str:
    parts = instrument_id.symbol.value.split("-")
    if len(parts) != 2 or not parts[0]:
        raise ValueError(
            f"Invalid Polymarket instrument ID format: expected "
            f"'{{condition_id}}-{{token_id}}', was '{instrument_id.symbol.value}'",
        )
    return parts[0]


def get_polymarket_token_id(instrument_id: InstrumentId) -> str:
    parts = instrument_id.symbol.value.split("-")
    if len(parts) != 2 or not parts[1]:
        raise ValueError(
            f"Invalid Polymarket instrument ID format: expected "
            f"'{{condition_id}}-{{token_id}}', was '{instrument_id.symbol.value}'",
        )
    return parts[1]
