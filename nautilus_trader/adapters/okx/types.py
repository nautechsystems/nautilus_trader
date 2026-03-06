from typing import Final

from nautilus_trader.core import nautilus_pyo3


OkxInstrument = (
    nautilus_pyo3.CurrencyPair
    | nautilus_pyo3.CryptoPerpetual
    | nautilus_pyo3.CryptoFuture
    | nautilus_pyo3.CryptoOption
)

OKX_INSTRUMENT_TYPES: Final[
    tuple[
        type[nautilus_pyo3.CurrencyPair],
        type[nautilus_pyo3.CryptoPerpetual],
        type[nautilus_pyo3.CryptoFuture],
        type[nautilus_pyo3.CryptoOption],
    ]
] = (
    nautilus_pyo3.CurrencyPair,
    nautilus_pyo3.CryptoPerpetual,
    nautilus_pyo3.CryptoFuture,
    nautilus_pyo3.CryptoOption,
)
