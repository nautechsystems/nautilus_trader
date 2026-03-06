from nautilus_trader.adapters.databento.enums import DatabentoSchema
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId


def instrument_id_to_pyo3(
    instrument_id: InstrumentId | nautilus_pyo3.InstrumentId,
) -> nautilus_pyo3.InstrumentId:
    if isinstance(instrument_id, nautilus_pyo3.InstrumentId):
        return instrument_id

    return nautilus_pyo3.InstrumentId.from_str(instrument_id.value)


def databento_schema_from_nautilus_bar_type(bar_type: BarType) -> DatabentoSchema:
    """
    Return the Databento bar aggregate schema string for the given Nautilus `bar_type`.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the schema.

    Returns
    -------
    str

    Raises
    ------
    ValueError
        If any property of `bar_type` is invalid to map to a Databento schema.

    """
    PyCondition.is_true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

    if not bar_type.spec.is_time_aggregated():
        raise ValueError(
            f"Invalid bar type '{bar_type}' (only time bars are aggregated by Databento).",
        )

    if bar_type.spec.price_type != PriceType.LAST:
        raise ValueError(
            f"Invalid bar type '{bar_type}' (only `LAST` price bars are aggregated by Databento).",
        )

    if bar_type.spec.step != 1:
        raise ValueError(
            f"Invalid bar type '{bar_type}' (only a step of 1 is supported by Databento).",
        )

    match bar_type.spec.aggregation:
        case BarAggregation.SECOND:
            return DatabentoSchema.OHLCV_1S
        case BarAggregation.MINUTE:
            return DatabentoSchema.OHLCV_1M
        case BarAggregation.HOUR:
            return DatabentoSchema.OHLCV_1H
        case BarAggregation.DAY:
            return DatabentoSchema.OHLCV_1D
        case _:
            raise ValueError(
                f"Invalid bar type '{bar_type}'. "
                "Use any of ['SECOND', 'MINUTE', 'HOUR', 'DAY'] time aggregations.",
            )
