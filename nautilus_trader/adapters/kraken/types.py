from typing import Final

from nautilus_trader.core import nautilus_pyo3


KrakenInstrument = nautilus_pyo3.CurrencyPair | nautilus_pyo3.CryptoPerpetual

KRAKEN_INSTRUMENT_TYPES: Final[
    tuple[
        type[nautilus_pyo3.CurrencyPair],
        type[nautilus_pyo3.CryptoPerpetual],
    ]
] = (
    nautilus_pyo3.CurrencyPair,
    nautilus_pyo3.CryptoPerpetual,
)
