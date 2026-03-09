from typing import Final

from nautilus_trader.core import nautilus_pyo3


BitmexInstrument = (
    nautilus_pyo3.CurrencyPair | nautilus_pyo3.CryptoPerpetual | nautilus_pyo3.CryptoFuture
)

BITMEX_INSTRUMENT_TYPES: Final[
    tuple[
        type[nautilus_pyo3.CurrencyPair],
        type[nautilus_pyo3.CryptoPerpetual],
        type[nautilus_pyo3.CryptoFuture],
    ]
] = (
    nautilus_pyo3.CurrencyPair,
    nautilus_pyo3.CryptoPerpetual,
    nautilus_pyo3.CryptoFuture,
)
