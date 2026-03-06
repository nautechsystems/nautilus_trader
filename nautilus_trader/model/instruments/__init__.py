"""
Defines tradable asset/contract instruments with specific properties dependent on the
asset class and instrument class.
"""

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.base import instruments_from_pyo3
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.instruments.binary_option import BinaryOption
from nautilus_trader.model.instruments.cfd import Cfd
from nautilus_trader.model.instruments.commodity import Commodity
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_option import CryptoOption
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.futures_contract import FuturesContract
from nautilus_trader.model.instruments.futures_spread import FuturesSpread
from nautilus_trader.model.instruments.index import IndexInstrument
from nautilus_trader.model.instruments.option_contract import OptionContract
from nautilus_trader.model.instruments.option_spread import OptionSpread
from nautilus_trader.model.instruments.perpetual_contract import PerpetualContract
from nautilus_trader.model.instruments.synthetic import SyntheticInstrument


__all__ = [
    "BettingInstrument",
    "BinaryOption",
    "Cfd",
    "Commodity",
    "CryptoFuture",
    "CryptoOption",
    "CryptoPerpetual",
    "CurrencyPair",
    "Equity",
    "FuturesContract",
    "FuturesSpread",
    "IndexInstrument",
    "Instrument",
    "OptionContract",
    "OptionSpread",
    "PerpetualContract",
    "SyntheticInstrument",
    "instruments_from_pyo3",
]
