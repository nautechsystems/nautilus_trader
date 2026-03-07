# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from typing import Final

from nautilus_trader.core import nautilus_pyo3


BitgetInstrument = (
    nautilus_pyo3.CurrencyPair
    | nautilus_pyo3.CryptoPerpetual
    | nautilus_pyo3.CryptoFuture
)

BITGET_INSTRUMENT_TYPES: Final[
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
